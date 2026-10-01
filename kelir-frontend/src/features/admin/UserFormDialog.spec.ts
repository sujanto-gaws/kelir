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
  validationReply,
  type FakeBackendHandle,
  type FakeHandler,
} from '@/lib/testing/fake-backend'
import { filler, searchedPage } from '@/lib/testing/searched-page'
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

/** The role catalogue `GET /identity/roles` searches, in role-code order. */
let catalogue: Role[] = []

/** The tsconfig lib target predates `Array.prototype.at`, so index by hand. */
function lastRequest(backend: FakeBackendHandle) {
  return backend.requests[backend.requests.length - 1]
}

function mountDialog(user: User | null): VueWrapper {
  return mount(UserFormDialog, {
    props: { open: true, user, rolesReadable: true },
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
    catalogue = [clerkRole, approverRole]
    // The picker's reads go to the catalogue whatever a test does with the rest.
    backend = installFakeBackend((request) =>
      request.method === 'get' && request.url === '/identity/roles'
        ? searchedPage(request, catalogue, ['roleCode', 'name'])
        : handler(request),
    )
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

    // The role picker's own read is the only request: nothing was sent.
    expect(backend.requests.filter((request) => request.method !== 'get')).toHaveLength(0)
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

  it('grants, by search, a role that sorts past the first hundred', async () => {
    // #525: the page read one page of a hundred roles and handed it in, so a
    // role that sorted after it could not be granted at all.
    const wanted: Role = { ...approverRole, id: 'r-zz', roleCode: 'ZZ-AUDITOR', name: 'Auditor' }
    catalogue = [
      ...filler(120, (n) => ({
        ...clerkRole,
        id: `r-${n}`,
        roleCode: `AA-${n}`,
        name: `Filler ${n}`,
      })),
      wanted,
    ]

    const wrapper = mountDialog(existingUser)
    await flushPromises()

    expect(wrapper.find('#user-role-r-zz').exists()).toBe(false)
    expect(wrapper.get('[data-testid="user-roles-status"]').text()).toContain('Showing 100 of 121')
    // Held already and outside the first page: still shown, still ticked.
    expect((wrapper.get('#user-role-r-1').element as HTMLInputElement).checked).toBe(true)

    await wrapper.get('[data-testid="user-roles-search"]').setValue('auditor')
    await wrapper.get('[data-testid="user-roles-search"]').trigger('keydown', { key: 'Enter' })
    await flushPromises()

    const reads = backend.requests.filter((request) => request.url === '/identity/roles')
    expect(reads[reads.length - 1].params).toEqual({ search: 'auditor', pageSize: 100 })

    await wrapper.get('#user-role-r-zz').setValue(true)
    handler = () => ({ status: 200, body: itemBody(existingUser) })
    await wrapper.find('form').trigger('submit')
    await flushPromises()

    expect(lastRequest(backend)?.body).toMatchObject({ roleIds: ['r-1', 'r-zz'] })
  })

  it('offers no role picker to a caller who cannot read roles, and keeps the grants', async () => {
    const wrapper = mount(UserFormDialog, {
      props: { open: true, user: existingUser, rolesReadable: false },
    })
    await flushPromises()

    expect(backend.countOf('/identity/roles')).toBe(0)
    expect(wrapper.find('[data-testid="user-roles-unavailable"]').exists()).toBe(true)

    handler = () => ({ status: 200, body: itemBody(existingUser) })
    await wrapper.find('form').trigger('submit')
    await flushPromises()

    expect(lastRequest(backend)?.body).toMatchObject({ roleIds: ['r-1'] })
  })

  describe('a 422 detail no input reads (#576)', () => {
    const UNPLACED = '[data-testid="user-unplaced-errors"]'

    function listed(wrapper: VueWrapper): string[] {
      return wrapper
        .find(UNPLACED)
        .findAll('li')
        .map((item) => item.text())
    }

    async function submit(wrapper: VueWrapper): Promise<void> {
      await wrapper.find('form').trigger('submit')
      await flushPromises()
    }

    it('lists it on the form, and announces it', async () => {
      // `departmentId` has an input but nowhere under it for a message, and
      // `colour` is no field of this form at all. Either way the save was
      // refused, and before #576 nothing on the form said why.
      handler = () =>
        validationReply(
          ['displayName', 'Display name is too long'],
          ['departmentId', 'Department must be a UUID'],
          ['colour', 'Unknown field'],
        )
      const wrapper = mountDialog(null)

      await fillCreateForm(wrapper)
      await submit(wrapper)

      expect(listed(wrapper)).toEqual([
        'departmentId: Department must be a UUID',
        'colour: Unknown field',
      ])
      expect(wrapper.find(UNPLACED).attributes('role')).toBe('alert')
      // The detail an input reads is under that input, and only there.
      expect(wrapper.find('#user-display-name-error').text()).toBe('Display name is too long')
      expect(wrapper.emitted('saved')).toBeUndefined()
    })

    it('lists a password detail when editing, where no password is asked for', async () => {
      handler = () => validationReply(['password', 'Password is too short'])
      const wrapper = mountDialog(existingUser)

      await submit(wrapper)

      expect(wrapper.find('#user-password-error').exists()).toBe(false)
      expect(listed(wrapper)).toEqual(['password: Password is too short'])
    })

    it('does not list a detail an input shows', async () => {
      handler = () =>
        validationReply(['username', 'Username is taken'], ['password', 'Password is too short'])
      const wrapper = mountDialog(null)

      await fillCreateForm(wrapper)
      await submit(wrapper)

      expect(wrapper.find('#user-username-error').text()).toBe('Username is taken')
      expect(wrapper.find('#user-password-error').text()).toBe('Password is too short')
      expect(wrapper.find(UNPLACED).exists()).toBe(false)
    })

    it('keeps a 422 with no details, and a denial, as the one message on the form', async () => {
      const wrapper = mountDialog(existingUser)

      handler = () => validationReply()
      await submit(wrapper)

      expect(wrapper.findAll('[role="alert"]').map((alert) => alert.text())).toEqual([
        'Validation failed',
      ])
      expect(wrapper.find(UNPLACED).exists()).toBe(false)

      handler = () => ({ status: 403, body: errorBody('FORBIDDEN', 'Access denied') })
      await submit(wrapper)

      expect(wrapper.findAll('[role="alert"]').map((alert) => alert.text())).toEqual([
        'Access denied',
      ])
      expect(wrapper.find(UNPLACED).exists()).toBe(false)
    })
  })
})
