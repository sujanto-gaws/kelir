import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'

import DocumentListPage from './DocumentListPage.vue'
import { installFakeBackend, type FakeBackendHandle } from '@/lib/testing/fake-backend'
import { useAuthStore } from '@/stores/auth'
import type { CurrentUser } from '@/types/auth'

/**
 * The document list (#171).
 *
 * **What is asserted here is the client's half and only the client's half.**
 * The visibility rule is enforced in the backend's query and
 * `kelir-backend/tests/documents_list.rs` is what holds it — a spec that
 * asserted "the list does not show another tenant's documents" against a fake
 * backend would be asserting that a stub returned what it was told to.
 *
 * What the client is responsible for is the part the backend cannot see: the
 * URL is the state, every filter goes on the wire rather than narrowing a
 * fetched page, and a blank filter is an absent parameter rather than an empty
 * one.
 */

const blank = { template: '<div />' }

function row(overrides: Record<string, unknown> = {}): unknown {
  return {
    id: '0199a1a0-0000-7000-8000-0000000000d1',
    documentRef: 'DOC-2026-000001',
    documentNumber: null,
    documentTypeId: '0199a1a0-0000-7000-8000-0000000000t1',
    documentTypeCode: 'PURCHASE_REQUISITION',
    title: 'Two standing desks',
    status: 'DRAFT',
    priority: 'NORMAL',
    entityType: null,
    entityId: null,
    submittedAt: null,
    createdAt: '2026-08-27T00:00:00Z',
    updatedAt: '2026-08-27T00:00:00Z',
    ...overrides,
  }
}

/**
 * The most recent request the fake backend saw.
 *
 * `Array.prototype.at` is ES2022 and this project's `lib` target predates it;
 * indexing by length is the same read without a `tsconfig` change that would
 * affect every file for the sake of one spec.
 */
function last<T>(items: T[]): T | undefined {
  return items[items.length - 1]
}

function principal(permissions: string[]): CurrentUser {
  return {
    id: '0199a1a0-0000-7000-8000-0000000000u1',
    username: 'clerk',
    displayName: 'Clerk',
    email: 'clerk@example.test',
    roles: [],
    permissions,
  }
}

describe('DocumentListPage', () => {
  let backend: FakeBackendHandle
  let router: Router
  let rows: unknown[]

  beforeEach(() => {
    setActivePinia(createPinia())
    window.localStorage.clear()

    rows = [row()]

    backend = installFakeBackend(() => ({
      status: 200,
      body: { success: true, data: rows, meta: { page: 1, pageSize: 20, total: rows.length } },
    }))

    router = createRouter({
      history: createMemoryHistory(),
      routes: [
        { path: '/documents', name: 'documents', component: blank },
        { path: '/documents/new', name: 'new-document', component: blank },
        { path: '/documents/:id', name: 'document', component: blank },
      ],
    })
  })

  afterEach(() => backend.restore())

  function lastRequest() {
    return last(backend.requests)
  }

  async function render(query = ''): Promise<VueWrapper> {
    await router.push(`/documents${query}`)
    await router.isReady()

    const wrapper = mount(DocumentListPage, { global: { plugins: [router] } })

    for (let round = 0; round < 4; round += 1) {
      await flushPromises()
    }

    return wrapper
  }

  it('sends the URL filters to the server rather than narrowing a fetched page', async () => {
    // AC1 and the reason `listDocuments` exists: a client that fetched a
    // population and filtered it would make `meta.total` disagree with the rows
    // under it, and on this surface it would also be a second visibility rule.
    await render('?status=SUBMITTED&search=desks')

    const request = lastRequest()

    expect(request?.params).toMatchObject({ status: 'SUBMITTED', search: 'desks' })
  })

  it('sends no parameter for a filter that is blank', async () => {
    // `?status=` is not in the backend's vocabulary and is a 422. An empty
    // select means "no filter", which is an absent parameter.
    await render('?status=')

    const request = lastRequest()

    expect(request?.params).not.toHaveProperty('status')
  })

  it('refuses a page number the URL cannot mean, rather than sending it', async () => {
    // `?page=0` and `?page=nonsense` are things a URL can contain and neither is
    // a page. #122 is open because the backend answers those outside the error
    // envelope, so clamping here keeps a bare 400 off a screen somebody reached
    // by editing the address bar — and does not close #122.
    await render('?page=0')

    const request = lastRequest()

    expect(request?.params?.page).toBe(1)
  })

  it('shows a draft by its reference, because it has no number yet', async () => {
    const wrapper = await render()

    expect(wrapper.get('[data-testid="document-row-DOC-2026-000001"]').text()).toContain(
      'DOC-2026-000001',
    )
  })

  it('offers New document only to a caller who may create one', async () => {
    // Cosmetic, and the comment on the button says so: the backend re-checks
    // every request and is the only thing that decides. What this asserts is
    // that a caller who would be refused is not offered the button, because a
    // button that always answers 403 says the product is broken rather than
    // that this person may not do it.
    const auth = useAuthStore()

    auth.user = principal(['document:read'])
    let wrapper = await render()
    expect(wrapper.find('[data-testid="new-document"]').exists()).toBe(false)

    auth.user = principal(['document:read', 'document:create'])
    wrapper = await render()
    expect(wrapper.find('[data-testid="new-document"]').exists()).toBe(true)
  })

  it('says nothing matches rather than rendering an empty table', async () => {
    rows = []

    const wrapper = await render()

    expect(wrapper.find('[data-testid="documents-empty"]').exists()).toBe(true)
    expect(wrapper.find('[data-testid="documents-table"]').exists()).toBe(false)
  })
})
