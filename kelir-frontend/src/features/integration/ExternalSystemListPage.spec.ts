import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'

import ExternalSystemListPage from './ExternalSystemListPage.vue'
import {
  errorBody,
  installFakeBackend,
  itemBody,
  type FakeBackendHandle,
  type FakeReply,
  type RecordedRequest,
} from '@/lib/testing/fake-backend'
import { useAuthStore } from '@/stores/auth'
import type { CurrentUser } from '@/types/auth'

/**
 * The external system registry list (#520).
 *
 * **The client's half.** Which systems exist, whether a code is taken and what
 * a valid base URL is are the backend's, held by its integration tests. What
 * is asserted here: the filters go on the wire rather than narrowing a page,
 * register is offered only with `:create`, and a refusal lands on the field it
 * names — `retryPolicy.maxRetries` included — or on the form when no input
 * has that path.
 */

function system(overrides: Record<string, unknown> = {}) {
  return {
    id: '0199a1a0-0000-7000-8000-00000000e501',
    systemCode: 'SAP_ERP',
    systemName: 'Corporate ERP',
    systemType: 'ERP',
    baseUrl: 'https://erp.example.com/api',
    authType: 'API_KEY',
    timeoutSeconds: 30,
    retryPolicy: {},
    description: null,
    status: 'ACTIVE',
    createdAt: '2026-09-25T00:00:00Z',
    updatedAt: '2026-09-25T00:00:00Z',
    ...overrides,
  }
}

function principal(permissions: string[]): CurrentUser {
  return {
    id: '0199a1a0-0000-7000-8000-0000000000u1',
    username: 'admin',
    displayName: 'Administrator',
    email: 'admin@example.test',
    roles: [],
    permissions,
  }
}

const blank = { template: '<div />' }

describe('ExternalSystemListPage', () => {
  let backend: FakeBackendHandle
  let router: Router
  let rows: unknown[]
  /** What `POST /integration/external-systems` answers. */
  let registerReply: FakeReply

  beforeEach(() => {
    setActivePinia(createPinia())
    window.localStorage.clear()

    rows = [system()]
    registerReply = { status: 201, body: itemBody(system({ id: 'new-id' })) }

    backend = installFakeBackend((request: RecordedRequest) => {
      if (request.method === 'post') {
        return registerReply
      }

      return {
        status: 200,
        body: { success: true, data: rows, meta: { page: 1, pageSize: 20, total: rows.length } },
      }
    })

    router = createRouter({
      history: createMemoryHistory(),
      routes: [
        { path: '/admin/external-systems', name: 'admin-external-systems', component: blank },
        { path: '/admin/external-systems/:id', name: 'admin-external-system', component: blank },
      ],
    })
  })

  afterEach(() => backend.restore())

  async function render(
    permissions: string[] = [
      'integration:external-system:read',
      'integration:external-system:create',
    ],
    query = '',
  ): Promise<VueWrapper> {
    useAuthStore().$patch({ user: principal(permissions) })
    await router.push(`/admin/external-systems${query}`)
    await router.isReady()

    const wrapper = mount(ExternalSystemListPage, { global: { plugins: [router] } })

    for (let round = 0; round < 4; round += 1) {
      await flushPromises()
    }

    return wrapper
  }

  async function openAndSubmit(wrapper: VueWrapper): Promise<void> {
    await wrapper.get('[data-testid="register-external-system"]').trigger('click')
    await wrapper.get('[data-testid="system-code"]').setValue('SAP_ERP')
    await wrapper.get('[data-testid="system-name"]').setValue('Corporate ERP')
    await wrapper.get('[data-testid="external-system-form"]').trigger('submit')
    await flushPromises()
    await flushPromises()
  }

  it('lists the systems the API returned, each linking to its page', async () => {
    const wrapper = await render()
    const row = wrapper.get('[data-testid="external-system-row-SAP_ERP"]')

    expect(row.text()).toContain('Corporate ERP')
    expect(row.find('a').attributes('href')).toBe(
      '/admin/external-systems/0199a1a0-0000-7000-8000-00000000e501',
    )
  })

  it('sends the URL filters to the server rather than narrowing a fetched page', async () => {
    await render(undefined, '?search=erp&status=MAINTENANCE&systemType=CRM&page=2')

    const last = backend.requests[backend.requests.length - 1]

    expect(last.url).toBe('/integration/external-systems')
    expect(last.params).toMatchObject({
      search: 'erp',
      status: 'MAINTENANCE',
      systemType: 'CRM',
      page: 2,
    })
  })

  it('writes a chosen filter to the URL and returns to page 1', async () => {
    const wrapper = await render(undefined, '?page=3')

    await wrapper.get('[data-testid="external-systems-status"]').setValue('INACTIVE')
    await flushPromises()

    expect(router.currentRoute.value.query).toEqual({ status: 'INACTIVE' })
  })

  it('says nothing matches rather than drawing an empty table', async () => {
    rows = []

    const wrapper = await render()

    expect(wrapper.find('[data-testid="external-systems-empty"]').exists()).toBe(true)
    expect(wrapper.find('[data-testid="external-systems-table"]').exists()).toBe(false)
  })

  it('offers registering only with the create permission', async () => {
    expect(
      (await render(['integration:external-system:read']))
        .find('[data-testid="register-external-system"]')
        .exists(),
    ).toBe(false)
    expect((await render()).find('[data-testid="register-external-system"]').exists()).toBe(true)
  })

  it('registers the system and opens its page', async () => {
    const wrapper = await render()

    await openAndSubmit(wrapper)

    const post = backend.requests.find((request) => request.method === 'post')

    expect(post?.body).toMatchObject({
      systemCode: 'SAP_ERP',
      systemName: 'Corporate ERP',
      timeoutSeconds: 30,
      retryPolicy: {},
    })
    // A blank optional field is absent on register, not an empty string.
    expect(post?.body).not.toHaveProperty('baseUrl')
    expect(router.currentRoute.value.fullPath).toBe('/admin/external-systems/new-id')
  })

  it('shows each 422 detail against the field it names, nested paths included', async () => {
    registerReply = {
      status: 422,
      body: errorBody('VALIDATION_ERROR', 'Validation failed', [
        {
          path: 'baseUrl',
          rule: 'url',
          code: 'CREDENTIALS_IN_URL',
          message: 'A base URL may not carry credentials',
        },
        {
          path: 'retryPolicy.maxRetries',
          rule: 'range',
          code: 'OUT_OF_RANGE',
          message: 'At most 20',
        },
      ]),
    }

    const wrapper = await render()

    await openAndSubmit(wrapper)

    expect(wrapper.get('[data-testid="baseUrl-error"]').text()).toContain('may not carry')
    expect(wrapper.get('[data-testid="retryPolicy.maxRetries-error"]').text()).toBe('At most 20')
    // The dialog stays open: the person has something to fix.
    expect(wrapper.find('[data-testid="external-system-form"]').exists()).toBe(true)
    expect(router.currentRoute.value.name).toBe('admin-external-systems')
  })

  it('lists a detail for a field the form has no input for rather than losing it', async () => {
    registerReply = {
      status: 422,
      body: errorBody('VALIDATION_ERROR', 'Validation failed', [
        { path: 'colour', rule: 'unknown', code: 'UNKNOWN_FIELD', message: 'Unknown field' },
      ]),
    }

    const wrapper = await render()

    await openAndSubmit(wrapper)

    expect(wrapper.get('[data-testid="external-system-unplaced-errors"]').text()).toContain(
      'colour: Unknown field',
    )
  })

  it('puts a duplicate code against the code input', async () => {
    registerReply = {
      status: 409,
      body: errorBody('CONFLICT', 'An external system with this code already exists'),
    }

    const wrapper = await render()

    await openAndSubmit(wrapper)

    expect(wrapper.get('[data-testid="systemCode-error"]').text()).toContain('already exists')
  })
})
