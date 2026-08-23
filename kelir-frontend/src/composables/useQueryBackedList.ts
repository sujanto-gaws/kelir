import { computed, ref, type ComputedRef, type Ref } from 'vue'

import { toApiError } from '@/api/client'
import type { Page, PageQuery } from '@/types/api'

/**
 * A paginated list whose page, search and filters live in the URL.
 *
 * [`usePaginatedList`](./usePaginatedList.ts) drives the identity tables, whose
 * endpoints take paging and nothing else — it says so, and offering controls it
 * could not honour would have been worse than offering none. The master-data
 * role views do take search and filters (FR-MDM-008), so this is the shape for
 * them; the two are kept apart rather than one grown a mode flag, because a
 * list with no filters and a list with filters are different components on the
 * screen as well as in the code.
 *
 * **The query string is the state, not a copy of it.** A filtered list has to
 * be linkable and has to survive a reload (#101 AC3), which means the URL is
 * the single source of truth and the composable reads it rather than shadowing
 * it. Every mutator writes the URL; the caller re-applies from the URL when it
 * changes, and one code path loads.
 *
 * **Nothing is filtered here.** `apply` sends the parameters and renders what
 * comes back. Narrowing a fetched page on the client would be the failure
 * FR-MDM-008 and NFR-PERF-002 exist to prevent, and would make `meta.total`
 * disagree with the rows under it.
 */

/** The query state a list carries, flat, exactly as it appears in the URL. */
export type ListQuery = Record<string, string>

/** What reaches the fetcher: the paging numbers, plus whatever else the URL had. */
export type ListFetchQuery = PageQuery & Record<string, string | number | undefined>

export interface QueryBackedList<T> {
  items: Ref<T[]>
  page: Ref<number>
  pageSize: Ref<number>
  total: Ref<number>
  totalPages: ComputedRef<number>
  isLoading: Ref<boolean>
  /** Why the last load failed, empty when it did not. */
  error: Ref<string>
  /** True only after a load that succeeded and returned nothing. */
  isEmpty: ComputedRef<boolean>
  hasPrevious: ComputedRef<boolean>
  hasNext: ComputedRef<boolean>
  /** The current filter values, keyed as the endpoint names them. */
  filters: Ref<ListQuery>
  /** Load the page named by `query`, replacing whatever is on screen. */
  apply(query: ListQuery): Promise<void>
}

/** How many rows a page asks for when the URL does not say. */
export const DEFAULT_PAGE_SIZE = 20

/**
 * A page number from a query string, or 1.
 *
 * `?page=0`, `?page=-3` and `?page=nonsense` are all things a URL can contain,
 * and none of them is a page. Clamping here rather than sending them keeps the
 * backend's 400 — which arrives outside the error envelope (#122) — off a
 * screen a user reached by editing the address bar.
 */
export function pageFromQuery(value: string | undefined): number {
  return positiveIntOr(value, 1)
}

/** A positive integer from a query string, or the fallback. */
function positiveIntOr(value: string | undefined, fallback: number): number {
  const parsed = Number.parseInt(value ?? '', 10)

  return Number.isFinite(parsed) && parsed > 0 ? parsed : fallback
}

export function useQueryBackedList<T>(
  fetcher: (query: ListFetchQuery) => Promise<Page<T>>,
): QueryBackedList<T> {
  const items = ref<T[]>([]) as Ref<T[]>
  const page = ref(1)
  const pageSize = ref(DEFAULT_PAGE_SIZE)
  const total = ref(0)
  const isLoading = ref(false)
  const error = ref('')
  const filters = ref<ListQuery>({})
  const hasLoaded = ref(false)

  const totalPages = computed(() =>
    Math.max(1, Math.ceil(total.value / Math.max(1, pageSize.value))),
  )
  const hasPrevious = computed(() => page.value > 1)
  const hasNext = computed(() => page.value < totalPages.value)
  // Three states, not two: a load that failed is not an empty result, and a
  // list that showed "nothing to show" over a failed request would be lying
  // (coding standard §3.4).
  const isEmpty = computed(
    () => hasLoaded.value && !isLoading.value && error.value === '' && items.value.length === 0,
  )

  /** The most recent apply, so a slower earlier one cannot overwrite it. */
  let latest = 0

  async function apply(query: ListQuery): Promise<void> {
    const { page: rawPage, pageSize: rawPageSize, ...rest } = query

    page.value = pageFromQuery(rawPage)
    pageSize.value = positiveIntOr(rawPageSize, DEFAULT_PAGE_SIZE)
    filters.value = rest

    const ticket = ++latest
    isLoading.value = true
    error.value = ''

    try {
      const result = await fetcher({ page: page.value, pageSize: pageSize.value, ...rest })

      if (ticket !== latest) {
        // A later apply has already answered. Typing in a search box makes
        // these overlap, and the older response is not the one on screen.
        return
      }

      items.value = result.items
      total.value = result.meta.total
      // The server echoes the *effective* values after clamping, so trust its
      // numbers over the ones we asked for (#101 AC4).
      page.value = result.meta.page
      pageSize.value = result.meta.pageSize
      hasLoaded.value = true
    } catch (caught) {
      if (ticket !== latest) {
        return
      }

      // A failed load leaves no rows: showing the previous page's data under a
      // new page number, or under a search that did not run, would be a lie.
      items.value = []
      total.value = 0
      error.value = toApiError(caught).message
      hasLoaded.value = true
    } finally {
      if (ticket === latest) {
        isLoading.value = false
      }
    }
  }

  return {
    items,
    page,
    pageSize,
    total,
    totalPages,
    isLoading,
    error,
    isEmpty,
    hasPrevious,
    hasNext,
    filters,
    apply,
  }
}
