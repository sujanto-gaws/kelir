import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import axios, {
  AxiosError,
  AxiosHeaders,
  type AxiosAdapter,
  type InternalAxiosRequestConfig,
} from 'axios'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import DashboardPage from './DashboardPage.vue'
import { useAuthStore } from '@/stores/auth'
import type { CurrentUser } from '@/types/auth'
import {
  errorBody,
  installFakeBackend,
  itemBody,
  type FakeBackendHandle,
  type FakeReply,
} from '@/lib/testing/fake-backend'

/**
 * The dashboard (FR-RPT-001, [#431]; FR-RPT-002, [#432]; FR-RPT-003, [#433]).
 *
 * # Seen to fail (coding standard §2.9)
 *
 * **Six mutations, run 2026-09-12, each red and each reddening its own test and
 * no other.** Stated as what was run rather than as what would be run, which is
 * the distinction [#404](https://github.com/sujanto-gaws/kelir/issues/404) is
 * still open about. Baseline first: **7 passed, nothing mutated.**
 *
 * | Mutation | Red |
 * |---|---|
 * | **M1** — `/version` restored as the page's fetch: the pre-#431 behaviour, so **the defect itself rather than a stand-in for it** | *asks the dashboard summary for its numbers, and asks nothing for a version* |
 * | **M2** — the `v-if="!canReadSummary"` branch removed from the template | *explains itself instead of erroring when the caller has no grant* |
 * | **M3** — the early return removed from `load`, so the request fires anyway | the same test, from the request side rather than the render side |
 * | **M4** — `tasksOverdue` promoted to a card of its own | *shows overdue as a caption on the waiting card, because it is a subset of it* |
 * | **M5** — the `isEmpty` branch removed (§3.4's third state) | *says so plainly when there is nothing waiting* |
 * | **M6** — the retry `Button` removed from the error state | *offers a retry when the summary fails* |
 *
 * **M1 is the one that found something, and it is recorded rather than tidied
 * away.** It first reddened **all seven** tests, which looked like a strong
 * result and was the opposite: `getOperational` goes around `apiClient` with
 * bare `axios`, `installFakeBackend` replaces only `apiClient`'s adapter, so the
 * restored `/version` call escaped the harness entirely and tried the network on
 * a 5-second timeout. Everything failed for the wrong reason — **and the
 * assertion that names the defect was passing vacuously**, because `/version`
 * could never be recorded whether the page called it or not.
 *
 * The global-adapter stub in `beforeEach` is what closed that, and M1 then
 * reddened exactly the one test it is about. **A mutation run that had only
 * checked "did something go red" would have called the first result a success**,
 * which is the argument §2.9 makes for naming the reddened test rather than
 * counting failures.
 *
 * ## The pending-task widget (FR-RPT-002, [#432])
 *
 * **Six mutations, run 2026-09-12, each red and each reddening exactly one
 * test.** Baseline first: **16 passed, nothing mutated.**
 *
 * | Mutation | Red |
 * |---|---|
 * | **F1** — the waiting card reads `pendingTasks.length` instead of `tasksWaiting` | *shows what is waiting and what is unsent* |
 * | **F2** — the widget's empty sentence never renders, leaving a blank list in its place | *says nothing is waiting rather than showing an empty card* |
 * | **F3** — the overdue badge derived from `dueAt` against the browser's clock | *marks a task late because the server said so, not because of the clock* |
 * | **F4** — the delegated-from line dropped from the row | *says whose work a delegated task is* |
 * | **F5** — `canOpenTasks` forced true, so every row is linked | *shows the rows without links when the caller cannot open the inbox* |
 * | **F6** — the *more waiting* line counts from the rows rather than the queue | *says how many more are waiting than it shows* |
 *
 * **F2 was run twice, and the first attempt is recorded rather than tidied
 * away.** Deleting the empty-state paragraph outright orphaned the `v-else` on
 * the list beside it, so the component stopped compiling and the whole file
 * failed with a template error and **zero assertions run**. That is not a
 * survivor and it is not a red either — it is a mutation that never reached the
 * code under test, and counting it as evidence would have been the same
 * mistake as counting a green one. Re-run as `v-if="false"` it compiles, leaves
 * a genuinely blank card, and reddens exactly the test that names it.
 *
 * **F1 and F6 are the pair worth having.** The count and the list now come from
 * one statement on the server, so the remaining way for a card to lie about how
 * much work is waiting is on this side: deriving the number from the rows it
 * was sent. Both mutations are that mistake, from the two ends it can be made.
 *
 * ## The recent-documents widget (FR-RPT-003, [#433])
 *
 * **Five mutations, run 2026-09-12, each red and each reddening exactly one
 * test.** Baseline first: **23 passed, nothing mutated.**
 *
 * | Mutation | Red |
 * |---|---|
 * | **G1** — the row's date read from `updatedAt` instead of `lastTouchedAt` | *shows when the caller last touched it rather than when it last changed* |
 * | **G2** — the empty sentence never renders, leaving a blank card | *says nothing has been worked on rather than showing an empty card* |
 * | **G3** — `canOpenDocuments` forced true, so every row is linked | *shows the rows without links when the caller cannot open documents* |
 * | **G4** — the card re-sorts the server's rows on `updatedAt` | *lists the documents the caller touched, newest touch first* |
 * | **G5** — the row always shows `documentRef`, never the number | *identifies a row by its number, or by its reference before it has one* |
 *
 * **G1 and G4 are the pair worth having, and they are the same mistake from two
 * ends.** `updatedAt` is the field on this row that looks interchangeable with
 * `lastTouchedAt` and is not: it moves when **anybody** changes the document,
 * while `lastTouchedAt` is `max` over *this caller's* own touch events and is
 * the value the server ordered by. A card reading it would answer *what changed
 * recently among things I have touched*, which is a different question from the
 * one the card is named after — and it would be wrong silently, on a screen
 * where nobody has a second source to check against.
 *
 * Both fixtures set the two timestamps **years** apart and in opposite
 * directions, which is what makes the tests able to tell them apart at all: on
 * realistic data the two fields usually agree, and a mutation run against a
 * fixture where they agreed would have called both of these covered.
 *
 * **G2 is F2's lesson applied rather than relearned.** The pending-task run
 * recorded that deleting an empty-state paragraph outright orphans the `v-else`
 * beside it, so the component stops compiling and the file fails with zero
 * assertions run — not a survivor and not a red. This one was written as
 * `v-if="false"` from the start.
 *
 * [#433]: https://github.com/sujanto-gaws/kelir/issues/433
 *
 * [#431]: https://github.com/sujanto-gaws/kelir/issues/431
 * [#432]: https://github.com/sujanto-gaws/kelir/issues/432
 */

const USER_ID = '0199a1a0-0000-7000-8000-0000000000f9'

function summary(overrides: Record<string, unknown> = {}): unknown {
  return {
    tasksWaiting: 3,
    tasksOverdue: 1,
    draftDocuments: 2,
    pendingTasks: [task()],
    recentDocuments: [recentDocument()],
    ...overrides,
  }
}

/**
 * One recent-documents row, in the shape the server sends (FR-RPT-003, #433).
 *
 * It is a `DocumentSummary` with `lastTouchedAt` beside it — the same row the
 * document list renders, plus the one field that is about *this caller's*
 * relationship to it rather than about the document. `updatedAt` is carried
 * deliberately: it is the field a card might read by mistake, and several tests
 * below set the two apart so that mistake is visible.
 */
function recentDocument(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    id: '0199a1a0-0000-7000-8000-0000000000d0',
    documentRef: 'DOC-2026-000002',
    documentNumber: 'PR-2026-000002',
    documentTypeId: '0199a1a0-0000-7000-8000-0000000000d5',
    documentTypeCode: 'PURCHASE_REQUISITION',
    title: 'Monitor stand for the design desk',
    status: 'DRAFT',
    priority: 'NORMAL',
    entityType: null,
    entityId: null,
    submittedAt: null,
    createdAt: '2026-09-11T02:00:00Z',
    updatedAt: '2026-09-11T02:00:00Z',
    lastTouchedAt: '2026-09-11T06:00:00Z',
    ...overrides,
  }
}

/**
 * One pending row, in the shape the server sends (FR-RPT-002, #432).
 *
 * It is an `InboxTask` — the same row `/api/v1/tasks` serves — so the fields
 * this fixture carries are the fields the inbox's own rows carry, `isOverdue`
 * included. That is the point of the shape being shared rather than copied.
 */
function task(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    id: '0199a1a0-0000-7000-8000-00000000000a',
    taskRef: 'TASK-2026-000001',
    taskName: 'Approve the request',
    taskType: 'APPROVAL_TASK',
    status: 'CREATED',
    priority: 'NORMAL',
    dueAt: null,
    assignment: 'ROLE',
    isOverdue: false,
    candidateRoleCode: 'APPROVER',
    delegatedFromUserId: null,
    delegatedFromDisplayName: null,
    workflowInstanceId: '0199a1a0-0000-7000-8000-00000000000b',
    workflowName: 'Standard approval',
    currentState: 'MANAGER_APPROVAL',
    documentId: '0199a1a0-0000-7000-8000-00000000000c',
    documentRef: 'DOC-2026-000001',
    documentNumber: 'PR-2026-000001',
    documentTitle: 'Laptop for the new analyst',
    createdAt: '2026-09-10T02:00:00Z',
    action: null,
    decisionComment: null,
    completedAt: null,
    ...overrides,
  }
}

describe('DashboardPage', () => {
  let backend: FakeBackendHandle
  let onSummary: () => FakeReply
  let requested: string[]
  let originalAdapter: AxiosAdapter | undefined

  beforeEach(() => {
    setActivePinia(createPinia())
    signIn(['reporting:dashboard:read'])

    requested = []
    onSummary = () => ({ status: 200, body: itemBody(summary()) })

    // **`installFakeBackend` cannot see a `/version` call, and this is why the
    // stub below exists.** It replaces `apiClient`'s adapter, and
    // `getOperational` — the helper this page used before #431 — deliberately
    // goes around `apiClient` with bare `axios`, because the operational
    // endpoints sit outside `/api/v1` and answer without the envelope.
    //
    // So an assertion that the page asks for no version would have been
    // **vacuous** against the shared harness alone: `/version` would never have
    // been recorded whether the page called it or not, and the test would have
    // passed over exactly the defect it names. Stubbing the global adapter is
    // what makes it observable — and answering 404 keeps this a recorder rather
    // than a second backend.
    originalAdapter = axios.defaults.adapter as AxiosAdapter | undefined
    axios.defaults.adapter = ((config: InternalAxiosRequestConfig) => {
      requested.push(String(config.url ?? ''))

      const error = new AxiosError('request failed with status 404', undefined, config)
      error.response = {
        status: 404,
        statusText: '',
        data: '',
        headers: new AxiosHeaders(),
        config,
      }

      return Promise.reject(error)
    }) as AxiosAdapter

    backend = installFakeBackend((request) => {
      requested.push(request.url)

      if (request.url.includes('/dashboard/summary')) {
        return onSummary()
      }

      return { status: 404, body: errorBody('NOT_FOUND', 'no') }
    })
  })

  afterEach(() => {
    backend.restore()
    axios.defaults.adapter = originalAdapter
  })

  function signIn(permissions: string[]): void {
    const user: CurrentUser = {
      id: USER_ID,
      username: 'ani',
      displayName: 'Ani Wijaya',
      email: 'ani@example.com',
      roles: ['REQUESTER'],
      permissions,
    }

    useAuthStore().user = user
  }

  async function render(): Promise<VueWrapper> {
    const wrapper = mount(DashboardPage, {
      global: { stubs: { RouterLink: { template: '<a><slot /></a>' } } },
    })
    await flushPromises()

    return wrapper
  }

  /**
   * **The row #431 exists to close.** This page showed whoever signed in the
   * backend's version string, and the issue asks for the read to go rather than
   * move. Asserting on the requests is what makes that checkable — a test on
   * the rendered cards alone would pass with a `/version` call still in flight
   * beside them.
   */
  it('asks the dashboard summary for its numbers, and asks nothing for a version', async () => {
    await render()

    expect(requested.some((url) => url.includes('/dashboard/summary'))).toBe(true)
    expect(requested.some((url) => url.includes('/version'))).toBe(false)
  })

  it('shows what is waiting and what is unsent', async () => {
    const wrapper = await render()

    expect(wrapper.find('[data-testid="tasks-waiting"]').text()).toContain('3')
    expect(wrapper.find('[data-testid="draft-documents"]').text()).toContain('2')
  })

  /**
   * `tasksOverdue ⊂ tasksWaiting`, which the server states and the screen has
   * to respect: two tiles would read as two populations and invite a reader to
   * add them to four.
   */
  it('shows overdue as a caption on the waiting card, because it is a subset of it', async () => {
    const wrapper = await render()

    expect(wrapper.find('[data-testid="tasks-waiting"]').text()).toContain('1 past its date')
    expect(wrapper.find('[data-testid="tasks-overdue"]').exists()).toBe(false)
  })

  it('says nothing is late when nothing is', async () => {
    onSummary = () => ({ status: 200, body: itemBody(summary({ tasksOverdue: 0 })) })

    const wrapper = await render()

    expect(wrapper.find('[data-testid="tasks-waiting"]').text()).toContain('Nothing late')
  })

  /**
   * §3.4's third state. A dashboard that renders two zeroes and no sentence
   * reads as a page that failed to load.
   */
  it('says so plainly when there is nothing waiting', async () => {
    onSummary = () => ({
      status: 200,
      body: itemBody(
        summary({ tasksWaiting: 0, tasksOverdue: 0, draftDocuments: 0, pendingTasks: [] }),
      ),
    })

    const wrapper = await render()

    expect(wrapper.find('[data-testid="empty"]').exists()).toBe(true)
  })

  it('offers a retry when the summary fails', async () => {
    onSummary = () => ({ status: 500, body: errorBody('INTERNAL_ERROR', 'nope') })

    const wrapper = await render()

    expect(wrapper.find('[data-testid="error"]').exists()).toBe(true)
    expect(wrapper.find('[data-testid="error"]').text()).toContain('Try again')
  })

  /**
   * **The dashboard is the home route and stays permission-exempt**
   * (`router/index.spec.ts`), so a caller without the grant has to land
   * somewhere that explains itself rather than on a 403 they did not ask for.
   *
   * It also asserts the request is never made, which is the half that matters:
   * a page that renders an explanation *and* fires a doomed request has not
   * degraded, it has hidden an error.
   */
  // -------------------------------------------------------------------------
  // FR-RPT-002 — the pending-task widget (#432)
  // -------------------------------------------------------------------------

  it('lists the tasks the server says are waiting', async () => {
    onSummary = () => ({
      status: 200,
      body: itemBody(
        summary({
          tasksWaiting: 2,
          pendingTasks: [task(), task({ id: 'b', taskName: 'Countersign the order' })],
        }),
      ),
    })

    const wrapper = await render()
    const rows = wrapper.findAll('[data-testid="pending-task"]')

    expect(rows).toHaveLength(2)
    expect(rows[0].text()).toContain('Approve the request')
    expect(rows[0].text()).toContain('Laptop for the new analyst')
    expect(rows[1].text()).toContain('Countersign the order')
  })

  /**
   * **The rows render in the order they arrive.**
   *
   * The server sends the top of the caller's inbox, in the inbox's own order,
   * so a person who reads the card and then opens their queue finds the same
   * rows at the top in the same sequence. A card that sorted them again would
   * be a second opinion about which work matters most, taken by a widget rather
   * than by the screen that owns the queue.
   */
  it('keeps the order the server sent', async () => {
    onSummary = () => ({
      status: 200,
      body: itemBody(
        summary({
          tasksWaiting: 3,
          pendingTasks: [
            task({ id: 'a', taskName: 'Newest' }),
            task({ id: 'b', taskName: 'Middle', isOverdue: true, dueAt: '2026-01-01T00:00:00Z' }),
            task({ id: 'c', taskName: 'Oldest' }),
          ],
        }),
      ),
    })

    const wrapper = await render()
    const names = wrapper
      .findAll('[data-testid="pending-task"]')
      .map((row) => row.text().split('\n')[0].trim())

    expect(names[0]).toContain('Newest')
    expect(names[1]).toContain('Middle')
    expect(names[2]).toContain('Oldest')
  })

  /**
   * **`pendingTasks.length` is not the count** (#432 AC5).
   *
   * The server caps the list at five and sends `tasksWaiting` for the whole
   * queue, so a card that counted its own rows would be wrong by exactly the
   * number of tasks the reader cannot see — and would say so most confidently
   * to the busiest person.
   */
  it('says how many more are waiting than it shows', async () => {
    onSummary = () => ({
      status: 200,
      body: itemBody(
        summary({
          tasksWaiting: 12,
          pendingTasks: [1, 2, 3, 4, 5].map((n) => task({ id: `t${n}` })),
        }),
      ),
    })

    const wrapper = await render()

    expect(wrapper.findAll('[data-testid="pending-task"]')).toHaveLength(5)
    expect(wrapper.find('[data-testid="pending-more"]').text()).toContain('7 more waiting')
  })

  it('does not say there are more when it is showing all of them', async () => {
    onSummary = () => ({
      status: 200,
      body: itemBody(summary({ tasksWaiting: 1, pendingTasks: [task()] })),
    })

    const wrapper = await render()

    expect(wrapper.find('[data-testid="pending-more"]').exists()).toBe(false)
  })

  /**
   * **#432 AC4.** An empty card is indistinguishable from one that failed to
   * load, so the widget owes the reader a sentence rather than a blank.
   *
   * The fixture leaves a draft in place deliberately: this asserts the
   * *widget's* empty state, not the page-level one, which only appears when
   * there is nothing at all.
   */
  it('says nothing is waiting rather than showing an empty card', async () => {
    onSummary = () => ({
      status: 200,
      body: itemBody(summary({ tasksWaiting: 0, tasksOverdue: 0, pendingTasks: [] })),
    })

    const wrapper = await render()

    expect(wrapper.find('[data-testid="empty"]').exists()).toBe(false)
    expect(wrapper.find('[data-testid="pending-empty"]').text()).toContain('Nothing is waiting')
    expect(wrapper.findAll('[data-testid="pending-task"]')).toHaveLength(0)
  })

  /**
   * **`isOverdue` is read, never derived** (#185 AC4).
   *
   * The row below is late by the server's answer and carries **no** due date
   * the browser could have judged for itself, and the badge still appears. A
   * card comparing `dueAt` to `Date.now()` would show nothing here, which is
   * what makes this the assertion that catches the derivation rather than a
   * test that happens to agree with it.
   */
  it('marks a task late because the server said so, not because of the clock', async () => {
    onSummary = () => ({
      status: 200,
      body: itemBody(
        summary({ tasksWaiting: 1, pendingTasks: [task({ isOverdue: true, dueAt: null })] }),
      ),
    })

    const wrapper = await render()

    expect(wrapper.find('[data-testid="pending-task-overdue"]').exists()).toBe(true)
  })

  /**
   * **A delegate has to be told whose approval it is** (#184).
   *
   * Delegation is the case that fails silently: a widget that dropped it shows
   * somebody five tasks with no reason they are theirs.
   */
  it('says whose work a delegated task is', async () => {
    onSummary = () => ({
      status: 200,
      body: itemBody(
        summary({
          tasksWaiting: 1,
          pendingTasks: [
            task({ delegatedFromUserId: 'x', delegatedFromDisplayName: 'Ani Wijaya' }),
          ],
        }),
      ),
    })

    const wrapper = await render()

    expect(wrapper.find('[data-testid="pending-task-delegated"]').text()).toContain(
      "On Ani Wijaya's behalf",
    )
  })

  /**
   * **The rows are shown; only the links are withheld.**
   *
   * The summary is served behind `reporting:dashboard:read` alone, because
   * every row on it is the caller's own waiting work (ADR-0039). The task
   * screen holds `workflow:task:read`, so a viewer can legitimately have the
   * dashboard and not the inbox — and a link would take them to `/forbidden`.
   *
   * This is a courtesy, not a control: it hides a link that would not work, and
   * it hides nothing the server sent.
   */
  it('shows the rows without links when the caller cannot open the inbox', async () => {
    signIn(['reporting:dashboard:read'])

    const wrapper = await render()

    expect(wrapper.find('[data-testid="pending-task"]').text()).toContain('Approve the request')
    expect(wrapper.find('[data-testid="pending-task"] a').exists()).toBe(false)
  })

  it('links each row to its task when the caller can open the inbox', async () => {
    signIn(['reporting:dashboard:read', 'workflow:task:read'])

    const wrapper = await render()

    expect(wrapper.find('[data-testid="pending-task"] a').exists()).toBe(true)
  })

  it('explains itself instead of erroring when the caller has no grant', async () => {
    signIn(['document:read'])

    const wrapper = await render()

    expect(wrapper.find('[data-testid="no-permission"]').exists()).toBe(true)
    expect(wrapper.find('[data-testid="no-permission"]').text()).toContain(
      'reporting:dashboard:read',
    )
    expect(requested).toHaveLength(0)
  })

  // -------------------------------------------------------------------------
  // FR-RPT-003 — the recent-documents widget (#433)
  // -------------------------------------------------------------------------

  /**
   * **Shows what the caller touched last, in the order the server chose.**
   *
   * The rows and their sequence are the server's answer: most recent touch
   * first, by the `lastTouchedAt` each row carries. A card that re-sorted would
   * be a second opinion about what *recent* means, and the fixture is
   * deliberately given rows whose `updatedAt` runs the **other way** — so a
   * component sorting on the field that looks interchangeable fails here.
   */
  it('lists the documents the caller touched, newest touch first', async () => {
    onSummary = () => ({
      status: 200,
      body: itemBody(
        summary({
          recentDocuments: [
            recentDocument({
              id: '0199a1a0-0000-7000-8000-0000000000d1',
              title: 'Touched most recently',
              lastTouchedAt: '2026-09-12T09:00:00Z',
              updatedAt: '2026-01-01T00:00:00Z',
            }),
            recentDocument({
              id: '0199a1a0-0000-7000-8000-0000000000d2',
              title: 'Touched a while ago',
              lastTouchedAt: '2026-09-01T09:00:00Z',
              updatedAt: '2026-12-01T00:00:00Z',
            }),
          ],
        }),
      ),
    })

    const wrapper = await render()
    const rows = wrapper.findAll('[data-testid="recent-document"]')

    expect(rows).toHaveLength(2)
    expect(rows[0].text()).toContain('Touched most recently')
    expect(rows[1].text()).toContain('Touched a while ago')
  })

  /**
   * **Says so in words when the caller has touched nothing** (#433 AC5).
   *
   * The third state §3.4 asks for, and the reason it is a sentence rather than
   * an absence: a card with nothing in it is indistinguishable from one that
   * failed to load, and the reader has no way to tell which they are looking
   * at.
   */
  it('says nothing has been worked on rather than showing an empty card', async () => {
    onSummary = () => ({
      status: 200,
      body: itemBody(summary({ recentDocuments: [] })),
    })

    const wrapper = await render()

    expect(wrapper.find('[data-testid="recent-documents"]').exists()).toBe(true)
    expect(wrapper.find('[data-testid="recent-empty"]').text()).toContain(
      'have not worked on any documents',
    )
    expect(wrapper.findAll('[data-testid="recent-document"]')).toHaveLength(0)
  })

  /**
   * **The rows render without links when the caller cannot open documents.**
   *
   * The summary is served behind `reporting:dashboard:read` alone, because every
   * row on it is the caller's own work (ADR-0039) — so a viewer can legitimately
   * be shown the documents they acted on and still not hold `document:read`,
   * whose route would answer 403.
   *
   * **This is a courtesy and not a control**: it hides a link that would not
   * work, and hides nothing the server sent. So the assertion is that the title
   * is still on screen, not merely that the anchor is gone.
   */
  it('shows the rows without links when the caller cannot open documents', async () => {
    signIn(['reporting:dashboard:read'])
    onSummary = () => ({
      status: 200,
      body: itemBody(
        summary({ recentDocuments: [recentDocument({ title: 'Still legible' })] }),
      ),
    })

    const wrapper = await render()
    const card = wrapper.find('[data-testid="recent-documents"]')

    expect(card.text()).toContain('Still legible')
    expect(card.findAll('a')).toHaveLength(0)
  })

  /** And the other half, so the assertion above is about the grant. */
  it('links the rows when the caller holds document:read', async () => {
    signIn(['reporting:dashboard:read', 'document:read'])
    onSummary = () => ({
      status: 200,
      body: itemBody(
        summary({ recentDocuments: [recentDocument({ title: 'Openable' })] }),
      ),
    })

    const wrapper = await render()
    const card = wrapper.find('[data-testid="recent-documents"]')

    expect(card.text()).toContain('Openable')
    expect(card.findAll('a').length).toBeGreaterThan(0)
  })

  /**
   * **The widget adds no request of its own** (#433 AC1, ADR-0039).
   *
   * The decision this row was most likely to reverse, asserted rather than
   * assumed: its rows genuinely are documents, so the two things that would
   * have served them are a second endpoint and a RAD list definition. **Neither
   * is called.** The page still makes exactly one request on sign-in, which is
   * the whole of what one contract buys.
   */
  it('asks for no second endpoint and renders no list definition', async () => {
    onSummary = () => ({
      status: 200,
      body: itemBody(summary({ recentDocuments: [recentDocument()] })),
    })

    await render()

    // Every request the page made, not merely the ones it was expected to: a
    // second endpoint would show up here as an extra entry.
    expect(requested).toEqual([expect.stringContaining('/dashboard/summary')])
    expect(requested.some((url) => url.includes('/documents'))).toBe(false)
    expect(requested.some((url) => url.includes('/rad/lists'))).toBe(false)
  })

  /**
   * **The date shown is `lastTouchedAt` and not the document's own timestamps.**
   *
   * `updatedAt` is the field that looks interchangeable and is not: it moves
   * when **anybody** changes the document, so a card reading it would answer
   * *what changed recently among things I have touched* — a different question
   * from the one the card is named after. The fixture sets the two years apart
   * so only one of them can be on screen.
   */
  it('shows when the caller last touched it rather than when it last changed', async () => {
    onSummary = () => ({
      status: 200,
      body: itemBody(
        summary({
          recentDocuments: [
            recentDocument({
              lastTouchedAt: '2026-09-12T09:00:00Z',
              updatedAt: '2024-03-04T05:06:00Z',
            }),
          ],
        }),
      ),
    })

    const wrapper = await render()
    const touched = wrapper.find('[data-testid="recent-touched"]').text()

    expect(touched).toContain('2026')
    expect(touched).not.toContain('2024')
  })

  /**
   * **The row carries the document's own identifiers**, because it is a
   * `DocumentSummary` rather than a widget shape — the same argument the
   * pending-task rows make for being `InboxTask`s.
   *
   * `documentNumber` where there is one, falling back to `documentRef` for a
   * draft that has not been numbered yet, which is what the document list does.
   */
  it('identifies a row by its number, or by its reference before it has one', async () => {
    onSummary = () => ({
      status: 200,
      body: itemBody(
        summary({
          recentDocuments: [
            recentDocument({
              id: '0199a1a0-0000-7000-8000-0000000000d3',
              documentNumber: 'PR-2026-000009',
              documentRef: 'DOC-2026-000009',
            }),
            recentDocument({
              id: '0199a1a0-0000-7000-8000-0000000000d4',
              documentNumber: null,
              documentRef: 'DOC-2026-000010',
            }),
          ],
        }),
      ),
    })

    const wrapper = await render()
    const rows = wrapper.findAll('[data-testid="recent-document"]')

    expect(rows[0].text()).toContain('PR-2026-000009')
    expect(rows[1].text()).toContain('DOC-2026-000010')
  })
})
