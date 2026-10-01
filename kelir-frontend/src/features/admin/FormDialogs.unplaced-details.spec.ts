import { createPinia, setActivePinia } from 'pinia'
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import DelegationFormDialog from './DelegationFormDialog.vue'
import RoleFormDialog from './RoleFormDialog.vue'
import TenantFormDialog from './TenantFormDialog.vue'
import UserFormDialog from './UserFormDialog.vue'
import { registerSessionBridge } from '@/api/session'
import {
  errorBody,
  installFakeBackend,
  itemBody,
  validationReply,
  type FakeBackendHandle,
  type FakeReply,
} from '@/lib/testing/fake-backend'
import { searchedPage } from '@/lib/testing/searched-page'
import { useAuthStore } from '@/stores/auth'
import type { Permission, Role, User } from '@/types/identity'
import type { Tenant } from '@/types/organization'

/**
 * The independent campaign on #576: every 422 detail is seen once, whatever
 * its path, in every mode the four admin dialogs are drawn in.
 *
 * **Nothing here reads a dialog's `placed` list.** Each harness names the paths
 * its *template* puts a message under and the element that message lands in,
 * read off the markup; every other path is expected in the list. So a `placed`
 * that goes stale against its template fails here in either direction: a detail
 * shown under its input and listed too, or a detail shown nowhere at all.
 */

const catalogue: Permission[] = [
  { id: 'p-1', permissionCode: 'identity:user:read', module: 'identity', description: null },
  { id: 'p-2', permissionCode: 'identity:user:create', module: 'identity', description: null },
]

const clerkRole: Role = {
  id: 'r-1',
  roleCode: 'ROLE-CLERK',
  name: 'Clerk',
  description: 'Front desk',
  isSystem: false,
  permissions: [catalogue[0]],
}

const ana: User = {
  id: 'u-ani',
  username: 'ani',
  email: 'ani@example.com',
  displayName: 'Ani Wijaya',
  status: 'ACTIVE',
  departmentId: null,
  mustChangePassword: false,
  lastLoginAt: null,
  createdAt: '2026-01-01T00:00:00Z',
  roles: [clerkRole],
}

const budi: User = {
  ...ana,
  id: 'u-budi',
  username: 'budi',
  email: 'budi@example.com',
  displayName: 'Budi Santoso',
  roles: [],
}

const acme: Tenant = {
  id: 't-2',
  tenantCode: 'TNT-001',
  name: 'Acme Limited',
  status: 'ACTIVE',
  isDefault: false,
  userCount: 12,
  createdAt: '2026-08-20T00:00:00Z',
}

const travel = { id: 't-travel', typeCode: 'TRAVEL', name: 'Travel request', status: 'ACTIVE' }

/** What `datetime-local` produces, `days` ahead: no zone, minute precision. */
function soon(days: number): string {
  return new Date(Date.now() + days * 86_400_000).toISOString().slice(0, 16)
}

async function fill(wrapper: VueWrapper, values: Record<string, string>): Promise<void> {
  for (const [id, value] of Object.entries(values)) {
    await wrapper.find(`#${id}`).setValue(value)
  }
}

interface Harness {
  /** The dialog and the mode it is drawn in. */
  name: string
  /** The test id the dialog gives its list. */
  testId: string
  /** Mounted open, and filled so that a submit reaches the server. */
  open(): Promise<VueWrapper>
  /** The id of a required input: blanked, it stops the next submit locally. */
  required: string
  /** Path to the id of the element the template shows that path's message in. */
  slots: Record<string, string>
  /** Paths the template shows no message for, in this mode. */
  elsewhere: string[]
  /** What a save that works answers. */
  saved: unknown
}

async function openUser(user: User | null): Promise<VueWrapper> {
  const wrapper = mount(UserFormDialog, { props: { open: true, user, rolesReadable: true } })
  await flushPromises()

  if (user === null) {
    await fill(wrapper, {
      'user-username': 'bima',
      'user-email': 'bima@example.com',
      'user-display-name': 'Bima Santoso',
      'user-password': 'a-long-enough-password',
    })
  }

  return wrapper
}

async function openTenant(tenant: Tenant | null): Promise<VueWrapper> {
  const wrapper = mount(TenantFormDialog, { props: { open: true, tenant } })
  await flushPromises()

  if (tenant === null) {
    await fill(wrapper, {
      'tenant-code': 'TNT-002',
      'tenant-name': 'Borneo Timber',
      'tenant-admin-username': 'borneo.admin',
      'tenant-admin-email': 'admin@borneo.example',
      'tenant-admin-display-name': 'Borneo Administrator',
      'tenant-admin-password': 'a-sufficiently-long-password',
    })
  }

  return wrapper
}

async function openRole(role: Role | null): Promise<VueWrapper> {
  const wrapper = mount(RoleFormDialog, { props: { open: true, role, permissions: catalogue } })
  await flushPromises()

  if (role === null) {
    await fill(wrapper, { 'role-code': 'ROLE-BUYER', 'role-name': 'Buyer' })
  }

  return wrapper
}

async function openDelegation(narrowed: boolean): Promise<VueWrapper> {
  useAuthStore().user = {
    id: ana.id,
    username: ana.username,
    displayName: ana.displayName,
    email: ana.email,
    roles: ['APPROVER'],
    permissions: ['identity:delegation:create', 'identity:user:read'],
  }

  const wrapper = mount(DelegationFormDialog, { props: { open: true } })
  await flushPromises()

  await fill(wrapper, {
    'delegation-delegate': budi.id,
    'delegation-starts': soon(1),
    'delegation-ends': soon(8),
  })

  if (narrowed) {
    await fill(wrapper, { 'delegation-scope': 'DOCUMENT_TYPE' })
    await flushPromises()
    await fill(wrapper, { 'delegation-type': travel.id })
  }

  return wrapper
}

const USER_SLOTS = {
  username: 'user-username-error',
  email: 'user-email-error',
  displayName: 'user-display-name-error',
}

/** Shapes a serde path takes (`roleIds[0]`), a domain one (`roleIds.0`), a casing slip, a nesting. */
const USER_ELSEWHERE = [
  'departmentId',
  'status',
  'roleIds',
  'roleIds[0]',
  'roleIds.0',
  'Email',
  'user.email',
  'email ',
  'body',
  'colour',
]

const TENANT_ADMINISTRATOR = [
  'administrator.username',
  'administrator.email',
  'administrator.displayName',
  'administrator.password',
]

/** The administrator's fields without their prefix are not this form's paths. */
const TENANT_ELSEWHERE = [
  'status',
  'administrator',
  'administrator.locale',
  'username',
  'email',
  'Name',
  'colour',
]

const ROLE_SLOTS = { roleCode: 'role-code-error', name: 'role-name-error' }

const ROLE_ELSEWHERE = [
  'description',
  'permissionIds',
  'permissionIds.1',
  'permissionIds[1]',
  'RoleCode',
  'role.name',
  'colour',
]

const DELEGATION_SLOTS = {
  delegateUserId: 'delegation-delegate-error',
  startsAt: 'delegation-starts-error',
  endsAt: 'delegation-ends-error',
}

const DELEGATION_ELSEWHERE = ['scope', 'reason', 'delegatorUserId', 'StartsAt', 'colour']

const harnesses: Harness[] = [
  {
    name: 'UserFormDialog, creating',
    testId: 'user-unplaced-errors',
    open: () => openUser(null),
    required: 'user-email',
    slots: { ...USER_SLOTS, password: 'user-password-error' },
    elsewhere: USER_ELSEWHERE,
    saved: ana,
  },
  {
    name: 'UserFormDialog, editing',
    testId: 'user-unplaced-errors',
    open: () => openUser(ana),
    required: 'user-email',
    slots: USER_SLOTS,
    elsewhere: ['password', ...USER_ELSEWHERE],
    saved: ana,
  },
  {
    name: 'TenantFormDialog, creating',
    testId: 'tenant-unplaced-errors',
    open: () => openTenant(null),
    required: 'tenant-name',
    slots: {
      tenantCode: 'tenant-code-error',
      name: 'tenant-name-error',
      'administrator.username': 'tenant-admin-username-error',
      'administrator.email': 'tenant-admin-email-error',
      'administrator.displayName': 'tenant-admin-display-name-error',
      'administrator.password': 'tenant-admin-password-error',
    },
    elsewhere: TENANT_ELSEWHERE,
    saved: acme,
  },
  {
    name: 'TenantFormDialog, editing',
    testId: 'tenant-unplaced-errors',
    open: () => openTenant(acme),
    required: 'tenant-name',
    slots: { tenantCode: 'tenant-code-error', name: 'tenant-name-error' },
    elsewhere: [...TENANT_ADMINISTRATOR, ...TENANT_ELSEWHERE],
    saved: acme,
  },
  {
    name: 'RoleFormDialog, creating',
    testId: 'role-unplaced-errors',
    open: () => openRole(null),
    required: 'role-name',
    slots: ROLE_SLOTS,
    elsewhere: ROLE_ELSEWHERE,
    saved: clerkRole,
  },
  {
    name: 'RoleFormDialog, editing',
    testId: 'role-unplaced-errors',
    open: () => openRole(clerkRole),
    required: 'role-name',
    slots: ROLE_SLOTS,
    elsewhere: ROLE_ELSEWHERE,
    saved: clerkRole,
  },
  {
    name: 'DelegationFormDialog, covering everything',
    testId: 'delegation-unplaced-errors',
    open: () => openDelegation(false),
    required: 'delegation-ends',
    slots: DELEGATION_SLOTS,
    elsewhere: ['documentTypeId', ...DELEGATION_ELSEWHERE],
    saved: { id: 'd-1' },
  },
  {
    name: 'DelegationFormDialog, narrowed to one type',
    testId: 'delegation-unplaced-errors',
    open: () => openDelegation(true),
    required: 'delegation-ends',
    slots: { ...DELEGATION_SLOTS, documentTypeId: 'delegation-type-error' },
    elsewhere: DELEGATION_ELSEWHERE,
    saved: { id: 'd-1' },
  },
]

describe('a 422 detail in the admin dialogs: the independent campaign (#576)', () => {
  let backend: FakeBackendHandle
  /** What the next save is answered with. Every read is answered as the endpoint would. */
  let saveReply: FakeReply

  beforeEach(() => {
    setActivePinia(createPinia())
    window.localStorage.clear()

    saveReply = { status: 200, body: itemBody({}) }
    backend = installFakeBackend((request) => {
      if (request.method !== 'get') {
        return saveReply
      }

      if (request.url === '/identity/roles') {
        return searchedPage(request, [clerkRole], ['roleCode', 'name'])
      }

      if (request.url === '/document-types') {
        return searchedPage(request, [travel], ['typeCode', 'name'])
      }

      return searchedPage(request, [ana, budi], ['username', 'email', 'displayName'])
    })
  })

  afterEach(() => {
    backend.restore()
    registerSessionBridge(null)
  })

  function saves(): number {
    return backend.requests.filter((request) => request.method !== 'get').length
  }

  async function submit(wrapper: VueWrapper): Promise<void> {
    await wrapper.find('form').trigger('submit')
    await flushPromises()
  }

  function list(wrapper: VueWrapper, harness: Harness) {
    return wrapper.find(`[data-testid="${harness.testId}"]`)
  }

  function listed(wrapper: VueWrapper, harness: Harness): string[] {
    return list(wrapper, harness)
      .findAll('li')
      .map((item) => item.text())
  }

  function alerts(wrapper: VueWrapper): string[] {
    return wrapper.findAll('[role="alert"]').map((alert) => alert.text())
  }

  /** How many times `message` is on screen anywhere in the dialog. */
  function shown(wrapper: VueWrapper, message: string): number {
    return wrapper.text().split(message).length - 1
  }

  /** Two digits, so no message is the start of another. */
  function refusal(index: number): string {
    return `refusal-${String(index).padStart(2, '0')}.`
  }

  describe.each(harnesses)('$name', (harness) => {
    it('shows every detail exactly once: under its input, or in the list', async () => {
      const placed = Object.keys(harness.slots)
      const paths = [...placed, ...harness.elsewhere]

      saveReply = validationReply(
        ...paths.map((path, index): [string, string] => [path, refusal(index)]),
      )
      const wrapper = await harness.open()

      await submit(wrapper)

      expect(saves()).toBe(1)

      // Once each, wherever it is: not dropped, and not said twice.
      expect(paths.filter((path, index) => shown(wrapper, refusal(index)) !== 1)).toEqual([])

      for (const [path, id] of Object.entries(harness.slots)) {
        expect(wrapper.find(`#${id}`).text()).toBe(refusal(paths.indexOf(path)))
      }

      // In the order the server sent them, each named by its path as sent.
      expect(listed(wrapper, harness)).toEqual(
        harness.elsewhere.map((path) => `${path}: ${refusal(paths.indexOf(path))}`),
      )
      expect(list(wrapper, harness).attributes('role')).toBe('alert')
      expect(wrapper.emitted('saved')).toBeFalsy()
    })

    it('drops the list when the dialog is closed and opened again', async () => {
      saveReply = validationReply(['colour', 'Unknown field'])
      const wrapper = await harness.open()

      await submit(wrapper)
      expect(listed(wrapper, harness)).toEqual(['colour: Unknown field'])

      await wrapper.setProps({ open: false })
      await wrapper.setProps({ open: true })
      await flushPromises()

      expect(list(wrapper, harness).exists()).toBe(false)
      expect(alerts(wrapper)).toEqual([])
    })

    it('drops the list when the next submit is stopped by a blank field', async () => {
      // Nothing is sent, so nothing is answered: the list would otherwise stay
      // from a refusal the form has since moved on from.
      saveReply = validationReply(['colour', 'Unknown field'])
      const wrapper = await harness.open()

      await submit(wrapper)
      expect(listed(wrapper, harness)).toEqual(['colour: Unknown field'])

      await fill(wrapper, { [harness.required]: '' })
      await submit(wrapper)

      expect(saves()).toBe(1)
      expect(list(wrapper, harness).exists()).toBe(false)
    })

    it('replaces the list with the next refusal, and never adds to it', async () => {
      saveReply = validationReply(['colour', 'Unknown field'], ['size', 'Unknown field'])
      const wrapper = await harness.open()

      await submit(wrapper)
      expect(listed(wrapper, harness)).toHaveLength(2)

      // A denial is the form's one message; the list of the refusal before it goes.
      saveReply = { status: 403, body: errorBody('FORBIDDEN', 'Access denied') }
      await submit(wrapper)

      expect(alerts(wrapper)).toEqual(['Access denied'])
      expect(list(wrapper, harness).exists()).toBe(false)

      saveReply = validationReply(['weight', 'Unknown field'])
      await submit(wrapper)

      expect(alerts(wrapper)).toEqual(['weight: Unknown field'])
    })

    it('saves once the refusal is corrected, with no list left behind', async () => {
      saveReply = validationReply(['colour', 'Unknown field'])
      const wrapper = await harness.open()

      await submit(wrapper)
      expect(listed(wrapper, harness)).toEqual(['colour: Unknown field'])

      saveReply = { status: 200, body: itemBody(harness.saved) }
      await submit(wrapper)

      expect(wrapper.emitted('saved')).toHaveLength(1)
      const opens = wrapper.emitted('update:open') ?? []
      expect(opens[opens.length - 1]).toEqual([false])

      // The page closes it and opens it for the next record.
      await wrapper.setProps({ open: false })
      await wrapper.setProps({ open: true })
      await flushPromises()

      expect(alerts(wrapper)).toEqual([])
    })

    it('renders a path and a message as text, never as markup', async () => {
      // An unknown field's path is the caller's own key, echoed back.
      saveReply = validationReply([
        'colour<b>bold</b>',
        '<img src="x" onerror="window.pwned = true"> is not allowed',
      ])
      const wrapper = await harness.open()

      await submit(wrapper)

      expect(listed(wrapper, harness)).toEqual([
        'colour<b>bold</b>: <img src="x" onerror="window.pwned = true"> is not allowed',
      ])
      expect(list(wrapper, harness).find('img').exists()).toBe(false)
      expect(list(wrapper, harness).find('b').exists()).toBe(false)
    })
  })

  describe('details of an unusual shape, in the user dialog', () => {
    const user = harnesses[0]

    it('shows the first of two details for one path, placed or not', async () => {
      // `ApiError.fieldErrors` keeps one reason per path. The second is not
      // shown; what matters is that the path is never left with none.
      saveReply = validationReply(
        ['email', 'Email is not valid'],
        ['email', 'Email is too long'],
        ['departmentId', 'Department must be a UUID'],
        ['departmentId', 'No such department'],
      )
      const wrapper = await user.open()

      await submit(wrapper)

      expect(wrapper.find('#user-email-error').text()).toBe('Email is not valid')
      expect(listed(wrapper, user)).toEqual(['departmentId: Department must be a UUID'])
    })

    it('lists a detail whose message is empty, by its path', async () => {
      saveReply = validationReply(['colour', ''])
      const wrapper = await user.open()

      await submit(wrapper)

      expect(listed(wrapper, user)).toEqual(['colour:'])
    })

    it('lists a detail whose path is empty, by its message', async () => {
      saveReply = validationReply(['', 'The body is not an object'])
      const wrapper = await user.open()

      await submit(wrapper)

      expect(listed(wrapper, user)).toEqual([': The body is not an object'])
    })
  })

  describe('the delegation scope, changed while a refusal is on screen', () => {
    const narrowed = harnesses[7]

    it('moves the document type detail to the list, and back under the chooser', async () => {
      saveReply = validationReply(['documentTypeId', 'No such document type'])
      const wrapper = await narrowed.open()

      await submit(wrapper)

      expect(wrapper.find('#delegation-type-error').text()).toBe('No such document type')
      expect(list(wrapper, narrowed).exists()).toBe(false)

      // The chooser goes, and its message would go with it.
      await fill(wrapper, { 'delegation-scope': 'ALL' })

      expect(wrapper.find('#delegation-type-error').exists()).toBe(false)
      expect(listed(wrapper, narrowed)).toEqual(['documentTypeId: No such document type'])

      await fill(wrapper, { 'delegation-scope': 'DOCUMENT_TYPE' })
      await flushPromises()

      expect(wrapper.find('#delegation-type-error').text()).toBe('No such document type')
      expect(list(wrapper, narrowed).exists()).toBe(false)
    })
  })
})
