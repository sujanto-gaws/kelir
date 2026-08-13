import { computed, ref, type ComputedRef, type Ref } from 'vue'

import { toApiError } from '@/api/client'
import type { Page, PageQuery } from '@/types/api'

/**
 * Drives a paginated list endpoint: current page, rows, and the loading and
 * failure states a table needs to render honestly.
 *
 * The backend's `meta` reports `total` only — no `totalPages`, no `hasNext` —
 * so the page count is derived here rather than in every table.
 *
 * There are no search, filter or sort parameters on the identity list endpoints
 * (`ORDER BY username` and `ORDER BY role_code` are fixed server-side), so this
 * deliberately offers none: a control that silently did nothing would be worse
 * than its absence.
 */
export interface PaginatedList<T> {
  items: Ref<T[]>
  page: Ref<number>
  pageSize: Ref<number>
  total: Ref<number>
  totalPages: ComputedRef<number>
  isLoading: Ref<boolean>
  /** Why the last load failed, empty when it did not. */
  error: Ref<string>
  hasPrevious: ComputedRef<boolean>
  hasNext: ComputedRef<boolean>
  load(): Promise<void>
  goToPage(next: number): Promise<void>
  /** Reload the current page — after a create, edit or delete. */
  refresh(): Promise<void>
}

export function usePaginatedList<T>(
  fetcher: (query: PageQuery) => Promise<Page<T>>,
  initialPageSize = 20,
): PaginatedList<T> {
  const items = ref<T[]>([]) as Ref<T[]>
  const page = ref(1)
  const pageSize = ref(initialPageSize)
  const total = ref(0)
  const isLoading = ref(false)
  const error = ref('')

  const totalPages = computed(() =>
    Math.max(1, Math.ceil(total.value / Math.max(1, pageSize.value))),
  )
  const hasPrevious = computed(() => page.value > 1)
  const hasNext = computed(() => page.value < totalPages.value)

  async function load(): Promise<void> {
    isLoading.value = true
    error.value = ''

    try {
      const result = await fetcher({ page: page.value, pageSize: pageSize.value })

      items.value = result.items
      total.value = result.meta.total
      // The server echoes the *effective* values after clamping, so trust its
      // numbers over the ones we asked for.
      page.value = result.meta.page
      pageSize.value = result.meta.pageSize
    } catch (caught) {
      // A failed load leaves no rows: showing the previous page's data under a
      // new page number would be a lie.
      items.value = []
      total.value = 0
      error.value = toApiError(caught).message
    } finally {
      isLoading.value = false
    }
  }

  async function goToPage(next: number): Promise<void> {
    const target = Math.min(Math.max(1, next), totalPages.value)

    if (target === page.value) {
      return
    }

    page.value = target
    await load()
  }

  async function refresh(): Promise<void> {
    // A deletion can empty the last page; step back rather than stranding the
    // user on a page that no longer exists.
    if (items.value.length === 1 && page.value > 1) {
      page.value -= 1
    }

    await load()
  }

  return {
    items,
    page,
    pageSize,
    total,
    totalPages,
    isLoading,
    error,
    hasPrevious,
    hasNext,
    load,
    goToPage,
    refresh,
  }
}
