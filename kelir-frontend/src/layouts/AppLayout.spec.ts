import { createPinia, setActivePinia } from 'pinia'
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'

import AppLayout from './AppLayout.vue'
import { registerSessionBridge } from '@/api/session'
import {
  installFakeBackend,
  itemBody,
  pageBody,
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
  let menus: unknown[]
  let router: Router

  beforeEach(() => {
    setActivePinia(createPinia())
    window.localStorage.clear()

    permissions = []
    menus = []
    backend = installFakeBackend((request): FakeReply => {
      if (request.url.startsWith('/rad/menus')) {
        return { status: 200, body: pageBody(menus) }
      }

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
        { path: '/admin/external-systems', name: 'admin-external-systems', component: blank },
        { path: '/admin/integration-logs', name: 'admin-integration-logs', component: blank },
        { path: '/admin/workflows', name: 'admin-workflows', component: blank },
        { path: '/admin/menus', name: 'admin-menus', component: blank },
        { path: '/forms', component: blank },
        { path: '/mystery', component: blank },
        { path: '/approvals', component: blank },
        { path: '/reports', component: blank },
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

  it('links to the external system registry only with its read permission', async () => {
    // #520. Hidden without the grant; the route guard and the API are what refuse.
    expect((await renderSignedIn()).find('a[href="/admin/external-systems"]').exists()).toBe(false)

    permissions = ['integration:external-system:read']

    expect((await renderSignedIn()).find('a[href="/admin/external-systems"]').exists()).toBe(true)
  })

  it('links to the integration log only with its own read permission', async () => {
    // #548 AC7. The registry's permission does not reveal the log, nor the reverse.
    permissions = ['integration:external-system:read']

    expect((await renderSignedIn()).find('a[href="/admin/integration-logs"]').exists()).toBe(false)

    permissions = ['integration:log:read']
    const wrapper = await renderSignedIn()

    expect(wrapper.find('a[href="/admin/integration-logs"]').exists()).toBe(true)
    expect(wrapper.find('a[href="/admin/external-systems"]').exists()).toBe(false)
  })

  it('links to the workflows only with their read permission', async () => {
    // #426, test-engineer campaign 2026-10-10. Writing one is not reading the list.
    permissions = ['workflow:definition:create', 'workflow:definition:update']

    expect((await renderSignedIn()).find('a[href="/admin/workflows"]').exists()).toBe(false)

    permissions = ['workflow:definition:read']

    expect((await renderSignedIn()).find('a[href="/admin/workflows"]').exists()).toBe(true)
  })

  function menuEntry(id: string, label: string, icon: string, sortOrder: number) {
    return {
      id,
      menuKey: id,
      label,
      icon,
      parentMenuId: null,
      routePath: `/${id}`,
      requiredPermission: null,
      source: 'CONFIG',
      sortOrder,
      isEnabled: true,
      createdAt: '2026-10-10T00:00:00Z',
      updatedAt: '2026-10-10T00:00:00Z',
    }
  }

  /** Lets each configured icon's own chunk load, as a browser fetches it. */
  async function iconsLoaded(): Promise<void> {
    for (let round = 0; round < 4; round += 1) {
      await vi.dynamicImportSettled()
      await flushPromises()
    }
  }

  /**
   * **#698: a configured entry's `icon` is looked up by name in `@lucide/vue`**,
   * converted from kebab-case as the namespace lookup did. Since #688's fix to
   * the swap's first-load growth, each icon is loaded on its own
   * (`menuIcon.ts`), so a renamed export or a broken loader would turn every
   * tenant's icon into the fallback with nothing failing but this.
   */
  it('draws a configured entry with the Lucide icon its name gives', async () => {
    menus = [
      menuEntry('forms', 'Forms', 'file-cog', 900),
      menuEntry('mystery', 'Mystery', 'not-an-icon-anywhere', 910),
    ]
    permissions = ['rad:menu:read']

    const wrapper = await renderSignedIn()

    await iconsLoaded()

    const named = wrapper.find('[data-testid="nav-config:forms"] svg')
    const unknown = wrapper.find('[data-testid="nav-config:mystery"] svg')

    expect(named.classes()).toContain('lucide-file-cog')
    // An unknown name falls back to the neutral icon rather than to nothing.
    expect(unknown.classes()).toContain('lucide-circle')
  })

  it('loads a menu icon on its own, after the menu renders, and resolves an old name', async () => {
    menus = [
      menuEntry('approvals', 'Approvals', 'alert-triangle', 900),
      menuEntry('reports', 'Reports', 'ChartBar', 910),
    ]
    permissions = ['rad:menu:read']

    const wrapper = await renderSignedIn()

    await flushPromises()

    // The entries are drawn before their icons arrive: the icon is a chunk of
    // its own, not part of the layout.
    expect(wrapper.find('[data-testid="nav-config:approvals"]').text()).toContain('Approvals')

    await iconsLoaded()

    // `alert-triangle` is the old name `@lucide/vue` still exports for
    // `triangle-alert`; a PascalCase name passes through as it always did.
    expect(wrapper.find('[data-testid="nav-config:approvals"] svg').classes()).toContain(
      'lucide-triangle-alert',
    )
    expect(wrapper.find('[data-testid="nav-config:reports"] svg').classes()).toContain(
      'lucide-chart-bar',
    )
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
