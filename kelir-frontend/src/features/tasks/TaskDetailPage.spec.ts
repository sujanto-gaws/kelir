import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'

import TaskDetailPage from './TaskDetailPage.vue'
import { useAuthStore } from '@/stores/auth'
import type { CurrentUser } from '@/types/auth'
import {
  errorBody,
  installFakeBackend,
  itemBody,
  type FakeBackendHandle,
  type FakeReply,
  type RecordedRequest,
} from '@/lib/testing/fake-backend'

/**
 * One task, and what it is asking (#179 AC4).
 *
 * **Driven through the real API client** — the fake backend replaces the axios
 * adapter and nothing else, so envelope unwrapping and `ApiError` classification
 * run for real. A spec that stubbed `getTask` would be asserting that the screen
 * calls a function.
 *
 * What is **not** here is whether the backend decides correctly: that is
 * `kelir-backend/tests/workflow_engine.rs`. What is here is the screen's half —
 * that a person is told what is being decided and about what, that a transition
 * this release cannot perform is shown and not offered, and that the reason for
 * a decision travels with it (FR-TASK-006, #182) under the rule the definition
 * set rather than one this screen invented.
 */

const blank = { template: '<div />' }

const CALLER_ID = '0199a1a0-0000-7000-8000-0000000000c9'
const TASK_ID = '0199a1a0-0000-7000-8000-0000000000a1'
const DOCUMENT_ID = '0199a1a0-0000-7000-8000-0000000000d1'

/** Who the hand-over picker is offered, when the caller may read users. */
const people = [
  {
    id: CALLER_ID,
    username: 'ani',
    email: 'ani@example.com',
    displayName: 'Ani Wijaya',
    status: 'ACTIVE',
    departmentId: null,
    mustChangePassword: false,
    lastLoginAt: null,
    lockedUntil: null,
    createdAt: '2026-01-01T00:00:00Z',
    roles: [],
  },
  {
    id: 'u-budi',
    username: 'budi',
    email: 'budi@example.com',
    displayName: 'Budi Santoso',
    status: 'ACTIVE',
    departmentId: null,
    mustChangePassword: false,
    lastLoginAt: null,
    lockedUntil: null,
    createdAt: '2026-01-01T00:00:00Z',
    roles: [],
  },
]

function detail(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    id: TASK_ID,
    taskRef: 'TASK-2026-000001',
    taskName: 'Approve the request',
    taskType: 'APPROVAL_TASK',
    status: 'CREATED',
    priority: 'NORMAL',
    dueAt: null,
    assignment: 'ROLE',
    candidateRoleCode: 'FINANCE_APPROVER',
    delegatedFromUserId: null,
    delegatedFromDisplayName: null,
    workflowInstanceId: '0199a1a0-0000-7000-8000-0000000000b1',
    workflowName: 'Standard approval',
    workflowKey: 'purchase_requisition_standard',
    currentState: 'MANAGER_APPROVAL',
    currentStateName: 'Manager approval',
    documentId: DOCUMENT_ID,
    documentRef: 'DOC-2026-000001',
    documentNumber: 'PR-2026-000001',
    documentTitle: 'Two standing desks',
    createdAt: '2026-08-28T00:00:00Z',
    decisions: [
      {
        action: 'APPROVE',
        toState: 'COMPLETED',
        toStateName: 'Completed',
        supported: true,
        requiresComment: false,
      },
      {
        action: 'REJECT',
        toState: 'REJECTED',
        toStateName: 'Rejected',
        supported: true,
        requiresComment: true,
      },
    ],
    ...overrides,
  }
}

describe('TaskDetailPage', () => {
  let backend: FakeBackendHandle
  let router: Router
  let current: Record<string, unknown>
  let onDecision: (request: RecordedRequest) => FakeReply
  let onDelegation: (request: RecordedRequest) => FakeReply

  beforeEach(() => {
    setActivePinia(createPinia())
    current = detail()

    onDecision = () => ({
      status: 200,
      body: itemBody({
        taskId: TASK_ID,
        workflowInstanceId: current.workflowInstanceId,
        documentId: DOCUMENT_ID,
        action: 'APPROVE',
        previousState: 'MANAGER_APPROVAL',
        currentState: 'COMPLETED',
        documentStatus: 'COMPLETED',
      }),
    })

    onDelegation = () => ({ status: 200, body: itemBody(current) })

    backend = installFakeBackend((request) => {
      if (request.url.includes('/decision')) {
        return onDecision(request)
      }

      if (request.url.includes('/delegation')) {
        return onDelegation(request)
      }

      if (request.url.startsWith('/identity/users')) {
        return {
          status: 200,
          body: {
            success: true,
            data: people,
            meta: { page: 1, pageSize: 100, total: people.length },
          },
        }
      }

      if (request.url.includes('/claim')) {
        return { status: 200, body: itemBody(current) }
      }

      return { status: 200, body: itemBody(current) }
    })

    router = createRouter({
      history: createMemoryHistory(),
      routes: [
        { path: '/tasks/:id', name: 'task', component: blank },
        { path: '/tasks', name: 'tasks', component: blank },
        { path: '/documents/:id', name: 'document', component: blank },
      ],
    })
  })

  afterEach(() => {
    backend.restore()
  })

  /**
   * Signs the caller in with the permissions given.
   *
   * The hand-over picker is gated on `identity:user:read` — see the component,
   * which says why it borrows an administrative permission and what that costs.
   */
  function signIn(permissions: string[]): void {
    const user: CurrentUser = {
      id: CALLER_ID,
      username: 'ani',
      displayName: 'Ani Wijaya',
      email: 'ani@example.com',
      roles: ['APPROVER'],
      permissions,
    }

    useAuthStore().user = user
  }

  async function render(): Promise<VueWrapper> {
    await router.push({ name: 'task', params: { id: TASK_ID } })
    await router.isReady()

    const wrapper = mount(TaskDetailPage, { global: { plugins: [router] } })
    await flushPromises()

    return wrapper
  }

  it('says what is being decided, and about which document', async () => {
    // AC4: a task that says only "approve?" is a task its holder cannot
    // responsibly action.
    const wrapper = await render()

    expect(wrapper.get('[data-testid="task-name"]').text()).toBe('Approve the request')
    expect(wrapper.get('[data-testid="task-document"]').text()).toContain('PR-2026-000001')
    expect(wrapper.get('[data-testid="task-document"]').text()).toContain('Two standing desks')

    // The definition's own name for the step, not the code the database holds.
    expect(wrapper.text()).toContain('Manager approval')
  })

  it('offers the decisions the definition declares, named by where they lead', async () => {
    const wrapper = await render()

    expect(wrapper.get('[data-testid="decide-APPROVE"]').text()).toContain('Completed')
    expect(wrapper.get('[data-testid="decide-REJECT"]').text()).toContain('Rejected')
  })

  it('shows a transition this release cannot perform without offering it', async () => {
    // A definition may declare `DELEGATE` — FR-WF-009 is #184 — and a button for
    // it would produce a 422 from a control the product itself drew. It was
    // `RETURN` until #183 built that one.
    current = detail({
      decisions: [
        {
          action: 'APPROVE',
          toState: 'COMPLETED',
          toStateName: 'Completed',
          supported: true,
          requiresComment: false,
        },
        {
          action: 'DELEGATE',
          toState: 'MANAGER_APPROVAL',
          toStateName: 'Manager approval',
          supported: false,
          requiresComment: false,
        },
      ],
    })

    const wrapper = await render()

    expect(wrapper.find('[data-testid="decide-DELEGATE"]').exists()).toBe(false)
    expect(wrapper.get('[data-testid="task-deferred"]').text()).toContain('DELEGATE')
  })

  it('offers a claim only for work that is going spare', async () => {
    const wrapper = await render()
    expect(wrapper.find('[data-testid="claim-task"]').exists()).toBe(true)

    current = detail({ assignment: 'MINE', status: 'ASSIGNED' })
    const mine = await render()
    expect(mine.find('[data-testid="claim-task"]').exists()).toBe(false)
  })

  it('sends the reason with the decision, in the same request', async () => {
    // FR-TASK-006. One request, because a decision and the reason for it are
    // entered together — a screen that recorded the decision and then asked for
    // a reason would have already committed the half nobody can take back.
    const wrapper = await render()

    await wrapper.get('#decision-comment').setValue('  Within budget for Q3.  ')
    await wrapper.get('[data-testid="decide-APPROVE"]').trigger('click')
    await flushPromises()

    const decision = backend.requests.find((request) => request.url.includes('/decision'))

    expect(decision?.body).toEqual({ action: 'APPROVE', comment: 'Within budget for Q3.' })
  })

  it('omits the comment entirely when there is none to send', async () => {
    // Not `null`, and not `""`. An approval on an edge that asks for no reason
    // is a one-field request, which is what it is.
    const wrapper = await render()

    await wrapper.get('[data-testid="decide-APPROVE"]').trigger('click')
    await flushPromises()

    const decision = backend.requests.find((request) => request.url.includes('/decision'))

    expect(decision?.body).toEqual({ action: 'APPROVE' })
  })

  it('refuses a required reason before sending anything', async () => {
    // #182 AC4, this end. The REJECT edge of the fixture is marked
    // `requiresComment`, so pressing it with an empty box must not reach the
    // API — a request the server will refuse from a button the product drew is
    // the failure the flag exists to prevent.
    const wrapper = await render()

    await wrapper.get('[data-testid="decide-REJECT"]').trigger('click')
    await flushPromises()

    expect(wrapper.get('[data-testid="comment-required"]').text()).toContain('needs a reason')
    expect(backend.requests.some((request) => request.url.includes('/decision'))).toBe(false)
  })

  it('is not satisfied by a box full of spaces', async () => {
    // The requirement defeated by the space bar, which is the same rule
    // `normalize_comment` holds on the server: blank is absent.
    const wrapper = await render()

    await wrapper.get('#decision-comment').setValue('    ')
    await wrapper.get('[data-testid="decide-REJECT"]').trigger('click')
    await flushPromises()

    expect(wrapper.get('[data-testid="comment-required"]').text()).toContain('needs a reason')
    expect(backend.requests.some((request) => request.url.includes('/decision'))).toBe(false)
  })

  it('does not decide for itself which actions need a reason', async () => {
    // The flag is read, never derived. Here the definition marks the APPROVE
    // and leaves the REJECT alone — the opposite of the intuition a
    // client-side rule would encode — and the screen follows the definition.
    current = detail({
      decisions: [
        {
          action: 'APPROVE',
          toState: 'COMPLETED',
          toStateName: 'Completed',
          supported: true,
          requiresComment: true,
        },
        {
          action: 'REJECT',
          toState: 'REJECTED',
          toStateName: 'Rejected',
          supported: true,
          requiresComment: false,
        },
      ],
    })

    const wrapper = await render()

    await wrapper.get('[data-testid="decide-APPROVE"]').trigger('click')
    await flushPromises()
    expect(backend.requests.some((request) => request.url.includes('/decision'))).toBe(false)

    await wrapper.get('[data-testid="decide-REJECT"]').trigger('click')
    await flushPromises()
    expect(backend.requests.some((request) => request.url.includes('/decision'))).toBe(true)
  })

  it("surfaces the server's refusal when the two ends disagree", async () => {
    // The client check is a courtesy; the server's is the control. A workflow
    // whose two APPROVE edges differ, or a stale screen, produces this — and
    // the backend's own words say which field was wrong.
    onDecision = () => ({
      status: 422,
      body: errorBody(
        'VALIDATION_ERROR',
        'the `APPROVE` transition out of `MANAGER_APPROVAL` requires a comment',
        [
          {
            path: 'comment',
            rule: 'requiresComment',
            code: 'COMMENT_REQUIRED',
            message: 'the `APPROVE` transition out of `MANAGER_APPROVAL` requires a comment',
          },
        ],
      ),
    })

    const wrapper = await render()

    await wrapper.get('[data-testid="decide-APPROVE"]').trigger('click')
    await flushPromises()

    expect(wrapper.get('[data-testid="task-problem"]').text()).toContain('requires a comment')
  })

  it('reports where the document ended up after a decision', async () => {
    const wrapper = await render()

    await wrapper.get('[data-testid="decide-APPROVE"]').trigger('click')
    await flushPromises()

    expect(wrapper.get('[data-testid="task-notice"]').text()).toContain('Completed')
    expect(backend.requests.some((request) => request.url.includes('/decision'))).toBe(true)
  })

  it("shows the backend's own words when somebody else decided first", async () => {
    // A 409 here is a real situation, not a bug: the person who lost has to be
    // able to tell "you were too late" from "the server broke". Repeating the
    // reason in our words risks contradicting it (coding standard §3.3).
    onDecision = () => ({
      status: 409,
      body: errorBody(
        'CONFLICT',
        'this task is COMPLETED and a decision has already been recorded against it',
      ),
    })

    const wrapper = await render()

    await wrapper.get('[data-testid="decide-APPROVE"]').trigger('click')
    await flushPromises()

    expect(wrapper.get('[data-testid="task-problem"]').text()).toContain('already been recorded')
  })

  it('shows a decided task without offering anything to do to it', async () => {
    current = detail({ status: 'COMPLETED', assignment: 'MINE' })

    const wrapper = await render()

    expect(wrapper.get('[data-testid="task-decided"]').text()).toContain('has been decided')
    expect(wrapper.find('[data-testid="decide-APPROVE"]').exists()).toBe(false)
    expect(wrapper.find('[data-testid="claim-task"]').exists()).toBe(false)
  })

  it('does not distinguish a task that is not there from one that is not mine', async () => {
    // The detail answers 404 for both, and saying more would confirm the task
    // exists — a fact a caller who may not see it has no business establishing.
    backend.restore()
    backend = installFakeBackend(() => ({
      status: 404,
      body: errorBody('NOT_FOUND', 'Task not found'),
    }))

    const wrapper = await render()

    expect(wrapper.get('[data-testid="task-error"]').text()).toContain('could not be opened')
  })

  // -------------------------------------------------------------------------
  // Handing it over (FR-WF-009, FR-TASK-008; #184)
  // -------------------------------------------------------------------------

  it('does not offer to hand over a task nobody has taken', async () => {
    // The server refuses an unclaimed role task with a 409 saying to claim it
    // first: it has no holder, and giving it to one named person would take it
    // out of every other holder's queue without anybody asking. Drawing the
    // control and being refused would be the product refusing itself.
    signIn(['workflow:task:execute', 'identity:user:read'])

    const wrapper = await render()

    expect(wrapper.find('[data-testid="start-hand-over"]').exists()).toBe(false)
  })

  it('offers to hand over a task that is already yours', async () => {
    signIn(['workflow:task:execute', 'identity:user:read'])
    current = detail({ assignment: 'MINE', status: 'ASSIGNED' })

    const wrapper = await render()

    expect(wrapper.find('[data-testid="start-hand-over"]').exists()).toBe(true)
  })

  it('offers everybody but you, and says so plainly when it cannot see anybody', async () => {
    // The picker borrows `identity:user:read`, which is administrative. A
    // deployment that has not granted it to its approvers gets a sentence
    // saying why rather than an empty box.
    signIn(['workflow:task:execute', 'identity:user:read'])
    current = detail({ assignment: 'MINE', status: 'ASSIGNED' })

    const wrapper = await render()
    await wrapper.get('[data-testid="start-hand-over"]').trigger('click')
    await flushPromises()

    const labels = wrapper
      .get('#delegate-user')
      .findAll('option')
      .map((option) => option.text())

    expect(labels).toContain('Budi Santoso')
    expect(labels).not.toContain('Ani Wijaya')
    expect(wrapper.find('[data-testid="hand-over-unavailable"]').exists()).toBe(false)
  })

  it('says why the picker is empty when the caller cannot read users', async () => {
    signIn(['workflow:task:execute'])
    current = detail({ assignment: 'MINE', status: 'ASSIGNED' })

    const wrapper = await render()
    await wrapper.get('[data-testid="start-hand-over"]').trigger('click')
    await flushPromises()

    expect(wrapper.get('[data-testid="hand-over-unavailable"]').text()).toContain(
      'permission to view users',
    )
    expect(wrapper.find('[data-testid="confirm-hand-over"]').exists()).toBe(false)
  })

  it('hands the task over through its own route, not through a decision', async () => {
    // It is not a fourth `DecisionAction`: nothing about the document is
    // answered and the process does not move.
    signIn(['workflow:task:execute', 'identity:user:read'])
    current = detail({ assignment: 'MINE', status: 'ASSIGNED' })

    const wrapper = await render()
    await wrapper.get('[data-testid="start-hand-over"]').trigger('click')
    await flushPromises()

    await wrapper.get('#delegate-user').setValue('u-budi')
    await wrapper.get('#hand-over-reason').setValue('  Off from tomorrow  ')
    await wrapper.get('[data-testid="confirm-hand-over"]').trigger('click')
    await flushPromises()

    const posted = backend.requests.find((request) => request.url.includes('/delegation'))

    expect(posted?.url).toBe(`/workflow/tasks/${TASK_ID}/delegation`)
    expect(posted?.body).toMatchObject({
      delegateUserId: 'u-budi',
      comment: 'Off from tomorrow',
    })
    expect(backend.requests.some((request) => request.url.includes('/decision'))).toBe(false)
    expect(wrapper.get('[data-testid="task-notice"]').text()).toContain('Budi Santoso has it now')
  })

  it("keeps the panel closed over the server's refusal rather than reporting success", async () => {
    signIn(['workflow:task:execute', 'identity:user:read'])
    current = detail({ assignment: 'MINE', status: 'ASSIGNED' })

    onDelegation = () => ({
      status: 409,
      body: errorBody('CONFLICT', 'this task changed hands while it was being handed over'),
    })

    const wrapper = await render()
    await wrapper.get('[data-testid="start-hand-over"]').trigger('click')
    await flushPromises()

    await wrapper.get('#delegate-user').setValue('u-budi')
    await wrapper.get('[data-testid="confirm-hand-over"]').trigger('click')
    await flushPromises()

    expect(wrapper.get('[data-testid="task-problem"]').text()).toContain('changed hands')
    expect(wrapper.find('[data-testid="task-notice"]').exists()).toBe(false)
  })

  it('says whose approval a delegated task is', async () => {
    // Beside "Mine" rather than instead of it: the task *is* theirs to decide,
    // and whose approval it is is a second fact (#184 AC4).
    signIn(['workflow:task:execute'])
    current = detail({
      assignment: 'MINE',
      status: 'ASSIGNED',
      delegatedFromUserId: '0199a1a0-0000-7000-8000-0000000000f2',
      delegatedFromDisplayName: 'Ani Wijaya',
    })

    const wrapper = await render()

    expect(wrapper.get('[data-testid="task-on-behalf-of"]').text()).toContain('Ani Wijaya')
    expect(wrapper.get('[data-testid="task-assignment"]').text()).toBe('Mine')
  })
})
