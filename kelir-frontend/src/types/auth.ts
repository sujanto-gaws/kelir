/**
 * Wire types for the identity endpoints.
 *
 * Names mirror the backend DTOs in `kelir-backend/src/modules/auth/handlers.rs`
 * (`SessionResponse`, `CurrentUser`) so a field can be traced across the wire
 * without a translation table.
 */

/** The token pair issued by `/auth/login` and `/auth/refresh`. */
export interface SessionResponse {
  accessToken: string
  refreshToken: string
  tokenType: string
  /** Seconds until the access token expires. */
  expiresIn: number
  userId: string
  username: string
}

/** The signed-in principal returned by `/auth/me`. */
export interface CurrentUser {
  id: string
  username: string
  displayName: string
  email: string
  roles: string[]
  /** Permission codes in `module:resource:action` form. */
  permissions: string[]
}

/** Credentials posted to `/auth/login`. */
export interface SignInRequest {
  username: string
  password: string
  /**
   * Which tenant to authenticate against (FR-IDM-009).
   *
   * Optional, and the backend has accepted it since Sprint 4 — what was missing
   * until decision **D-18** was any way for this client to know when to send it
   * (#67). It is required only on a multi-tenant deployment, which
   * `GET /deployment` reports; a single-tenant deployment **ignores** it and
   * resolves its configured default, so sending it there cannot reach another
   * tenant's users.
   */
  tenantCode?: string
}

/** Body posted to `/auth/change-password`. */
export interface ChangePasswordRequest {
  currentPassword: string
  newPassword: string
}

/** Body posted to `/auth/forgot-password`. */
export interface RequestResetRequest {
  /** A username or an email address — the same identifier sign-in takes. */
  username: string
}

/** Body posted to `/auth/reset-password`. */
export interface ResetPasswordRequest {
  /** The opaque token out of the emailed link. */
  token: string
  newPassword: string
}
