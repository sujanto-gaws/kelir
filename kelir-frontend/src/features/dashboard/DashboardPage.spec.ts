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
 * The dashboard (FR-RPT-001, [#431]).
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
 * [#431]: https://github.com/sujanto-gaws/kelir/issues/431
 */

const USER_ID = '0199a1a0-0000-7000-8000-0000000000f9'

function summary(overrides: Record<string, unknown> = {}): unknown {
  return { tasksWaiting: 3, tasksOverdue: 1, draftDocuments: 2, ...overrides }
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
      body: itemBody(summary({ tasksWaiting: 0, tasksOverdue: 0, draftDocuments: 0 })),
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
  it('explains itself instead of erroring when the caller has no grant', async () => {
    signIn(['document:read'])

    const wrapper = await render()

    expect(wrapper.find('[data-testid="no-permission"]').exists()).toBe(true)
    expect(wrapper.find('[data-testid="no-permission"]').text()).toContain(
      'reporting:dashboard:read',
    )
    expect(requested).toHaveLength(0)
  })
})
