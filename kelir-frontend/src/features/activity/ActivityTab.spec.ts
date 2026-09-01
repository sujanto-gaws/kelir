import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import ActivityTab from './ActivityTab.vue'
import { useAuthStore } from '@/stores/auth'
import type { CurrentUser } from '@/types/auth'
import {
  errorBody,
  installFakeBackend,
  type FakeBackendHandle,
  type FakeReply,
} from '@/lib/testing/fake-backend'

/**
 * The document activity timeline (FR-ACT-005, MVP criterion 12; [#250]).
 *
 * # Seen to fail (coding standard §2.9)
 *
 * Every mutation below was run against this file and the reddened test named,
 * on 2026-09-01:
 *
 * - **M1** — `actor()` changed to join a current name
 *   (`event.actorUserId ? 'the user now' : 'The system'`), which is AC4
 *   inverted. Red: *renders the actor as recorded, not as they are now*.
 * - **M2** — the `activity-category` chip deleted from the template, which is
 *   AC3's visibility removed. Red: *shows every source in one list, labelled*.
 * - **M3** — `load` changed to replace rather than append
 *   (`events.value = result.items`), which is the paging defect AC1 and AC6 are
 *   about. Red: *appends the next page rather than replacing what is read*.
 * - **M4** — the `activity-not-audit` note deleted (AC5). Red: *says plainly
 *   that this is not the audit trail*.
 * - **M5** — a `canRead = auth.can('activity:read')` gate added around the
 *   fetch, which is D-47 undone. Red: *reads with the document's permission and
 *   no other*.
 *
 * [#250]: https://github.com/sujanto-gaws/kelir/issues/250
 */

const DOCUMENT_ID = '0199a1a0-0000-7000-8000-0000000000d1'

function pageBody(rows: Record<string, unknown>[], total = rows.length): unknown {
  return { success: true, data: rows, meta: { page: 1, pageSize: 20, total } }
}

function event(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    id: '0199a1a0-0000-7000-8000-0000000000e1',
    documentId: DOCUMENT_ID,
    workflowInstanceId: null,
    taskId: null,
    attachmentId: null,
    commentId: null,
    eventType: 'Document.Created',
    eventCategory: 'DOCUMENT',
    actorUserId: '0199a1a0-0000-7000-8000-0000000000f9',
    actorName: 'ani',
    actionSummary: 'Created the document',
    details: {},
    occurredAt: '2026-08-31T10:00:00Z',
    ...overrides,
  }
}

describe('ActivityTab', () => {
  let backend: FakeBackendHandle
  let onList: (page: number) => FakeReply

  beforeEach(() => {
    setActivePinia(createPinia())
    signIn(['document:read'])

    onList = () => ({ status: 200, body: pageBody([event()]) })

    backend = installFakeBackend((request) => {
      if (request.url.includes('/activity')) {
        // **From `params`, not from the URL.** The client sends the page as an
        // axios parameter, so a handler reading the path alone would answer
        // page one to every request and the append below would look correct
        // while testing nothing.
        return onList(Number(request.params.page ?? 1))
      }

      return { status: 404, body: errorBody('NOT_FOUND', 'no') }
    })
  })

  afterEach(() => {
    backend.restore()
  })

  /**
   * **`document:read` and nothing else**, which is the permission set a person
   * who raises documents actually holds.
   */
  function signIn(permissions: string[]): void {
    const user: CurrentUser = {
      id: '0199a1a0-0000-7000-8000-0000000000f9',
      username: 'ani',
      displayName: 'Ani Wijaya',
      email: 'ani@example.com',
      roles: ['REQUESTER'],
      permissions,
    }

    useAuthStore().user = user
  }

  async function render(): Promise<VueWrapper> {
    const wrapper = mount(ActivityTab, { props: { documentId: DOCUMENT_ID } })
    await flushPromises()

    return wrapper
  }

  it('shows what happened, who did it and when', async () => {
    const wrapper = await render()

    expect(wrapper.find('[data-testid="activity-summary"]').text()).toBe('Created the document')
    expect(wrapper.find('[data-testid="activity-row"]').text()).toContain('ani')
  })

  /**
   * **AC2, and D-47 in one assertion.**
   *
   * This account holds `document:read` and no permission whose name contains
   * `activity` — the state a deployment leaves whoever raises documents in.
   * Before D-47 the server refused them and this panel would have had to gate
   * on a permission they do not hold, showing a refusal on their own document.
   *
   * **Seen red (M5)** with a `can('activity:read')` gate added around the
   * fetch: no request is made and no row renders.
   */
  it('reads with the document permission and no other', async () => {
    const wrapper = await render()

    expect(useAuthStore().can('activity:read')).toBe(false)
    expect(backend.requests.some((request) => request.url.includes('/activity'))).toBe(true)
    expect(wrapper.find('[data-testid="activity-row"]').exists()).toBe(true)
  })

  /**
   * **AC3 — a timeline showing three sources of four is worse than one showing
   * none**, because the reader cannot tell an empty category from a missing
   * one. The label is what makes the difference visible.
   *
   * **Seen red (M2)** with the category chip deleted.
   */
  it('shows every source in one list, labelled', async () => {
    onList = () => ({
      status: 200,
      body: pageBody([
        event({ id: 'e1', eventType: 'Attachment.Added', eventCategory: 'ATTACHMENT' }),
        event({ id: 'e2', eventType: 'Comment.Added', eventCategory: 'COMMENT' }),
        event({ id: 'e3', eventType: 'Workflow.Decided', eventCategory: 'WORKFLOW' }),
        event({ id: 'e4', eventType: 'Document.Created', eventCategory: 'DOCUMENT' }),
      ]),
    })

    const wrapper = await render()
    const labels = wrapper.findAll('[data-testid="activity-category"]').map((chip) => chip.text())

    expect(labels).toEqual(['Attachment', 'Comment', 'Workflow', 'Document'])
  })

  /**
   * **AC4 — the name as it was, not as it is.**
   *
   * A history has the people who were there. `actorName` is denormalized at
   * write time for exactly this, and a screen that joined a live name would
   * throw that away at the render — which is the one place the property is
   * observable.
   *
   * **Seen red (M1)** with `actor()` resolving a current name instead.
   */
  it('renders the actor as recorded, not as they are now', async () => {
    onList = () => ({
      status: 200,
      body: pageBody([event({ actorName: 'ani.wijaya-as-she-was' })]),
    })

    const wrapper = await render()

    expect(wrapper.find('[data-testid="activity-row"]').text()).toContain('ani.wijaya-as-she-was')
  })

  /** An event the system wrote has no actor, and the row still reads. */
  it('names the system for an event nobody performed', async () => {
    onList = () => ({
      status: 200,
      body: pageBody([event({ actorUserId: null, actorName: null })]),
    })

    const wrapper = await render()

    expect(wrapper.find('[data-testid="activity-row"]').text()).toContain('The system')
  })

  /**
   * **AC1 and AC6 — paging appends.**
   *
   * A long-running document is exactly where an unpaginated list stops working,
   * and a *replacing* pager on a newest-first list takes away the entries the
   * reader came for to show them older ones. The total order that stops a row
   * appearing twice across the boundary is the server's (`created_at DESC, id
   * DESC`); what this asserts is that the client keeps what it has already been
   * given.
   *
   * **Seen red (M3)** with `load` replacing rather than appending.
   */
  it('appends the next page rather than replacing what is read', async () => {
    onList = (page) =>
      page === 2
        ? { status: 200, body: pageBody([event({ id: 'older', actionSummary: 'Older thing' })], 2) }
        : { status: 200, body: pageBody([event({ id: 'newer', actionSummary: 'Newer thing' })], 2) }

    const wrapper = await render()
    expect(wrapper.findAll('[data-testid="activity-row"]')).toHaveLength(1)

    await wrapper.find('[data-testid="activity-more"]').trigger('click')
    await flushPromises()

    const rows = wrapper.findAll('[data-testid="activity-row"]')
    expect(rows).toHaveLength(2)
    expect(rows[0].text()).toContain('Newer thing')
    expect(rows[1].text()).toContain('Older thing')
  })

  /** No more to fetch, no control offering to. */
  it('offers no more-button when the page is the whole list', async () => {
    const wrapper = await render()

    expect(wrapper.find('[data-testid="activity-more"]').exists()).toBe(false)
  })

  /**
   * **AC5 — this is the surface where somebody would otherwise merge them.**
   *
   * #247 states the distinction in four places nobody reading this screen will
   * open. A person looking at a list of who did what, on a screen next to a tab
   * called History, is the person about to take it for the compliance record.
   *
   * **Seen red (M4)** with the note deleted.
   */
  it('says plainly that this is not the audit trail', async () => {
    const wrapper = await render()
    const note = wrapper.find('[data-testid="activity-not-audit"]').text()

    expect(note).toContain('not the audit trail')
    expect(note).toContain('History tab')
  })

  it('distinguishes nothing-recorded from could-not-be-read', async () => {
    onList = () => ({ status: 200, body: pageBody([]) })
    const empty = await render()
    expect(empty.find('[data-testid="activity-empty"]').exists()).toBe(true)
    expect(empty.find('[data-testid="activity-error"]').exists()).toBe(false)

    onList = () => ({ status: 500, body: errorBody('INTERNAL', 'the database is away') })
    const failed = await render()
    expect(failed.find('[data-testid="activity-error"]').exists()).toBe(true)
    expect(failed.find('[data-testid="activity-empty"]').exists()).toBe(false)
  })
})
