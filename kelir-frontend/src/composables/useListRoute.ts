import { computed, watch, type ComputedRef } from 'vue'
import { useRoute, useRouter, type LocationQuery, type RouteParamsRaw } from 'vue-router'

import type { ListQuery, QueryBackedList } from './useQueryBackedList'

/**
 * The URL half of a [`useQueryBackedList`](./useQueryBackedList.ts) page.
 *
 * Every page that keeps its page, search and filters in the query string read
 * the URL, wrote changes back to it and turned its pages the same way, in the
 * same few dozen lines, nine times over. This is those lines once. What still
 * differs between the pages stays on them: which keys a control writes, whether
 * a value is trimmed, and what the empty table says.
 *
 * **The URL is still the state** (#101 AC3). `navigate` writes the URL and
 * nothing else; the watch below loads from what the URL then says, so there is
 * one load path whether the change came from a control, the back button or a
 * pasted link.
 */

/** Where the list lives: its route's name and, for a routed list, its params. */
export interface ListRouteTarget {
  name: string
  params?: RouteParamsRaw
}

export interface ListRouteOptions {
  /**
   * Keys the URL holds that are not the list's: the integration log's open row
   * is one. They are left out of `listQuery`, so changing them loads nothing,
   * and any change the list makes drops them, closing whatever they opened.
   */
  exclude?: string[]
  /**
   * False when the page loads the list itself, because its load waits for
   * something besides the URL, such as a list definition, or does more than
   * load. Default true.
   */
  load?: boolean
}

export interface ListRoute {
  /** Every key the URL holds, flat: the first value of each, blanks dropped. */
  currentQuery: ComputedRef<ListQuery>
  /** `currentQuery` without the excluded keys: what the list is loaded with. */
  listQuery: ComputedRef<ListQuery>
  /**
   * Write `changes` to the URL. Any change but the page itself goes back to
   * page 1: staying on page 7 of a list just narrowed to two rows would show
   * an empty table under a pager that disagrees with it. A blank value removes
   * its key.
   */
  navigate(changes: ListQuery, resetPage?: boolean): void
  /** Go to page `next`, held between the first page and the last. */
  goToPage(next: number): void
  /**
   * Read the page again, after a save or a delete.
   *
   * A delete can empty the last page, so it steps back a page rather than leave
   * the person on one that no longer exists. The URL changes, and the load
   * reads the page it names.
   */
  refresh(afterDelete?: boolean): Promise<void>
}

/** The query string as flat strings: the first value of each key, blanks dropped. */
export function queryFromRoute(query: LocationQuery): ListQuery {
  const flat: ListQuery = {}

  for (const [key, value] of Object.entries(query)) {
    const first = Array.isArray(value) ? value[0] : value

    if (typeof first === 'string' && first !== '') {
      flat[key] = first
    }
  }

  return flat
}

export function useListRoute<T>(
  list: QueryBackedList<T>,
  target: () => ListRouteTarget,
  options: ListRouteOptions = {},
): ListRoute {
  const route = useRoute()
  const router = useRouter()
  const exclude = options.exclude ?? []

  const currentQuery = computed(() => queryFromRoute(route.query))

  const listQuery = computed(() => {
    const own: ListQuery = { ...currentQuery.value }

    for (const key of exclude) {
      delete own[key]
    }

    return own
  })

  function navigate(changes: ListQuery, resetPage = true): void {
    const next: ListQuery = { ...listQuery.value, ...changes }

    if (resetPage) {
      delete next.page
    }

    for (const [key, value] of Object.entries(next)) {
      if (value === '') {
        delete next[key]
      }
    }

    void router.push({ ...target(), query: next })
  }

  function goToPage(next: number): void {
    navigate({ page: String(Math.min(Math.max(1, next), list.totalPages.value)) }, false)
  }

  async function refresh(afterDelete = false): Promise<void> {
    if (afterDelete && list.items.value.length === 1 && list.page.value > 1) {
      goToPage(list.page.value - 1)
      return
    }

    await list.apply(listQuery.value)
  }

  if (options.load ?? true) {
    // Compared as text, so the list loads when what it is asked for changes
    // and not on every navigation: opening the integration log's row changes
    // the URL but not the rows. The target is in it because a routed list,
    // the master-data views, changes its params and keeps its query.
    // `immediate` is what makes a deep link load its own filters rather than
    // the whole population. A route that is not the list's is the page being
    // left, by a link to a row or the menu, and its query is not this list's.
    watch(
      () => JSON.stringify([target(), listQuery.value]),
      () => {
        if (route.name !== target().name) {
          return
        }

        void list.apply(listQuery.value)
      },
      { immediate: true },
    )
  }

  return { currentQuery, listQuery, navigate, goToPage, refresh }
}
