import { createPinia, setActivePinia } from 'pinia'
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'

import ResetPasswordPage from './ResetPasswordPage.vue'
import {
  errorBody,
  installFakeBackend,
  type FakeBackendHandle,
  type FakeHandler,
} from '@/lib/testing/fake-backend'

const blank = { template: '<div />' }

const succeeds: FakeHandler = () => ({ status: 204 })

const LONG_ENOUGH = 'a-sufficiently-long-password'

describe('ResetPasswordPage', () => {
  let backend: FakeBackendHandle
  let handler: FakeHandler
  let router: Router

  beforeEach(() => {
    setActivePinia(createPinia())
    window.localStorage.clear()

    handler = succeeds
    backend = installFakeBackend((request) => handler(request))

    router = createRouter({
      history: createMemoryHistory(),
      routes: [
        { path: '/login', name: 'login', component: blank },
        { path: '/forgot-password', name: 'forgot-password', component: blank },
        { path: '/reset-password', name: 'reset-password', component: blank },
      ],
    })
  })

  afterEach(() => backend.restore())

  async function renderAt(path = '/reset-password?token=tok-1'): Promise<VueWrapper> {
    await router.push(path)
    await router.isReady()

    return mount(ResetPasswordPage, { global: { plugins: [router] } })
  }

  async function submit(
    wrapper: VueWrapper,
    password = LONG_ENOUGH,
    confirmation = password,
  ): Promise<void> {
    await wrapper.find('#new-password').setValue(password)
    await wrapper.find('#confirm-password').setValue(confirmation)
    await wrapper.find('form').trigger('submit')
    await flushPromises()
  }

  it('sends the token from the link with the new password', async () => {
    const wrapper = await renderAt()

    await submit(wrapper)

    expect(backend.requests).toHaveLength(1)
    expect(backend.requests[0]?.url).toBe('/auth/reset-password')
    expect(backend.requests[0]?.body).toEqual({ token: 'tok-1', newPassword: LONG_ENOUGH })
  })

  /**
   * The spent token must not survive in the address bar.
   *
   * It is a bearer credential in a URL, which is the one place a secret gets
   * copied by accident — shoulder-surfed, pasted into a bug report, synced to
   * another device's history. The server has already refused to reuse it, so
   * this is not what stops a replay; it is what stops the value being handed
   * around as though it were still live.
   */
  it('drops the token out of the URL once it is spent', async () => {
    const wrapper = await renderAt()

    await submit(wrapper)

    expect(router.currentRoute.value.query.token).toBeUndefined()
    expect(wrapper.text()).toContain('Your password is changed')
  })

  it('offers no form when the link carried no token', async () => {
    const wrapper = await renderAt('/reset-password')

    expect(wrapper.find('form').exists()).toBe(false)
    expect(wrapper.text()).toContain('This link is not complete')
    expect(backend.requests).toHaveLength(0)
  })

  it('refuses to submit a password below the policy floor', async () => {
    const wrapper = await renderAt()

    await submit(wrapper, 'short')

    expect(backend.requests).toHaveLength(0)
    expect(wrapper.text()).toContain('Use at least 12 characters')
  })

  it('refuses to submit when the two entries differ', async () => {
    const wrapper = await renderAt()

    await submit(wrapper, LONG_ENOUGH, `${LONG_ENOUGH}!`)

    expect(backend.requests).toHaveLength(0)
    expect(wrapper.text()).toContain('Both entries must match')
  })

  /**
   * A rejected token reports against `token`, which is no input on this form.
   * Mapped onto the form rather than onto a field, because attaching "that
   * reset link is not valid" to the password box would blame the wrong thing.
   */
  it('shows a rejected token on the form and keeps the form usable', async () => {
    handler = () => ({
      status: 422,
      body: errorBody('VALIDATION_ERROR', 'Validation failed', [
        {
          path: 'token',
          rule: 'exists',
          code: 'INVALID_TOKEN',
          message: 'That reset link is not valid, has already been used, or has expired.',
        },
      ]),
    })

    const wrapper = await renderAt()
    await submit(wrapper)

    expect(wrapper.text()).toContain('That reset link is not valid')
    expect(wrapper.find('form').exists()).toBe(true)
  })

  /**
   * The shared validator names the field `password`; this form calls it
   * `newPassword`. Without the mapping the message matches no input and
   * disappears, which is the failure this pins down — the page would look like
   * it had accepted a password the server refused.
   */
  it('lands the server password message on the new-password field', async () => {
    handler = () => ({
      status: 422,
      body: errorBody('VALIDATION_ERROR', 'Validation failed', [
        {
          path: 'password',
          rule: 'minLength',
          code: 'TOO_SHORT',
          message: 'Password must be at least 12 characters',
        },
      ]),
    })

    const wrapper = await renderAt()
    await submit(wrapper)

    expect(wrapper.find('#new-password-error').text()).toContain('at least 12 characters')
  })
})
