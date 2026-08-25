import { createPinia, setActivePinia } from 'pinia'
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'

import ForgotPasswordPage from './ForgotPasswordPage.vue'
import {
  errorBody,
  installFakeBackend,
  type FakeBackendHandle,
  type FakeHandler,
} from '@/lib/testing/fake-backend'

const blank = { template: '<div />' }

const accepted: FakeHandler = () => ({ status: 202 })

describe('ForgotPasswordPage', () => {
  let backend: FakeBackendHandle
  let handler: FakeHandler
  let router: Router

  beforeEach(() => {
    setActivePinia(createPinia())
    window.localStorage.clear()

    handler = accepted
    backend = installFakeBackend((request) => handler(request))

    router = createRouter({
      history: createMemoryHistory(),
      routes: [
        { path: '/login', name: 'login', component: blank },
        { path: '/forgot-password', name: 'forgot-password', component: blank },
      ],
    })
  })

  afterEach(() => backend.restore())

  async function render(): Promise<VueWrapper> {
    await router.push('/forgot-password')
    await router.isReady()

    return mount(ForgotPasswordPage, { global: { plugins: [router] } })
  }

  async function request(wrapper: VueWrapper, identifier = 'ana'): Promise<void> {
    await wrapper.find('#username').setValue(identifier)
    await wrapper.find('form').trigger('submit')
    await flushPromises()
  }

  it('posts the identifier to the forgot-password endpoint', async () => {
    const wrapper = await render()

    await request(wrapper, '  ana  ')

    expect(backend.requests).toHaveLength(1)
    expect(backend.requests[0]?.url).toBe('/auth/forgot-password')
    // Trimmed, matching sign-in: a trailing space pasted out of a password
    // manager should not be the reason nothing arrives.
    expect(backend.requests[0]?.body).toEqual({ username: 'ana' })
  })

  it('does not send an empty identifier', async () => {
    const wrapper = await render()

    await wrapper.find('form').trigger('submit')
    await flushPromises()

    expect(backend.requests).toHaveLength(0)
    expect(wrapper.text()).toContain('Enter your username or email address')
  })

  /**
   * The security property of the whole page, and the reason it is asserted as a
   * *sameness* rather than as one expected string.
   *
   * The backend answers 202 for an account that exists and for one that does
   * not, so what a person reads must not differ either. A test that only
   * checked the happy path would stay green if somebody later added "no account
   * with that email" to the failure branch — because there is no failure branch
   * to add it to, and that is exactly what this pins down.
   */
  it('reads the same whether or not the identifier belongs to an account', async () => {
    const known = await render()
    await request(known, 'ana')
    const forKnown = known.text()

    backend.requests.length = 0
    const unknown = await render()
    await request(unknown, 'nobody@example.com')

    expect(unknown.text()).toBe(forKnown)
    // And it does not claim delivery, which the server never confirmed.
    expect(forKnown).toContain('If that username or email belongs to an account')
  })

  it('reports a server it could not reach, which is the one honest failure', async () => {
    handler = () => ({ status: 0, networkError: true })

    const wrapper = await render()
    await request(wrapper)

    expect(wrapper.find('form').exists()).toBe(true)
    expect(wrapper.text()).not.toContain('Check your email')
  })

  it('shows a validation message the server sent rather than swallowing it', async () => {
    handler = () => ({
      status: 422,
      body: errorBody('VALIDATION_ERROR', 'Username is required', [
        { path: 'username', rule: 'required', code: 'REQUIRED', message: 'Username is required' },
      ]),
    })

    const wrapper = await render()
    await request(wrapper)

    expect(wrapper.text()).toContain('Username is required')
  })
})
