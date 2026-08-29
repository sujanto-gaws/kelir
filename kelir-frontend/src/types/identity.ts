/**
 * Wire types for the identity administration endpoints (`/api/v1/identity/*`).
 *
 * Names mirror the backend DTOs in `kelir-backend/src/modules/identity/domain.rs`
 * (`User`, `Role`, `Permission`, `CreateUserRequest`, …) so a field can be traced
 * across the wire without a translation table.
 *
 * Every struct there carries `#[serde(rename_all = "camelCase")]`, so the wire
 * names are camelCase — `roleIds`, not `role_ids`. This matters more than usual:
 * the backend sets `#[serde(default)]` on the id arrays and does not deny unknown
 * fields, so a snake_case key is silently dropped and the user is created with no
 * roles and no error.
 */

/**
 * Account state. Only `ACTIVE` can sign in; setting anything else revokes the
 * account's refresh tokens server-side.
 */
export type UserStatus = 'ACTIVE' | 'INACTIVE' | 'LOCKED' | 'PENDING_ACTIVATION'

/** The statuses an administrator may set, with the wording shown in the UI. */
export const USER_STATUS_LABELS: Record<UserStatus, string> = {
  ACTIVE: 'Active',
  INACTIVE: 'Inactive',
  LOCKED: 'Locked',
  PENDING_ACTIVATION: 'Pending activation',
}

/** A role as embedded in a user record — identity only, no permissions. */
export interface RoleSummary {
  id: string
  roleCode: string
  name: string
}

export interface User {
  id: string
  username: string
  email: string
  displayName: string
  status: UserStatus
  /**
   * There is no departments endpoint yet (`modules/organization` is a stub), so
   * this stays a raw optional UUID rather than something we can name.
   */
  departmentId: string | null
  mustChangePassword: boolean
  lastLoginAt: string | null
  createdAt: string
  roles: RoleSummary[]
}

export interface Permission {
  id: string
  /** `module:resource:action`, matched exactly by the backend. */
  permissionCode: string
  module: string
  description: string | null
}

export interface Role {
  id: string
  roleCode: string
  name: string
  description: string | null
  /**
   * A system role (`ROLE-ADMIN`) cannot be deleted — the backend answers 409.
   * Used to disable the control rather than let the user discover it by failing.
   */
  isSystem: boolean
  permissions: Permission[]
}

export interface CreateUserRequest {
  username: string
  email: string
  password: string
  displayName: string
  departmentId?: string | null
  roleIds?: string[]
}

/**
 * Every field is optional and the backend COALESCEs it, so an omitted field
 * keeps its current value — and `null` is indistinguishable from omitted, which
 * means `departmentId` cannot be cleared through this API.
 *
 * `roleIds` is a full replacement when present: `[]` strips every role, omitting
 * it leaves the grants untouched.
 */
export interface UpdateUserRequest {
  email?: string
  displayName?: string
  status?: UserStatus
  departmentId?: string | null
  roleIds?: string[]
}

export interface CreateRoleRequest {
  roleCode: string
  name: string
  description?: string | null
  permissionIds?: string[]
}

/** `roleCode` is settable only at creation, so it is absent here by design. */
export interface UpdateRoleRequest {
  name?: string
  description?: string | null
  permissionIds?: string[]
}

export interface SetPasswordRequest {
  password: string
}

/**
 * How wide a delegation window is (`delegations.scope`).
 *
 * **`ROLE` is not here**, and that is the backend's rule rather than an
 * omission on this side: a window redirects a task that resolves to a *person*,
 * and a task offered to a role has no assignee to redirect — every other holder
 * of the role is still being offered it. The API refuses a `ROLE`-scoped window
 * with that reason, so a control offering one would be the product drawing a
 * choice it then declines.
 */
export type DelegationScope = 'ALL' | 'DOCUMENT_TYPE'

/**
 * One person's approvals reaching another for a stretch of time (FR-IDM-006,
 * #184).
 *
 * Both parties carry a display name beside their id, because a list of windows
 * naming people by UUID is a list nobody can read.
 */
export interface Delegation {
  id: string
  delegatorUserId: string
  delegatorDisplayName: string
  delegateUserId: string
  delegateDisplayName: string
  scope: DelegationScope
  documentTypeId: string | null
  startsAt: string
  endsAt: string
  reason: string | null
  /**
   * Whether the window is still standing.
   *
   * **Not the same as whether it is routing**, which is why both are on the
   * wire. A window ends by being switched off *or* by its end passing, and a
   * screen showing only this flag would report finished cover as live.
   */
  isActive: boolean
  /** Whether it would redirect work right now. Computed by the server. */
  isRouting: boolean
  createdAt: string
}

/**
 * What opening a window asks for.
 *
 * **There is no `delegatorUserId`.** The delegator is always the caller: nobody
 * hands over another person's authority, and a field here would be the one shape
 * of this feature that escalates — a holder of `identity:delegation:create`
 * could point somebody else's approvals at themselves, and the row would look
 * exactly like legitimate cover.
 */
export interface CreateDelegationRequest {
  delegateUserId: string
  startsAt: string
  endsAt: string
  scope?: DelegationScope
  documentTypeId?: string
  reason?: string
}
