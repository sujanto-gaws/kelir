import { defineStore } from 'pinia'
import { computed, ref } from 'vue'

import * as authApi from '@/api/auth'
import { registerSessionBridge } from '@/api/session'
import type { CurrentUser, SessionResponse } from '@/types/auth'

/**
 * Where the token pair is kept across reloads.
 *
 * localStorage is a deliberate trade-off: it survives a reload and a new tab,
 * which the sign-in flow requires, but any script injected into the page can
 * read it. The stronger option — an HTTP-only, SameSite cookie — needs the
 * backend to set and read it, which the current contract does not do. Flagged
 * for review; nothing in docs/ decides this yet.
 *
 * Only the tokens are persisted. The profile (roles, permissions) is fetched
 * fresh from `/auth/me` on every load, so a permission change never lingers in
 * a stale cache.
 */
const STORAGE_KEY = 'kelir.auth'

interface PersistedSession {
  accessToken: string
  refreshToken: string
}

function readPersistedSession(): PersistedSession | null {
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY)

    if (!raw) {
      return null
    }

    const parsed: unknown = JSON.parse(raw)
    const candidate = parsed as Partial<PersistedSession>

    if (typeof candidate?.accessToken !== 'string' || typeof candidate.refreshToken !== 'string') {
      return null
    }

    return { accessToken: candidate.accessToken, refreshToken: candidate.refreshToken }
  } catch {
    // Unavailable (private mode) or corrupt: treat it as no session rather than
    // failing the whole app on start-up.
    return null
  }
}

function writePersistedSession(session: PersistedSession | null): void {
  try {
    if (session) {
      window.localStorage.setItem(STORAGE_KEY, JSON.stringify(session))
    } else {
      window.localStorage.removeItem(STORAGE_KEY)
    }
  } catch {
    // Storage is a convenience here; the in-memory session still works for
    // this tab, so a failure to persist must not break signing in.
  }
}

/**
 * The session: tokens, the signed-in principal, and the operations that change
 * them. The only place authentication state is mutated (coding standard §3.3).
 */
export const useAuthStore = defineStore('auth', () => {
  const accessToken = ref<string | null>(null)
  const refreshToken = ref<string | null>(null)
  const user = ref<CurrentUser | null>(null)

  /** A session exists as far as this tab knows; the server still decides. */
  const isAuthenticated = computed(() => accessToken.value !== null)
  const permissions = computed(() => new Set(user.value?.permissions ?? []))
  const roles = computed(() => user.value?.roles ?? [])
  const displayName = computed(() => user.value?.displayName ?? user.value?.username ?? '')

  // Single-flight latches. A refresh spends the refresh token — presenting an
  // already-rotated one revokes the whole session family — so a second
  // concurrent refresh would sign the user out. Every caller shares one promise.
  let refreshInFlight: Promise<boolean> | null = null
  let profileInFlight: Promise<boolean> | null = null

  function applySession(session: SessionResponse): void {
    accessToken.value = session.accessToken
    refreshToken.value = session.refreshToken
    writePersistedSession({
      accessToken: session.accessToken,
      refreshToken: session.refreshToken,
    })
  }

  /** Drop everything locally. No network call — used when the server has already said no. */
  function clearSession(): void {
    accessToken.value = null
    refreshToken.value = null
    user.value = null
    writePersistedSession(null)
  }

  /** Reload the tokens saved by a previous page load. */
  function restore(): void {
    const persisted = readPersistedSession()

    if (persisted) {
      accessToken.value = persisted.accessToken
      refreshToken.value = persisted.refreshToken
    }
  }

  /**
   * Sign in. Throws the `ApiError` as it came back — 401 for bad credentials,
   * 429 when rate limited, 422 with field details — so the page decides what
   * the user reads (coding standard §3.4: user-facing strings live in the
   * component).
   */
  async function signIn(username: string, password: string): Promise<void> {
    const session = await authApi.signIn({ username, password })

    applySession(session)

    // A session without a profile has no permissions, so the shell would render
    // as if the user could do nothing. Fetch it before reporting success.
    user.value = await authApi.fetchCurrentUser()
  }

  /**
   * Make sure the profile behind the stored token is loaded, returning false
   * when there is no usable session. Safe to call on every navigation: it is a
   * no-op once loaded, and concurrent callers share one request.
   */
  function ensureProfile(): Promise<boolean> {
    if (!accessToken.value) {
      return Promise.resolve(false)
    }

    if (user.value) {
      return Promise.resolve(true)
    }

    profileInFlight ??= loadProfile().finally(() => {
      profileInFlight = null
    })

    return profileInFlight
  }

  async function loadProfile(): Promise<boolean> {
    try {
      // A 401 here is handled inside the API client: it refreshes once and
      // retries. Reaching this catch means that failed too.
      user.value = await authApi.fetchCurrentUser()
      return true
    } catch {
      clearSession()
      return false
    }
  }

  /**
   * Rotate the token pair. Resolves true when a usable access token is in
   * place, false after signing out — never throws, because it is called from
   * an interceptor whose own failure path is the original request's error.
   */
  function refresh(): Promise<boolean> {
    refreshInFlight ??= rotateTokens().finally(() => {
      refreshInFlight = null
    })

    return refreshInFlight
  }

  async function rotateTokens(): Promise<boolean> {
    const presented = refreshToken.value

    if (!presented) {
      clearSession()
      return false
    }

    try {
      applySession(await authApi.refreshSession(presented))
      return true
    } catch {
      // The token was unknown, expired or already spent. Retrying would present
      // it again and revoke the session family, so stop here and sign out.
      clearSession()
      return false
    }
  }

  /** Sign out here and on the server. Local state is cleared either way. */
  async function signOut(): Promise<void> {
    const presented = refreshToken.value

    clearSession()

    if (!presented) {
      return
    }

    try {
      await authApi.signOut(presented)
    } catch {
      // The endpoint is idempotent and the local session is already gone, so a
      // failure here changes nothing the user can act on.
    }
  }

  /**
   * Whether the signed-in user holds a permission, for hiding controls they
   * cannot use.
   *
   * This is cosmetic only. The server is the authority and re-checks every
   * request; a caller who forges a true here gains nothing but a 403. Never
   * treat it as a security boundary.
   *
   * Matching is exact, mirroring the backend: `master-data:party` does not
   * grant `master-data:party:update`.
   */
  function can(permission: string): boolean {
    return permissions.value.has(permission)
  }

  restore()

  // Hand the client the two things it needs, rather than letting it import this
  // store and create a cycle.
  registerSessionBridge({
    accessToken: () => accessToken.value,
    refresh,
  })

  return {
    accessToken,
    refreshToken,
    user,
    isAuthenticated,
    permissions,
    roles,
    displayName,
    signIn,
    signOut,
    ensureProfile,
    refresh,
    clearSession,
    can,
  }
})
