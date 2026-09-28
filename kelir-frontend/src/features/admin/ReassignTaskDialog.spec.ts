import { createPinia, setActivePinia } from 'pinia'
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import ReassignTaskDialog from './ReassignTaskDialog.vue'
import { registerSessionBridge } from '@/api/session'
import {
  errorBody,
  installFakeBackend,
  itemBody,
  type FakeBackendHandle,
  type FakeHandler,
  type RecordedRequest,
} from '@/lib/testing/fake-backend'
import type { PageMeta } from '@/types/api'
import type { OpenTaskNeedingRole, Role, User } from '@/types/identity'

const clerkRole: Role = {
  id: 'r-2',
  roleCode: 'ROLE-CLERK',
  name: 'Clerk',
  description: null,
  isSystem: false,
  permissions: [],
}

const financeRole: Role = {
  id: 'r-3',
  roleCode: 'ROLE-FINANCE',
  name: 'Finance',
  description: null,
  isSystem: false,
  permissions: [],
}

function userOf(id: string, displayName: string, status: User['status'] = 'ACTIVE'): User {
  return {
    id,
    username: displayName.toLowerCase().replace(' ', '.'),
    email: `${id}@example.com`,
    displayName,
    status,
    departmentId: null,
    mustChangePassword: false,
    lastLoginAt: null,
    createdAt: '2026-09-01T00:00:00Z',
    roles: [],
  }
}

const budi = userOf('u-9', 'Budi Santoso')
const gone = userOf('u-8', 'Gone User', 'INACTIVE')

const task: OpenTaskNeedingRole = {
  id: 't-1',
  taskRef: 'TSK-0001',
  documentNumber: 'PR-2026-0007',
  documentTitle: 'Printer paper',
  currentState: 'APPROVAL',
  status: 'CREATED',
  assigneeUserId: null,
  assigneeDisplayName: null,
  why: 'offered to the role, and unclaimed',
}

const reassigned = {
  id: 't-1',
  taskRef: 'TSK-0001',
  status: 'CREATED',
  assigneeUserId: null,
  candidateRoleCode: 'ROLE-FINANCE',
}

function listBody(data: unknown[], meta?: PageMeta): unknown {
  return { success: true, data, meta: meta ?? { page: 1, pageSize: 100, total: data.length } }
}

/** The users and roles, and `onReassign` for the reassign itself. */
function backendWith(onReassign: FakeHandler): FakeHandler {
  return (request) => {
    if (request.url === '/identity/users') {
      return { status: 200, body: listBody([budi, gone]) }
    }

    if (request.url === '/identity/roles') {
      return { status: 200, body: listBody([clerkRole, financeRole]) }
    }

    return onReassign(request)
  }
}

async function mountDialog(): Promise<VueWrapper> {
  const wrapper = mount(ReassignTaskDialog, {
    props: { open: true, task, fromRole: clerkRole },
  })
  await flushPromises()

  return wrapper
}

function optionsOf(wrapper: VueWrapper, id: string): string[] {
  return wrapper
    .findAll(`#${id} option`)
    .map((option) => option.text())
    .filter((text) => !text.startsWith('Choose'))
}

async function chooseUser(wrapper: VueWrapper, id: string): Promise<void> {
  await wrapper.find('input[type="radio"][value="user"]').setValue()
  await wrapper.find('#reassign-task-user').setValue(id)
}

async function submit(wrapper: VueWrapper): Promise<void> {
  await wrapper.find('form').trigger('submit')
  await flushPromises()
}

function reassigns(requests: RecordedRequest[]): RecordedRequest[] {
  return requests.filter((request) => request.url.endsWith('/reassign'))
}

describe('ReassignTaskDialog', () => {
  let backend: FakeBackendHandle
  let handler: FakeHandler

  beforeEach(() => {
    setActivePinia(createPinia())
    handler = backendWith(() => ({ status: 200, body: itemBody(reassigned) }))
    backend = installFakeBackend((request) => handler(request))
  })

  afterEach(() => {
    backend.restore()
    registerSessionBridge(null)
  })

  it('offers every other role, and only active users', async () => {
    const wrapper = await mountDialog()

    expect(wrapper.find('[role="dialog"]').text()).toContain('Reassign TSK-0001')
    // The role the task is moved away from would change nothing.
    expect(optionsOf(wrapper, 'reassign-task-role')).toEqual(['Finance (ROLE-FINANCE)'])

    await wrapper.find('input[type="radio"][value="user"]').setValue()

    expect(optionsOf(wrapper, 'reassign-task-user')).toEqual(['Budi Santoso (budi.santoso)'])
  })

  it('reads every page of roles, not the first hundred', async () => {
    handler = backendWith(() => ({ status: 200, body: itemBody(reassigned) }))
    const inner = handler
    handler = (request) =>
      request.url === '/identity/roles'
        ? {
            status: 200,
            body:
              request.params.page === 2
                ? listBody([financeRole], { page: 2, pageSize: 1, total: 2 })
                : listBody([clerkRole], { page: 1, pageSize: 1, total: 2 }),
          }
        : inner(request)

    const wrapper = await mountDialog()

    expect(optionsOf(wrapper, 'reassign-task-role')).toEqual(['Finance (ROLE-FINANCE)'])
  })

  it('sends a role by its code, and the comment', async () => {
    const wrapper = await mountDialog()

    await wrapper.find('#reassign-task-role').setValue('ROLE-FINANCE')
    await wrapper.find('#reassign-task-comment').setValue('  Finance decides these now  ')
    await submit(wrapper)

    const [sent] = reassigns(backend.requests)
    expect(sent.url).toBe('/workflow/tasks/t-1/reassign')
    expect(sent.method).toBe('post')
    expect(sent.body).toEqual({ roleCode: 'ROLE-FINANCE', comment: 'Finance decides these now' })
    expect(wrapper.emitted('reassigned')?.[0]).toEqual([reassigned])
    expect(wrapper.emitted('update:open')?.slice(-1)).toEqual([[false]])
  })

  it('sends a user by id and no comment when none was given', async () => {
    const wrapper = await mountDialog()

    await chooseUser(wrapper, 'u-9')
    await submit(wrapper)

    expect(reassigns(backend.requests)[0].body).toEqual({ userId: 'u-9' })
  })

  it('never names both: switching drops the other choice', async () => {
    const wrapper = await mountDialog()

    await wrapper.find('#reassign-task-role').setValue('ROLE-FINANCE')
    await chooseUser(wrapper, 'u-9')

    // And back: the user chosen in between is dropped in turn, so the role
    // has to be chosen again rather than coming back with the user beside it.
    await wrapper.find('input[type="radio"][value="role"]').setValue()
    expect((wrapper.find('#reassign-task-role').element as HTMLSelectElement).value).toBe('')

    await wrapper.find('#reassign-task-role').setValue('ROLE-FINANCE')
    await submit(wrapper)

    expect(reassigns(backend.requests).map((request) => request.body)).toEqual([
      { roleCode: 'ROLE-FINANCE' },
    ])
  })

  it('sends only the user when a role was chosen first', async () => {
    const wrapper = await mountDialog()

    await wrapper.find('#reassign-task-role').setValue('ROLE-FINANCE')
    await chooseUser(wrapper, 'u-9')
    await submit(wrapper)

    expect(reassigns(backend.requests)[0].body).toEqual({ userId: 'u-9' })
  })

  it('sends nothing until a target is chosen', async () => {
    const wrapper = await mountDialog()

    await submit(wrapper)

    expect(reassigns(backend.requests)).toHaveLength(0)
    expect(wrapper.find('#reassign-task-role-error').text()).toBe(
      'Choose the role it is offered to next',
    )
  })

  it("places TARGET_CANNOT_DECIDE under the role, in the server's words", async () => {
    const message =
      'a decision in `APPROVAL` needs ROLE:ROLE-CLERK; the role named could take none of them, ' +
      'so the task would be held by somebody who cannot decide it'
    handler = backendWith(() => ({
      status: 422,
      body: errorBody('VALIDATION_ERROR', 'Validation failed', [
        { path: 'roleCode', rule: 'canDecide', code: 'TARGET_CANNOT_DECIDE', message },
      ]),
    }))

    const wrapper = await mountDialog()

    await wrapper.find('#reassign-task-role').setValue('ROLE-FINANCE')
    await submit(wrapper)

    expect(wrapper.find('#reassign-task-role-error').text()).toBe(message)
    expect(wrapper.find('[data-testid="reassign-task-unplaced"]').exists()).toBe(false)
    expect(wrapper.emitted('reassigned')).toBeUndefined()
    // Still open, so the administrator can choose again.
    expect(wrapper.find('[role="dialog"]').exists()).toBe(true)
  })

  it('places ASSIGNMENT_UNRESOLVED under the user', async () => {
    const message =
      'no live user `u-9` in this tenant. A task is reassigned only to somebody who can act on ' +
      'it, so it stays with whoever holds it now'
    handler = backendWith(() => ({
      status: 422,
      body: errorBody('VALIDATION_ERROR', 'Validation failed', [
        { path: 'userId', rule: 'assignment', code: 'ASSIGNMENT_UNRESOLVED', message },
      ]),
    }))

    const wrapper = await mountDialog()

    await chooseUser(wrapper, 'u-9')
    await submit(wrapper)

    expect(wrapper.find('#reassign-task-user-error').text()).toBe(message)
  })

  it('lists a detail no input on screen reads', async () => {
    handler = backendWith(() => ({
      status: 422,
      body: errorBody('VALIDATION_ERROR', 'Validation failed', [
        { path: 'userId', rule: 'oneOf', code: 'ONE_TARGET_REQUIRED', message: 'name one' },
        { path: 'roleCode', rule: 'oneOf', code: 'ONE_TARGET_REQUIRED', message: 'name one' },
      ]),
    }))

    const wrapper = await mountDialog()

    await wrapper.find('#reassign-task-role').setValue('ROLE-FINANCE')
    await submit(wrapper)

    expect(wrapper.find('#reassign-task-role-error').text()).toBe('name one')
    // The user field is not drawn while a role is chosen, so its detail is listed.
    expect(wrapper.find('[data-testid="reassign-task-unplaced"]').text()).toBe('userId: name one')
  })

  it('shows a 409 as the message on the form', async () => {
    const message = 'this task is COMPLETED and no longer open; only an open task is reassigned'
    handler = backendWith(() => ({ status: 409, body: errorBody('CONFLICT', message) }))

    const wrapper = await mountDialog()

    await wrapper.find('#reassign-task-role').setValue('ROLE-FINANCE')
    await submit(wrapper)

    expect(wrapper.find('[data-testid="reassign-task-error"]').text()).toBe(message)
    expect(wrapper.find('#reassign-task-role-error').exists()).toBe(false)
    expect(wrapper.emitted('reassigned')).toBeUndefined()
  })

  it('says when the choices could not be listed', async () => {
    handler = (request) =>
      request.url === '/identity/users'
        ? { status: 403, body: errorBody('FORBIDDEN', 'Missing identity:user:read') }
        : { status: 200, body: listBody([clerkRole, financeRole]) }

    const wrapper = await mountDialog()

    expect(wrapper.text()).toContain(
      'The users and roles could not be listed: Missing identity:user:read',
    )
  })
})
