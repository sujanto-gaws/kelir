import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import { useAuthStore } from './auth'
import { getItem } from '@/api/client'
import { registerSessionBridge } from '@/api/session'
import {
  errorBody,
  installFakeBackend,
  itemBody,
  type FakeBackendHandle,
  type FakeHandler,
  type FakeReply,
} from '@/lib/testing/fake-backend'
import type { CurrentUser, SessionResponse } from '@/types/auth'

const STORAGE_KEY = 'kelir.auth'

function session(generation: number): SessionResponse {
  return {
    accessToken: `access-${generation}`,
    refreshToken: `refresh-${generation}`,
    tokenType: 'Bearer',
    expiresIn: 900,
    userId: 'u-1',
    username: 'ana',
  }
}

const profile: CurrentUser = {
  id: 'u-1',
  username: 'ana',
  displayName: 'Ana Putri',
  email: 'ana@example.com',
  roles: ['clerk'],
  permissions: ['document:read', 'master-data:party:update'],
}

const unauthorized: FakeReply = {
  status: 401,
  body: errorBody('UNAUTHORIZED', 'Authentication required'),
}

/**
 * These are integration tests on purpose: the real store, the real API client
 * and the real interceptors, with only the network replaced. The invariant that
 * matters most — one refresh across concurrent 401s — is a property of the
 * three working together, so stubbing any of them would test the stub.
 */
describe('useAuthStore', () => {
  let backend: FakeBackendHandle
  let handler: FakeHandler

  beforeEach(() => {
    setActivePinia(createPinia())
    window.localStorage.clear()

    handler = () => ({ status: 404, body: errorBody('NOT_FOUND', 'not stubbed') })
    backend = installFakeBackend((request) => handler(request))
  })

  afterEach(() => {
    backend.restore()
    registerSessionBridge(null)
  })

  describe('sign in', () => {
    beforeEach(() => {
      handler = (request) =>
        request.url === '/auth/login'
          ? { status: 200, body: itemBody(session(1)) }
          : { status: 200, body: itemBody(profile) }
    })

    it('keeps the token pair and the profile', async () => {
      const store = useAuthStore()

      await store.signIn('ana', 'correct horse')

      expect(store.isAuthenticated).toBe(true)
      expect(store.accessToken).toBe('access-1')
      expect(store.user).toEqual(profile)
      expect(store.displayName).toBe('Ana Putri')
    })

    it('sends the credentials to the login endpoint', async () => {
      await useAuthStore().signIn('ana', 'correct horse')

      expect(backend.requests[0]).toMatchObject({
        url: '/auth/login',
        method: 'post',
        body: { username: 'ana', password: 'correct horse' },
      })
    })

    it('survives a reload by persisting the tokens', async () => {
      await useAuthStore().signIn('ana', 'correct horse')

      // A reload is a fresh pinia reading the same storage.
      setActivePinia(createPinia())
      const reloaded = useAuthStore()

      expect(reloaded.isAuthenticated).toBe(true)
      expect(reloaded.accessToken).toBe('access-1')
      // The profile is deliberately not persisted; it is fetched fresh so a
      // revoked permission cannot survive in a cache.
      expect(reloaded.user).toBeNull()
    })
  })

  it('reports a failed sign-in without establishing a session', async () => {
    handler = () => unauthorized
    const store = useAuthStore()

    await expect(store.signIn('ana', 'wrong')).rejects.toMatchObject({ status: 401 })

    expect(store.isAuthenticated).toBe(false)
    expect(window.localStorage.getItem(STORAGE_KEY)).toBeNull()
  })

  describe('sign out', () => {
    beforeEach(() => {
      handler = (request) => {
        if (request.url === '/auth/login') {
          return { status: 200, body: itemBody(session(1)) }
        }
        if (request.url === '/auth/logout') {
          return { status: 204 }
        }
        return { status: 200, body: itemBody(profile) }
      }
    })

    it('clears the session here and on the server', async () => {
      const store = useAuthStore()
      await store.signIn('ana', 'correct horse')

      await store.signOut()

      expect(store.isAuthenticated).toBe(false)
      expect(store.user).toBeNull()
      expect(window.localStorage.getItem(STORAGE_KEY)).toBeNull()
      expect(backend.requests[backend.requests.length - 1]).toMatchObject({
        url: '/auth/logout',
        body: { refreshToken: 'refresh-1' },
      })
    })

    it('still clears locally when the server call fails', async () => {
      const store = useAuthStore()
      await store.signIn('ana', 'correct horse')
      handler = () => ({ status: 500, body: errorBody('INTERNAL_ERROR', 'boom') })

      await store.signOut()

      expect(store.isAuthenticated).toBe(false)
    })
  })

  describe('refresh', () => {
    beforeEach(() => {
      window.localStorage.setItem(
        STORAGE_KEY,
        JSON.stringify({ accessToken: 'access-1', refreshToken: 'refresh-1' }),
      )
    })

    it('rotates the pair and keeps the new one', async () => {
      handler = () => ({ status: 200, body: itemBody(session(2)) })
      const store = useAuthStore()

      await expect(store.refresh()).resolves.toBe(true)

      expect(store.accessToken).toBe('access-2')
      expect(store.refreshToken).toBe('refresh-2')
      expect(backend.requests[0]).toMatchObject({
        url: '/auth/refresh',
        body: { refreshToken: 'refresh-1' },
      })
    })

    it('presents the refresh token exactly once when called concurrently', async () => {
      // Presenting an already-rotated token tells the backend the token was
      // stolen and it revokes the whole family. Two callers must therefore
      // share one rotation, not race.
      handler = () => ({ status: 200, body: itemBody(session(2)) })
      const store = useAuthStore()

      const [first, second] = await Promise.all([store.refresh(), store.refresh()])

      expect(first).toBe(true)
      expect(second).toBe(true)
      expect(backend.countOf('/auth/refresh')).toBe(1)
    })

    it('signs out instead of looping when the refresh is rejected', async () => {
      handler = () => unauthorized
      const store = useAuthStore()

      await expect(store.refresh()).resolves.toBe(false)

      expect(store.isAuthenticated).toBe(false)
      expect(store.refreshToken).toBeNull()
      expect(store.user).toBeNull()
      expect(window.localStorage.getItem(STORAGE_KEY)).toBeNull()
      // One attempt. A retry would present the same spent token again.
      expect(backend.countOf('/auth/refresh')).toBe(1)
    })

    it('signs out without calling the server when there is no refresh token', async () => {
      window.localStorage.clear()
      const store = useAuthStore()

      await expect(store.refresh()).resolves.toBe(false)

      expect(backend.countOf('/auth/refresh')).toBe(0)
    })

    it('allows a later refresh once the first has settled', async () => {
      let generation = 1
      handler = () => ({ status: 200, body: itemBody(session(++generation)) })
      const store = useAuthStore()

      await store.refresh()
      await store.refresh()

      expect(backend.countOf('/auth/refresh')).toBe(2)
      expect(store.accessToken).toBe('access-3')
    })
  })

  describe('concurrent 401s', () => {
    beforeEach(() => {
      window.localStorage.setItem(
        STORAGE_KEY,
        JSON.stringify({ accessToken: 'access-1', refreshToken: 'refresh-1' }),
      )
    })

    it('refreshes once and retries both requests', async () => {
      // The whole point of the single-flight latch, end to end: two expired
      // requests, one rotation.
      handler = (request) => {
        if (request.url === '/auth/refresh') {
          return { status: 200, body: itemBody(session(2)) }
        }

        return request.authorization === 'Bearer access-2'
          ? { status: 200, body: itemBody({ url: request.url }) }
          : unauthorized
      }

      const store = useAuthStore()
      // Touch the store so the bridge is registered before the requests fly.
      expect(store.isAuthenticated).toBe(true)

      const [documents, tasks] = await Promise.all([
        getItem<{ url: string }>('/documents'),
        getItem<{ url: string }>('/tasks'),
      ])

      expect(documents.url).toBe('/documents')
      expect(tasks.url).toBe('/tasks')
      expect(backend.countOf('/auth/refresh')).toBe(1)
      expect(backend.countOf('/documents')).toBe(2)
      expect(backend.countOf('/tasks')).toBe(2)
    })

    it('signs the user out when the shared refresh fails', async () => {
      handler = () => unauthorized

      const store = useAuthStore()
      expect(store.isAuthenticated).toBe(true)

      await expect(Promise.all([getItem('/documents'), getItem('/tasks')])).rejects.toMatchObject({
        status: 401,
      })

      expect(store.isAuthenticated).toBe(false)
      expect(backend.countOf('/auth/refresh')).toBe(1)
      // Neither request was retried after the refresh failed.
      expect(backend.countOf('/documents')).toBe(1)
      expect(backend.countOf('/tasks')).toBe(1)
    })
  })

  describe('ensureProfile', () => {
    it('loads the profile once for concurrent callers', async () => {
      window.localStorage.setItem(
        STORAGE_KEY,
        JSON.stringify({ accessToken: 'access-1', refreshToken: 'refresh-1' }),
      )
      handler = () => ({ status: 200, body: itemBody(profile) })
      const store = useAuthStore()

      const [first, second] = await Promise.all([store.ensureProfile(), store.ensureProfile()])

      expect(first).toBe(true)
      expect(second).toBe(true)
      expect(backend.countOf('/auth/me')).toBe(1)
      expect(store.user).toEqual(profile)
    })

    it('is false and harmless without a session', async () => {
      const store = useAuthStore()

      await expect(store.ensureProfile()).resolves.toBe(false)
      expect(backend.requests).toHaveLength(0)
    })

    it('clears the session when the profile cannot be loaded', async () => {
      window.localStorage.setItem(
        STORAGE_KEY,
        JSON.stringify({ accessToken: 'access-1', refreshToken: 'refresh-1' }),
      )
      handler = () => unauthorized
      const store = useAuthStore()

      await expect(store.ensureProfile()).resolves.toBe(false)

      expect(store.isAuthenticated).toBe(false)
    })
  })

  describe('can', () => {
    beforeEach(async () => {
      handler = (request) =>
        request.url === '/auth/login'
          ? { status: 200, body: itemBody(session(1)) }
          : { status: 200, body: itemBody(profile) }

      await useAuthStore().signIn('ana', 'correct horse')
    })

    it('is true for a granted permission', () => {
      expect(useAuthStore().can('document:read')).toBe(true)
    })

    it('is false for one that was not granted', () => {
      expect(useAuthStore().can('document:delete')).toBe(false)
    })

    it('matches exactly, mirroring the backend', () => {
      // A prefix never grants a longer permission, and vice versa.
      expect(useAuthStore().can('master-data:party')).toBe(false)
      expect(useAuthStore().can('document:read:all')).toBe(false)
    })
  })

  it('grants nothing before a profile is loaded', () => {
    expect(useAuthStore().can('document:read')).toBe(false)
  })

  /**
   * Only the server may end a session.
   *
   * Clearing on any failure meant a two-second connectivity drop wiped valid
   * tokens and forced a re-authentication. These pin the distinction: only a
   * refusal of the credentials — 401 or 403 — ends a session, and everything
   * else is retryable.
   */
  describe('a server that cannot be reached', () => {
    const offline: FakeReply = { status: 0, networkError: true }

    beforeEach(() => {
      window.localStorage.setItem(
        STORAGE_KEY,
        JSON.stringify({ accessToken: 'access-1', refreshToken: 'refresh-1' }),
      )
    })

    it('keeps the session when the profile call never arrives', async () => {
      handler = () => offline
      const store = useAuthStore()

      await expect(store.ensureProfile()).resolves.toBe(false)

      expect(store.isAuthenticated).toBe(true)
      expect(store.refreshToken).toBe('refresh-1')
      expect(window.localStorage.getItem(STORAGE_KEY)).not.toBeNull()
    })

    it('keeps the session when the refresh never arrives', async () => {
      handler = () => offline
      const store = useAuthStore()

      await expect(store.refresh()).resolves.toBe(false)

      expect(store.isAuthenticated).toBe(true)
      expect(store.refreshToken).toBe('refresh-1')
    })

    it('keeps the session when the server fails to process the refresh', async () => {
      // A 5xx is the backend falling over, not a judgement on the token.
      handler = () => ({ status: 500, body: errorBody('INTERNAL_ERROR', 'boom') })
      const store = useAuthStore()

      await expect(store.refresh()).resolves.toBe(false)

      expect(store.isAuthenticated).toBe(true)
    })

    it('keeps the session when the refresh times out server-side', async () => {
      // 408 is the exact class of failure this distinction exists to survive.
      // A range over 4xx would treat it as a refusal.
      handler = () => ({ status: 408, body: errorBody('TIMEOUT', 'Request timeout') })
      const store = useAuthStore()

      await expect(store.refresh()).resolves.toBe(false)

      expect(store.isAuthenticated).toBe(true)
      expect(store.refreshToken).toBe('refresh-1')
    })

    it('keeps the session when the refresh is rate limited', async () => {
      // The authentication rate limiter reaches /auth/refresh, so signing out
      // here would punish the user for being throttled.
      handler = () => ({
        status: 429,
        body: errorBody('TOO_MANY_REQUESTS', 'Too many attempts. Try again in 60 seconds'),
      })
      const store = useAuthStore()

      await expect(store.refresh()).resolves.toBe(false)

      expect(store.isAuthenticated).toBe(true)
      expect(store.refreshToken).toBe('refresh-1')
    })

    it('signs out when the profile call is forbidden', async () => {
      // 403 is the other genuine refusal: the account is no longer allowed in.
      handler = () => ({ status: 403, body: errorBody('FORBIDDEN', 'Forbidden') })
      const store = useAuthStore()

      await expect(store.ensureProfile()).resolves.toBe(false)

      expect(store.isAuthenticated).toBe(false)
      expect(window.localStorage.getItem(STORAGE_KEY)).toBeNull()
    })

    it('still signs out when the server rejects the refresh', async () => {
      handler = () => unauthorized
      const store = useAuthStore()

      await expect(store.refresh()).resolves.toBe(false)

      expect(store.isAuthenticated).toBe(false)
    })

    it('recovers once the server is reachable again', async () => {
      // The point of keeping the tokens: the caller retries and is back in,
      // with no credentials retyped.
      let reachable = false
      handler = () => (reachable ? { status: 200, body: itemBody(profile) } : offline)
      const store = useAuthStore()

      await expect(store.ensureProfile()).resolves.toBe(false)

      reachable = true

      await expect(store.ensureProfile()).resolves.toBe(true)
      expect(store.user).toEqual(profile)
    })
  })

  /**
   * The in-process latch covers one store instance, and every tab has its own.
   * Without these, tab A rotates, tab B presents the token A has already spent,
   * and the backend correctly reads the replay as theft — killing both.
   */
  describe('another tab', () => {
    function storeSession(generation: number): void {
      window.localStorage.setItem(
        STORAGE_KEY,
        JSON.stringify({
          accessToken: `access-${generation}`,
          refreshToken: `refresh-${generation}`,
        }),
      )
    }

    function announceStorageChange(): void {
      // The listener re-reads storage rather than trusting `newValue`, so the
      // event only has to name the key.
      window.dispatchEvent(new StorageEvent('storage', { key: STORAGE_KEY }))
    }

    beforeEach(() => {
      storeSession(1)
    })

    it('adopts a pair another tab has rotated', async () => {
      const store = useAuthStore()
      expect(store.refreshToken).toBe('refresh-1')

      storeSession(2)
      announceStorageChange()

      expect(store.accessToken).toBe('access-2')
      expect(store.refreshToken).toBe('refresh-2')
    })

    it('does not spend a token another tab has already replaced', async () => {
      const store = useAuthStore()

      // The other tab rotated between this tab's 401 and its refresh.
      storeSession(2)

      await expect(store.refresh()).resolves.toBe(true)

      // Presenting refresh-1 here is what would have killed the family.
      expect(backend.countOf('/auth/refresh')).toBe(0)
      expect(store.refreshToken).toBe('refresh-2')
    })

    it('signs out here when another tab signs out', async () => {
      const store = useAuthStore()

      window.localStorage.clear()
      announceStorageChange()

      expect(store.isAuthenticated).toBe(false)
      expect(store.user).toBeNull()
    })

    it('ignores a change to somebody else’s storage key', async () => {
      const store = useAuthStore()

      window.localStorage.setItem('unrelated', 'value')
      window.dispatchEvent(new StorageEvent('storage', { key: 'unrelated' }))

      expect(store.refreshToken).toBe('refresh-1')
    })

    it('serialises the rotation across tabs when Web Locks is available', async () => {
      // jsdom has no lock manager, so this asserts the store asks for one by
      // name — the mutual exclusion itself is the browser's job.
      const requested: string[] = []
      const locks = {
        request: (name: string, callback: () => Promise<boolean>) => {
          requested.push(name)
          return callback()
        },
      }
      Object.defineProperty(window.navigator, 'locks', { value: locks, configurable: true })

      handler = () => ({ status: 200, body: itemBody(session(2)) })

      try {
        await expect(useAuthStore().refresh()).resolves.toBe(true)
        expect(requested).toEqual(['kelir.auth.refresh'])
      } finally {
        Reflect.deleteProperty(window.navigator, 'locks')
      }
    })
  })
})
