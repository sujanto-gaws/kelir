import { createPinia, setActivePinia } from 'pinia'
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'

import LoginPage from './LoginPage.vue'
import { fetchDeployment } from '@/api/deployment'
import { registerSessionBridge } from '@/api/session'
import {
  errorBody,
  installFakeBackend,
  itemBody,
  type FakeBackendHandle,
  type FakeHandler,
} from '@/lib/testing/fake-backend'
import { useAuthStore } from '@/stores/auth'

/**
 * `GET /deployment` is an *operational* endpoint: it sits at the root, outside
 * `/api/v1`, and the fake backend cannot see it — that helper replaces
 * `apiClient`'s adapter, and this call deliberately does not go through
 * `apiClient`. So it is mocked at the module boundary.
 *
 * The default rejection is not laziness. It is the exact condition the page has
 * to survive: the probe failing tells the form nothing about which mode the
 * deployment is in, and every test outside the tenancy block below asserts that
 * a failed probe leaves the single-tenant form completely unchanged.
 */
vi.mock('@/api/deployment', () => ({
  fetchDeployment: vi.fn(() => Promise.reject(new Error('unreachable'))),
}))

const deploymentProbe = vi.mocked(fetchDeployment)

const blank = { template: '<div />' }

const sessionBody = itemBody({
  accessToken: 'access-1',
  refreshToken: 'refresh-1',
  tokenType: 'Bearer',
  expiresIn: 900,
  userId: 'u-1',
  username: 'ana',
})

const profileBody = itemBody({
  id: 'u-1',
  username: 'ana',
  displayName: 'Ana Putri',
  email: 'ana@example.com',
  roles: ['clerk'],
  permissions: ['document:read'],
})

const signInSucceeds: FakeHandler = (request) =>
  request.url === '/auth/login'
    ? { status: 200, body: sessionBody }
    : { status: 200, body: profileBody }

describe('LoginPage', () => {
  let backend: FakeBackendHandle
  let handler: FakeHandler
  let router: Router

  beforeEach(() => {
    setActivePinia(createPinia())
    window.localStorage.clear()

    handler = signInSucceeds
    backend = installFakeBackend((request) => handler(request))
    deploymentProbe.mockRejectedValue(new Error('unreachable'))

    router = createRouter({
      history: createMemoryHistory(),
      routes: [
        { path: '/login', name: 'login', component: blank },
        { path: '/forgot-password', name: 'forgot-password', component: blank },
        { path: '/', name: 'dashboard', component: blank },
        { path: '/documents/:id', name: 'document-detail', component: blank },
      ],
    })
  })

  afterEach(() => {
    backend.restore()
    registerSessionBridge(null)
    vi.useRealTimers()
  })

  async function renderAt(path = '/login'): Promise<VueWrapper> {
    await router.push(path)
    await router.isReady()

    return mount(LoginPage, { global: { plugins: [router] } })
  }

  async function signIn(wrapper: VueWrapper, password = 'correct horse'): Promise<void> {
    await wrapper.find('#username').setValue('ana')
    await wrapper.find('#password').setValue(password)
    await wrapper.find('form').trigger('submit')
    await flushPromises()
  }

  describe('before anything is sent', () => {
    it('reports both fields as required when submitted empty', async () => {
      const wrapper = await renderAt()

      await wrapper.find('form').trigger('submit')

      expect(wrapper.text()).toContain('Enter your username or email address')
      expect(wrapper.text()).toContain('Enter your password')
      expect(backend.requests).toHaveLength(0)
    })

    it('marks invalid inputs for assistive technology', async () => {
      const wrapper = await renderAt()

      await wrapper.find('form').trigger('submit')

      expect(wrapper.find('#username').attributes('aria-invalid')).toBe('true')
    })

    it('uses a password input so the value is masked', async () => {
      const wrapper = await renderAt()

      expect(wrapper.find('#password').attributes('type')).toBe('password')
    })
  })

  describe('signing in', () => {
    it('establishes the session and goes to the dashboard', async () => {
      const wrapper = await renderAt()

      await signIn(wrapper)

      expect(useAuthStore().isAuthenticated).toBe(true)
      expect(router.currentRoute.value.name).toBe('dashboard')
    })

    it('returns the caller to where the guard stopped them', async () => {
      const wrapper = await renderAt('/login?redirect=/documents/7')

      await signIn(wrapper)

      expect(router.currentRoute.value.fullPath).toBe('/documents/7')
    })

    it('ignores an off-site return path', async () => {
      // An open redirect out of our own login page would be a phishing hand-off.
      const wrapper = await renderAt('/login?redirect=//evil.example/pwned')

      await signIn(wrapper)

      expect(router.currentRoute.value.name).toBe('dashboard')
    })
  })

  describe('when the server refuses', () => {
    it('shows one generic message for bad credentials', async () => {
      handler = () => ({ status: 401, body: errorBody('UNAUTHORIZED', 'Authentication required') })
      const wrapper = await renderAt()

      await signIn(wrapper, 'wrong')

      expect(wrapper.text()).toContain('Your username or password is not correct.')
      // The backend's own wording is for API callers, and naming the failing
      // half would confirm whether an account exists.
      expect(wrapper.text()).not.toContain('Authentication required')
      expect(router.currentRoute.value.name).toBe('login')
    })

    it('clears the password so the next attempt starts clean', async () => {
      handler = () => ({ status: 401, body: errorBody('UNAUTHORIZED', 'Authentication required') })
      const wrapper = await renderAt()

      await signIn(wrapper, 'wrong')

      expect((wrapper.find('#password').element as HTMLInputElement).value).toBe('')
    })

    it('puts envelope validation details against the fields they name', async () => {
      handler = () => ({
        status: 422,
        body: errorBody('VALIDATION_ERROR', 'Validation failed', [
          {
            path: 'username',
            rule: 'required',
            code: 'REQUIRED',
            message: 'Username is required',
          },
          {
            path: 'password',
            rule: 'minLength',
            code: 'TOO_SHORT',
            message: 'Password is too short',
          },
        ]),
      })
      const wrapper = await renderAt()

      await signIn(wrapper)

      expect(wrapper.find('#username-error').text()).toBe('Username is required')
      expect(wrapper.find('#password-error').text()).toBe('Password is too short')
      expect(wrapper.find('#username').attributes('aria-invalid')).toBe('true')
    })

    it('still surfaces a detail that names no field of ours', async () => {
      handler = () => ({
        status: 422,
        body: errorBody('VALIDATION_ERROR', 'Validation failed', [
          { path: 'tenant', rule: 'required', code: 'REQUIRED', message: 'Tenant is required' },
        ]),
      })
      const wrapper = await renderAt()

      await signIn(wrapper)

      expect(wrapper.text()).toContain('Validation failed')
    })

    it('reports a network failure rather than failing silently', async () => {
      handler = () => ({
        status: 500,
        body: errorBody('INTERNAL_ERROR', 'An unexpected error occurred'),
      })
      const wrapper = await renderAt()

      await signIn(wrapper)

      expect(wrapper.text()).toContain('An unexpected error occurred')
    })
  })

  describe('when rate limited', () => {
    const rateLimited = {
      status: 429,
      body: errorBody('TOO_MANY_REQUESTS', 'Too many attempts. Try again in 30 seconds.'),
    }

    it("shows the backend's wait instead of a bare error", async () => {
      handler = () => rateLimited
      const wrapper = await renderAt()

      await signIn(wrapper)

      expect(wrapper.text()).toContain('Too many attempts. Try again in 30 seconds.')
    })

    it('holds the button closed for the wait, then lets them try again', async () => {
      vi.useFakeTimers()
      handler = () => rateLimited
      const wrapper = await renderAt()

      await signIn(wrapper)

      const button = wrapper.find('button[type="submit"]')
      expect(button.attributes('disabled')).toBeDefined()
      expect(button.text()).toContain('Try again in 30s')

      vi.advanceTimersByTime(30_000)
      await wrapper.vm.$nextTick()

      expect(wrapper.find('button[type="submit"]').attributes('disabled')).toBeUndefined()
      expect(wrapper.find('button[type="submit"]').text()).toBe('Sign in')
    })

    it('sends nothing while the wait is running', async () => {
      vi.useFakeTimers()
      handler = () => rateLimited
      const wrapper = await renderAt()

      await signIn(wrapper)
      const afterFirstAttempt = backend.requests.length

      await wrapper.find('form').trigger('submit')
      await wrapper.vm.$nextTick()

      expect(backend.requests).toHaveLength(afterFirstAttempt)
    })
  })

  /**
   * A transport failure now leaves the tokens in place, so the guard can land a
   * caller here whose session is perfectly good. Asking them to retype
   * credentials they never lost would be the wrong remedy.
   */
  describe('arriving with a session that could not be confirmed', () => {
    beforeEach(() => {
      window.localStorage.setItem(
        'kelir.auth',
        JSON.stringify({ accessToken: 'access-1', refreshToken: 'refresh-1' }),
      )
    })

    it('offers a retry instead of the sign-in form', async () => {
      const wrapper = await renderAt()

      expect(wrapper.text()).toContain('You are still signed in')
      expect(wrapper.find('form').exists()).toBe(false)
    })

    it('goes where the caller was headed once the server answers', async () => {
      handler = () => ({ status: 200, body: profileBody })
      const wrapper = await renderAt('/login?redirect=/documents/7')

      await wrapper.findAll('button')[0].trigger('click')
      await flushPromises()

      expect(router.currentRoute.value.fullPath).toBe('/documents/7')
    })

    it('says so and stays put when the server is still unreachable', async () => {
      handler = () => ({ status: 0, networkError: true })
      const wrapper = await renderAt()

      await wrapper.findAll('button')[0].trigger('click')
      await flushPromises()

      expect(wrapper.text()).toContain('Still could not reach the server')
      expect(useAuthStore().isAuthenticated).toBe(true)
    })

    it('falls back to the form when the server rejects the session', async () => {
      handler = () => ({
        status: 401,
        body: errorBody('UNAUTHORIZED', 'Authentication required'),
      })
      const wrapper = await renderAt()

      await wrapper.findAll('button')[0].trigger('click')
      await flushPromises()

      // The tokens are gone, so this is a genuine sign-in again.
      expect(useAuthStore().isAuthenticated).toBe(false)
      expect(wrapper.find('form').exists()).toBe(true)
    })

    it('lets the caller abandon the session and sign in as somebody else', async () => {
      const wrapper = await renderAt()

      await wrapper.findAll('button')[1].trigger('click')
      await wrapper.vm.$nextTick()

      expect(useAuthStore().isAuthenticated).toBe(false)
      expect(wrapper.find('form').exists()).toBe(true)
    })
  })

  /**
   * #67, closed by decision D-18.
   *
   * `SignInRequest` has carried `tenantCode` since Sprint 4 and this form had no
   * field for it, so turning `KELIR_MULTI_TENANT` on produced a 422 whose
   * per-field message the page could not show and whose remedy the user could
   * not type. The backend refused to start in that mode rather than serve it.
   * Both halves are gone: the mode runs, and this is the half that makes it
   * usable.
   */
  describe('on a multi-tenant deployment', () => {
    async function renderMultiTenant(): Promise<VueWrapper> {
      deploymentProbe.mockResolvedValue({ multiTenant: true })

      const wrapper = await renderAt()
      // The probe resolves after mount, so the field appears a tick later.
      await flushPromises()

      return wrapper
    }

    it('asks for a tenant code', async () => {
      const wrapper = await renderMultiTenant()

      expect(wrapper.find('#tenantCode').exists()).toBe(true)
      expect(wrapper.text()).toContain('serves more than one organization')
    })

    it('sends the code with the credentials', async () => {
      const wrapper = await renderMultiTenant()

      await wrapper.find('#tenantCode').setValue('  acme  ')
      await signIn(wrapper)

      const login = backend.requests.find((request) => request.url === '/auth/login')
      expect(login?.body).toMatchObject({
        username: 'ana',
        password: 'correct horse',
        tenantCode: 'acme',
      })
    })

    it('will not send an attempt with the code left blank', async () => {
      const wrapper = await renderMultiTenant()

      await wrapper.find('#username').setValue('ana')
      await wrapper.find('#password').setValue('correct horse')
      await wrapper.find('form').trigger('submit')
      await flushPromises()

      expect(wrapper.text()).toContain('Enter the tenant code you were given')
      expect(backend.requests).toHaveLength(0)
    })
  })

  describe('when the deployment could not be asked which mode it is in', () => {
    it('shows the unchanged single-tenant form and sends no code', async () => {
      // The default in this file: the probe rejects. Every deployment that has
      // not turned the flag on is single-tenant, so this is the right guess and
      // the request must stay identical to what clients sent before the field
      // existed.
      const wrapper = await renderAt()
      await flushPromises()

      expect(wrapper.find('#tenantCode').exists()).toBe(false)

      await signIn(wrapper)

      const login = backend.requests.find((request) => request.url === '/auth/login')
      expect(login?.body).toEqual({ username: 'ana', password: 'correct horse' })
    })

    it('reveals the field when the server says one was required', async () => {
      // The recovery path, and the part of #67 that was actually broken: the
      // backend wrote a per-field message, the form had no field to show it
      // against, and the user was left with a generic error and nothing to type
      // into. A wrong guess now costs one attempt instead of locking them out.
      handler = () => ({
        status: 422,
        body: errorBody('VALIDATION_ERROR', 'Validation failed', [
          {
            path: 'tenantCode',
            rule: 'required',
            code: 'REQUIRED',
            message: 'A tenant code is required on this deployment',
          },
        ]),
      })
      const wrapper = await renderAt()
      await flushPromises()

      expect(wrapper.find('#tenantCode').exists()).toBe(false)

      await signIn(wrapper)

      expect(wrapper.find('#tenantCode').exists()).toBe(true)
      expect(wrapper.find('#tenantCode-error').text()).toBe(
        'A tenant code is required on this deployment',
      )
    })
  })
})
