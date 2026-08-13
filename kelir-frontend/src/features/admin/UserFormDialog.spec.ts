import { createPinia, setActivePinia } from 'pinia'
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import UserFormDialog from './UserFormDialog.vue'
import { getUser } from '@/api/identity'
import { registerSessionBridge } from '@/api/session'
import {
  errorBody,
  installFakeBackend,
  itemBody,
  type FakeBackendHandle,
  type FakeHandler,
} from '@/lib/testing/fake-backend'
import type { Role, User } from '@/types/identity'

const clerkRole: Role = {
  id: 'r-1',
  roleCode: 'ROLE-CLERK',
  name: 'Clerk',
  description: null,
  isSystem: false,
  permissions: [],
}

const approverRole: Role = {
  id: 'r-2',
  roleCode: 'ROLE-APPROVER',
  name: 'Approver',
  description: null,
  isSystem: false,
  permissions: [],
}

const existingUser: User = {
  id: 'u-1',
  username: 'ana',
  email: 'ana@example.com',
  displayName: 'Ana Putri',
  status: 'ACTIVE',
  departmentId: null,
  mustChangePassword: false,
  lastLoginAt: null,
  createdAt: '2026-01-01T00:00:00Z',
  roles: [clerkRole],
}

/** The tsconfig lib target predates `Array.prototype.at`, so index by hand. */
function lastRequest(backend: FakeBackendHandle) {
  return backend.requests[backend.requests.length - 1]
}

function mountDialog(user: User | null): VueWrapper {
  return mount(UserFormDialog, {
    props: { open: true, user, roles: [clerkRole, approverRole] },
  })
}

async function fillCreateForm(wrapper: VueWrapper): Promise<void> {
  await wrapper.find('#user-username').setValue('bima')
  await wrapper.find('#user-email').setValue('bima@example.com')
  await wrapper.find('#user-display-name').setValue('Bima Santoso')
  await wrapper.find('#user-password').setValue('a-long-enough-password')
}

describe('UserFormDialog', () => {
  let backend: FakeBackendHandle
  let handler: FakeHandler

  beforeEach(() => {
    setActivePinia(createPinia())
    window.localStorage.clear()

    handler = () => ({ status: 201, body: itemBody(existingUser) })
    backend = installFakeBackend((request) => handler(request))
  })

  afterEach(() => {
    backend.restore()
    registerSessionBridge(null)
  })

  it('sends the camelCase field names the backend deserialises', async () => {
    // The backend puts `#[serde(default)]` on the id arrays and does not reject
    // unknown fields, so `role_ids` would be dropped in silence and the user
    // created with no roles and no error. This test is the guard against that.
    const wrapper = mountDialog(null)

    await fillCreateForm(wrapper)
    await wrapper.find('#user-role-r-1').setValue(true)
    await wrapper.find('#user-role-r-2').setValue(true)
    await wrapper.find('form').trigger('submit')
    await flushPromises()

    const request = lastRequest(backend)
    expect(request?.method).toBe('post')
    expect(request?.url).toBe('/identity/users')
    expect(request?.body).toMatchObject({
      username: 'bima',
      email: 'bima@example.com',
      displayName: 'Bima Santoso',
      roleIds: ['r-1', 'r-2'],
    })
    expect(request?.body).not.toHaveProperty('role_ids')
    expect(request?.body).not.toHaveProperty('display_name')
  })

  it('round-trips a user created with two roles through GET /users/{id}', async () => {
    // FR-IDM-003: the grants have to survive the write, not merely be sent.
    //
    // The fake backend resolves the ids the POST actually carried and serves
    // that back, rather than a canned reply. That is the difference between
    // testing the round trip and testing the fixture: strip `roleIds` from the
    // outbound request and the fetched user returns with no roles.
    const catalogue = new Map([
      [clerkRole.id, clerkRole],
      [approverRole.id, approverRole],
    ])
    let stored: User | null = null

    handler = (request) => {
      if (request.method === 'post') {
        const body = request.body as { username?: string; roleIds?: string[] }

        stored = {
          ...existingUser,
          id: 'u-9',
          username: body.username ?? '',
          roles: (body.roleIds ?? []).flatMap((id) => {
            const role = catalogue.get(id)
            return role ? [role] : []
          }),
        }

        // The create response deliberately omits the roles, so nothing but the
        // subsequent read can satisfy the assertion below.
        return { status: 201, body: itemBody({ ...stored, roles: [] }) }
      }

      return stored === null
        ? { status: 404, body: errorBody('NOT_FOUND', 'No such user') }
        : { status: 200, body: itemBody(stored) }
    }

    const wrapper = mountDialog(null)

    await fillCreateForm(wrapper)
    await wrapper.find('#user-role-r-1').setValue(true)
    await wrapper.find('#user-role-r-2').setValue(true)
    await wrapper.find('form').trigger('submit')
    await flushPromises()

    const fetched = await getUser('u-9')

    expect(fetched.username).toBe('bima')
    expect(fetched.roles.map((role) => role.roleCode)).toEqual(['ROLE-CLERK', 'ROLE-APPROVER'])
  })

  it('shows a duplicate as a field error rather than a detached message', async () => {
    // The 409 carries no `details` and names both candidates, so it lands on
    // both inputs. A toast would leave them looking valid.
    handler = () => ({
      status: 409,
      body: errorBody('CONFLICT', 'That username or email address is already in use'),
    })

    const wrapper = mountDialog(null)

    await fillCreateForm(wrapper)
    await wrapper.find('form').trigger('submit')
    await flushPromises()

    expect(wrapper.find('#user-username-error').text()).toContain('already in use')
    expect(wrapper.find('#user-email-error').text()).toContain('already in use')
    expect(wrapper.find('[role="alert"]').exists()).toBe(false)
    expect(wrapper.emitted('saved')).toBeUndefined()
    expect(wrapper.emitted('update:open')).toBeUndefined()
  })

  it('puts a duplicate on the email alone when editing, where the username is fixed', async () => {
    handler = () => ({
      status: 409,
      body: errorBody('CONFLICT', 'That username or email address is already in use'),
    })

    const wrapper = mountDialog(existingUser)

    await wrapper.find('#user-email').setValue('taken@example.com')
    await wrapper.find('form').trigger('submit')
    await flushPromises()

    expect(wrapper.find('#user-email-error').exists()).toBe(true)
    expect(wrapper.find('#user-username-error').exists()).toBe(false)
  })

  it('binds the backend validation details to their fields by path', async () => {
    handler = () => ({
      status: 422,
      body: errorBody('VALIDATION_ERROR', 'Validation failed', [
        {
          path: 'displayName',
          rule: 'required',
          code: 'REQUIRED',
          message: 'Display name is required',
        },
      ]),
    })

    const wrapper = mountDialog(null)

    await fillCreateForm(wrapper)
    await wrapper.find('form').trigger('submit')
    await flushPromises()

    expect(wrapper.find('#user-display-name-error').text()).toBe('Display name is required')
  })

  it('surfaces a denial instead of closing over an unchanged user', async () => {
    handler = () => ({
      status: 403,
      body: errorBody('FORBIDDEN', 'You do not have permission to perform this action'),
    })

    const wrapper = mountDialog(existingUser)

    await wrapper.find('form').trigger('submit')
    await flushPromises()

    expect(wrapper.find('[role="alert"]').text()).toContain('do not have permission')
    expect(wrapper.emitted('saved')).toBeUndefined()
  })

  it('seeds the form from the user being edited, including its roles', async () => {
    const wrapper = mountDialog(existingUser)
    await flushPromises()

    expect((wrapper.find('#user-username').element as HTMLInputElement).value).toBe('ana')
    expect((wrapper.find('#user-username').element as HTMLInputElement).disabled).toBe(true)
    expect((wrapper.find('#user-role-r-1').element as HTMLInputElement).checked).toBe(true)
    expect((wrapper.find('#user-role-r-2').element as HTMLInputElement).checked).toBe(false)
    // A password is not part of an edit; it has its own endpoint.
    expect(wrapper.find('#user-password').exists()).toBe(false)
  })

  it('always sends roleIds on edit so every role can be removed', async () => {
    // The backend leaves grants untouched when the key is absent, so omitting it
    // when the list is empty would make deselecting the last role impossible.
    handler = () => ({ status: 200, body: itemBody({ ...existingUser, roles: [] }) })

    const wrapper = mountDialog(existingUser)
    await flushPromises()

    await wrapper.find('#user-role-r-1').setValue(false)
    await wrapper.find('form').trigger('submit')
    await flushPromises()

    expect(lastRequest(backend)?.body).toMatchObject({ roleIds: [] })
  })

  it('does not submit while a required field is blank', async () => {
    const wrapper = mountDialog(null)

    await wrapper.find('form').trigger('submit')
    await flushPromises()

    expect(backend.requests).toHaveLength(0)
    expect(wrapper.find('#user-username-error').exists()).toBe(true)
    expect(wrapper.find('#user-password-error').text()).toContain('12 characters')
  })

  it('emits the saved user and closes on success', async () => {
    const wrapper = mountDialog(null)

    await fillCreateForm(wrapper)
    await wrapper.find('form').trigger('submit')
    await flushPromises()

    expect(wrapper.emitted('saved')?.[0]).toEqual([existingUser])
    const openEvents = wrapper.emitted('update:open') ?? []
    expect(openEvents[openEvents.length - 1]).toEqual([false])
  })
})
