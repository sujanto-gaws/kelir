import { getPage } from './client'
import type { ListFetchQuery } from '@/composables/useQueryBackedList'
import type { Page, PageQuery } from '@/types/api'
import type { MasterDataRow } from '@/types/master-data'

/**
 * The master-data list endpoints.
 *
 * Thin by design, like `identity.ts`: one call each through the shared client,
 * so envelope unwrapping and error normalisation happen in exactly one place
 * (coding standard §3.3).
 *
 * **The server paginates and the server filters.** Every parameter below goes
 * on the wire; nothing fetches a population and narrows it here, which is the
 * failure FR-MDM-008 and NFR-PERF-002 exist to prevent.
 */

/** Parties, paged. This endpoint takes no search or filter parameters. */
export function listParties(query: PageQuery): Promise<Page<MasterDataRow>> {
  return getPage<MasterDataRow>('/master-data/parties', query)
}

/**
 * One of the three role views, paged, searched and filtered.
 *
 * `path` is chosen by the caller from [`MASTER_DATA_VIEWS`], never composed
 * from user input: the backend has no role parameter to get wrong and neither
 * should the client.
 *
 * Blank values are dropped rather than sent. `?search=` means *everything* to
 * the backend and it would be harmless, but `?statusId=` is not in the
 * vocabulary and would be a 422 — so an empty select is an absent parameter,
 * which is what "no filter" means.
 */
export function listRoleView(path: string, query: ListFetchQuery): Promise<Page<MasterDataRow>> {
  const params: Record<string, string | number> = {}

  for (const [key, value] of Object.entries(query)) {
    if (value !== undefined && value !== null && value !== '') {
      params[key] = value as string | number
    }
  }

  return getPage<MasterDataRow>(path, params)
}
