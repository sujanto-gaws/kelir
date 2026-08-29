import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import WorkflowTab from './WorkflowTab.vue'
import {
  errorBody,
  installFakeBackend,
  itemBody,
  type FakeBackendHandle,
  type FakeReply,
} from '@/lib/testing/fake-backend'

/**
 * The process deciding a document, and how it got here (#178, #181, #182 AC2).
 *
 * **Driven through the real API client**, like every other screen spec: the fake
 * backend replaces the axios adapter and nothing else, so envelope unwrapping,
 * pagination metadata and `ApiError` classification all run for real.
 *
 * The subject of most of what is below is the **history**, because that is where
 * a decision's reason becomes visible to a person — and a reason recorded where
 * nobody sees it would not have been worth capturing.
 */

const DOCUMENT_ID = '0199a1a0-0000-7000-8000-0000000000d1'
const INSTANCE_ID = '0199a1a0-0000-7000-8000-0000000000b1'

function workflowBody(): unknown {
  return itemBody({
    instance: {
      id: INSTANCE_ID,
      instanceRef: 'WFI-2026-000001',
      documentId: DOCUMENT_ID,
      workflowDefinitionId: '0199a1a0-0000-7000-8000-0000000000c1',
      workflowKey: 'purchase_requisition_standard',
      workflowName: 'Standard approval',
      definitionVersion: 1,
      status: 'COMPLETED',
      currentState: 'REJECTED',
      currentStateName: 'Rejected',
      outcome: 'REJECTED',
      businessKey: 'PR-2026-000001',
      startedBy: null,
      startedAt: '2026-08-28T00:00:00Z',
      completedAt: '2026-08-28T01:00:00Z',
      variables: [],
    },
    tasks: [
      {
        id: '0199a1a0-0000-7000-8000-0000000000a1',
        taskRef: 'TASK-2026-000001',
        workflowInstanceId: INSTANCE_ID,
        documentId: DOCUMENT_ID,
        taskDefinitionKey: 'manager_approval',
        taskName: 'Approve the request',
        taskType: 'APPROVAL_TASK',
        status: 'COMPLETED',
        assigneeUserId: null,
        candidateRoleId: null,
        candidateRoleCode: 'FINANCE_APPROVER',
        candidateDepartmentId: null,
        priority: 'NORMAL',
        dueAt: null,
        action: 'REJECT',
        completedBy: null,
        completedAt: '2026-08-28T01:00:00Z',
        createdAt: '2026-08-28T00:00:00Z',
      },
    ],
  })
}

function historyBody(
  entries: Record<string, unknown>[],
  meta = { page: 1, pageSize: 20, total: entries.length },
): unknown {
  return { success: true, data: entries, meta }
}

function entry(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    id: '0199a1a0-0000-7000-8000-0000000000e1',
    fromState: 'MANAGER_APPROVAL',
    toState: 'REJECTED',
    action: 'REJECT',
    taskId: '0199a1a0-0000-7000-8000-0000000000a1',
    comment: null,
    actorUserId: '0199a1a0-0000-7000-8000-0000000000f1',
    actorUsername: 'dina',
    onBehalfOfUserId: null,
    onBehalfOfUsername: null,
    occurredAt: '2026-08-28T01:00:00Z',
    ...overrides,
  }
}

describe('WorkflowTab', () => {
  let backend: FakeBackendHandle
  let onHistory: () => FakeReply

  beforeEach(() => {
    onHistory = () => ({ status: 200, body: historyBody([entry()]) })

    backend = installFakeBackend((request) => {
      if (request.url.includes('/workflow/history')) {
        return onHistory()
      }

      if (request.url.includes('/workflow')) {
        return { status: 200, body: workflowBody() }
      }

      return { status: 404, body: errorBody('NOT_FOUND', 'no') }
    })
  })

  afterEach(() => {
    backend.restore()
  })

  async function render(): Promise<VueWrapper> {
    const wrapper = mount(WorkflowTab, { props: { documentId: DOCUMENT_ID } })
    await flushPromises()

    return wrapper
  }

  it('shows the decision comment where the decision is', async () => {
    // #182 AC2 in one assertion: the reason an approver gave is on the screen
    // that shows what they decided. A comment persisted and displayed nowhere
    // was not worth capturing.
    onHistory = () => ({
      status: 200,
      body: historyBody([entry({ comment: 'The figure does not match the quotation.' })]),
    })

    const wrapper = await render()

    expect(wrapper.get('[data-testid="history-comment"]').text()).toContain(
      'does not match the quotation',
    )
  })

  it('names who moved it and where it moved to', async () => {
    const wrapper = await render()
    const history = wrapper.get('[data-testid="workflow-history"]').text()

    expect(history).toContain('MANAGER_APPROVAL')
    expect(history).toContain('REJECTED')
    expect(history).toContain('REJECT')
    expect(history).toContain('dina')
  })

  it('says "the system" for a move nobody made', async () => {
    // An entry with no actor is the engine having moved the process. A blank
    // beside a timestamp reads as missing data rather than as an answer.
    onHistory = () => ({
      status: 200,
      body: historyBody([
        entry({ fromState: null, toState: 'MANAGER_APPROVAL', action: null, actorUsername: null }),
      ]),
    })

    const wrapper = await render()

    expect(wrapper.get('[data-testid="workflow-history"]').text()).toContain('The system')
  })

  it('pages the history rather than asking for all of it', async () => {
    // A long-running approval is exactly where an unpaginated list stops
    // working, which is why the endpoint pages — and a client that ignored that
    // would make the pagination decorative.
    onHistory = () => ({
      status: 200,
      body: historyBody([entry()], { page: 1, pageSize: 20, total: 25 }),
    })

    const wrapper = await render()
    expect(wrapper.find('[data-testid="history-more"]').exists()).toBe(true)

    const request = backend.requests.find((call) => call.url.includes('/workflow/history'))
    expect(request?.params).toMatchObject({ page: 1, pageSize: 20 })
  })

  it('reports a history it could not read without hiding the process', async () => {
    // The instance and its tasks have already loaded and answer most of what
    // somebody opened this panel for. Replacing all of it with one error would
    // cost more than the failure did.
    onHistory = () => ({
      status: 403,
      body: errorBody('FORBIDDEN', 'Missing workflow:instance:read'),
    })

    const wrapper = await render()

    expect(wrapper.find('[data-testid="workflow-instance"]').exists()).toBe(true)
    expect(wrapper.get('[data-testid="workflow-history-problem"]').text()).toContain('Missing')
  })

  it('does not ask for the history of a document nothing is deciding', async () => {
    // A 404 on the workflow means no process, which is a valid configuration
    // and not an error. Asking for its history would print a failure beside a
    // panel that has just correctly said there is no approval.
    backend.restore()
    backend = installFakeBackend(() => ({
      status: 404,
      body: errorBody('NOT_FOUND', 'Workflow instance not found'),
    }))

    const wrapper = await render()

    expect(wrapper.get('[data-testid="workflow-none"]').text()).toContain('No approval')
    expect(backend.requests.some((call) => call.url.includes('/workflow/history'))).toBe(false)
  })

  it('names both parties on a decision somebody took for somebody else', async () => {
    // #184 AC4. A delegated approval that showed only the delegate would answer
    // *who decided* and lose *on whose authority* — which is the accountability
    // delegation exists to preserve, and the reason the pair is on the row a
    // person reads rather than only in the formal decision record.
    onHistory = () => ({
      status: 200,
      body: historyBody([
        entry({
          actorUsername: 'budi',
          onBehalfOfUserId: '0199a1a0-0000-7000-8000-0000000000f2',
          onBehalfOfUsername: 'ani',
        }),
      ]),
    })

    const wrapper = await render()

    expect(wrapper.get('[data-testid="history-on-behalf-of"]').text()).toContain('ani')
    expect(wrapper.get('[data-testid="workflow-history"]').text()).toContain('budi')
  })

  it('says nothing about a second party where there was not one', async () => {
    // The other half of the pair. Writing the actor into both would make
    // *acting for myself* and *acting for somebody who happens to be me* read
    // the same.
    const wrapper = await render()

    expect(wrapper.find('[data-testid="history-on-behalf-of"]').exists()).toBe(false)
  })
})
