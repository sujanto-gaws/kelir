import type { LocationQuery, RouteLocationNormalized, RouteLocationRaw, Router } from 'vue-router'

import { onSessionLost } from '@/stores/session-events'
import { useAuthStore } from '@/stores/auth'

/** The sign-in route, and where a signed-in caller goes instead of it. */
export const LOGIN_ROUTE_NAME = 'login'
export const HOME_ROUTE_NAME = 'dashboard'

/** Where a signed-in caller lands when they lack a route's permission. */
export const FORBIDDEN_ROUTE_NAME = 'forbidden'

/** Query parameter carrying where the caller was headed before being stopped. */
export const RETURN_QUERY_KEY = 'redirect'

/**
 * Query parameter saying the caller *had* a session and no longer does.
 *
 * Without it the login page cannot tell "you were signed in and something ended
 * it" from "you opened a deep link and were never signed in" — the two arrive
 * identically, both carrying only `redirect`. The second needs no explanation;
 * the first needs one, and #68 is about a user who currently gets none.
 *
 * It says nothing about *why*, deliberately. Whether the server refused the
 * session or could not be reached is something the login page reads off what
 * survived — tokens still present means unreachable (#66) — and a reason here
 * would be a second source of that answer to keep in step with the first.
 */
export const SESSION_ENDED_QUERY_KEY = 'sessionEnded'

/**
 * The path to return to after signing in, or null when there is nothing safe
 * to honour.
 *
 * Only same-site absolute paths are accepted. `//evil.example` and
 * `https://evil.example` are both valid values for a query parameter and both
 * navigate off-site, so an unchecked redirect here would be an open redirect —
 * a phishing primitive pointed at our own login page.
 */
export function safeReturnPath(query: LocationQuery): string | null {
  const raw = query[RETURN_QUERY_KEY]
  const value = Array.isArray(raw) ? raw[0] : raw

  if (typeof value !== 'string' || !value.startsWith('/') || value.startsWith('//')) {
    return null
  }

  return value
}

/**
 * The permission a route requires, or null when it names none.
 *
 * Exported so the route table can be checked against it rather than against a
 * second copy of the same rule.
 */
export function requiredPermission(meta: RouteLocationNormalized['meta']): string | null {
  return typeof meta.permission === 'string' && meta.permission !== '' ? meta.permission : null
}

/**
 * The single navigation guard.
 *
 * Protection is driven by route meta alone — `meta.requiresAuth` for a session,
 * `meta.permission` for a specific grant — so a new route opts in by declaring
 * them and nothing here needs to change. Vue Router merges `meta` down the
 * matched chain, so a flag on a parent covers its children.
 */
export async function authGuard(to: RouteLocationNormalized): Promise<boolean | RouteLocationRaw> {
  const auth = useAuthStore()
  const required = requiredPermission(to.meta)

  // Read before anything can clear it. `ensureProfile` below may end the
  // session, and the difference between "your session ended" and "you were
  // never signed in" is only visible from here.
  const hadSession = auth.isAuthenticated

  // A route naming a permission requires a session by definition — there is
  // nobody to hold a permission otherwise. Deriving that rather than trusting
  // two independent flags to agree is what stops a route being left wide open
  // by declaring only one of them: the pair cannot disagree if only one is
  // written down.
  const requiresAuth = to.meta.requiresAuth === true || required !== null

  if (requiresAuth) {
    // A stored token is only a claim. `ensureProfile` settles it against
    // `/auth/me` — refreshing once through the client if the access token has
    // expired, and clearing the session when even that fails.
    if (auth.isAuthenticated && (await auth.ensureProfile())) {
      // Hiding a link is not enough: a pasted URL reaches the route directly.
      // This is still only cosmetic — the backend re-checks every request and
      // is what actually decides — but it means the caller gets an explanation
      // instead of a page of failed calls.
      if (required !== null && !auth.can(required)) {
        return { name: FORBIDDEN_ROUTE_NAME }
      }

      return true
    }

    return {
      name: LOGIN_ROUTE_NAME,
      query: {
        [RETURN_QUERY_KEY]: to.fullPath,
        ...(hadSession ? { [SESSION_ENDED_QUERY_KEY]: '1' } : {}),
      },
    }
  }

  // Signing in again while signed in is a dead end; send them to the app. The
  // profile check keeps a dead token from bouncing them straight back.
  if (to.name === LOGIN_ROUTE_NAME && auth.isAuthenticated && (await auth.ensureProfile())) {
    return { name: HOME_ROUTE_NAME }
  }

  return true
}

/**
 * Whether a route is one only a signed-in caller may be on.
 *
 * The same derivation the guard uses, so a route cannot be protected on entry
 * and abandoned on session loss, or the reverse.
 */
function needsSession(route: RouteLocationNormalized): boolean {
  return route.meta.requiresAuth === true || requiredPermission(route.meta) !== null
}

/**
 * Leave a page the session no longer entitles the caller to be on.
 *
 * The guard runs on navigation, and a session lost *without* one leaves the
 * caller sitting on a page whose data is stale and whose every action will fail
 * — an administrator editing a role submits the form, gets a 401, and has no
 * idea why (#68). This is the other half: the store announces the loss, and
 * this moves them.
 *
 * Two cases are deliberately left alone. A public route — the login page
 * itself, a password reset opened from an email — is somewhere the caller is
 * entitled to be with no session at all, and yanking them off it would
 * interrupt what they came to do. And a caller already on the login page has
 * arrived; sending them there again would replace the state
 * `hasUnverifiedSession` is reading.
 *
 * `redirect` carries where they were, so signing in returns them to it. That is
 * the same parameter the guard writes and the same one `safeReturnPath`
 * refuses to honour when it does not name a same-site path.
 *
 * **This does not stand down while a navigation is in flight, and a version
 * that did was removed rather than kept.** The worry was real enough to build:
 * the store announces a loss from inside `authGuard`'s own await — the profile
 * fetch 401s, the refresh is refused — so a navigation started here would race
 * the guard, and the guard's target is the better one, being where the caller
 * asked to go rather than where they were. But the race has no loser worth
 * naming: both navigations end at the login page, the guard's redirect is
 * applied last, and disabling the stand-down leaves
 * `keeps_the_target_the_caller_asked_for_when_the_session_dies_mid_navigation`
 * green. Machinery that cannot be shown to do anything is worse than none, so
 * it is not here (coding standard §2.9).
 */
export function leaveDeadPage(router: Router): void {
  const current = router.currentRoute.value

  if (current.name === LOGIN_ROUTE_NAME || !needsSession(current)) {
    return
  }

  void router
    .replace({
      name: LOGIN_ROUTE_NAME,
      query: {
        [RETURN_QUERY_KEY]: current.fullPath,
        [SESSION_ENDED_QUERY_KEY]: '1',
      },
    })
    // A navigation already under way cancels this one, and that is the right
    // outcome: the guard is about to make the same decision with better
    // information. An aborted navigation must not surface as an unhandled
    // rejection.
    .catch(() => undefined)
}

/**
 * Installs the guard and the session-loss listener. Called once, from the
 * router module.
 *
 * Returns the call that removes the listener again — module-level listeners
 * outlive any one router, and a test that builds several would otherwise
 * accumulate them.
 */
export function registerGuards(router: Router): () => void {
  router.beforeEach(authGuard)

  return onSessionLost(() => leaveDeadPage(router))
}
