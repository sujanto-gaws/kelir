import type { LocationQuery, RouteLocationNormalized, RouteLocationRaw, Router } from 'vue-router'

import { useAuthStore } from '@/stores/auth'

/** The sign-in route, and where a signed-in caller goes instead of it. */
export const LOGIN_ROUTE_NAME = 'login'
export const HOME_ROUTE_NAME = 'dashboard'

/** Query parameter carrying where the caller was headed before being stopped. */
export const RETURN_QUERY_KEY = 'redirect'

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
 * The single navigation guard.
 *
 * Protection is driven only by `meta.requiresAuth`, so a new route opts in by
 * declaring it and nothing here needs to change. Vue Router merges `meta` down
 * the matched chain, so a flag on a parent covers its children.
 */
export async function authGuard(to: RouteLocationNormalized): Promise<boolean | RouteLocationRaw> {
  const auth = useAuthStore()
  const requiresAuth = to.meta.requiresAuth === true

  if (requiresAuth) {
    // A stored token is only a claim. `ensureProfile` settles it against
    // `/auth/me` — refreshing once through the client if the access token has
    // expired, and clearing the session when even that fails.
    if (auth.isAuthenticated && (await auth.ensureProfile())) {
      return true
    }

    return { name: LOGIN_ROUTE_NAME, query: { [RETURN_QUERY_KEY]: to.fullPath } }
  }

  // Signing in again while signed in is a dead end; send them to the app. The
  // profile check keeps a dead token from bouncing them straight back.
  if (to.name === LOGIN_ROUTE_NAME && auth.isAuthenticated && (await auth.ensureProfile())) {
    return { name: HOME_ROUTE_NAME }
  }

  return true
}

/** Installs the guard. Called once, from the router module. */
export function registerGuards(router: Router): void {
  router.beforeEach(authGuard)
}
