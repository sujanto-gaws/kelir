import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'

import { registerGuards, safeReturnPath } from './guards'
import { registerSessionBridge } from '@/api/session'
import {
  errorBody,
  installFakeBackend,
  itemBody,
  type FakeBackendHandle,
  type FakeHandler,
} from '@/lib/testing/fake-backend'
import { useAuthStore } from '@/stores/auth'

const STORAGE_KEY = 'kelir.auth'

const blank = { template: '<div />' }

const profile = {
  id: 'u-1',
  username: 'ana',
  displayName: 'Ana Putri',
  email: 'ana@example.com',
  roles: ['clerk'],
  permissions: ['document:read'],
}

function testRouter(): Router {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/login', name: 'login', component: blank, meta: { requiresAuth: false } },
      { path: '/', name: 'dashboard', component: blank, meta: { requiresAuth: true } },
      {
        // A protected child under an unannotated parent: Vue Router merges meta
        // down the chain, so the guard must see the child's flag.
        path: '/documents',
        component: blank,
        children: [
          {
            path: ':id',
            name: 'document-detail',
            component: blank,
            meta: { requiresAuth: true },
          },
        ],
      },
      { path: '/about', name: 'about', component: blank, meta: { requiresAuth: false } },
    ],
  })

  registerGuards(router)

  return router
}

function storeSession(): void {
  window.localStorage.setItem(
    STORAGE_KEY,
    JSON.stringify({ accessToken: 'access-1', refreshToken: 'refresh-1' }),
  )
}

describe('safeReturnPath', () => {
  it('accepts a same-site path with its query', () => {
    expect(safeReturnPath({ redirect: '/documents/7?tab=history' })).toBe(
      '/documents/7?tab=history',
    )
  })

  it('rejects a protocol-relative url', () => {
    // `//evil.example` is a valid query value and a valid off-site URL: honoured
    // blindly it turns our own login page into a phishing redirector.
    expect(safeReturnPath({ redirect: '//evil.example/pwned' })).toBeNull()
  })

  it('rejects an absolute url', () => {
    expect(safeReturnPath({ redirect: 'https://evil.example/pwned' })).toBeNull()
  })

  it('is null when absent or repeated in a way we cannot trust', () => {
    expect(safeReturnPath({})).toBeNull()
    expect(safeReturnPath({ redirect: null })).toBeNull()
    expect(safeReturnPath({ redirect: ['/first', '/second'] })).toBe('/first')
  })
})

describe('authGuard', () => {
  let backend: FakeBackendHandle
  let handler: FakeHandler
  let router: Router

  beforeEach(() => {
    setActivePinia(createPinia())
    window.localStorage.clear()

    handler = () => ({ status: 200, body: itemBody(profile) })
    backend = installFakeBackend((request) => handler(request))
    router = testRouter()
  })

  afterEach(() => {
    backend.restore()
    registerSessionBridge(null)
  })

  it('sends an unauthenticated caller to the login page', async () => {
    await router.push('/')

    expect(router.currentRoute.value.name).toBe('login')
  })

  it('remembers where they were headed, including nested routes and query', async () => {
    await router.push('/documents/7?tab=history')

    expect(router.currentRoute.value.name).toBe('login')
    expect(router.currentRoute.value.query.redirect).toBe('/documents/7?tab=history')
  })

  it('lets an authenticated caller through and loads the profile once', async () => {
    storeSession()

    await router.push('/')

    expect(router.currentRoute.value.name).toBe('dashboard')
    expect(useAuthStore().user).toEqual(profile)
    expect(backend.countOf('/auth/me')).toBe(1)

    await router.push('/documents/7')

    expect(router.currentRoute.value.name).toBe('document-detail')
    expect(backend.countOf('/auth/me')).toBe(1)
  })

  it('leaves public routes alone', async () => {
    await router.push('/about')

    expect(router.currentRoute.value.name).toBe('about')
    expect(backend.requests).toHaveLength(0)
  })

  it('sends a signed-in caller away from the login page', async () => {
    storeSession()

    await router.push('/login')

    expect(router.currentRoute.value.name).toBe('dashboard')
  })

  it('keeps a caller on the login page when the stored token is dead', async () => {
    // A token in storage is only a claim; the guard settles it against the
    // server rather than bouncing the user into an app they cannot use.
    storeSession()
    handler = () => ({ status: 401, body: errorBody('UNAUTHORIZED', 'Authentication required') })

    await router.push('/login')

    expect(router.currentRoute.value.name).toBe('login')
    expect(useAuthStore().isAuthenticated).toBe(false)
  })

  it('signs out and redirects when a stored token can no longer be renewed', async () => {
    storeSession()
    handler = () => ({ status: 401, body: errorBody('UNAUTHORIZED', 'Authentication required') })

    await router.push('/documents/7')

    expect(router.currentRoute.value.name).toBe('login')
    expect(router.currentRoute.value.query.redirect).toBe('/documents/7')
    expect(useAuthStore().isAuthenticated).toBe(false)
    expect(window.localStorage.getItem(STORAGE_KEY)).toBeNull()
  })
})

describe('authGuard permission gate', () => {
  let backend: FakeBackendHandle
  let handler: FakeHandler

  function gatedRouter(): Router {
    const router = createRouter({
      history: createMemoryHistory(),
      routes: [
        { path: '/login', name: 'login', component: blank, meta: { requiresAuth: false } },
        { path: '/', name: 'dashboard', component: blank, meta: { requiresAuth: true } },
        { path: '/forbidden', name: 'forbidden', component: blank, meta: { requiresAuth: true } },
        {
          path: '/admin/users',
          name: 'admin-users',
          component: blank,
          meta: { requiresAuth: true, permission: 'identity:user:read' },
        },
        {
          // Deliberately misconfigured: a permission with no `requiresAuth`.
          // This is the shape a new admin route is most likely to arrive in,
          // and it used to fall straight through the guard to `return true`.
          path: '/admin/misconfigured',
          name: 'admin-misconfigured',
          component: blank,
          meta: { permission: 'identity:role:read' },
        },
      ],
    })

    registerGuards(router)

    return router
  }

  beforeEach(() => {
    setActivePinia(createPinia())
    window.localStorage.clear()

    handler = () => ({ status: 200, body: itemBody(profile) })
    backend = installFakeBackend((request) => handler(request))
  })

  afterEach(() => {
    backend.restore()
    registerSessionBridge(null)
  })

  it('lets a caller holding the permission through', async () => {
    storeSession()
    handler = () => ({
      status: 200,
      body: itemBody({ ...profile, permissions: ['identity:user:read'] }),
    })

    const router = gatedRouter()
    await router.push('/admin/users')

    expect(router.currentRoute.value.name).toBe('admin-users')
  })

  it('stops a caller who lacks it, even on a pasted url', async () => {
    // Hiding the nav entry is not enough: the route is reachable by typing it.
    storeSession()

    const router = gatedRouter()
    await router.push('/admin/users')

    expect(router.currentRoute.value.name).toBe('forbidden')
  })

  it('leaves a protected route without a permission requirement alone', async () => {
    storeSession()

    const router = gatedRouter()
    await router.push('/')

    expect(router.currentRoute.value.name).toBe('dashboard')
  })

  it('asks an unauthenticated caller to sign in before judging permissions', async () => {
    // Order matters: sending them to /forbidden would tell them to ask for a
    // role when what they actually need is to sign in.
    const router = gatedRouter()
    await router.push('/admin/users')

    expect(router.currentRoute.value.name).toBe('login')
    expect(router.currentRoute.value.query.redirect).toBe('/admin/users')
  })

  /**
   * A route naming a permission but omitting `requiresAuth` is a
   * misconfiguration, and it used to be an open door: the guard only consulted
   * `meta.permission` inside the `requiresAuth` branch, so such a route fell
   * through to `return true` and rendered for anyone — silently, and with a
   * green suite.
   *
   * The guard now derives the session requirement from the permission, so the
   * mistake fails closed instead.
   */
  describe('a route naming a permission without requiresAuth', () => {
    it('still demands a session', async () => {
      const router = gatedRouter()
      await router.push('/admin/misconfigured')

      expect(router.currentRoute.value.name).toBe('login')
      expect(router.currentRoute.value.query.redirect).toBe('/admin/misconfigured')
    })

    it('still refuses a caller who lacks the permission', async () => {
      storeSession()

      const router = gatedRouter()
      await router.push('/admin/misconfigured')

      expect(router.currentRoute.value.name).toBe('forbidden')
    })

    it('admits a caller who holds it', async () => {
      storeSession()
      handler = () => ({
        status: 200,
        body: itemBody({ ...profile, permissions: ['identity:role:read'] }),
      })

      const router = gatedRouter()
      await router.push('/admin/misconfigured')

      expect(router.currentRoute.value.name).toBe('admin-misconfigured')
    })
  })
})
