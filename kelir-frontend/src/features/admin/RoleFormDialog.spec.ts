import { createPinia, setActivePinia } from 'pinia'
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import RoleFormDialog from './RoleFormDialog.vue'
import { registerSessionBridge } from '@/api/session'
import {
  errorBody,
  installFakeBackend,
  itemBody,
  type FakeBackendHandle,
  type FakeHandler,
} from '@/lib/testing/fake-backend'
import type { Permission, Role } from '@/types/identity'

const catalogue: Permission[] = [
  {
    id: 'p-1',
    permissionCode: 'identity:user:read',
    module: 'identity',
    description: 'View users',
  },
  {
    id: 'p-2',
    permissionCode: 'identity:user:create',
    module: 'identity',
    description: null,
  },
  {
    id: 'p-3',
    permissionCode: 'document:read',
    module: 'document',
    description: 'View documents',
  },
]

const clerkRole: Role = {
  id: 'r-1',
  roleCode: 'ROLE-CLERK',
  name: 'Clerk',
  description: 'Front desk',
  isSystem: false,
  permissions: [catalogue[0]],
}

/** The tsconfig lib target predates `Array.prototype.at`, so index by hand. */
function lastRequest(backend: FakeBackendHandle) {
  return backend.requests[backend.requests.length - 1]
}

function mountDialog(role: Role | null): VueWrapper {
  return mount(RoleFormDialog, {
    props: { open: true, role, permissions: catalogue },
  })
}

describe('RoleFormDialog', () => {
  let backend: FakeBackendHandle
  let handler: FakeHandler

  beforeEach(() => {
    setActivePinia(createPinia())
    window.localStorage.clear()

    handler = () => ({ status: 201, body: itemBody(clerkRole) })
    backend = installFakeBackend((request) => handler(request))
  })

  afterEach(() => {
    backend.restore()
    registerSessionBridge(null)
  })

  it('lists the whole permission catalogue as checkboxes, grouped by module', async () => {
    const wrapper = mountDialog(null)
    await flushPromises()

    for (const permission of catalogue) {
      expect(wrapper.find(`#role-permission-${permission.id}`).exists()).toBe(true)
      expect(wrapper.text()).toContain(permission.permissionCode)
    }

    // Grouped, and the groups ordered, so the list does not reshuffle between
    // loads.
    const modules = wrapper.findAll('.uppercase').map((node) => node.text())
    expect(modules).toEqual(['document', 'identity'])
  })

  it('submits the selected permissions as permissionIds', async () => {
    // `permission_ids` would be dropped by serde in silence and the role saved
    // with no permissions, so the wire name is worth asserting.
    const wrapper = mountDialog(null)

    await wrapper.find('#role-code').setValue('ROLE-AUDITOR')
    await wrapper.find('#role-name').setValue('Auditor')
    await wrapper.find('#role-permission-p-1').setValue(true)
    await wrapper.find('#role-permission-p-3').setValue(true)
    await wrapper.find('form').trigger('submit')
    await flushPromises()

    const request = lastRequest(backend)
    expect(request?.url).toBe('/identity/roles')
    expect(request?.body).toMatchObject({
      roleCode: 'ROLE-AUDITOR',
      name: 'Auditor',
      permissionIds: ['p-1', 'p-3'],
    })
    expect(request?.body).not.toHaveProperty('permission_ids')
    expect(request?.body).not.toHaveProperty('role_code')
  })

  it('ticks the permissions a role already holds when editing', async () => {
    const wrapper = mountDialog(clerkRole)
    await flushPromises()

    expect((wrapper.find('#role-permission-p-1').element as HTMLInputElement).checked).toBe(true)
    expect((wrapper.find('#role-permission-p-3').element as HTMLInputElement).checked).toBe(false)
    expect((wrapper.find('#role-code').element as HTMLInputElement).value).toBe('ROLE-CLERK')
    // A role code is fixed once the role exists.
    expect((wrapper.find('#role-code').element as HTMLInputElement).disabled).toBe(true)
  })

  it('surfaces a denial from the backend rather than failing silently', async () => {
    // FR-IDM-005: a caller lacking `identity:role:update` must be told, not left
    // watching a dialog close over an unchanged role.
    handler = () => ({
      status: 403,
      body: errorBody('FORBIDDEN', 'You do not have permission to perform this action'),
    })

    const wrapper = mountDialog(clerkRole)
    await flushPromises()

    await wrapper.find('#role-permission-p-3').setValue(true)
    await wrapper.find('form').trigger('submit')
    await flushPromises()

    expect(wrapper.find('[role="alert"]').text()).toContain('do not have permission')
    expect(wrapper.emitted('saved')).toBeUndefined()
    expect(wrapper.emitted('update:open')).toBeUndefined()
  })

  it('shows a duplicate role code against the field that caused it', async () => {
    handler = () => ({
      status: 409,
      body: errorBody('CONFLICT', 'That role code is already in use'),
    })

    const wrapper = mountDialog(null)

    await wrapper.find('#role-code').setValue('ROLE-CLERK')
    await wrapper.find('#role-name').setValue('Clerk again')
    await wrapper.find('form').trigger('submit')
    await flushPromises()

    expect(wrapper.find('#role-code-error').text()).toBe('That role code is already in use')
    expect(wrapper.find('[role="alert"]').exists()).toBe(false)
  })

  it('does not submit a role without a code or a name', async () => {
    // The backend validates neither, so an empty code would reach the database.
    const wrapper = mountDialog(null)

    await wrapper.find('form').trigger('submit')
    await flushPromises()

    expect(backend.requests).toHaveLength(0)
    expect(wrapper.find('#role-code-error').exists()).toBe(true)
    expect(wrapper.find('#role-name-error').exists()).toBe(true)
  })

  it('omits the role code when editing, since the backend cannot change it', async () => {
    handler = () => ({ status: 200, body: itemBody(clerkRole) })

    const wrapper = mountDialog(clerkRole)
    await flushPromises()

    await wrapper.find('#role-name').setValue('Senior clerk')
    await wrapper.find('form').trigger('submit')
    await flushPromises()

    const request = lastRequest(backend)
    expect(request?.method).toBe('put')
    expect(request?.url).toBe('/identity/roles/r-1')
    expect(request?.body).not.toHaveProperty('roleCode')
    expect(request?.body).toMatchObject({ name: 'Senior clerk', permissionIds: ['p-1'] })
  })

  it('explains itself when the catalogue could not be loaded', async () => {
    const wrapper = mount(RoleFormDialog, {
      props: { open: true, role: null, permissions: [] },
    })
    await flushPromises()

    expect(wrapper.text()).toContain('permission catalogue could not be loaded')
  })
})
