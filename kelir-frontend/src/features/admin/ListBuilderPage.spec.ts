import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'

import ListBuilderPage from './ListBuilderPage.vue'
import {
  errorBody,
  installFakeBackend,
  type FakeBackendHandle,
  type RecordedRequest,
} from '@/lib/testing/fake-backend'
import { useAuthStore } from '@/stores/auth'
import type { CurrentUser } from '@/types/auth'

/**
 * The list builder (#374).
 *
 * **The property this screen exists for is not that it edits a definition.** It
 * is that an author learns their definition cannot be drawn — which
 * [SDD](../../../docs/design/01.%20System%20Design%20Document.md) §8.2.4 made
 * this screen's job when it accepted that the storage API stores a broken
 * definition happily: *the builder calls the same function to show an author
 * the problem before they save*.
 *
 * So what is asserted here is the three answers the render endpoint can give
 * and that each is shown as what it is. Whether a column key resolves is the
 * backend's question and `kelir-backend/tests/rad_list_render.rs` holds it.
 *
 * # Seen to fail (coding standard §2.9)
 *
 * Four mutations, run 2026-09-08. Three red first time; the fourth was green,
 * and the fixture comment above says why.
 *
 * | Mutation | Reddened |
 * |---|---|
 * | The save stops calling `askWhetherItDraws` | *asks whether the list draws, after every save* |
 * | The `LIST_NOT_BOUND` branch is dropped, so an unbound list reads as broken | *reports a list nothing binds as its own state* |
 * | `save` sends `columns` and not `filters` | *sends both collections, because either alone replaces the other* |
 * | `sortableColumns` drops its `SORTABLE` check | **green first** — the payload column was `isSortable: false`, so the allow-list was never reached. Red once the fixture marked it `true`: *offers only sortable columns as the opening sort* |
 *
 * **The fourth is the second green mutation this sprint**, after
 * `rad_list_render.rs`'s declared-sort fixture ([#376]). Both were the same
 * shape: a fixture in which two conditions coincide, so the test passes for a
 * reason other than the one it claims.
 */

const LIST_ID = '0199a1a0-0000-7000-8000-0000000000c1'
const blank = { template: '<div />' }

function definition(overrides: Record<string, unknown> = {}) {
  return {
    id: LIST_ID,
    listKey: 'purchase_requisitions',
    title: 'Purchase requisitions',
    entityId: null,
    defaultSort: [{ key: 'title', dir: 'asc' }],
    pageSize: 20,
    status: 'ACTIVE',
    columns: [
      { columnKey: 'title', label: 'Subject', isSortable: true },
      // **`isSortable: true` on a payload column, deliberately.** The
      // definition is allowed to say it and `render::plan` overrules it — *a
      // `form_data.*` column is never sortable however the definition marks
      // it*, because ordering by a JSONB path needs an index nothing creates.
      // A fixture that set `false` here would be excluded by the `isSortable`
      // check alone and would never exercise the allow-list: the mutation
      // dropping `SORTABLE.has` was run on 2026-09-08 against exactly that
      // fixture and came back **green**.
      { columnKey: 'form_data.amount', label: 'Amount', isSortable: true },
    ],
    filters: [{ filterKey: 'status', label: 'Status', filterType: 'ENUM', isDefault: false }],
    createdAt: '2026-09-08T00:00:00Z',
    updatedAt: '2026-09-08T00:00:00Z',
    ...overrides,
  }
}

function principal(permissions: string[]): CurrentUser {
  return {
    id: '0199a1a0-0000-7000-8000-0000000000u1',
    username: 'admin',
    displayName: 'Administrator',
    email: 'admin@example.test',
    roles: [],
    permissions,
  }
}

const EVERY_PERMISSION = ['rad:list:read', 'rad:list:update']

describe('ListBuilderPage', () => {
  let backend: FakeBackendHandle
  let current: Record<string, unknown>
  /** What `GET /rad/lists/by-key/{key}` answers. */
  let renderResponse: { status: number; body: unknown }
  let router: Router

  beforeEach(() => {
    setActivePinia(createPinia())
    window.localStorage.clear()

    current = definition()
    renderResponse = { status: 200, body: { success: true, data: { id: LIST_ID, columns: [] } } }

    backend = installFakeBackend((request: RecordedRequest) => {
      if (request.url.includes('/by-key/')) {
        return renderResponse
      }

      return { status: 200, body: { success: true, data: current } }
    })

    router = createRouter({
      history: createMemoryHistory(),
      routes: [{ path: '/admin/lists/:id', name: 'admin-list-builder', component: blank }],
    })
  })

  afterEach(() => backend.restore())

  async function render(permissions: string[] = EVERY_PERMISSION): Promise<VueWrapper> {
    useAuthStore().$patch({ user: principal(permissions) })

    await router.push(`/admin/lists/${LIST_ID}`)
    await router.isReady()

    const wrapper = mount(ListBuilderPage, { global: { plugins: [router] } })

    for (let round = 0; round < 6; round += 1) {
      await flushPromises()
    }

    return wrapper
  }

  it('edits a definition, column by column', async () => {
    const wrapper = await render()

    expect(wrapper.find('[data-testid="column-0"]').exists()).toBe(true)
    expect(wrapper.find('[data-testid="column-1"]').exists()).toBe(true)
    expect(wrapper.find('[data-testid="filter-0"]').exists()).toBe(true)
  })

  /**
   * **§8.2.4's condition, met.** The storage API accepted the definition; the
   * question the author needs answered is whether anybody can open it, and
   * this is where they find out.
   */
  it('shows the key the server could not resolve, rather than an empty table', async () => {
    renderResponse = {
      status: 422,
      body: errorBody('VALIDATION_FAILED', 'This list cannot be drawn', [
        {
          path: 'columns.1.columnKey',
          rule: 'columnKey',
          code: 'COLUMN_NOT_RENDERABLE',
          message: '`form_data.` names no field',
        },
      ]),
    }

    const wrapper = await render()

    const panel = wrapper.find('[data-testid="render-broken"]')

    expect(panel.exists()).toBe(true)
    expect(panel.text()).toContain('form_data.')
    expect(panel.text()).toContain('columns.1.columnKey')
  })

  /**
   * **A list nothing binds is not a broken list.** It is the ordinary state of
   * one just authored, and reporting it as an error teaches authors to ignore
   * the panel — which would cost the panel its only job.
   *
   * **The response shape here is the server's, and the first version of this
   * test invented one.** It faked a detail-less 404, the code branched on
   * *has details*, and both agreed with each other and with nothing else:
   * `require_bound` answers `AppError::validation` with a `LIST_NOT_BOUND`
   * detail on `listId`, so every freshly authored list landed in the broken
   * panel. The browser flow caught it on the first CI run of #385; this spec
   * had passed. Copied from `document::service::list::require_bound` rather
   * than from memory.
   */
  it('reports a list nothing binds as its own state', async () => {
    renderResponse = {
      status: 422,
      body: errorBody('VALIDATION_FAILED', 'This list has no rows to show', [
        {
          path: 'listId',
          rule: 'binding',
          code: 'LIST_NOT_BOUND',
          message: 'no document type in this tenant names this list',
        },
      ]),
    }

    const wrapper = await render()

    expect(wrapper.find('[data-testid="render-unbound"]').exists()).toBe(true)
    expect(wrapper.find('[data-testid="render-broken"]').exists()).toBe(false)
  })

  it('asks whether the list draws, after every save', async () => {
    const wrapper = await render()
    const before = backend.requests.filter((request) => request.url.includes('/by-key/')).length

    await wrapper.find('[data-testid="save-list"]').trigger('click')
    await flushPromises()

    const after = backend.requests.filter((request) => request.url.includes('/by-key/')).length

    expect(after).toBeGreaterThan(before)
  })

  /**
   * **A collection that is sent replaces the stored set wholesale**, which the
   * backend's own doc comment says — so sending one would delete the other.
   */
  it('sends both collections, because either alone replaces the other', async () => {
    const wrapper = await render()

    await wrapper.find('[data-testid="save-list"]').trigger('click')
    await flushPromises()

    const put = backend.requests.find((request) => request.method === 'put')
    const sent = put?.body as { columns?: unknown[]; filters?: unknown[] }

    expect(sent.columns).toHaveLength(2)
    expect(sent.filters).toHaveLength(1)
  })

  /**
   * `plan_sort` resolves the declared sort against a column the query can
   * order by, and a `form_data.` path never is — ordering by a JSONB path needs
   * an index nothing creates.
   */
  it('offers only sortable columns as the opening sort', async () => {
    const wrapper = await render()
    const options = wrapper.find('[data-testid="sort-key"]').findAll('option')
    const values = options.map((option) => option.attributes('value'))

    expect(values).toContain('title')
    expect(values).not.toContain('form_data.amount')
  })

  it('offers no edit to a caller who may only read', async () => {
    const wrapper = await render(['rad:list:read'])

    expect(wrapper.find('[data-testid="save-list"]').exists()).toBe(false)
    expect(wrapper.find('[data-testid="add-column"]').attributes('disabled')).toBeDefined()
  })
})
