import { getItem, postItem, postVoid } from './client'
import type {
  ChangePasswordRequest,
  CurrentUser,
  RequestResetRequest,
  ResetPasswordRequest,
  SessionResponse,
  SignInRequest,
} from '@/types/auth'

/**
 * The identity endpoints (`/api/v1/auth/*`).
 *
 * Thin by design: every function is one call through the shared client, so the
 * envelope unwrapping and error normalisation happen in exactly one place
 * (coding standard §3.3). Messages shown to a user are chosen by the component,
 * never composed here.
 */

/** Exchange credentials for a token pair. 401 on bad credentials, 429 when rate limited. */
export function signIn(request: SignInRequest): Promise<SessionResponse> {
  return postItem<SessionResponse>('/auth/login', request)
}

/**
 * Rotate the token pair.
 *
 * The presented refresh token is revoked by the server as it is spent, and
 * replaying an already-rotated token revokes the entire session family — so
 * exactly one caller may hold a given refresh token. The auth store enforces
 * that by making refresh single-flight; nothing else should call this.
 */
export function refreshSession(refreshToken: string): Promise<SessionResponse> {
  return postItem<SessionResponse>('/auth/refresh', { refreshToken })
}

/** End the session server-side. Idempotent: a spent or unknown token still answers 204. */
export function signOut(refreshToken: string): Promise<void> {
  return postVoid('/auth/logout', { refreshToken })
}

/** The signed-in principal, with the roles and permissions the UI reads. */
export function fetchCurrentUser(): Promise<CurrentUser> {
  return getItem<CurrentUser>('/auth/me')
}

/**
 * Change the password. Succeeds with 204.
 *
 * Every refresh token for the account is revoked, so no session can be
 * extended. An access token already issued stays valid until it expires — up
 * to fifteen minutes — because access tokens are stateless and checked against
 * no revocation list. The wording used to claim every session ended (#60).
 */
export function changePassword(request: ChangePasswordRequest): Promise<void> {
  return postVoid('/auth/change-password', request)
}

/**
 * Ask for a reset link. Answers 202 whether or not the identifier exists.
 *
 * **A caller cannot learn anything from the outcome, and must not try.** The
 * backend answers the same way for an unknown identifier, a suspended account,
 * a resend still inside its cooldown, and a mail server that is down — so a
 * page that branched on anything but "the request was accepted" would be
 * reporting a difference the server deliberately refuses to make.
 */
export function requestPasswordReset(request: RequestResetRequest): Promise<void> {
  return postVoid('/auth/forgot-password', request)
}

/**
 * Redeem a reset token and set the new password. Succeeds with 204.
 *
 * A token that is unknown, expired or already spent is a 422 with the same
 * message as one that is malformed — the same reasoning as `signIn`'s generic
 * 401. The account's sessions are revoked, so the person signs in afresh.
 */
export function resetPassword(request: ResetPasswordRequest): Promise<void> {
  return postVoid('/auth/reset-password', request)
}
