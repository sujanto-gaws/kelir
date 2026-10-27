import { flushPromises, mount } from '@vue/test-utils'
import { defineComponent, h, ref } from 'vue'
import { describe, expect, it } from 'vitest'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'

import { queryFromRoute, useListRoute, type ListRoute, type ListRouteOptions } from './useListRoute'
import { useQueryBackedList, type ListFetchQuery } from './useQueryBackedList'
import type { Page } from '@/types/api'

interface Row {
  id: string
}

const blank = { template: '<div />' }

/** What the list was asked for, one entry a load, and the page each answers. */
interface Harness {
  router: Router
  calls: ListFetchQuery[]
  listRoute: ListRoute
  /** The view a routed list is on; changing it changes the route's params. */
  view: ReturnType<typeof ref<string>>
}

async function mountList(
  path: string,
  options: ListRouteOptions = {},
  answer: (query: ListFetchQuery) => Page<Row> = (query) => ({
    items: [{ id: 'a' }],
    meta: { page: Number(query.page), pageSize: 20, total: 100 },
  }),
): Promise<Harness> {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/rows/:view', name: 'rows', component: blank },
      { path: '/row/:id', name: 'row', component: blank },
    ],
  })
  await router.push(path)
  await router.isReady()

  const calls: ListFetchQuery[] = []
  const view = ref(String(router.currentRoute.value.params.view))
  let listRoute: ListRoute | undefined

  const ListPage = defineComponent({
    setup() {
      const list = useQueryBackedList<Row>((query) => {
        calls.push(query)
        return Promise.resolve(answer(query))
      })
      listRoute = useListRoute(
        list,
        () => ({ name: 'rows', params: { view: view.value } }),
        options,
      )
      return () => h('div')
    },
  })

  mount(ListPage, { global: { plugins: [router] } })
  await flushPromises()

  return { router, calls, listRoute: listRoute as ListRoute, view }
}

describe('queryFromRoute', () => {
  it('keeps the first value of each key and drops the blank ones', () => {
    expect(
      queryFromRoute({ search: 'acme', status: ['ACTIVE', 'INACTIVE'], page: '', type: null }),
    ).toEqual({ search: 'acme', status: 'ACTIVE' })
  })
})

describe('useListRoute', () => {
  it('loads the page and the search a deep link names, once', async () => {
    const { calls } = await mountList('/rows/a?page=3&search=acme')

    expect(calls).toEqual([{ page: 3, pageSize: 20, search: 'acme' }])
  })

  it('writes a change to the URL, back on page 1, and loads from what the URL says', async () => {
    const { router, calls, listRoute } = await mountList('/rows/a?page=3&search=acme')

    listRoute.navigate({ status: 'ACTIVE' })
    await flushPromises()

    expect(router.currentRoute.value.query).toEqual({ search: 'acme', status: 'ACTIVE' })
    expect(calls[calls.length - 1]).toEqual({
      page: 1,
      pageSize: 20,
      search: 'acme',
      status: 'ACTIVE',
    })
  })

  it('removes a key whose new value is blank', async () => {
    const { router, listRoute } = await mountList('/rows/a?search=acme&status=ACTIVE')

    listRoute.navigate({ search: '' })
    await flushPromises()

    expect(router.currentRoute.value.query).toEqual({ status: 'ACTIVE' })
  })

  it('keeps the page when asked to', async () => {
    const { router, listRoute } = await mountList('/rows/a?page=3&search=acme')

    listRoute.navigate({ sort: 'code' }, false)
    await flushPromises()

    expect(router.currentRoute.value.query).toEqual({ page: '3', search: 'acme', sort: 'code' })
  })

  it.each([
    [0, '1'],
    [4, '4'],
    [9, '5'],
  ])('holds page %i between the first page and the last', async (requested, landed) => {
    // A hundred rows, twenty a page: five pages.
    const { router, listRoute } = await mountList('/rows/a?page=2')

    listRoute.goToPage(requested)
    await flushPromises()

    expect(router.currentRoute.value.query.page).toBe(landed)
  })

  it('loads again when the params change and the query does not', async () => {
    // The master-data views are one route: changing the view changes only
    // the params, and the rows must be the new view's.
    const { router, calls, view } = await mountList('/rows/a')

    view.value = 'b'
    await router.push('/rows/b')
    await flushPromises()

    expect(calls).toHaveLength(2)
  })

  it('does not load when the URL changes and what the list is asked for does not', async () => {
    const { router, calls, listRoute } = await mountList('/rows/a?search=acme', {
      exclude: ['open'],
    })

    await router.push('/rows/a?search=acme&open=row-1')
    await flushPromises()
    await router.push('/rows/a?search=acme')
    await flushPromises()

    expect(calls).toHaveLength(1)
    expect(listRoute.currentQuery.value).toEqual({ search: 'acme' })
  })

  it('leaves the excluded keys out of the list and drops them on a change', async () => {
    const { router, calls, listRoute } = await mountList('/rows/a?search=acme&open=row-1', {
      exclude: ['open'],
    })

    expect(listRoute.currentQuery.value).toEqual({ search: 'acme', open: 'row-1' })
    expect(listRoute.listQuery.value).toEqual({ search: 'acme' })
    expect(calls[0]).toEqual({ page: 1, pageSize: 20, search: 'acme' })

    listRoute.goToPage(2)
    await flushPromises()

    expect(router.currentRoute.value.query).toEqual({ search: 'acme', page: '2' })
  })

  it('does not load the route the page is being left for', async () => {
    const { router, calls } = await mountList('/rows/a?search=acme')

    await router.push('/row/1')
    await flushPromises()

    expect(calls).toHaveLength(1)
  })

  it('loads nothing itself when the page loads the list', async () => {
    const { router, calls, listRoute } = await mountList('/rows/a?search=acme', { load: false })

    expect(calls).toHaveLength(0)

    listRoute.navigate({ search: 'other' })
    await flushPromises()

    expect(router.currentRoute.value.query).toEqual({ search: 'other' })
    expect(calls).toHaveLength(0)

    await listRoute.refresh()

    expect(calls).toEqual([{ page: 1, pageSize: 20, search: 'other' }])
  })

  it('steps back a page when a delete empties the last one', async () => {
    const { router, calls, listRoute } = await mountList('/rows/a?page=5')

    await listRoute.refresh(true)
    await flushPromises()

    expect(router.currentRoute.value.query).toEqual({ page: '4' })
    expect(calls[calls.length - 1]).toMatchObject({ page: 4 })
  })

  it('reads the first page again when a delete empties it', async () => {
    // There is no page to step back to, and `?page=1` in the URL would be a
    // change of address for the same rows.
    const { router, calls, listRoute } = await mountList('/rows/a?search=acme')

    await listRoute.refresh(true)

    expect(router.currentRoute.value.query).toEqual({ search: 'acme' })
    expect(calls).toHaveLength(2)
    expect(calls[1]).toMatchObject({ page: 1, search: 'acme' })
  })

  it('reads the same page again after a delete that leaves rows on it', async () => {
    const { router, calls, listRoute } = await mountList('/rows/a?page=2', {}, (query) => ({
      items: [{ id: 'a' }, { id: 'b' }],
      meta: { page: Number(query.page), pageSize: 20, total: 100 },
    }))

    await listRoute.refresh(true)

    expect(router.currentRoute.value.query).toEqual({ page: '2' })
    expect(calls).toHaveLength(2)
    expect(calls[1]).toMatchObject({ page: 2 })
  })
})
