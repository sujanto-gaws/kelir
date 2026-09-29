import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'

import NewDocumentPage from './NewDocumentPage.vue'
import {
  installFakeBackend,
  itemBody,
  type FakeBackendHandle,
  type RecordedRequest,
} from '@/lib/testing/fake-backend'
import { filler, searchedPage } from '@/lib/testing/searched-page'

/**
 * Choosing the type a document is raised from (#167, #525).
 *
 * The chooser read one page of a hundred types and kept the active ones, so a
 * type that sorted past the hundredth row could not be raised at all — the
 * audit behind #525 turned six browser tests red by seeding 105 types ahead of
 * theirs. These tests do the same against a fake that searches and filters by
 * status as `GET /document-types` does.
 */

const blank = { template: '<div />' }

function type(n: string, overrides: Record<string, unknown> = {}) {
  return {
    id: `t-${n}`,
    typeCode: `AA_${n}`,
    name: `Filler ${n}`,
    category: null,
    formId: `f-${n}`,
    status: 'ACTIVE',
    createdAt: '2026-09-01T00:00:00Z',
    updatedAt: '2026-09-01T00:00:00Z',
    ...overrides,
  }
}

const wanted = type('zz', { typeCode: 'ZZ_REQUISITION', name: 'Requisition' })
const formless = type('zy', { typeCode: 'ZY_UNBOUND', name: 'Not ready', formId: null })

describe('NewDocumentPage', () => {
  let backend: FakeBackendHandle
  let router: Router
  let types: ReturnType<typeof type>[]

  beforeEach(() => {
    setActivePinia(createPinia())

    types = [
      ...filler(110, (n) => type(n)),
      ...filler(8, (n) => type(`d${n}`, { status: 'DEPRECATED' })),
      formless,
      wanted,
    ]

    backend = installFakeBackend((request: RecordedRequest) => {
      if (request.method === 'post') {
        return { status: 201, body: itemBody({ id: 'doc-1' }) }
      }

      return searchedPage(request, types, ['typeCode', 'name'])
    })

    router = createRouter({
      history: createMemoryHistory(),
      routes: [
        { path: '/documents/new', name: 'new-document', component: blank },
        { path: '/documents/:id', name: 'document', component: blank },
      ],
    })
  })

  afterEach(() => backend.restore())

  async function render(): Promise<VueWrapper> {
    await router.push('/documents/new')
    await router.isReady()

    const wrapper = mount(NewDocumentPage, { global: { plugins: [router] } })
    await flushPromises()

    return wrapper
  }

  async function search(wrapper: VueWrapper, text: string): Promise<void> {
    const box = wrapper.get('[data-testid="document-type-search"]')
    await box.setValue(text)
    await box.trigger('keydown', { key: 'Enter' })
    await flushPromises()
  }

  it('asks the server for active types, and says there are more than it shows', async () => {
    const wrapper = await render()

    expect(backend.requests[0].params).toEqual({ pageSize: 100, status: 'ACTIVE' })
    expect(wrapper.findAll('input[type="radio"]')).toHaveLength(100)
    expect(wrapper.get('[data-testid="document-type-status"]').text()).toContain(
      'Showing 100 of 112',
    )
    expect(wrapper.find(`[data-testid="type-${wanted.typeCode}"]`).exists()).toBe(false)
  })

  it('raises a document from a type that sorts past the first hundred', async () => {
    const wrapper = await render()

    await search(wrapper, 'zz_req')

    expect(backend.requests.slice(-1)[0]?.params).toEqual({
      search: 'zz_req',
      pageSize: 100,
      status: 'ACTIVE',
    })

    const row = wrapper.get(`[data-testid="type-${wanted.typeCode}"]`)
    expect(row.text()).toContain('Requisition')

    await row.get('input[type="radio"]').setValue(true)
    await wrapper.get('[data-testid="new-document-title"]').setValue('Two standing desks')
    await wrapper.get('[data-testid="create-document"]').trigger('click')
    await flushPromises()

    const posted = backend.requests.find((request) => request.method === 'post')

    expect(posted?.url).toBe('/documents')
    expect(posted?.body).toEqual({ documentTypeId: wanted.id, title: 'Two standing desks' })
    expect(router.currentRoute.value.fullPath).toBe('/documents/doc-1')
  })

  it('shows a type with no form, greyed and explained, and does not let it be chosen', async () => {
    const wrapper = await render()

    await search(wrapper, 'ZY_')

    const row = wrapper.get(`[data-testid="type-${formless.typeCode}"]`)

    expect(row.text()).toContain('no form bound to it yet')
    expect((row.get('input[type="radio"]').element as HTMLInputElement).disabled).toBe(true)
  })

  it('says no type is active when the server has none', async () => {
    types = [type('01', { status: 'DRAFT' })]

    const wrapper = await render()

    expect(wrapper.find('[data-testid="no-types"]').exists()).toBe(true)
    expect(
      (wrapper.get('[data-testid="create-document"]').element as HTMLButtonElement).disabled,
    ).toBe(true)
  })
})
