import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'

import { leaveDeadPage, registerGuards, safeReturnPath } from './guards'
import { registerSessionBridge } from '@/api/session'
import { notifySessionLost, resetSessionListeners } from '@/stores/session-events'
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

/**
 * Let every pending navigation finish.
 *
 * `leaveDeadPage` starts one and does not await it — it is called from a
 * listener, which has nobody to hand a promise to — so a test has to wait for
 * the router rather than for the call.
 */
function settled(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 0))
}

/** A router whose routes match the real table closely enough for the redirect. */
function routerAt(path: string): Promise<Router> {
  const router = testRouter()

  return router.push(path).then(() => router)
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
    resetSessionListeners()
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

describe('leaving a dead page (#68)', () => {
  // The guard runs on navigation. A session lost while the user is sitting on a
  // page — a refresh refused, a token revoked server-side, an administrator
  // deactivating the account, another tab signing out — moves nothing, so
  // nothing redirects them. They stay on a page whose data is stale and whose
  // every action will fail: the administrator editing a role submits the form,
  // gets a 401, and has no idea why.
  let backend: FakeBackendHandle
  let handler: FakeHandler

  beforeEach(() => {
    setActivePinia(createPinia())
    window.localStorage.clear()
    resetSessionListeners()
    handler = () => ({ status: 200, body: itemBody(profile) })
    backend = installFakeBackend((request) => handler(request))
  })

  afterEach(() => {
    backend.restore()
    registerSessionBridge(null)
    resetSessionListeners()
  })

  it('redirects off a protected page when the session ends without a navigation', async () => {
    storeSession()
    const router = await routerAt('/documents/7?tab=history')
    expect(router.currentRoute.value.name).toBe('document-detail')

    useAuthStore().clearSession()
    notifySessionLost()
    await settled()

    expect(router.currentRoute.value.name).toBe('login')
    expect(router.currentRoute.value.query.redirect).toBe('/documents/7?tab=history')
    expect(router.currentRoute.value.query.sessionEnded).toBe('1')
  })

  it('leaves a caller on a public page alone', async () => {
    // Somewhere they are entitled to be with no session at all — a password
    // reset opened from an email is the case that matters. Yanking them off it
    // would interrupt what they came to do.
    const router = await routerAt('/about')

    notifySessionLost()
    await settled()

    expect(router.currentRoute.value.name).toBe('about')
  })

  it('leaves a caller already on the login page alone', async () => {
    // They have arrived. Replacing the route would also replace the state the
    // page is reading to decide what to tell them.
    const router = await routerAt('/login')

    notifySessionLost()
    await settled()

    expect(router.currentRoute.value.name).toBe('login')
    expect(router.currentRoute.value.query.sessionEnded).toBeUndefined()
  })

  it('keeps the target the caller asked for when the session dies mid-navigation', async () => {
    // The case where both halves of #68 fire at once: the caller is on `/`,
    // clicks through to `/documents/7`, and the profile fetch behind that
    // navigation 401s. The store announces the loss from inside `authGuard`'s
    // own await, so `leaveDeadPage` starts a navigation while one is already
    // being decided.
    //
    // What must survive is the target. `leaveDeadPage` knows only where the
    // caller *was*; the guard knows where they asked to go, and that is the one
    // they should return to after signing in. Both navigations end at the login
    // page and the guard's redirect is applied last, which is why this holds
    // without any co-ordination between them — see `leaveDeadPage`'s note on
    // the stand-down that was tried and removed.
    storeSession()
    const router = await routerAt('/')
    expect(router.currentRoute.value.name).toBe('dashboard')

    handler = () => ({ status: 401, body: errorBody('UNAUTHORIZED', 'Authentication required') })
    // The profile is cached after the first navigation, so the second would
    // never ask the server. Dropping it — and only it — is what a revoked
    // session looks like from here: the tokens are still in hand and the next
    // call is the one that finds out they are worthless.
    useAuthStore().user = null

    await router.push('/documents/7')
    await settled()

    expect(router.currentRoute.value.name).toBe('login')
    expect(router.currentRoute.value.query.redirect).toBe('/documents/7')
  })

  it('does nothing when there is no router navigation to make', async () => {
    // `leaveDeadPage` is called directly here rather than through the listener,
    // so a router that has never navigated cannot throw on the way out.
    const router = testRouter()

    expect(() => leaveDeadPage(router)).not.toThrow()
  })
})

describe('the session-ended marker', () => {
  let backend: FakeBackendHandle
  let handler: FakeHandler

  beforeEach(() => {
    setActivePinia(createPinia())
    window.localStorage.clear()
    resetSessionListeners()
    handler = () => ({ status: 200, body: itemBody(profile) })
    backend = installFakeBackend((request) => handler(request))
  })

  afterEach(() => {
    backend.restore()
    registerSessionBridge(null)
    resetSessionListeners()
  })

  it('is set when a caller who had a session is sent to sign in again', async () => {
    storeSession()
    handler = () => ({ status: 401, body: errorBody('UNAUTHORIZED', 'Authentication required') })
    const router = testRouter()

    await router.push('/documents/7')

    expect(router.currentRoute.value.name).toBe('login')
    expect(router.currentRoute.value.query.sessionEnded).toBe('1')
  })

  it('is absent for a caller who never had one', async () => {
    // A deep link opened by somebody who is simply not signed in. The two
    // arrivals are identical apart from this, and only one needs explaining.
    const router = testRouter()

    await router.push('/documents/7')

    expect(router.currentRoute.value.name).toBe('login')
    expect(router.currentRoute.value.query.sessionEnded).toBeUndefined()
    expect(router.currentRoute.value.query.redirect).toBe('/documents/7')
  })
})
