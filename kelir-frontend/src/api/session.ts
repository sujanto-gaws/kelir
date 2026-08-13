/**
 * The seam between the API client and the auth store.
 *
 * The client needs the access token and a way to refresh it; the store needs
 * the client to call the identity endpoints. Importing each other directly
 * would be a cycle, so the store registers this small bridge on creation and
 * the client only ever sees the two functions it actually needs.
 */
export interface SessionBridge {
  /** The bearer token to attach, or null when there is no session. */
  accessToken(): string | null

  /**
   * Rotate the token pair, resolving true when a usable access token is in
   * place. Implementations MUST be single-flight: the backend revokes the
   * presented refresh token on use and treats a replay of an already-rotated
   * token as theft, killing the whole session family. Two parallel refreshes
   * would therefore sign the user out.
   */
  refresh(): Promise<boolean>
}

let bridge: SessionBridge | null = null

/** Called by the auth store when it is created. */
export function registerSessionBridge(next: SessionBridge | null): void {
  bridge = next
}

/** Null before the auth store exists — the client then sends no token. */
export function currentSessionBridge(): SessionBridge | null {
  return bridge
}
