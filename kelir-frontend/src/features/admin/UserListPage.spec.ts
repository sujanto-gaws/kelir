import { createPinia, setActivePinia } from 'pinia'
import { flushPromises, mount, type DOMWrapper, type VueWrapper } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import UserListPage from './UserListPage.vue'
import { registerSessionBridge } from '@/api/session'
import {
  errorBody,
  installFakeBackend,
  type FakeBackendHandle,
  type FakeHandler,
  type FakeReply,
} from '@/lib/testing/fake-backend'
import { useAuthStore } from '@/stores/auth'
import type { PageMeta } from '@/types/api'
import type { CurrentUser } from '@/types/auth'
import type { Role, RoleSummary, User } from '@/types/identity'

const clerkSummary: RoleSummary = { id: 'r-1', roleCode: 'ROLE-CLERK', name: 'Clerk' }
const approverSummary: RoleSummary = { id: 'r-2', roleCode: 'ROLE-APPROVER', name: 'Approver' }

const clerkRole: Role = {
  id: 'r-1',
  roleCode: 'ROLE-CLERK',
  name: 'Clerk',
  description: null,
  isSystem: false,
  permissions: [],
}

const ana: User = {
  id: 'u-1',
  username: 'ana',
  email: 'ana@example.com',
  displayName: 'Ana Putri',
  status: 'ACTIVE',
  departmentId: null,
  mustChangePassword: false,
  lastLoginAt: null,
  createdAt: '2026-01-01T00:00:00Z',
  roles: [clerkSummary, approverSummary],
}

const bima: User = {
  id: 'u-2',
  username: 'bima',
  email: 'bima@example.com',
  displayName: 'Bima Santoso',
  status: 'INACTIVE',
  departmentId: null,
  mustChangePassword: false,
  lastLoginAt: null,
  createdAt: '2026-01-02T00:00:00Z',
  roles: [clerkSummary],
}

/** The signed-in principal, so the self-deactivation guard has something to match. */
const SIGNED_IN_ID = ana.id

/** A list envelope. `itemBody` covers the item shape; lists carry `meta` as well. */
function listBody(data: unknown[], meta?: PageMeta): unknown {
  return { success: true, data, meta: meta ?? { page: 1, pageSize: 20, total: data.length } }
}

function usersReply(items: User[], meta?: PageMeta): FakeReply {
  return { status: 200, body: listBody(items, meta) }
}

function rolesReply(items: Role[]): FakeReply {
  return { status: 200, body: listBody(items) }
}

function signIn(permissions: string[]): void {
  const user: CurrentUser = {
    id: SIGNED_IN_ID,
    username: 'ana',
    displayName: 'Ana Putri',
    email: 'ana@example.com',
    roles: ['ROLE-ADMIN'],
    permissions,
  }

  useAuthStore().user = user
}

async function mountPage(permissions: string[]): Promise<VueWrapper> {
  // Seed before mounting: `onMounted` reads the permissions to decide whether
  // the role catalogue is fetched at all.
  signIn(permissions)

  const wrapper = mount(UserListPage)
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

describe('UserListPage', () => {
  let backend: FakeBackendHandle
  let handler: FakeHandler

  beforeEach(() => {
    setActivePinia(createPinia())
    window.localStorage.clear()

    handler = (request) =>
      request.url === '/identity/roles' ? rolesReply([clerkRole]) : usersReply([ana, bima])
    backend = installFakeBackend((request) => handler(request))
  })

  afterEach(() => {
    backend.restore()
    registerSessionBridge(null)
  })

  it('hides every action from a caller who can only read', async () => {
    const wrapper = await mountPage(['identity:user:read'])
    const buttons = wrapper.findAll('button')

    expect(rowsOf(wrapper)).toHaveLength(2)
    expect(buttonLabelled(buttons, 'New user')).toBeUndefined()
    expect(buttonLabelled(buttons, 'Edit')).toBeUndefined()
    expect(buttonLabelled(buttons, 'Deactivate')).toBeUndefined()
    // With nothing to act on, the column itself goes too rather than leaving a
    // permanently empty cell in every row.
    expect(wrapper.text()).not.toContain('Actions')
  })

  it('renders the rows but no controls for a caller holding no permissions at all', async () => {
    const wrapper = await mountPage([])
    const labels = wrapper.findAll('button').map((button) => button.text())

    expect(rowsOf(wrapper)).toHaveLength(2)
    // Pagination is navigation, not an action, so it is all that survives.
    expect(labels).toEqual(['Previous', 'Next'])
    // The role catalogue is behind `identity:role:read`, so it is not even asked
    // for — an empty picker would look like a broken one.
    expect(backend.countOf('/identity/roles')).toBe(0)
  })

  it('shows the create control to a caller who may create', async () => {
    const wrapper = await mountPage(['identity:user:read', 'identity:user:create'])

    expect(buttonLabelled(wrapper.findAll('button'), 'New user')).toBeDefined()
  })

  it('renders one row per user with its status label and a badge per role', async () => {
    const wrapper = await mountPage(['identity:user:read'])
    const rows = rowsOf(wrapper)

    expect(rows).toHaveLength(2)

    const first = rows[0]
    expect(first.text()).toContain('ana')
    expect(first.text()).toContain('Ana Putri')
    expect(first.text()).toContain('ana@example.com')
    // The label, not the wire enum: `USER_STATUS_LABELS` is what a person reads.
    expect(first.text()).toContain('Active')
    expect(first.text()).not.toContain('ACTIVE')
    expect(rows[1].text()).toContain('Inactive')
    expect(rows[1].text()).not.toContain('INACTIVE')

    // The roles cell wraps its badges in a single span, so the badges are the
    // nested ones — one for each role granted.
    const rolesCell = first.findAll('td')[4]
    expect(rolesCell.findAll('span > span')).toHaveLength(2)
    expect(rolesCell.text()).toContain('Clerk')
    expect(rolesCell.text()).toContain('Approver')
  })

  it('disables deactivation of the signed-in account alone', async () => {
    const wrapper = await mountPage(['identity:user:read', 'identity:user:delete'])
    const rows = rowsOf(wrapper)

    const own = buttonLabelled(rows[0].findAll('button'), 'Deactivate')
    const other = buttonLabelled(rows[1].findAll('button'), 'Deactivate')

    expect(own?.element.disabled).toBe(true)
    expect(other?.element.disabled).toBe(false)
  })

  it('keeps the confirmation open and shows the refusal when deactivation fails', async () => {
    handler = (request) => {
      if (request.method === 'delete') {
        return {
          status: 400,
          body: errorBody('BAD_REQUEST', 'You cannot deactivate your own account'),
        }
      }

      return request.url === '/identity/roles' ? rolesReply([clerkRole]) : usersReply([ana, bima])
    }

    const wrapper = await mountPage(['identity:user:read', 'identity:user:delete'])

    await buttonLabelled(rowsOf(wrapper)[1].findAll('button'), 'Deactivate')?.trigger('click')

    const dialog = wrapper.find('[role="dialog"]')
    expect(dialog.exists()).toBe(true)

    await buttonLabelled(dialog.findAll('button'), 'Deactivate')?.trigger('click')
    await flushPromises()

    // The server's own words, relayed verbatim and next to the button that
    // caused them — and the dialog stays put so they can be read.
    expect(wrapper.find('[role="dialog"] [role="alert"]').text()).toContain(
      'You cannot deactivate your own account',
    )
    expect(wrapper.find('[role="dialog"]').exists()).toBe(true)
  })

  it('offers a retry instead of rows when the list cannot be loaded', async () => {
    handler = (request) =>
      request.url === '/identity/users'
        ? { status: 500, body: errorBody('INTERNAL_ERROR', 'The user directory is unavailable') }
        : rolesReply([clerkRole])

    const wrapper = await mountPage(['identity:user:read', 'identity:role:read'])

    expect(rowsOf(wrapper)).toHaveLength(0)
    expect(wrapper.text()).toContain('The user directory is unavailable')

    const retry = buttonLabelled(wrapper.findAll('button'), 'Try again')
    expect(retry).toBeDefined()

    handler = () => usersReply([ana, bima])
    await retry?.trigger('click')
    await flushPromises()

    expect(rowsOf(wrapper)).toHaveLength(2)
    expect(wrapper.text()).not.toContain('The user directory is unavailable')
  })

  it('derives the page count and loads the next page', async () => {
    const meta: PageMeta = { page: 1, pageSize: 20, total: 45 }
    let loads = 0

    handler = (request) => {
      if (request.url !== '/identity/users') {
        return rolesReply([clerkRole])
      }

      loads += 1
      // The server echoes the effective page, and the component trusts it over
      // what it asked for, so the reply is what proves the move.
      return usersReply([ana, bima], { ...meta, page: loads === 1 ? 1 : 2 })
    }

    const wrapper = await mountPage(['identity:user:read'])

    // 45 rows at 20 a page is 3, derived here: `meta` reports only `total`.
    expect(wrapper.text()).toContain('Page 1 of 3')

    const previous = buttonLabelled(wrapper.findAll('button'), 'Previous')
    const next = buttonLabelled(wrapper.findAll('button'), 'Next')

    expect(previous?.element.disabled).toBe(true)
    expect(next?.element.disabled).toBe(false)

    await next?.trigger('click')
    await flushPromises()

    // `getPage` puts pagination in axios `params`, which the fake backend does
    // not record, so the evidence is the second request plus the page it landed on.
    expect(backend.countOf('/identity/users')).toBe(2)
    expect(wrapper.text()).toContain('Page 2 of 3')
    expect(buttonLabelled(wrapper.findAll('button'), 'Previous')?.element.disabled).toBe(false)
  })
})
