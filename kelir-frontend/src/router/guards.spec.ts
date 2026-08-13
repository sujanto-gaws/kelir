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
