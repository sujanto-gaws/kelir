import { createPinia, setActivePinia } from 'pinia'
import { flushPromises, mount, type DOMWrapper, type VueWrapper } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import RoleListPage from './RoleListPage.vue'
import { registerSessionBridge } from '@/api/session'
import {
  errorBody,
  installFakeBackend,
  itemBody,
  type FakeBackendHandle,
  type FakeHandler,
  type FakeReply,
} from '@/lib/testing/fake-backend'
import { useAuthStore } from '@/stores/auth'
import type { PageMeta } from '@/types/api'
import type { CurrentUser } from '@/types/auth'
import type { Permission, Role } from '@/types/identity'

/**
 * The backend's refusal, hard-coded rather than imported from the component.
 *
 * `RoleListPage.vue` mirrors this string from
 * `kelir-backend/src/modules/identity/service.rs`. Importing the constant would
 * make the assertion pass for any wording at all; spelling it out here is what
 * makes a silent edit on either side fail a test.
 */
const SYSTEM_ROLE_REFUSAL =
  'System roles cannot be deleted; a tenant would be left unable to grant permissions'

const readUsers: Permission = {
  id: 'p-1',
  permissionCode: 'identity:user:read',
  module: 'identity',
  description: 'Read users',
}

const createUsers: Permission = {
  id: 'p-2',
  permissionCode: 'identity:user:create',
  module: 'identity',
  description: null,
}

const adminRole: Role = {
  id: 'r-1',
  roleCode: 'ROLE-ADMIN',
  name: 'Administrator',
  description: 'Full access',
  isSystem: true,
  permissions: [readUsers, createUsers],
}

const clerkRole: Role = {
  id: 'r-2',
  roleCode: 'ROLE-CLERK',
  name: 'Clerk',
  description: null,
  isSystem: false,
  permissions: [readUsers],
}

/** A list envelope. `itemBody` covers the item shape; lists carry `meta` as well. */
function listBody(data: unknown[], meta?: PageMeta): unknown {
  return { success: true, data, meta: meta ?? { page: 1, pageSize: 20, total: data.length } }
}

function rolesReply(items: Role[], meta?: PageMeta): FakeReply {
  return { status: 200, body: listBody(items, meta) }
}

/** The catalogue answers with an item envelope wrapping an array — no `meta`. */
function permissionsReply(items: Permission[]): FakeReply {
  return { status: 200, body: itemBody(items) }
}

function signIn(permissions: string[]): void {
  const user: CurrentUser = {
    id: 'u-1',
    username: 'ana',
    displayName: 'Ana Putri',
    email: 'ana@example.com',
    roles: ['ROLE-ADMIN'],
    permissions,
  }

  useAuthStore().user = user
}

async function mountPage(permissions: string[]): Promise<VueWrapper> {
  // Seed before mounting: `onMounted` fires on the first render, and the
  // controls it decides on are gated by these codes.
  signIn(permissions)

  const wrapper = mount(RoleListPage)
  await flushPromises()

  return wrapper
}

function rowsOf(wrapper: VueWrapper): DOMWrapper<Element>[] {
  return wrapper.findAll('tbody tr')
}

function buttonLabelled(
  buttons: DOMWrapper<HTMLButtonElement>[],
  label: string,
): DOMWrapper<HTMLButtonElement> | undefined {
  return buttons.find((button) => button.text() === label)
}

describe('RoleListPage', () => {
  let backend: FakeBackendHandle
  let handler: FakeHandler

  beforeEach(() => {
    setActivePinia(createPinia())
    window.localStorage.clear()

    handler = (request) =>
      request.url === '/identity/permissions'
        ? permissionsReply([readUsers, createUsers])
        : rolesReply([adminRole, clerkRole])
    backend = installFakeBackend((request) => handler(request))
  })

  afterEach(() => {
    backend.restore()
    registerSessionBridge(null)
  })

  it('hides every action from a caller who can only read', async () => {
    const wrapper = await mountPage(['identity:role:read'])
    const buttons = wrapper.findAll('button')

    expect(rowsOf(wrapper)).toHaveLength(2)
    expect(buttonLabelled(buttons, 'New role')).toBeUndefined()
    expect(buttonLabelled(buttons, 'Edit')).toBeUndefined()
    expect(buttonLabelled(buttons, 'Delete')).toBeUndefined()
    expect(wrapper.text()).not.toContain('Actions')
  })

  it('renders the rows but no controls for a caller holding no permissions at all', async () => {
    const wrapper = await mountPage([])
    const labels = wrapper.findAll('button').map((button) => button.text())

    expect(rowsOf(wrapper)).toHaveLength(2)
    // Pagination is navigation, not an action, so it is all that survives.
    expect(labels).toEqual(['Previous', 'Next'])
  })

  it('shows the create control to a caller who may create', async () => {
    const wrapper = await mountPage(['identity:role:read', 'identity:role:create'])

    expect(buttonLabelled(wrapper.findAll('button'), 'New role')).toBeDefined()
  })

  it('renders each role with its description and permission count', async () => {
    const wrapper = await mountPage(['identity:role:read'])
    const rows = rowsOf(wrapper)

    expect(rows).toHaveLength(2)

    const admin = rows[0].findAll('td')
    expect(admin[0].text()).toContain('ROLE-ADMIN')
    expect(admin[1].text()).toBe('Administrator')
    expect(admin[2].text()).toBe('Full access')
    expect(admin[3].text()).toBe('2')

    const clerk = rows[1].findAll('td')
    expect(clerk[0].text()).toContain('ROLE-CLERK')
    // A null description is a dash, not a blank cell that reads as a layout bug.
    expect(clerk[2].text()).toBe('—')
    expect(clerk[3].text()).toBe('1')
  })

  it('marks a system role and refuses to delete it', async () => {
    const wrapper = await mountPage(['identity:role:read', 'identity:role:delete'])
    const rows = rowsOf(wrapper)

    expect(rows[0].text()).toContain('System')
    expect(rows[1].text()).not.toContain('System')

    const systemDelete = buttonLabelled(rows[0].findAll('button'), 'Delete')
    const ordinaryDelete = buttonLabelled(rows[1].findAll('button'), 'Delete')

    expect(systemDelete?.element.disabled).toBe(true)
    expect(ordinaryDelete?.element.disabled).toBe(false)
  })

  it('states why a system role cannot be deleted', async () => {
    const wrapper = await mountPage(['identity:role:read', 'identity:role:delete'])

    // Literal, not imported: this is the drift guard against the backend's
    // wording in `identity/service.rs`.
    expect(wrapper.text()).toContain(SYSTEM_ROLE_REFUSAL)
  })

  it('keeps the confirmation open and shows the refusal when a delete is rejected', async () => {
    handler = (request) => {
      if (request.method === 'delete') {
        return {
          status: 409,
          body: errorBody('CONFLICT', 'That role is still granted to 3 users'),
        }
      }

      return request.url === '/identity/permissions'
        ? permissionsReply([readUsers, createUsers])
        : rolesReply([adminRole, clerkRole])
    }

    const wrapper = await mountPage(['identity:role:read', 'identity:role:delete'])

    await buttonLabelled(rowsOf(wrapper)[1].findAll('button'), 'Delete')?.trigger('click')

    const dialog = wrapper.find('[role="dialog"]')
    expect(dialog.exists()).toBe(true)

    await buttonLabelled(dialog.findAll('button'), 'Delete')?.trigger('click')
    await flushPromises()

    // The server's own words, next to the button that caused them, with the
    // dialog still open so they can be read.
    expect(wrapper.find('[role="dialog"] [role="alert"]').text()).toContain(
      'That role is still granted to 3 users',
    )
    expect(wrapper.find('[role="dialog"]').exists()).toBe(true)
  })

  it('still lists the roles when the permission catalogue cannot be loaded', async () => {
    handler = (request) =>
      request.url === '/identity/permissions'
        ? {
            status: 500,
            body: errorBody('INTERNAL_ERROR', 'The permission catalogue is unavailable'),
          }
        : rolesReply([adminRole, clerkRole])

    const wrapper = await mountPage(['identity:role:read', 'identity:role:update'])

    // The two loads are independent: losing the catalogue costs the editor its
    // permission picker, not the page its rows.
    expect(rowsOf(wrapper)).toHaveLength(2)
    expect(wrapper.text()).toContain('The permission catalogue could not be loaded')
    expect(wrapper.text()).toContain('The permission catalogue is unavailable')
  })
})
