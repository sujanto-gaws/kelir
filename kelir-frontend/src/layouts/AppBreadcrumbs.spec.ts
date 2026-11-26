import { createPinia, setActivePinia } from 'pinia'
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { beforeEach, describe, expect, it } from 'vitest'
import { defineComponent, h, ref, watch } from 'vue'
import { createMemoryHistory, createRouter, RouterView, useRoute, type Router } from 'vue-router'

import { useBreadcrumbName } from '@/composables/useBreadcrumbName'

import AppBreadcrumbs from './AppBreadcrumbs.vue'

/**
 * The trail as the shell draws it, with a page that publishes its record's
 * name the way the eight record pages do (#968 AC5, AC6).
 */

interface Doc {
  id: string
  title: string
}

/** One pending load per record id, settled by the test. */
let loads: Map<string, { resolve: (doc: Doc) => void; reject: (error: Error) => void }>

/**
 * Like `DocumentWorkspace`: the previous record stays in place while the next
 * one loads, and a failed load leaves nothing.
 */
const RecordPage = defineComponent({
  setup() {
    const route = useRoute()
    const record = ref<Doc | null>(null)

    watch(
      () => String(route.params.id ?? ''),
      async (id) => {
        try {
          record.value = await new Promise<Doc>((resolve, reject) =>
            loads.set(id, { resolve, reject }),
          )
        } catch {
          record.value = null
        }
      },
      { immediate: true },
    )

    useBreadcrumbName(() => record.value && { key: record.value.id, name: record.value.title })

    return () => h('div', { 'data-testid': 'page' })
  },
})

/** The shell's arrangement: the trail, then the page. */
const Shell = { render: () => h('div', [h(AppBreadcrumbs), h(RouterView)]) }

const blank = { render: () => h('div') }

let router: Router

async function open(path: string): Promise<VueWrapper> {
  await router.push(path)
  await router.isReady()
  const wrapper = mount(Shell, { global: { plugins: [router] } })
  await flushPromises()
  return wrapper
}

function crumbs(wrapper: VueWrapper): string[] {
  return wrapper.findAll('nav[aria-label="Breadcrumb"] li').map((item) => item.text())
}

function current(wrapper: VueWrapper): string | undefined {
  return crumbs(wrapper).slice(-1)[0]
}

beforeEach(() => {
  setActivePinia(createPinia())
  loads = new Map()
  router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/', name: 'dashboard', component: blank, meta: { title: 'Dashboard' } },
      {
        path: '/documents',
        name: 'documents',
        component: blank,
        meta: { title: 'Documents', breadcrumb: {} },
      },
      {
        path: '/documents/:id',
        name: 'document',
        component: RecordPage,
        meta: { title: 'Document', breadcrumb: { parent: 'documents', record: 'id' } },
      },
    ],
  })
})

describe('AppBreadcrumbs markup (AC4, AC6)', () => {
  it('is a navigation landmark named Breadcrumb holding an ordered list', async () => {
    const wrapper = await open('/documents/doc-a')
    const nav = wrapper.get('nav')

    expect(nav.attributes('aria-label')).toBe('Breadcrumb')
    expect(nav.find('ol').exists()).toBe(true)
    expect(crumbs(wrapper)).toEqual(['Dashboard', 'Documents', 'Document'])
  })

  it('links the earlier crumbs and marks the last as the current page, unlinked', async () => {
    const wrapper = await open('/documents/doc-a')
    const items = wrapper.findAll('li')

    expect(items[0]?.get('a').attributes('href')).toBe('/')
    expect(items[1]?.get('a').attributes('href')).toBe('/documents')
    expect(items[2]?.find('a').exists()).toBe(false)
    expect(items[2]?.get('[aria-current="page"]').text()).toBe('Document')
    expect(wrapper.findAll('[aria-current]')).toHaveLength(1)
  })

  it('hides the separators from assistive technology', async () => {
    const wrapper = await open('/documents/doc-a')
    const separators = wrapper.findAll('[data-testid="breadcrumb-separator"]')

    expect(separators).toHaveLength(2)
    for (const separator of separators) {
      expect(separator.attributes('aria-hidden')).toBe('true')
    }
  })

  it('draws nothing where there is no trail', async () => {
    const wrapper = await open('/')

    expect(wrapper.find('nav').exists()).toBe(false)
  })
})

describe('a record name the page publishes (AC5)', () => {
  it('shows the label while loading, then the name, never the id', async () => {
    const wrapper = await open('/documents/doc-a')

    expect(current(wrapper)).toBe('Document')
    expect(wrapper.text()).not.toContain('doc-a')

    loads.get('doc-a')?.resolve({ id: 'doc-a', title: 'Purchase of chairs' })
    await flushPromises()

    expect(current(wrapper)).toBe('Purchase of chairs')
    expect(wrapper.get('[aria-current="page"]').attributes('title')).toBe('Purchase of chairs')
  })

  it('keeps the label when the load fails', async () => {
    const wrapper = await open('/documents/doc-a')

    loads.get('doc-a')?.reject(new Error('404'))
    await flushPromises()

    expect(current(wrapper)).toBe('Document')
  })

  it('shows no name from the previous record while the next one loads', async () => {
    const wrapper = await open('/documents/doc-a')
    loads.get('doc-a')?.resolve({ id: 'doc-a', title: 'Purchase of chairs' })
    await flushPromises()

    await router.push('/documents/doc-b')
    await flushPromises()

    expect(current(wrapper)).toBe('Document')

    loads.get('doc-b')?.resolve({ id: 'doc-b', title: 'Travel to Surabaya' })
    await flushPromises()

    expect(current(wrapper)).toBe('Travel to Surabaya')
  })

  it('withdraws the name when the page goes', async () => {
    const wrapper = await open('/documents/doc-a')
    loads.get('doc-a')?.resolve({ id: 'doc-a', title: 'Purchase of chairs' })
    await flushPromises()

    await router.push('/documents')
    await flushPromises()
    // Back to the same record: a fresh page, still loading, publishes nothing yet.
    await router.push('/documents/doc-a')
    await flushPromises()

    expect(current(wrapper)).toBe('Document')
  })

  it('renders a name as text, never as HTML', async () => {
    const wrapper = await open('/documents/doc-a')
    loads.get('doc-a')?.resolve({ id: 'doc-a', title: '<b>bold</b>' })
    await flushPromises()

    expect(wrapper.find('nav b').exists()).toBe(false)
    expect(current(wrapper)).toBe('<b>bold</b>')
  })
})
