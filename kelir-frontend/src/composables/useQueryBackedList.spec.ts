import { describe, expect, it, vi } from 'vitest'

import { pageFromQuery, useQueryBackedList, DEFAULT_PAGE_SIZE } from './useQueryBackedList'
import { ApiError, CLIENT_ERROR_CODES } from '@/api/error'
import type { Page } from '@/types/api'

interface Row {
  id: string
}

function page(items: Row[], meta?: { page: number; pageSize: number; total: number }): Page<Row> {
  return {
    items,
    meta: meta ?? { page: 1, pageSize: DEFAULT_PAGE_SIZE, total: items.length },
  }
}

/** A fetcher that resolves only when the test says so. */
function deferred(): {
  fetcher: (query: Record<string, unknown>) => Promise<Page<Row>>
  resolve(index: number, value: Page<Row>): void
  calls: Record<string, unknown>[]
} {
  const resolvers: ((value: Page<Row>) => void)[] = []
  const calls: Record<string, unknown>[] = []

  return {
    calls,
    fetcher: (query) => {
      calls.push(query)
      return new Promise<Page<Row>>((resolve) => {
        resolvers.push(resolve)
      })
    },
    resolve: (index, value) => resolvers[index]?.(value),
  }
}

describe('pageFromQuery', () => {
  it('reads a page number a URL can actually carry', () => {
    expect(pageFromQuery('3')).toBe(3)
  })

  it.each(['0', '-3', 'nonsense', '', undefined])(
    'falls back to the first page for %s',
    (value) => {
      // All five are things an address bar can contain and none of them is a
      // page. Sending them would earn a 400 that arrives outside the error
      // envelope (#122), on a screen the user reached by editing the URL.
      expect(pageFromQuery(value)).toBe(1)
    },
  )
})

describe('useQueryBackedList', () => {
  it('sends the paging and the filters the query string named', async () => {
    const calls: Record<string, unknown>[] = []
    const list = useQueryBackedList<Row>((query) => {
      calls.push(query)
      return Promise.resolve(page([{ id: 'a' }]))
    })

    await list.apply({ page: '2', search: 'ACME', statusId: 'PARTY_ENABLED' })

    expect(calls).toEqual([
      { page: 2, pageSize: DEFAULT_PAGE_SIZE, search: 'ACME', statusId: 'PARTY_ENABLED' },
    ])
    expect(list.filters.value).toEqual({ search: 'ACME', statusId: 'PARTY_ENABLED' })
  })

  it('trusts the page size the server says it used, not the one it asked for', async () => {
    // The backend clamps to MAX_PAGE_SIZE rather than refusing, so a pager
    // built on the requested size would render the wrong number of pages
    // (#101 AC4).
    const list = useQueryBackedList<Row>(() =>
      Promise.resolve(page([{ id: 'a' }], { page: 1, pageSize: 100, total: 250 })),
    )

    await list.apply({ pageSize: '5000' })

    expect(list.pageSize.value).toBe(100)
    expect(list.totalPages.value).toBe(3)
    expect(list.hasNext.value).toBe(true)
    expect(list.hasPrevious.value).toBe(false)
  })

  it('tells an empty result from a failed one', async () => {
    // Three states, not two (coding standard §3.4). A screen that showed
    // "nothing to show" over a failed request would be lying about the data.
    const empty = useQueryBackedList<Row>(() => Promise.resolve(page([])))
    await empty.apply({})

    expect(empty.isEmpty.value).toBe(true)
    expect(empty.error.value).toBe('')

    const failing = useQueryBackedList<Row>(() =>
      Promise.reject(new ApiError(CLIENT_ERROR_CODES.network, 'The server did not respond', 0)),
    )
    await failing.apply({})

    expect(failing.isEmpty.value).toBe(false)
    expect(failing.error.value).toBe('The server did not respond')
  })

  it('reports nothing as empty before the first load has happened', async () => {
    // Otherwise a table renders "nothing matches" for the instant before its
    // first request answers, which reads as a result rather than as a wait.
    const list = useQueryBackedList<Row>(() => Promise.resolve(page([{ id: 'a' }])))

    expect(list.isEmpty.value).toBe(false)

    await list.apply({})

    expect(list.isEmpty.value).toBe(false)
  })

  it('drops the rows a failed load cannot vouch for', async () => {
    let fail = false
    const list = useQueryBackedList<Row>(() =>
      fail
        ? Promise.reject(new ApiError(CLIENT_ERROR_CODES.network, 'offline', 0))
        : Promise.resolve(page([{ id: 'a' }], { page: 1, pageSize: 20, total: 1 })),
    )

    await list.apply({})
    expect(list.items.value).toHaveLength(1)

    fail = true
    await list.apply({ search: 'ACME' })

    // Keeping the previous rows under a search that did not run would show the
    // user results for a query the server never saw.
    expect(list.items.value).toEqual([])
    expect(list.total.value).toBe(0)
  })

  it('ignores a response that a later request has already overtaken', async () => {
    // Typing in a search box makes these overlap. The first response arriving
    // last would put results for "AC" on a screen that says "ACME".
    const { fetcher, resolve } = deferred()
    const list = useQueryBackedList<Row>(fetcher)

    const first = list.apply({ search: 'AC' })
    const second = list.apply({ search: 'ACME' })

    resolve(1, page([{ id: 'acme' }]))
    resolve(0, page([{ id: 'stale' }, { id: 'also-stale' }]))
    await Promise.all([first, second])

    expect(list.items.value).toEqual([{ id: 'acme' }])
    expect(list.isLoading.value).toBe(false)
  })

  it('does not narrow what it was given', async () => {
    // The server paginates and the server filters. A composable that filtered
    // locally would make meta.total disagree with the rows under it, which is
    // the failure FR-MDM-008 and NFR-PERF-002 exist to prevent.
    const rows = [{ id: 'a' }, { id: 'b' }, { id: 'c' }]
    const list = useQueryBackedList<Row>(() =>
      Promise.resolve(page(rows, { page: 1, pageSize: 20, total: 3 })),
    )

    await list.apply({ search: 'zzz' })

    expect(list.items.value).toEqual(rows)
    expect(list.total.value).toBe(3)
  })

  it('is loading while it waits and not after', async () => {
    const { fetcher, resolve } = deferred()
    const list = useQueryBackedList<Row>(fetcher)

    const pending = list.apply({})
    expect(list.isLoading.value).toBe(true)

    resolve(0, page([{ id: 'a' }]))
    await pending

    expect(list.isLoading.value).toBe(false)
  })

  it('clears the previous failure when a load is retried', async () => {
    let fail = true
    const list = useQueryBackedList<Row>(() =>
      fail
        ? Promise.reject(new ApiError(CLIENT_ERROR_CODES.network, 'offline', 0))
        : Promise.resolve(page([{ id: 'a' }])),
    )

    await list.apply({})
    expect(list.error.value).toBe('offline')

    fail = false
    await list.apply({})

    expect(list.error.value).toBe('')
    expect(list.items.value).toHaveLength(1)
  })

  it('asks for the default page size when the URL names none', async () => {
    const fetcher = vi.fn(() => Promise.resolve(page([])))
    const list = useQueryBackedList<Row>(fetcher)

    await list.apply({})

    expect(fetcher).toHaveBeenCalledWith({ page: 1, pageSize: DEFAULT_PAGE_SIZE })
  })
})
