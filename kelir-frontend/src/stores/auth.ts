import { defineStore } from 'pinia'
import { computed, ref } from 'vue'

import * as authApi from '@/api/auth'
import { ApiError } from '@/api/error'
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

/**
 * Name of the cross-tab mutex held while `/auth/refresh` is in flight.
 *
 * The in-process latch below only covers one store instance, and every tab has
 * its own. Two tabs refreshing in parallel is not merely wasteful: the backend
 * revokes a refresh token as it is spent and reads a replay of an already
 * rotated one as theft, killing the whole session family — so the loser of that
 * race signs both tabs out.
 */
const REFRESH_LOCK = 'kelir.auth.refresh'

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

/**
 * The only statuses that mean the credentials themselves were refused.
 *
 * Enumerated rather than expressed as a range. "Any 4xx" reads like the same
 * rule and is not: 408 Request Timeout is precisely the failure this whole
 * distinction exists to survive, and 429 is reachable on `/auth/refresh` once
 * the authentication rate limiter covers it — so a range would sign out the
 * user who was merely rate limited, which is the outcome being prevented,
 * arriving through a different door.
 *
 * A new status is retryable here until somebody decides otherwise, which is the
 * safe direction: the cost of keeping a dead session is one more rejected
 * request, and the cost of dropping a live one is lost work.
 */
const SESSION_ENDING_STATUSES: readonly number[] = [401, 403]

/**
 * Whether a failure is the server refusing the credentials, rather than a
 * statement about the transport or the server's own health.
 *
 * Anything else — a response that never arrived (`status` 0: offline, DNS,
 * timeout), a 5xx, a timeout, a rate limit — says nothing about whether the
 * token is still good. Treating those as a refusal turns a two-second
 * connectivity drop into a forced re-authentication: tokens wiped from storage,
 * credentials retyped, work in progress lost.
 *
 * An error that is not an `ApiError` at all reached us from somewhere
 * unexpected, so it is not treated as a verdict either. The session is left for
 * the server to refuse on the next call — which it will, if it is genuinely
 * dead.
 */
function endsSession(error: unknown): boolean {
  return error instanceof ApiError && SESSION_ENDING_STATUSES.includes(error.status)
}

/**
 * Run `rotate` while holding the cross-tab lock, so exactly one tab presents a
 * given refresh token.
 *
 * Web Locks is unavailable in jsdom and in older Safari. There the call runs
 * unguarded and the `storage` listener is what narrows the window — a tab that
 * sees a newer pair land adopts it instead of spending its own.
 */
async function withRefreshLock(rotate: () => Promise<boolean>): Promise<boolean> {
  const locks: LockManager | undefined =
    typeof navigator === 'undefined' ? undefined : navigator.locks

  if (!locks) {
    return rotate()
  }

  // `request` types its result as `Promise<T>` with `T` inferred as the
  // callback's own promise, so this awaits the nesting the runtime has already
  // flattened.
  const rotated = await locks.request(REFRESH_LOCK, rotate)

  return rotated
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

/** The `storage` listener belonging to the live store, so it can be replaced. */
let activeStorageListener: ((event: StorageEvent) => void) | null = null

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
    } catch (error) {
      // Only a refusal of the credentials ends a session. If the server never
      // answered, or answered with something that is not about the token, the
      // tokens are still good and the caller can try again — see `endsSession`.
      if (endsSession(error)) {
        clearSession()
      }

      return false
    }
  }

  /**
   * Rotate the token pair. Resolves true when a usable access token is in
   * place, false after signing out — never throws, because it is called from
   * an interceptor whose own failure path is the original request's error.
   */
  function refresh(): Promise<boolean> {
    refreshInFlight ??= withRefreshLock(rotateTokens).finally(() => {
      refreshInFlight = null
    })

    return refreshInFlight
  }

  async function rotateTokens(): Promise<boolean> {
    // Another tab may have rotated the pair while this one waited for the lock.
    // Adopting its result is the whole point of holding the lock: presenting a
    // token that tab has already spent is what trips replay detection.
    if (adoptPersistedSession()) {
      return true
    }

    const presented = refreshToken.value

    if (!presented) {
      clearSession()
      return false
    }

    try {
      applySession(await authApi.refreshSession(presented))
      return true
    } catch (error) {
      // A refusal means the token was unknown, expired or already spent, and
      // presenting it again would revoke the session family — so stop and sign
      // out. Anything else says nothing about the token, so the session
      // survives and the caller can retry.
      if (endsSession(error)) {
        clearSession()
      }

      return false
    }
  }

  /**
   * Take on a token pair another tab has written, returning whether there was
   * one worth taking.
   *
   * Never writes back: the pair is already in storage, and re-writing it would
   * only add noise to the other tabs' listeners.
   */
  function adoptPersistedSession(): boolean {
    const persisted = readPersistedSession()

    if (!persisted || persisted.refreshToken === refreshToken.value) {
      return false
    }

    accessToken.value = persisted.accessToken
    refreshToken.value = persisted.refreshToken

    return true
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

  /**
   * Follow what other tabs do to the session.
   *
   * A tab that signs in, refreshes or signs out writes to `localStorage`, and
   * every other tab hears about it here. Without this a tab keeps the pair it
   * read at start-up, and eventually presents a refresh token another tab has
   * already spent — which the backend correctly reads as theft.
   */
  function onExternalSessionChange(event: StorageEvent): void {
    // A null key means the whole store was cleared.
    if (event.key !== null && event.key !== STORAGE_KEY) {
      return
    }

    if (readPersistedSession()) {
      adoptPersistedSession()
      return
    }

    // Another tab signed out. Drop the session here too, without writing back.
    accessToken.value = null
    refreshToken.value = null
    user.value = null
  }

  restore()

  // Replace the listener from any previous store instance rather than stacking
  // another one; tests build several pinias over the same window.
  if (activeStorageListener) {
    window.removeEventListener('storage', activeStorageListener)
  }

  activeStorageListener = onExternalSessionChange
  window.addEventListener('storage', activeStorageListener)

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
