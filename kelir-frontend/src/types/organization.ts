/**
 * Wire types for tenant administration (`/api/v1/organization/tenants`).
 *
 * Names mirror the backend DTOs in
 * `kelir-backend/src/modules/organization/domain.rs` (`TenantView`,
 * `CreateTenantRequest`, `UpdateTenantRequest`) so a field can be traced across
 * the wire without a translation table. Every struct there carries
 * `#[serde(rename_all = "camelCase")]` and `deny_unknown_fields`, so a
 * misspelled key is refused rather than dropped.
 */

/**
 * Tenant lifecycle. Only `ACTIVE` admits sign-in; moving a tenant to anything
 * else also revokes its refresh tokens server-side, so existing sessions end
 * rather than merely failing to renew.
 */
export type TenantStatus = 'ACTIVE' | 'SUSPENDED' | 'INACTIVE'

/** The statuses an administrator may set, with the wording shown in the UI. */
export const TENANT_STATUS_LABELS: Record<TenantStatus, string> = {
  ACTIVE: 'Active',
  SUSPENDED: 'Suspended',
  INACTIVE: 'Inactive',
}

export interface Tenant {
  id: string
  /** Business identifier, `TNT-001` shaped. The handle users sign in with. */
  tenantCode: string
  name: string
  status: TenantStatus
  /**
   * The tenant administration is performed *from* — the deployment's default
   * tenant. It cannot be suspended or deleted (the backend answers 400), so the
   * controls are disabled rather than left to fail.
   */
  isDefault: boolean
  /** Live users. Suspending or deleting the tenant ends all of their sessions. */
  userCount: number
  createdAt: string
}

/**
 * Creating a tenant creates its first administrator in the same request.
 *
 * Not two calls: a tenant with no users would be a row nobody can sign in to,
 * which is the state the whole surface was held back to avoid (decision D-18).
 */
export interface CreateTenantRequest {
  tenantCode: string
  name: string
  administrator: TenantAdministratorInput
}

export interface TenantAdministratorInput {
  username: string
  email: string
  displayName: string
  password: string
}

/**
 * `tenantCode` is absent by design — the backend does not accept it and refuses
 * the request rather than ignoring the field.
 *
 * The code is what users type at sign-in, and no session carries it (the token
 * carries the tenant's id), so changing it would strand a tenant's users at a
 * login form with nothing failing loudly enough to notice.
 */
export interface UpdateTenantRequest {
  name?: string
  status?: TenantStatus
}
