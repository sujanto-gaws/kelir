import { createPinia, setActivePinia } from 'pinia'
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'

import AppLayout from './AppLayout.vue'
import { registerSessionBridge } from '@/api/session'
import {
  installFakeBackend,
  itemBody,
  type FakeBackendHandle,
  type FakeReply,
} from '@/lib/testing/fake-backend'
import { useAuthStore } from '@/stores/auth'

const blank = { template: '<div />' }

function profileBody(permissions: string[]): unknown {
  return itemBody({
    id: 'u-1',
    username: 'ana',
    displayName: 'Ana Putri',
    email: 'ana@example.com',
    roles: ['clerk'],
    permissions,
  })
}

describe('AppLayout', () => {
  let backend: FakeBackendHandle
  let permissions: string[]
  let router: Router

  beforeEach(() => {
    setActivePinia(createPinia())
    window.localStorage.clear()

    permissions = []
    backend = installFakeBackend((request): FakeReply => {
      if (request.url === '/auth/login') {
        return {
          status: 200,
          body: itemBody({
            accessToken: 'access-1',
            refreshToken: 'refresh-1',
            tokenType: 'Bearer',
            expiresIn: 900,
            userId: 'u-1',
            username: 'ana',
          }),
        }
      }

      if (request.url === '/auth/logout') {
        return { status: 204 }
      }

      return { status: 200, body: profileBody(permissions) }
    })

    router = createRouter({
      history: createMemoryHistory(),
      routes: [
        { path: '/', name: 'dashboard', component: blank },
        { path: '/login', name: 'login', component: blank },
        // Real as of #101: the entry is a link now rather than a disabled
        // label, and `RouterLink` cannot resolve a name the router has never
        // heard of.
        {
          path: '/master-data/:view(parties|suppliers|customers|employees)?',
          name: 'master-data',
          component: blank,
        },
        { path: '/admin/users', name: 'admin-users', component: blank },
        { path: '/admin/roles', name: 'admin-roles', component: blank },
      ],
    })
  })

  afterEach(() => {
    backend.restore()
    registerSessionBridge(null)
  })

  async function renderSignedIn(): Promise<VueWrapper> {
    await useAuthStore().signIn('ana', 'correct horse')
    await router.push('/')
    await router.isReady()

    return mount(AppLayout, { global: { plugins: [router] } })
  }

  it('hides a destination the user has no permission for', async () => {
    const wrapper = await renderSignedIn()

    expect(wrapper.text()).toContain('Dashboard')
    expect(wrapper.text()).not.toContain('Master Data')
  })

  it('shows it once the permission is granted', async () => {
    // Cosmetic only: the backend re-checks every request either way.
    permissions = ['master-data:party:read']
    const wrapper = await renderSignedIn()

    expect(wrapper.text()).toContain('Master Data')
  })

  it('hides the administration entries from a caller who cannot use them', async () => {
    const wrapper = await renderSignedIn()

    expect(wrapper.find('a[href="/admin/users"]').exists()).toBe(false)
    expect(wrapper.find('a[href="/admin/roles"]').exists()).toBe(false)
  })

  it('links to each administration screen the caller may read', async () => {
    // The two are separate grants, so holding one must not reveal the other.
    permissions = ['identity:user:read']
    const wrapper = await renderSignedIn()

    expect(wrapper.find('a[href="/admin/users"]').exists()).toBe(true)
    expect(wrapper.find('a[href="/admin/roles"]').exists()).toBe(false)
  })

  it('names the signed-in user', async () => {
    const wrapper = await renderSignedIn()

    expect(wrapper.text()).toContain('Ana Putri')
  })

  it('signs out and returns to the login page', async () => {
    const wrapper = await renderSignedIn()

    await wrapper.find('button[aria-label="Sign out"]').trigger('click')
    await flushPromises()

    expect(useAuthStore().isAuthenticated).toBe(false)
    expect(router.currentRoute.value.name).toBe('login')
    expect(backend.requests.some((request) => request.url === '/auth/logout')).toBe(true)
  })
})
