/**
 * The signal a session sends when it ends without being asked to (#68).
 *
 * **Why this exists.** The route guard runs on navigation. A session lost while
 * the user is sitting on a page — a refresh that fails with a 4xx, a token
 * revoked server-side, an administrator deactivating the account, another tab
 * signing out — moves nothing, so nothing redirects them. They stay on a page
 * whose data is stale and whose every subsequent action will fail. #66 covered
 * the case where a navigation happens; this covers the case where none does.
 *
 * **Why it is a module rather than part of the store.** The router installs its
 * guards when `router/index.ts` is imported, which is before any Pinia store
 * exists — `useAuthStore()` at that moment would throw. A module-level registry
 * lets the listener be installed once, at import time, and the store push to it
 * whenever it is created. It is the same seam `api/session.ts` uses in the other
 * direction, and for the same reason: the two sides must not import each other.
 *
 * **What it deliberately does not carry.** No reason code, no error. The
 * listener's job is to get the user off a dead page; *why* the session ended is
 * something the login page works out from what survived — tokens still present
 * means the server could not be reached, tokens gone means it refused (#66).
 * Passing a reason here would be a second source of that answer to keep in step
 * with the first.
 */
export type SessionLostListener = () => void

const listeners = new Set<SessionLostListener>()

/**
 * Register a listener, returning the call that removes it again.
 *
 * Listeners are module-scoped and outlive any one store or router instance, so
 * a caller that can be created more than once — a test, a re-mounted app — must
 * unregister rather than accumulate.
 */
export function onSessionLost(listener: SessionLostListener): () => void {
  listeners.add(listener)

  return () => {
    listeners.delete(listener)
  }
}

/**
 * Announce that the session has ended on its own.
 *
 * Called by the auth store, and only from the paths where the *server* ended
 * the session or another tab did. A deliberate sign-out from this tab does not
 * come through here: the caller of `signOut` is already navigating, and firing
 * this as well would race a redirect against a redirect.
 *
 * A listener that throws must not stop the others from hearing, or the first
 * one registered would decide whether the rest run.
 */
export function notifySessionLost(): void {
  for (const listener of [...listeners]) {
    try {
      listener()
    } catch {
      // A listener's own failure is its problem; the session is still gone.
    }
  }
}

/** Drops every listener. For tests, which build the world repeatedly. */
export function resetSessionListeners(): void {
  listeners.clear()
}
