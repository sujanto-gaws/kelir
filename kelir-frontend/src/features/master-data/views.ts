import { listParties, listRoleView } from '@/api/master-data'
import type { ListFetchQuery } from '@/composables/useQueryBackedList'
import type { Page } from '@/types/api'
import type { MasterDataRow } from '@/types/master-data'

/**
 * The four master-data lists, as data rather than as four components.
 *
 * The backend shaped the role-view row so that one client component can render
 * all three (`domain/role_view.rs`); this is that decision honoured. A view
 * differs in its path, its title, the permissions it needs, and whether its
 * endpoint accepts filters — nothing else, so nothing else is duplicated.
 */
export interface MasterDataView {
  /** Route parameter and `v-for` key. */
  readonly key: string
  readonly title: string
  readonly description: string
  /**
   * Every permission the endpoint behind this view requires.
   *
   * The role views need `master-data:party:read` **and**
   * `master-data:party-role:read`: a row is a party summary with a supplier
   * number on it, so it is made of both surfaces, and a view needing only one
   * would be a way around the other (`service/role_view.rs`).
   */
  readonly permissions: readonly string[]
  /**
   * Whether the endpoint accepts `search` and the three filters.
   *
   * `/parties` does not. Offering the controls anyway would put a search box on
   * a screen that ignores it — the failure `usePaginatedList` avoided by
   * offering no filters at all, made specific rather than global.
   */
  readonly filterable: boolean
  /** Shown in the row-number column header; absent on the party list. */
  readonly numberLabel?: string
  fetch(query: ListFetchQuery): Promise<Page<MasterDataRow>>
}

export const MASTER_DATA_VIEWS: readonly MasterDataView[] = [
  {
    key: 'parties',
    title: 'Parties',
    description: 'Everyone and everything the business deals with, whatever role they hold.',
    permissions: ['master-data:party:read'],
    filterable: false,
    fetch: (query) => listParties(query),
  },
  {
    key: 'suppliers',
    title: 'Suppliers',
    description: 'Parties holding the SUPPLIER role, with the supplier number that makes them one.',
    permissions: ['master-data:party:read', 'master-data:party-role:read'],
    filterable: true,
    numberLabel: 'Supplier no.',
    fetch: (query) => listRoleView('/master-data/suppliers', query),
  },
  {
    key: 'customers',
    title: 'Customers',
    description: 'Parties holding the CUSTOMER role, with their customer number.',
    permissions: ['master-data:party:read', 'master-data:party-role:read'],
    filterable: true,
    numberLabel: 'Customer no.',
    fetch: (query) => listRoleView('/master-data/customers', query),
  },
  {
    key: 'employees',
    title: 'Employees',
    description: 'Parties holding the EMPLOYEE role, with their employee number.',
    permissions: ['master-data:party:read', 'master-data:party-role:read'],
    filterable: true,
    numberLabel: 'Employee no.',
    fetch: (query) => listRoleView('/master-data/employees', query),
  },
] as const

/** The view a route parameter names, or `undefined` for one that names none. */
export function viewByKey(key: string): MasterDataView | undefined {
  return MASTER_DATA_VIEWS.find((view) => view.key === key)
}
