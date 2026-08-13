import { describe, expect, it, vi } from 'vitest'

import { usePaginatedList } from './usePaginatedList'
import { ApiError } from '@/api/error'
import type { Page, PageQuery } from '@/types/api'

interface Row {
  id: string
}

function pageOf(items: Row[], page: number, pageSize: number, total: number): Page<Row> {
  return { items, meta: { page, pageSize, total } }
}

describe('usePaginatedList', () => {
  it('loads the first page and derives the page count the backend does not send', async () => {
    // `meta` carries `total` only — no `totalPages`, no `hasNext`.
    const fetcher = vi.fn(async () => pageOf([{ id: 'a' }], 1, 20, 45))
    const list = usePaginatedList<Row>(fetcher)

    await list.load()

    expect(fetcher).toHaveBeenCalledWith({ page: 1, pageSize: 20 })
    expect(list.items.value).toEqual([{ id: 'a' }])
    expect(list.total.value).toBe(45)
    expect(list.totalPages.value).toBe(3)
    expect(list.hasPrevious.value).toBe(false)
    expect(list.hasNext.value).toBe(true)
  })

  it('trusts the page numbers the server echoes back over the ones it asked for', async () => {
    // The backend clamps `pageSize` to 100 and reports the effective value.
    const fetcher = vi.fn(async () => pageOf([], 1, 100, 0))
    const list = usePaginatedList<Row>(fetcher, 5_000)

    await list.load()

    expect(list.pageSize.value).toBe(100)
  })

  it('moves between pages', async () => {
    const fetcher = vi.fn(async (query: PageQuery) =>
      pageOf([{ id: 'b' }], query.page ?? 1, 20, 45),
    )
    const list = usePaginatedList<Row>(fetcher)

    await list.load()
    await list.goToPage(2)

    expect(list.page.value).toBe(2)
    expect(fetcher).toHaveBeenLastCalledWith({ page: 2, pageSize: 20 })
    expect(list.hasPrevious.value).toBe(true)
  })

  it('refuses to walk past either end', async () => {
    const fetcher = vi.fn(async (query: PageQuery) =>
      pageOf([{ id: 'b' }], query.page ?? 1, 20, 45),
    )
    const list = usePaginatedList<Row>(fetcher)

    await list.load()
    await list.goToPage(0)
    expect(list.page.value).toBe(1)

    await list.goToPage(99)
    expect(list.page.value).toBe(3)

    // Already there: no request worth spending.
    const before = fetcher.mock.calls.length
    await list.goToPage(3)
    expect(fetcher.mock.calls.length).toBe(before)
  })

  it('steps back when the last row on a page is removed', async () => {
    // Otherwise a deletion strands the user on a page that no longer exists.
    const fetcher = vi.fn(async (query: PageQuery) =>
      pageOf([{ id: 'b' }], query.page ?? 1, 20, 21),
    )
    const list = usePaginatedList<Row>(fetcher)

    await list.load()
    await list.goToPage(2)
    await list.refresh()

    expect(list.page.value).toBe(1)
  })

  it('stays put on refresh when the page still has other rows', async () => {
    const fetcher = vi.fn(async (query: PageQuery) =>
      pageOf([{ id: 'b' }, { id: 'c' }], query.page ?? 1, 20, 41),
    )
    const list = usePaginatedList<Row>(fetcher)

    await list.load()
    await list.goToPage(2)
    await list.refresh()

    expect(list.page.value).toBe(2)
  })

  it('shows nothing rather than stale rows when a load fails', async () => {
    let shouldFail = false
    const fetcher = vi.fn(async () => {
      if (shouldFail) {
        throw new ApiError('INTERNAL_ERROR', 'An unexpected error occurred', 500)
      }

      return pageOf([{ id: 'a' }], 1, 20, 1)
    })
    const list = usePaginatedList<Row>(fetcher)

    await list.load()
    shouldFail = true
    await list.load()

    expect(list.error.value).toBe('An unexpected error occurred')
    // Rows under a page number they do not belong to would be a lie.
    expect(list.items.value).toEqual([])
    expect(list.total.value).toBe(0)
  })

  it('clears a previous failure once a load succeeds', async () => {
    let shouldFail = true
    const fetcher = vi.fn(async () => {
      if (shouldFail) {
        throw new ApiError('NETWORK_ERROR', 'Could not reach the server', 0)
      }

      return pageOf([{ id: 'a' }], 1, 20, 1)
    })
    const list = usePaginatedList<Row>(fetcher)

    await list.load()
    expect(list.error.value).toBe('Could not reach the server')

    shouldFail = false
    await list.load()

    expect(list.error.value).toBe('')
    expect(list.items.value).toEqual([{ id: 'a' }])
  })

  it('reports at least one page when there is nothing to show', async () => {
    const fetcher = vi.fn(async () => pageOf([], 1, 20, 0))
    const list = usePaginatedList<Row>(fetcher)

    await list.load()

    expect(list.totalPages.value).toBe(1)
    expect(list.hasNext.value).toBe(false)
  })
})
