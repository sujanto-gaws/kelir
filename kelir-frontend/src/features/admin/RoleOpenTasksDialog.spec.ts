import { createPinia, setActivePinia } from 'pinia'
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { h } from 'vue'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import RoleOpenTasksDialog from './RoleOpenTasksDialog.vue'
import { registerSessionBridge } from '@/api/session'
import {
  errorBody,
  installFakeBackend,
  type FakeBackendHandle,
  type FakeHandler,
} from '@/lib/testing/fake-backend'
import type { PageMeta } from '@/types/api'
import type { OpenTaskNeedingRole, Role } from '@/types/identity'

const REFUSAL =
  '2 open tasks need this role to be decided. Deleting the role would leave them offered to ' +
  'nobody, or with a decision nobody could make. They need to be decided or reassigned first'

const clerkRole: Role = {
  id: 'r-2',
  roleCode: 'ROLE-CLERK',
  name: 'Clerk',
  description: null,
  isSystem: false,
  permissions: [],
}

const unclaimed: OpenTaskNeedingRole = {
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

const claimed: OpenTaskNeedingRole = {
  id: 't-2',
  taskRef: 'TSK-0002',
  documentNumber: null,
  documentTitle: null,
  currentState: 'REVIEW',
  status: 'ASSIGNED',
  assigneeUserId: 'u-9',
  assigneeDisplayName: 'Budi Santoso',
  why: 'a decision out of REVIEW is allowedBy the role',
}

function listBody(data: unknown[], meta?: PageMeta): unknown {
  return { success: true, data, meta: meta ?? { page: 1, pageSize: 20, total: data.length } }
}

async function mountDialog(
  options: { withActions?: boolean; role?: Role } = {},
): Promise<VueWrapper> {
  const wrapper = mount(RoleOpenTasksDialog, {
    props: { open: true, role: options.role ?? clerkRole, refusal: REFUSAL },
    slots: options.withActions
      ? {
          'task-actions': ({ task }: { task: OpenTaskNeedingRole }) =>
            h('button', { 'data-testid': 'row-action' }, `Act on ${task.taskRef} (${task.id})`),
        }
      : {},
  })
  await flushPromises()

  return wrapper
}

function rowsOf(wrapper: VueWrapper) {
  return wrapper.findAll('[data-testid="role-open-task"]')
}

function buttonNamed(wrapper: VueWrapper, name: string) {
  const button = wrapper.findAll('button').find((candidate) => candidate.text() === name)
  if (button === undefined) {
    throw new Error(`no ${name} button`)
  }

  return button
}

function isDisabled(wrapper: VueWrapper, name: string): boolean {
  return (buttonNamed(wrapper, name).element as HTMLButtonElement).disabled
}

/** The two tasks, one to a page. */
const pagedByOne: FakeHandler = (request) =>
  request.params.page === 2
    ? { status: 200, body: listBody([claimed], { page: 2, pageSize: 1, total: 2 }) }
    : { status: 200, body: listBody([unclaimed], { page: 1, pageSize: 1, total: 2 }) }

describe('RoleOpenTasksDialog', () => {
  let backend: FakeBackendHandle
  let handler: FakeHandler

  beforeEach(() => {
    setActivePinia(createPinia())
    handler = () => ({ status: 200, body: listBody([unclaimed, claimed]) })
    backend = installFakeBackend((request) => handler(request))
  })

  afterEach(() => {
    backend.restore()
    registerSessionBridge(null)
  })

  it("reads the role's open tasks and shows the refusal above them", async () => {
    const wrapper = await mountDialog()

    expect(backend.requests.map((request) => request.url)).toEqual([
      '/identity/roles/r-2/open-tasks',
    ])
    expect(wrapper.find('[role="dialog"]').text()).toContain('Open tasks need Clerk')
    expect(wrapper.find('[role="alert"]').text()).toBe(REFUSAL)
  })

  it('names each task, its document, its state, its holder and why', async () => {
    const wrapper = await mountDialog()
    const rows = rowsOf(wrapper)

    expect(rows).toHaveLength(2)

    const first = rows[0].findAll('td').map((cell) => cell.text())
    expect(first[0]).toBe('TSK-0001')
    expect(first[1]).toContain('PR-2026-0007')
    expect(first[1]).toContain('Printer paper')
    expect(first[2]).toBe('APPROVAL')
    // Unclaimed is said, not left as a blank cell.
    expect(first[3]).toBe('Nobody has claimed it')
    expect(first[4]).toBe('offered to the role, and unclaimed')

    const second = rows[1].findAll('td').map((cell) => cell.text())
    expect(second[1]).toBe('—')
    expect(second[2]).toBe('REVIEW')
    expect(second[3]).toBe('Budi Santoso')
    expect(second[4]).toBe('a decision out of REVIEW is allowedBy the role')
  })

  it("falls back to the holder's id when no name came with it", async () => {
    handler = () => ({
      status: 200,
      body: listBody([{ ...claimed, assigneeDisplayName: null }]),
    })

    const wrapper = await mountDialog()

    expect(wrapper.find('[data-testid="role-open-task-holder"]').text()).toBe('u-9')
  })

  it('draws no actions column unless a row action is given', async () => {
    const wrapper = await mountDialog()

    expect(wrapper.findAll('th').map((cell) => cell.text())).not.toContain('Actions')
    expect(rowsOf(wrapper)[0].findAll('td')).toHaveLength(5)
  })

  it('gives each row the slot a row action goes in', async () => {
    const wrapper = await mountDialog({ withActions: true })
    const actions = wrapper.findAll('[data-testid="row-action"]')

    expect(wrapper.findAll('th').map((cell) => cell.text())).toContain('Actions')
    expect(actions.map((action) => action.text())).toEqual([
      'Act on TSK-0001 (t-1)',
      'Act on TSK-0002 (t-2)',
    ])
  })

  it('pages when the tasks do not fit on one page', async () => {
    handler = pagedByOne

    const wrapper = await mountDialog()

    expect(wrapper.text()).toContain('Page 1 of 2')
    expect(wrapper.text()).toContain('2 open tasks')
    // There is no page before the first.
    expect(isDisabled(wrapper, 'Previous')).toBe(true)

    await buttonNamed(wrapper, 'Next').trigger('click')
    await flushPromises()

    expect(backend.requests[backend.requests.length - 1]?.params).toMatchObject({ page: 2 })
    expect(wrapper.text()).toContain('Page 2 of 2')
    expect(rowsOf(wrapper)[0].text()).toContain('TSK-0002')
    expect(isDisabled(wrapper, 'Previous')).toBe(false)
  })

  it('reads the first page again each time it is opened', async () => {
    handler = pagedByOne
    const wrapper = await mountDialog()

    await buttonNamed(wrapper, 'Next').trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain('Page 2 of 2')

    await wrapper.setProps({ open: false })
    await wrapper.setProps({ open: true })
    await flushPromises()

    // Left on page 2, the list would open there, on a page the next refusal
    // may not have.
    expect(backend.requests[backend.requests.length - 1]?.params).toMatchObject({ page: 1 })
    expect(wrapper.text()).toContain('Page 1 of 2')
    expect(rowsOf(wrapper)[0].text()).toContain('TSK-0001')
  })

  it('shows no paging for a single page', async () => {
    const wrapper = await mountDialog()

    const labels = wrapper.findAll('button').map((button) => button.text())
    expect(labels).not.toContain('Next')
    expect(labels).not.toContain('Previous')
  })

  it('says the delete can be tried again when the tasks were decided meanwhile', async () => {
    handler = () => ({ status: 200, body: listBody([]) })

    const wrapper = await mountDialog()

    expect(wrapper.find('[data-testid="role-open-tasks-empty"]').text()).toContain(
      'can be tried again',
    )
    expect(rowsOf(wrapper)).toHaveLength(0)
  })

  it('says why the list could not be read, and offers to read it again', async () => {
    let calls = 0
    handler = () => {
      calls += 1

      return calls === 1
        ? { status: 404, body: errorBody('NOT_FOUND', 'Role not found') }
        : { status: 200, body: listBody([unclaimed]) }
    }

    const wrapper = await mountDialog()

    expect(wrapper.text()).toContain('The open tasks could not be listed: Role not found')

    await wrapper
      .findAll('button')
      .find((button) => button.text() === 'Try again')
      ?.trigger('click')
    await flushPromises()

    expect(rowsOf(wrapper)).toHaveLength(1)
  })

  it('reads nothing while closed', async () => {
    mount(RoleOpenTasksDialog, {
      props: { open: false, role: clerkRole, refusal: REFUSAL },
    })
    await flushPromises()

    expect(backend.requests).toHaveLength(0)
  })

  describe('opened from a stranded role, with no refusal (#508)', () => {
    async function mountStranded(): Promise<VueWrapper> {
      const wrapper = mount(RoleOpenTasksDialog, { props: { open: true, role: clerkRole } })
      await flushPromises()

      return wrapper
    }

    it('says nobody holds the role in place of a refusal, and lists the tasks', async () => {
      const wrapper = await mountStranded()

      expect(wrapper.find('[role="alert"]').exists()).toBe(false)
      expect(wrapper.find('[data-testid="role-open-tasks-stranded"]').text()).toContain(
        'Nobody holds this role any more',
      )
      expect(rowsOf(wrapper)).toHaveLength(2)
    })

    it('does not speak of a delete to retry once the list is empty', async () => {
      handler = () => ({ status: 200, body: listBody([]) })

      const wrapper = await mountStranded()

      expect(wrapper.find('[data-testid="role-open-tasks-empty"]').text()).toBe(
        'No open task needs this role any more.',
      )
    })
  })
})
