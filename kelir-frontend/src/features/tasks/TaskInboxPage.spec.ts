import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'

import TaskInboxPage from './TaskInboxPage.vue'
import {
  installFakeBackend,
  type FakeBackendHandle,
  type RecordedRequest,
} from '@/lib/testing/fake-backend'

/**
 * The task inbox (#179).
 *
 * **What is asserted here is the client's half and only the client's half.**
 * The visibility rule — *mine, or offered to a role I hold* — is a predicate in
 * the backend's query, and `kelir-backend/tests/task_inbox.rs` is what holds it.
 * A spec that asserted "the inbox does not show another user's tasks" against a
 * fake backend would be asserting that a stub returned what it was told to,
 * which is the failure coding standard §2.9 is about in the other language.
 *
 * What the client is responsible for is the part the backend cannot see: that
 * the scope goes **on the wire** rather than narrowing a fetched page, and that
 * mine and unclaimed are rendered as the different situations they are.
 */

const blank = { template: '<div />' }

function task(overrides: Record<string, unknown> = {}): unknown {
  return {
    id: '0199a1a0-0000-7000-8000-0000000000a1',
    taskRef: 'TASK-2026-000001',
    taskName: 'Approve the request',
    taskType: 'APPROVAL_TASK',
    status: 'CREATED',
    priority: 'NORMAL',
    dueAt: null,
    isOverdue: false,
    assignment: 'ROLE',
    candidateRoleCode: 'FINANCE_APPROVER',
    workflowInstanceId: '0199a1a0-0000-7000-8000-0000000000b1',
    workflowName: 'Standard approval',
    currentState: 'MANAGER_APPROVAL',
    documentId: '0199a1a0-0000-7000-8000-0000000000d1',
    documentRef: 'DOC-2026-000001',
    documentNumber: 'PR-2026-000001',
    documentTitle: 'Two standing desks',
    createdAt: '2026-08-28T00:00:00Z',
    action: null,
    decisionComment: null,
    completedAt: null,
    ...overrides,
  }
}

/**
 * The most recent request the fake backend saw.
 *
 * `Array.prototype.at` is ES2022 and this project's `lib` target predates it,
 * which is why this is indexing rather than the obvious call — the same
 * workaround `DocumentListPage.spec.ts` carries.
 */
function lastRequest(backend: FakeBackendHandle): RecordedRequest {
  return backend.requests[backend.requests.length - 1]
}

/**
 * # Seen to fail (coding standard §2.9), for #256's additions
 *
 * Two mutations, run 2026-09-02:
 *
 * - **`completed` dropped from `scopeOptions`.** Red: *offers the whole axis on
 *   one control…*, *searches within the scope being shown…* — a point the API
 *   serves and the screen does not offer is exactly what the first of those
 *   exists to catch, and it caught it twice.
 * - **The decision hidden, so a finished row renders its status instead.** Red:
 *   *shows the decision and its reason on a task that has been decided*.
 */
describe('TaskInboxPage', () => {
  let backend: FakeBackendHandle
  let router: Router
  let rows: unknown[]

  beforeEach(() => {
    setActivePinia(createPinia())
    rows = [task()]

    backend = installFakeBackend(() => ({
      status: 200,
      body: { success: true, data: rows, meta: { page: 1, pageSize: 20, total: rows.length } },
    }))

    router = createRouter({
      history: createMemoryHistory(),
      routes: [
        { path: '/tasks', name: 'tasks', component: blank },
        { path: '/tasks/:id', name: 'task', component: blank },
      ],
    })
  })

  afterEach(() => {
    backend.restore()
  })

  async function render(query: Record<string, string> = {}): Promise<VueWrapper> {
    await router.push({ name: 'tasks', query })
    await router.isReady()

    const wrapper = mount(TaskInboxPage, { global: { plugins: [router] } })
    await flushPromises()

    return wrapper
  }

  it('lists what is waiting, and names the document each task is about', async () => {
    const wrapper = await render()

    const row = wrapper.get('[data-testid="task-row-TASK-2026-000001"]')

    expect(row.text()).toContain('Approve the request')
    // A person cannot act on "task 7 of instance 9". The document's number and
    // title are what make a row actionable at a glance.
    expect(row.text()).toContain('PR-2026-000001')
    expect(row.text()).toContain('Two standing desks')
  })

  it('shows an unclaimed task and one of my own as different things', async () => {
    // #179 AC1. Not derived from a null assignee here — the backend answers it,
    // and two places deriving it would derive it differently.
    rows = [
      task(),
      task({
        id: '0199a1a0-0000-7000-8000-0000000000a2',
        taskRef: 'TASK-2026-000002',
        assignment: 'MINE',
        status: 'ASSIGNED',
      }),
    ]

    const wrapper = await render()

    const unclaimed = wrapper.get('[data-testid="task-row-TASK-2026-000001"]')
    const mine = wrapper.get('[data-testid="task-row-TASK-2026-000002"]')

    expect(unclaimed.get('[data-testid="assignment-role"]').text()).toContain('Unclaimed')
    // The role it came out of, because somebody holding three roles needs to
    // know which queue this is.
    expect(unclaimed.get('[data-testid="assignment-role"]').text()).toContain('FINANCE_APPROVER')
    expect(mine.get('[data-testid="assignment-mine"]').text()).toBe('Mine')
  })

  it('sends the scope to the server rather than narrowing a fetched page', async () => {
    // The claim the URL cannot check: an inbox that fetched everything and
    // filtered locally would hit the same path with no parameters on it.
    const wrapper = await render()

    await wrapper.get('[data-testid="tasks-scope"]').setValue('all')
    await flushPromises()

    expect(lastRequest(backend).params.scope).toBe('all')
  })

  it('does not send a scope it was never given', async () => {
    // An absent parameter is what "the default" means; sending `scope=` would
    // be a value the backend has to have an opinion about.
    await render()

    expect(lastRequest(backend).params.scope).toBeUndefined()
  })

  it('says plainly when nothing is waiting', async () => {
    rows = []

    const wrapper = await render()

    expect(wrapper.get('[data-testid="tasks-empty"]').text()).toContain('Nothing is waiting')
  })

  // -------------------------------------------------------------------------
  // Due dates and the overdue indicator (FR-TASK-007, #185)
  // -------------------------------------------------------------------------

  it('marks a late task from the server\u2019s answer, not from its own clock', async () => {
    // #185 AC4. `dueAt` here is in the *future*, and `isOverdue` says true. A
    // screen that compared the date to `Date.now()` would disagree with the
    // server — which is exactly the second opinion the flag exists to prevent,
    // and this fixture is built so only the flag can produce a pass.
    rows = [task({ dueAt: '2099-01-01T00:00:00Z', isOverdue: true })]

    const wrapper = await render()

    expect(wrapper.find('[data-testid="task-overdue"]').exists()).toBe(true)
  })

  it('leaves a task that is merely due alone', async () => {
    // And the mirror of it: a date in the *past* with `isOverdue` false, which
    // is what a task decided after its deadline looks like. A screen deriving
    // lateness would mark this one.
    rows = [task({ dueAt: '2020-01-01T00:00:00Z', isOverdue: false })]

    const wrapper = await render()

    expect(wrapper.find('[data-testid="task-overdue"]').exists()).toBe(false)
    expect(wrapper.get('[data-testid="task-row-TASK-2026-000001"]').text()).toContain('2020')
  })

  it('shows a task with no deadline as having none, rather than as late', async () => {
    rows = [task({ dueAt: null, isOverdue: false })]

    const wrapper = await render()

    expect(wrapper.find('[data-testid="task-overdue"]').exists()).toBe(false)
    expect(wrapper.get('[data-testid="task-row-TASK-2026-000001"]').text()).toContain('\u2014')
  })

  it('offers the whole axis on one control and sends the point chosen', async () => {
    // One axis, four points: `overdue \u2283 open`, `completed`, both inside `all`.
    // The control is one Select, because the choice is one choice \u2014 the list
    // grew with #185 and again with #256, and this assertion is what makes a
    // point added to the API and forgotten on the screen visible.
    const wrapper = await render()

    const values = wrapper
      .get('[data-testid="tasks-scope"]')
      .findAll('option')
      .map((option) => (option.element as HTMLOptionElement).value)

    expect(values).toEqual(['open', 'overdue', 'completed', 'all'])

    await wrapper.get('[data-testid="tasks-scope"]').setValue('overdue')
    await flushPromises()

    expect(lastRequest(backend).params.scope).toBe('overdue')

    await wrapper.get('[data-testid="tasks-scope"]').setValue('completed')
    await flushPromises()

    expect(lastRequest(backend).params.scope).toBe('completed')
  })

  /**
   * **A decided task says what was decided and why** (#256 AC5).
   *
   * The reason is FR-TASK-006's record \u2014 the immutable one taken with the
   * approval \u2014 and until now it was readable only on the document's history.
   */
  it('shows the decision and its reason on a task that has been decided', async () => {
    rows = [
      task({
        status: 'COMPLETED',
        action: 'APPROVE',
        decisionComment: 'budget is available',
        completedAt: '2026-09-02T10:00:00Z',
      }),
    ]

    const wrapper = await render()

    expect(wrapper.get('[data-testid="task-decision"]').text()).toBe('APPROVE')
    expect(wrapper.get('[data-testid="task-decision-comment"]').text()).toBe('budget is available')
  })

  it('shows the status rather than a blank decision on a task still waiting', async () => {
    const wrapper = await render()

    expect(wrapper.find('[data-testid="task-decision"]').exists()).toBe(false)
  })

  /**
   * **The search is a second control on the same query, not a second view**
   * (#256 AC3): searching inside "Decided by me" stays inside it.
   *
   * It sends on submit rather than on every keystroke \u2014 a list that refetched
   * per character would race its own answers.
   */
  it('searches within the scope being shown, on submit', async () => {
    const wrapper = await render()

    await wrapper.get('[data-testid="tasks-scope"]').setValue('completed')
    await flushPromises()

    await wrapper.get('[data-testid="tasks-search"]').setValue('desks')
    expect(lastRequest(backend).params.q).toBeUndefined()

    await wrapper.get('[data-testid="tasks-search-submit"]').trigger('submit')
    await flushPromises()

    expect(lastRequest(backend).params.q).toBe('desks')
    expect(lastRequest(backend).params.scope).toBe('completed')
  })

  it('says the search found nothing rather than that the queue is empty', async () => {
    rows = []

    const wrapper = await render({ q: 'nothing here' })

    expect(wrapper.get('[data-testid="tasks-empty"]').text()).toContain('matches that search')
  })

  it('says what is empty, which is not always the queue', async () => {
    // "Nothing is waiting for you" under `overdue` says the queue is empty when
    // what is empty is the late part of it \u2014 the opposite of the news.
    rows = []

    const wrapper = await render({ scope: 'overdue' })

    expect(wrapper.get('[data-testid="tasks-empty"]').text()).toContain('Nothing of yours is late')
  })
})
