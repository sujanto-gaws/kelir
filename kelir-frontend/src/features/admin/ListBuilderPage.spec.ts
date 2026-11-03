import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'

import ListBuilderPage from './ListBuilderPage.vue'
import { registerSessionBridge } from '@/api/session'
import {
  errorBody,
  installFakeBackend,
  type FakeBackendHandle,
  type FakeReply,
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
  /** What `PUT /rad/lists/{id}` answers when set; the stored definition when not. */
  let saveReply: FakeReply | null
  let router: Router

  beforeEach(() => {
    setActivePinia(createPinia())
    window.localStorage.clear()

    current = definition()
    renderResponse = { status: 200, body: { success: true, data: { id: LIST_ID, columns: [] } } }
    saveReply = null

    backend = installFakeBackend((request: RecordedRequest) => {
      if (request.url.includes('/by-key/')) {
        return renderResponse
      }

      if (request.method === 'put' && saveReply) {
        return saveReply
      }

      return { status: 200, body: { success: true, data: current } }
    })

    router = createRouter({
      history: createMemoryHistory(),
      routes: [{ path: '/admin/lists/:id', name: 'admin-list-builder', component: blank }],
    })
  })

  afterEach(() => {
    backend.restore()
    registerSessionBridge(null)
  })

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

  /*
   * #641, #672: a refused save says why, under the input it is about or in
   * the list above the form, and never twice. The details are
   * `rad::domain::list`'s (`bounded`, `check_page_size`, `check_columns`,
   * `check_filters`) and `extract::detail`'s, as `validate_update_list` sends
   * them. The render check's answer stays its own channel.
   */
  describe('a refused save (#641, #672)', () => {
    type Detail = [path: string, rule: string, code: string, message: string]

    function refusal(...details: Detail[]): FakeReply {
      return {
        status: 422,
        body: errorBody(
          'VALIDATION_ERROR',
          'Validation failed',
          details.map(([path, rule, code, message]) => ({ path, rule, code, message })),
        ),
      }
    }

    const TITLE_REQUIRED: Detail = ['title', 'required', 'REQUIRED', 'title is required']
    const PAGE_SIZE_OUT_OF_RANGE: Detail = [
      'pageSize',
      'range',
      'OUT_OF_RANGE',
      'pageSize must be between 1 and 100',
    ]
    const COLUMN_LABEL_REQUIRED: Detail = [
      'columns.1.label',
      'required',
      'REQUIRED',
      'columns.1.label is required',
    ]
    const FILTER_LABEL_TOO_LONG: Detail = [
      'filters.0.label',
      'maxLength',
      'TOO_LONG',
      'filters.0.label must be at most 200 characters',
    ]
    const COLUMN_KEY_DUPLICATE: Detail = [
      'columns.1.columnKey',
      'unique',
      'DUPLICATE',
      '`title` appears more than once in this list',
    ]
    const FILTER_KEY_REQUIRED: Detail = [
      'filters.0.filterKey',
      'required',
      'REQUIRED',
      'filters.0.filterKey is required',
    ]
    const STATUS_UNKNOWN: Detail = [
      'status',
      'type',
      'INVALID_TYPE',
      'Unknown variant `RETIRED`, expected one of `DRAFT`, `ACTIVE`, `DEPRECATED`',
    ]
    const UNKNOWN_FIELD: Detail = [
      'unknownField',
      'unknownField',
      'UNKNOWN_FIELD',
      'Unknown field `unknownField`. Check the spelling and casing — the API is camelCase.',
    ]

    /** The four inputs #672 gave a slot, by test id, and the detail each shows. */
    const NEW_SLOTS = [
      ['title', 'list-title', TITLE_REQUIRED],
      ['page size', 'list-page-size', PAGE_SIZE_OUT_OF_RANGE],
      ['second column’s label', 'column-label-1', COLUMN_LABEL_REQUIRED],
      ['first filter’s label', 'filter-label-0', FILTER_LABEL_TOO_LONG],
    ] as const

    /** Every input that can be marked, for checking that only one is. */
    const MARKABLE = [
      'list-title',
      'list-page-size',
      'column-label-0',
      'column-label-1',
      'filter-label-0',
    ]

    async function save(wrapper: VueWrapper): Promise<void> {
      await wrapper.get('[data-testid="save-list"]').trigger('click')
      await flushPromises()
    }

    function list(wrapper: VueWrapper) {
      return wrapper.find('[data-testid="list-builder-unplaced-errors"]')
    }

    function listed(wrapper: VueWrapper): string[] {
      return list(wrapper)
        .findAll('li')
        .map((item) => item.text())
    }

    function shown(wrapper: VueWrapper, message: string): number {
      return wrapper.text().split(message).length - 1
    }

    function renderChecks(): number {
      return backend.requests.filter((request) => request.url.includes('/by-key/')).length
    }

    /** The text of the row (`column-N`, `filter-N`) a message sits in. */
    function rowText(wrapper: VueWrapper, testId: string): string {
      return wrapper.get(`[data-testid="${testId}"]`).text()
    }

    it('lists a detail at a path it shows no message for, and does not take the save', async () => {
      saveReply = refusal(UNKNOWN_FIELD)
      const wrapper = await render()
      const checks = renderChecks()

      await save(wrapper)

      expect(list(wrapper).attributes('role')).toBe('alert')
      expect(listed(wrapper)).toEqual([`unknownField: ${UNKNOWN_FIELD[3]}`])
      // A refused save is not followed by the render check a stored one is.
      expect(renderChecks()).toBe(checks)
    })

    it('lists a refusal of the status, which is a closed choice', async () => {
      saveReply = refusal(STATUS_UNKNOWN)
      const wrapper = await render()

      await save(wrapper)

      expect(listed(wrapper)).toEqual([`status: ${STATUS_UNKNOWN[3]}`])
    })

    it.each(NEW_SLOTS)(
      'places a refusal of the %s under its input, named by it',
      async (_, testId, detail) => {
        saveReply = refusal(detail)
        const wrapper = await render()

        await save(wrapper)

        const input = wrapper.get(`[data-testid="${testId}"]`)
        const message = wrapper.get(`[data-testid="${testId}-message"]`)

        expect(message.text()).toBe(detail[3])
        expect(input.attributes('aria-invalid')).toBe('true')
        expect(input.attributes('aria-describedby')).toBe(message.attributes('id'))
        expect(wrapper.find(`[id="${message.attributes('id')}"]`).element).toBe(message.element)
        // Drawn beside the input it describes, not beside another.
        expect(message.element.parentElement?.contains(input.element)).toBe(true)

        for (const other of MARKABLE.filter((candidate) => candidate !== testId)) {
          const control = wrapper.get(`[data-testid="${other}"]`)

          expect(control.attributes('aria-invalid')).toBe('false')
          expect(control.attributes('aria-describedby')).toBeUndefined()
        }

        expect(shown(wrapper, detail[3])).toBe(1)
        expect(list(wrapper).exists()).toBe(false)
        expect(wrapper.find('[data-testid="save-error"]').exists()).toBe(false)
      },
    )

    it('shows a cleared title’s REQUIRED under the title', async () => {
      saveReply = refusal(TITLE_REQUIRED)
      const wrapper = await render()

      await wrapper.get('[data-testid="list-title"]').setValue('   ')
      await save(wrapper)

      const put = backend.requests.find((request) => request.method === 'put')

      expect((put?.body as { title: string }).title).toBe('')
      expect(wrapper.get('[data-testid="list-title-message"]').text()).toBe('title is required')
      expect(wrapper.get('[data-testid="list-title"]').attributes('aria-invalid')).toBe('true')
    })

    it('still places a refusal of a column’s or a filter’s key under its row', async () => {
      saveReply = refusal(COLUMN_KEY_DUPLICATE, FILTER_KEY_REQUIRED)
      const wrapper = await render()

      await save(wrapper)

      expect(rowText(wrapper, 'column-1')).toContain(COLUMN_KEY_DUPLICATE[3])
      expect(rowText(wrapper, 'filter-0')).toContain(FILTER_KEY_REQUIRED[3])
      expect(list(wrapper).exists()).toBe(false)
    })

    it('lists a detail for a row that is not there', async () => {
      saveReply = refusal(
        ['columns.2.label', 'required', 'REQUIRED', 'columns.2.label is required'],
        ['columns.2.columnKey', 'required', 'REQUIRED', 'columns.2.columnKey is required'],
        ['filters.1.label', 'required', 'REQUIRED', 'filters.1.label is required'],
        ['filters.1.filterKey', 'required', 'REQUIRED', 'filters.1.filterKey is required'],
      )
      const wrapper = await render()

      await save(wrapper)

      // Two columns and one filter are drawn; index 2 and index 1 are not.
      expect(listed(wrapper)).toEqual([
        'columns.2.label: columns.2.label is required',
        'columns.2.columnKey: columns.2.columnKey is required',
        'filters.1.label: filters.1.label is required',
        'filters.1.filterKey: filters.1.filterKey is required',
      ])
    })

    it('places a detail for a row added since, and lists it once the row is removed', async () => {
      saveReply = refusal([
        'columns.2.label',
        'required',
        'REQUIRED',
        'columns.2.label is required',
      ])
      const wrapper = await render()

      await wrapper.get('[data-testid="add-column"]').trigger('click')
      await save(wrapper)

      expect(wrapper.get('[data-testid="column-label-2-message"]').text()).toBe(
        'columns.2.label is required',
      )
      expect(list(wrapper).exists()).toBe(false)

      await wrapper.get('[data-testid="column-remove-2"]').trigger('click')

      expect(listed(wrapper)).toEqual(['columns.2.label: columns.2.label is required'])
    })

    it('shows a placed and an unplaced detail once each, each in its own place', async () => {
      saveReply = refusal(PAGE_SIZE_OUT_OF_RANGE, UNKNOWN_FIELD)
      const wrapper = await render()

      await save(wrapper)

      expect(wrapper.get('[data-testid="list-page-size-message"]').text()).toBe(
        PAGE_SIZE_OUT_OF_RANGE[3],
      )
      expect(shown(wrapper, PAGE_SIZE_OUT_OF_RANGE[3])).toBe(1)
      expect(listed(wrapper)).toEqual([`unknownField: ${UNKNOWN_FIELD[3]}`])
    })

    it('does not also say "Validation failed" when the details say why', async () => {
      saveReply = refusal(TITLE_REQUIRED, UNKNOWN_FIELD)
      const wrapper = await render()

      await save(wrapper)

      expect(wrapper.find('[data-testid="save-error"]').exists()).toBe(false)
      expect(wrapper.text()).not.toContain('Validation failed')
    })

    it.each([
      ['a 422 naming no field', refusal(), 'Validation failed'],
      [
        'a 403',
        {
          status: 403,
          body: errorBody('FORBIDDEN', 'You do not have permission to perform this action'),
        },
        'You do not have permission to perform this action',
      ],
      [
        'a 409',
        { status: 409, body: errorBody('CONFLICT', 'list was changed by another request') },
        'list was changed by another request',
      ],
      [
        'a network failure',
        { status: 0, networkError: true },
        'Could not reach the server. Check your connection and try again.',
      ],
    ] as [string, FakeReply, string][])(
      'shows %s as one message, and no list',
      async (_, failure, message) => {
        saveReply = failure
        const wrapper = await render()

        await save(wrapper)

        expect(wrapper.get('[data-testid="save-error"]').text()).toBe(message)
        expect(shown(wrapper, message)).toBe(1)
        expect(list(wrapper).exists()).toBe(false)
      },
    )

    it('clears the last refusal when Save is pressed again', async () => {
      saveReply = refusal(TITLE_REQUIRED, UNKNOWN_FIELD)
      const wrapper = await render()

      await save(wrapper)
      expect(list(wrapper).exists()).toBe(true)
      expect(wrapper.find('[data-testid="list-title-message"]').exists()).toBe(true)

      saveReply = null
      await save(wrapper)

      expect(list(wrapper).exists()).toBe(false)
      expect(wrapper.find('[data-testid="list-title-message"]').exists()).toBe(false)
      expect(wrapper.get('[data-testid="list-title"]').attributes('aria-invalid')).toBe('false')
    })

    it('clears the last message when Save is pressed again', async () => {
      saveReply = { status: 403, body: errorBody('FORBIDDEN', 'Not yours to change') }
      const wrapper = await render()

      await save(wrapper)
      expect(wrapper.find('[data-testid="save-error"]').exists()).toBe(true)

      saveReply = null
      await save(wrapper)

      expect(wrapper.find('[data-testid="save-error"]').exists()).toBe(false)
    })

    /*
     * The render check's details are their own channel: they stay in the
     * render panel, a key's slot still reads them, and the save's refusal of
     * the same path is the one shown under the input.
     */
    describe('beside the render check', () => {
      const NOT_RENDERABLE = '`form_data.` names no field'

      beforeEach(() => {
        renderResponse = {
          status: 422,
          body: errorBody('VALIDATION_FAILED', 'This list cannot be drawn', [
            {
              path: 'columns.1.columnKey',
              rule: 'columnKey',
              code: 'COLUMN_NOT_RENDERABLE',
              message: NOT_RENDERABLE,
            },
          ]),
        }
      })

      it('still shows a render detail under the key it names', async () => {
        const wrapper = await render()

        expect(rowText(wrapper, 'column-1')).toContain(NOT_RENDERABLE)
        expect(wrapper.get('[data-testid="render-broken"]').text()).toContain(NOT_RENDERABLE)
        expect(list(wrapper).exists()).toBe(false)
      })

      it('shows the save’s refusal of a key before the render check’s', async () => {
        saveReply = refusal(COLUMN_KEY_DUPLICATE)
        const wrapper = await render()

        await save(wrapper)

        const row = rowText(wrapper, 'column-1')

        expect(row).toContain(COLUMN_KEY_DUPLICATE[3])
        expect(row).not.toContain(NOT_RENDERABLE)
        // The panel keeps the render check's answer, which the save did not change.
        expect(wrapper.get('[data-testid="render-broken"]').text()).toContain(NOT_RENDERABLE)
        expect(list(wrapper).exists()).toBe(false)
      })

      it('lists no render detail, and keeps the panel, when a save is refused', async () => {
        saveReply = refusal(UNKNOWN_FIELD)
        const wrapper = await render()

        await save(wrapper)

        expect(listed(wrapper)).toEqual([`unknownField: ${UNKNOWN_FIELD[3]}`])
        expect(wrapper.get('[data-testid="render-broken"]').text()).toContain(NOT_RENDERABLE)
      })
    })

    /*
     * test-engineer, row 22's gate. The cases above reach the column key's
     * render placement only; AC2 keeps all three. The render details are
     * `render::plan`'s own (`plan_filters`, `plan_sort`), under the code
     * `AppError::validation` answers with.
     */
    describe('the other two render placements, and shown once', () => {
      function cannotBeDrawn(path: string, rule: string, code: string, message: string) {
        return {
          status: 422,
          body: errorBody('VALIDATION_ERROR', 'Validation failed', [{ path, rule, code, message }]),
        }
      }

      const FILTER_NOT_RENDERABLE =
        '`stage` is not something a document list can be filtered by, so this control would do nothing — the parameters are `search`, `status`, `priority`, `entityType`, `entityId`'
      const SORT_NOT_RENDERABLE =
        'this list declares 2 sort keys and a document list is ordered by one — the rest would be silently ignored'

      it('still shows a render detail under the filter key it names', async () => {
        current = definition({
          filters: [{ filterKey: 'stage', label: 'Stage', filterType: 'TEXT', isDefault: false }],
        })
        renderResponse = cannotBeDrawn(
          'filters.0.filterKey',
          'filterKey',
          'FILTER_NOT_RENDERABLE',
          FILTER_NOT_RENDERABLE,
        )
        const wrapper = await render()

        expect(rowText(wrapper, 'filter-0')).toContain(FILTER_NOT_RENDERABLE)
        expect(wrapper.get('[data-testid="render-broken"]').text()).toContain(FILTER_NOT_RENDERABLE)
        expect(list(wrapper).exists()).toBe(false)
      })

      it('still shows a render detail under the opening sort', async () => {
        current = definition({
          defaultSort: [
            { key: 'title', dir: 'asc' },
            { key: 'createdAt', dir: 'desc' },
          ],
        })
        renderResponse = cannotBeDrawn(
          'defaultSort',
          'defaultSort',
          'SORT_NOT_RENDERABLE',
          SORT_NOT_RENDERABLE,
        )
        const wrapper = await render()

        const sortBlock = wrapper.get('[data-testid="sort-key"]').element.parentElement

        expect(sortBlock?.textContent).toContain(SORT_NOT_RENDERABLE)
        expect(list(wrapper).exists()).toBe(false)
      })

      // AC6 beside a broken render panel: the panel is the render check's,
      // and a save's refusal is not repeated in it.
      it('says a save’s refusal once while the render panel is up', async () => {
        renderResponse = cannotBeDrawn(
          'filters.0.filterKey',
          'filterKey',
          'FILTER_NOT_RENDERABLE',
          FILTER_NOT_RENDERABLE,
        )
        saveReply = refusal(COLUMN_KEY_DUPLICATE)
        const wrapper = await render()

        await save(wrapper)

        expect(rowText(wrapper, 'column-1')).toContain(COLUMN_KEY_DUPLICATE[3])
        expect(shown(wrapper, COLUMN_KEY_DUPLICATE[3])).toBe(1)
        expect(wrapper.get('[data-testid="render-broken"]').text()).not.toContain(
          COLUMN_KEY_DUPLICATE[3],
        )
      })
    })

    // test-engineer: a column's and a filter's label refused at one index,
    // so a slot reading the other collection's path is seen.
    it('places a column’s and a filter’s label refusal at one index, each in its own row', async () => {
      const COLUMN_LABEL_0: Detail = [
        'columns.0.label',
        'required',
        'REQUIRED',
        'columns.0.label is required',
      ]
      saveReply = refusal(COLUMN_LABEL_0, FILTER_LABEL_TOO_LONG)
      const wrapper = await render()

      await save(wrapper)

      expect(wrapper.get('[data-testid="column-label-0-message"]').text()).toBe(COLUMN_LABEL_0[3])
      expect(wrapper.get('[data-testid="filter-label-0-message"]').text()).toBe(
        FILTER_LABEL_TOO_LONG[3],
      )
      expect(shown(wrapper, COLUMN_LABEL_0[3])).toBe(1)
      expect(shown(wrapper, FILTER_LABEL_TOO_LONG[3])).toBe(1)
      expect(list(wrapper).exists()).toBe(false)
    })
  })
})
