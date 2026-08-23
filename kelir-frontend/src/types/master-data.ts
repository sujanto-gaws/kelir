/**
 * Wire types for the master-data list endpoints.
 *
 * These mirror `kelir-backend/src/modules/master_data/domain/` — the party
 * summary and the role-view row — and nothing here is inferred from a response
 * at runtime (coding standard §3.3).
 *
 * **One row type across all four lists.** `/parties` answers `PartySummary` and
 * the three role views answer `RoleViewRow`; the backend deliberately shaped
 * the second so that a client rendering all three needs one component and not
 * three (`domain/role_view.rs`). [`MasterDataRow`] is the union of the two, and
 * the members only the role views carry are optional here for the same reason
 * they are absent there: a party is not a supplier that has lost its number.
 */

/** `PERSON` or `PARTY_GROUP`. */
export type PartyType = 'PERSON' | 'PARTY_GROUP'

/** The party's own enabled flag, which is not its record lifecycle. */
export type PartyStatus = 'PARTY_ENABLED' | 'PARTY_DISABLED'

/** The status of a role assignment, which is not its removal. */
export type PartyRoleStatus = 'ACTIVE' | 'INACTIVE'

export const PARTY_TYPE_LABELS: Record<PartyType, string> = {
  PERSON: 'Person',
  PARTY_GROUP: 'Organisation',
}

export const PARTY_STATUS_LABELS: Record<PartyStatus, string> = {
  PARTY_ENABLED: 'Enabled',
  PARTY_DISABLED: 'Disabled',
}

export const ROLE_STATUS_LABELS: Record<PartyRoleStatus, string> = {
  ACTIVE: 'Active',
  INACTIVE: 'Inactive',
}

/** A row of `/master-data/parties`. */
export interface PartySummary {
  id: string
  partyId: string
  partyTypeId: PartyType
  statusId: PartyStatus
  name: string
  externalId: string | null
  createdStamp: string
  lastUpdatedStamp: string
}

/** A row of `/master-data/suppliers`, `/customers` or `/employees`. */
export interface RoleViewRow extends PartySummary {
  /** `SUPPLIER`, `CUSTOMER` or `EMPLOYEE` — which view produced the row. */
  roleTypeId: string
  /**
   * The supplier, customer or employee number.
   *
   * `null` when the party holds the role without a profile, which is legal:
   * hiding such a party would make the list disagree with the role it claims
   * to list.
   */
  roleNumber: string | null
  roleStatusId: PartyRoleStatus
  fromDate: string
  thruDate: string | null
}

/** What a table renders, whichever of the four endpoints produced it. */
export type MasterDataRow = PartySummary & Partial<Omit<RoleViewRow, keyof PartySummary>>

/**
 * The query parameters the role views accept.
 *
 * `/parties` accepts none of them — its endpoint takes paging only — which is
 * why [`MasterDataView.filterable`] exists rather than this being sent
 * everywhere. A control that silently did nothing would be worse than its
 * absence.
 */
export interface RoleViewQuery {
  search?: string
  statusId?: PartyStatus
  partyTypeId?: PartyType
  roleStatusId?: PartyRoleStatus
}
