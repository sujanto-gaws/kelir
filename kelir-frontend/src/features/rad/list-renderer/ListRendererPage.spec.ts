import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'

import ListRendererPage from './ListRendererPage.vue'
import {
  errorBody,
  installFakeBackend,
  itemBody,
  type FakeBackendHandle,
  type RecordedRequest,
} from '@/lib/testing/fake-backend'

/**
 * The dynamic list renderer (#340).
 *
 * **What is asserted here is that the screen comes from the definition and
 * nothing else** — AC1's *every part of it comes from the definition*, made
 * against a fake backend that can hand over any definition at all. That is the
 * assertion a browser flow cannot afford to repeat once per shape.
 *
 * **What is deliberately not asserted here:** which columns a definition may
 * declare, which filters the query understands, and which rows a caller may
 * see. All three live in the backend — `domain/render.rs` and the documents
 * query — and a spec that checked them against a stub would be asserting that
 * a fake returned what it was told to.
 */

const blank = { template: '<div />' }

function column(overrides: Record<string, unknown> = {}) {
  return {
    key: 'title',
    label: 'Title',
    dataType: null,
    format: null,
    width: null,
    sortable: true,
    ...overrides,
  }
}

function definition(overrides: Record<string, unknown> = {}) {
  return {
    id: '0199a1a0-0000-7000-8000-0000000000l1',
    listKey: 'requisitions',
    title: 'Requisitions',
    pageSize: 20,
    columns: [column()],
    filters: [],
    defaultSortKey: 'createdAt',
    defaultSortDescending: true,
    ...overrides,
  }
}

function row(cells: Record<string, unknown>, id = '0199a1a0-0000-7000-8000-0000000000d1') {
  return { id, cells }
}

function last<T>(items: T[]): T | undefined {
  return items[items.length - 1]
}

describe('ListRendererPage', () => {
  let backend: FakeBackendHandle
  let router: Router
  /** What `GET /rad/lists/by-key/…` answers. */
  let listReply: { status: number; body: unknown }
  let rows: unknown[]
  let actions: unknown[]

  beforeEach(() => {
    setActivePinia(createPinia())
    window.localStorage.clear()

    listReply = { status: 200, body: itemBody(definition()) }
    rows = [row({ title: 'Two standing desks' })]
    actions = []

    backend = installFakeBackend((request: RecordedRequest) => {
      if (request.url.includes('/rad/lists/by-key/')) {
        return listReply
      }

      if (request.url.includes('/rad/actions')) {
        return {
          status: 200,
          body: { success: true, data: actions, meta: { page: 1, pageSize: 20, total: actions.length } },
        }
      }

      return {
        status: 200,
        body: { success: true, data: rows, meta: { page: 1, pageSize: 20, total: rows.length } },
      }
    })

    router = createRouter({
      history: createMemoryHistory(),
      routes: [
        { path: '/lists/:listKey', name: 'rendered-list', component: blank },
        { path: '/documents/:id', name: 'document', component: blank },
      ],
    })
  })

  afterEach(() => backend.restore())

  function rowRequests() {
    return backend.requests.filter((request) => request.url.includes('/rows'))
  }

  async function render(query = '', listKey = 'requisitions'): Promise<VueWrapper> {
    await router.push(`/lists/${listKey}${query}`)
    await router.isReady()

    const wrapper = mount(ListRendererPage, { global: { plugins: [router] } })

    for (let round = 0; round < 6; round += 1) {
      await flushPromises()
    }

    return wrapper
  }

  // -- AC1: every part of it comes from the definition -------------------

  it('renders the columns the definition declares, in its order, with its labels', async () => {
    listReply = {
      status: 200,
      body: itemBody(
        definition({
          columns: [
            column({ key: 'documentNumber', label: 'Number' }),
            column({ key: 'title', label: 'Subject' }),
          ],
        }),
      ),
    }

    const wrapper = await render()
    const headers = wrapper.findAll('th').map((header) => header.text())

    expect(headers[0]).toContain('Number')
    expect(headers[1]).toContain('Subject')
  })

  it('renders a different definition as a different table, with no change here', async () => {
    // The whole claim of a *dynamic* renderer: nothing on the page names a
    // column, so a definition nobody anticipated produces a table anyway.
    listReply = {
      status: 200,
      body: itemBody(
        definition({ columns: [column({ key: 'form_data.amount', label: 'Amount' })] }),
      ),
    }
    rows = [row({ 'form_data.amount': 1234.5 })]

    const wrapper = await render()

    expect(wrapper.find('[data-testid="column-form_data.amount"]').text()).toContain('Amount')
    expect(wrapper.find('[data-testid="cell-form_data.amount"]').text()).toContain('1234.5')
  })

  it('reads each cell by the definition key rather than by position', async () => {
    // The cells arrive as an object. A renderer that walked them positionally
    // would put the right values in the wrong columns the moment the server
    // serialised them in another order.
    listReply = {
      status: 200,
      body: itemBody(
        definition({
          columns: [column({ key: 'title' }), column({ key: 'documentNumber', label: 'No.' })],
        }),
      ),
    }
    rows = [row({ documentNumber: 'PR-1', title: 'Desks' })]

    const wrapper = await render()

    expect(wrapper.find('[data-testid="cell-title"]').text()).toBe('Desks')
    expect(wrapper.find('[data-testid="cell-documentNumber"]').text()).toBe('PR-1')
  })

  it('renders the title the definition carries', async () => {
    listReply = { status: 200, body: itemBody(definition({ title: 'Open requisitions' })) }

    const wrapper = await render()

    expect(wrapper.find('[data-testid="list-title"]').text()).toBe('Open requisitions')
  })

  // -- AC2: sort and paging come from the definition ---------------------

  it('opens on the definition default sort without pinning it into the URL', async () => {
    // The default is the author's decision and it moves when they move it. A
    // URL that carried it would freeze whatever it was on the day somebody
    // copied the link.
    listReply = {
      status: 200,
      body: itemBody(definition({ defaultSortKey: 'title', defaultSortDescending: false })),
    }

    const wrapper = await render()

    expect(wrapper.find('[data-testid="column-title"]').attributes('aria-sort')).toBe('ascending')
    expect(last(rowRequests())?.params).not.toHaveProperty('sort')
  })

  it('sends the sort the URL asks for', async () => {
    await render('?sort=title&dir=desc')

    expect(last(rowRequests())?.params).toMatchObject({ sort: 'title', dir: 'desc' })
  })

  it('sends no pageSize, because the definition decides it', async () => {
    // A client that could widen the page would be overruling the configuration
    // it had just read.
    await render()

    expect(last(rowRequests())?.params).not.toHaveProperty('pageSize')
  })

  it('does not offer sorting on a column the server did not mark sortable', async () => {
    listReply = {
      status: 200,
      body: itemBody(
        definition({ columns: [column({ key: 'form_data.amount', sortable: false })] }),
      ),
    }

    const wrapper = await render()

    expect(wrapper.find('[data-testid="sort-form_data.amount"]').exists()).toBe(false)
  })

  // -- AC3: only the declared filters ------------------------------------

  it('offers exactly the filters the definition declares', async () => {
    listReply = {
      status: 200,
      body: itemBody(
        definition({
          filters: [
            {
              key: 'stage',
              label: 'Stage',
              filterType: 'ENUM',
              options: null,
              isDefault: false,
              parameter: 'status',
            },
          ],
        }),
      ),
    }

    const wrapper = await render()

    expect(wrapper.find('[data-testid="filter-stage"]').exists()).toBe(true)
    // The parameter the filter sets is the server's business; the control is
    // named by the definition's own key.
    expect(wrapper.find('[data-testid="filter-status"]').exists()).toBe(false)
  })

  it('offers no filter at all when the definition declares none', async () => {
    const wrapper = await render()

    expect(wrapper.findAll('[data-testid^="filter-"]')).toHaveLength(0)
  })

  it('sends a filter by the definition key rather than by the parameter', async () => {
    // The server maps `stage` to `status`. A client that sent `status` would be
    // carrying a copy of that mapping, and a definition that renamed its filter
    // would break it.
    listReply = {
      status: 200,
      body: itemBody(
        definition({
          filters: [
            {
              key: 'stage',
              label: 'Stage',
              filterType: 'TEXT',
              options: null,
              isDefault: false,
              parameter: 'status',
            },
          ],
        }),
      ),
    }

    await render('?stage=SUBMITTED')

    expect(last(rowRequests())?.params).toMatchObject({ stage: 'SUBMITTED' })
    expect(last(rowRequests())?.params).not.toHaveProperty('status')
  })

  // -- AC4: a failure is named, never an empty table ---------------------

  it('renders the refusal instead of the table when the definition cannot be drawn', async () => {
    listReply = {
      status: 422,
      body: errorBody('VALIDATION_ERROR', 'Validation failed', [
        {
          path: 'columns.0.columnKey',
          rule: 'columnKey',
          code: 'COLUMN_NOT_RENDERABLE',
          message: '`supplier_rating` is neither a field of a document nor a `form_data.` path',
        },
      ]),
    }

    const wrapper = await render()

    const refusal = wrapper.find('[data-testid="list-refusal"]')

    expect(refusal.exists()).toBe(true)
    expect(refusal.text()).toContain('supplier_rating')
    // **Instead of, not beside.** A refusal next to an empty table lets
    // somebody read the table.
    expect(wrapper.find('[data-testid="rendered-list"]').exists()).toBe(false)
  })

  it('renders the refusal when the list is a draft rather than an empty table', async () => {
    listReply = {
      status: 409,
      body: errorBody('CONFLICT', 'list `requisitions` is Draft and only an Active list is rendered', []),
    }

    const wrapper = await render()

    expect(wrapper.find('[data-testid="list-refusal"]').text()).toContain('Draft')
    expect(wrapper.find('[data-testid="rendered-list"]').exists()).toBe(false)
  })

  it('says a page is empty rather than showing nothing, when the definition is fine', async () => {
    // The other side of AC4: a *working* list with no matching documents still
    // has to say so, because a table with no rows and no message is the same
    // silence.
    rows = []

    const wrapper = await render()

    expect(wrapper.find('[data-testid="no-rows"]').exists()).toBe(true)
  })

  // -- Row actions -------------------------------------------------------

  it('renders every action the server returned, and disables none', async () => {
    // Everything returned is invocable: the server drops what the caller may
    // not do. A disabled button would publish an action the permission hides.
    actions = [
      {
        id: '0199a1a0-0000-7000-8000-0000000000a1',
        actionKey: 'export',
        label: 'Export',
        context: 'LIST',
        actionType: 'NAVIGATE',
        config: { route: '/documents/:id' },
      },
    ]

    const wrapper = await render()
    const button = wrapper.find('[data-testid="action-export"]')

    expect(button.exists()).toBe(true)
    expect(button.attributes('disabled')).toBeUndefined()
  })

  it('renders no action column when the catalogue is empty', async () => {
    const wrapper = await render()

    expect(wrapper.text()).not.toContain('Actions')
  })

  it('still renders the list when the action catalogue could not be loaded', async () => {
    // A configuration surface must not be a dependency of a reading one.
    backend.restore()
    backend = installFakeBackend((request: RecordedRequest) => {
      if (request.url.includes('/rad/lists/by-key/')) {
        return listReply
      }

      if (request.url.includes('/rad/actions')) {
        return { status: 403, body: errorBody('FORBIDDEN', 'Forbidden', []) }
      }

      return {
        status: 200,
        body: { success: true, data: rows, meta: { page: 1, pageSize: 20, total: rows.length } },
      }
    })

    const wrapper = await render()

    expect(wrapper.find('[data-testid="rendered-list"]').exists()).toBe(true)
    expect(wrapper.find('[data-testid="list-refusal"]').exists()).toBe(false)
  })
})
