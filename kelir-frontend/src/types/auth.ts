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
}

/** Body posted to `/auth/change-password`. */
export interface ChangePasswordRequest {
  currentPassword: string
  newPassword: string
}
