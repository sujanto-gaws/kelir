import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'

import ExternalSystemDetailPage from './ExternalSystemDetailPage.vue'
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
 * One external system's page (#520).
 *
 * The properties asserted are the ones only the client can hold:
 *
 * 1. **Each action is offered only with its own permission** — edit with
 *    `:update`, activate and deactivate with `:deactivate`, endpoint changes
 *    with the system's `:update`, credential changes with their own.
 * 2. **The credential section is absent, not failing,** for a caller without
 *    `integration:credential:read` — and is not even requested — and withdraws
 *    the same way when the server answers 403.
 * 3. **Nothing but the reference is shown**, and the input for it is a text box
 *    that says it takes a reference, never a password field.
 * 4. **Into and out of INACTIVE is the verbs'**: activate and deactivate post to
 *    their routes and the page shows the status the server returned, and an
 *    edit never sends `INACTIVE` — nor any status for an inactive system.
 */

const ID = '0199a1a0-0000-7000-8000-00000000e501'
const BASE = `/integration/external-systems/${ID}`
const blank = { template: '<div />' }

function system(overrides: Record<string, unknown> = {}) {
  return {
    id: ID,
    systemCode: 'SAP_ERP',
    systemName: 'Corporate ERP',
    systemType: 'ERP',
    baseUrl: 'https://erp.example.com/api',
    authType: 'API_KEY',
    timeoutSeconds: 30,
    retryPolicy: { maxRetries: 3 },
    description: null,
    status: 'ACTIVE',
    createdAt: '2026-09-25T00:00:00Z',
    updatedAt: '2026-09-25T00:00:00Z',
    ...overrides,
  }
}

function endpoint(overrides: Record<string, unknown> = {}) {
  return {
    id: 'ep-1',
    externalSystemId: ID,
    endpointCode: 'CREATE_PO',
    name: 'Create purchase order',
    method: 'POST',
    path: '/purchase-orders',
    description: null,
    status: 'ACTIVE',
    createdAt: '2026-09-25T00:00:00Z',
    updatedAt: '2026-09-25T00:00:00Z',
    ...overrides,
  }
}

function credential(overrides: Record<string, unknown> = {}) {
  return {
    id: 'cr-1',
    externalSystemId: ID,
    credentialType: 'API_KEY',
    secretReference: 'vault://kelir/erp/api-key',
    validFrom: '2026-09-01',
    validTo: null,
    isActive: true,
    createdAt: '2026-09-25T00:00:00Z',
    updatedAt: '2026-09-25T00:00:00Z',
    ...overrides,
  }
}

function page(data: unknown[]) {
  return { success: true, data, meta: { page: 1, pageSize: 20, total: data.length } }
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

const SYSTEM_ALL = [
  'integration:external-system:read',
  'integration:external-system:update',
  'integration:external-system:deactivate',
]
const CREDENTIAL_ALL = [
  'integration:credential:read',
  'integration:credential:create',
  'integration:credential:update',
  'integration:credential:delete',
]

describe('ExternalSystemDetailPage', () => {
  let backend: FakeBackendHandle
  let router: Router
  let current: ReturnType<typeof system>
  let systemReply: () => FakeReply
  let credentialsReply: () => FakeReply
  let toggleReply: (action: string) => FakeReply
  let putReply: () => FakeReply

  beforeEach(() => {
    setActivePinia(createPinia())
    window.localStorage.clear()

    current = system()
    systemReply = () => ({ status: 200, body: itemBody(current) })
    credentialsReply = () => ({ status: 200, body: page([credential()]) })
    toggleReply = (action) => ({
      status: 200,
      body: itemBody({ ...current, status: action === 'activate' ? 'ACTIVE' : 'INACTIVE' }),
    })
    putReply = () => ({ status: 200, body: itemBody(current) })

    backend = installFakeBackend((request: RecordedRequest): FakeReply => {
      if (request.url === BASE && request.method === 'get') return systemReply()
      if (request.url === BASE && request.method === 'put') return putReply()
      if (request.url.endsWith('/activate')) return toggleReply('activate')
      if (request.url.endsWith('/deactivate')) return toggleReply('deactivate')
      if (request.url === `${BASE}/endpoints`) return { status: 200, body: page([endpoint()]) }
      if (request.url.startsWith(`${BASE}/endpoints/`)) {
        return { status: 200, body: itemBody(endpoint({ status: 'INACTIVE' })) }
      }
      if (request.url === `${BASE}/credentials`) return credentialsReply()

      return { status: 404, body: errorBody('NOT_FOUND', 'Not found') }
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

  async function render(permissions: string[]): Promise<VueWrapper> {
    useAuthStore().$patch({ user: principal(permissions) })
    await router.push(`/admin/external-systems/${ID}`)
    await router.isReady()

    const wrapper = mount(ExternalSystemDetailPage, { global: { plugins: [router] } })

    for (let round = 0; round < 5; round += 1) {
      await flushPromises()
    }

    return wrapper
  }

  function has(wrapper: VueWrapper, testId: string): boolean {
    return wrapper.find(`[data-testid="${testId}"]`).exists()
  }

  it('shows the system the API returned', async () => {
    const wrapper = await render(SYSTEM_ALL)

    expect(wrapper.get('[data-testid="external-system-name"]').text()).toBe('Corporate ERP')
    expect(wrapper.get('[data-testid="external-system-status"]').text()).toBe('Active')
    expect(wrapper.get('[data-testid="external-system-retry-policy"]').text()).toContain(
      '3 retries',
    )
  })

  it('says a system does not exist rather than showing an empty page', async () => {
    systemReply = () => ({
      status: 404,
      body: errorBody('NOT_FOUND', 'External system not found'),
    })

    const wrapper = await render(SYSTEM_ALL)

    expect(wrapper.get('[data-testid="external-system-error"]').text()).toContain('does not exist')
  })

  // -- Permission gating ----------------------------------------------------

  it('offers no action to a caller who may only read', async () => {
    const wrapper = await render(['integration:external-system:read'])

    for (const testId of [
      'edit-external-system',
      'deactivate-external-system',
      'activate-external-system',
      'add-endpoint',
      'edit-endpoint-CREATE_PO',
      'retire-endpoint-CREATE_PO',
    ]) {
      expect(has(wrapper, testId), testId).toBe(false)
    }

    // The endpoints are still readable under the system's read permission.
    expect(has(wrapper, 'endpoint-row-CREATE_PO')).toBe(true)
  })

  it('offers edit and endpoint changes with update, and the toggle only with deactivate', async () => {
    const updater = await render([
      'integration:external-system:read',
      'integration:external-system:update',
    ])

    expect(has(updater, 'edit-external-system')).toBe(true)
    expect(has(updater, 'add-endpoint')).toBe(true)
    expect(has(updater, 'retire-endpoint-CREATE_PO')).toBe(true)
    expect(has(updater, 'deactivate-external-system')).toBe(false)

    const toggler = await render([
      'integration:external-system:read',
      'integration:external-system:deactivate',
    ])

    expect(has(toggler, 'deactivate-external-system')).toBe(true)
    expect(has(toggler, 'edit-external-system')).toBe(false)
  })

  // -- Credential references ------------------------------------------------

  it('has no credential section, and asks for none, without credential:read', async () => {
    const wrapper = await render([...SYSTEM_ALL, 'integration:credential:create'])

    expect(has(wrapper, 'credentials-section')).toBe(false)
    expect(wrapper.text()).not.toContain('Credential references')
    expect(backend.requests.some((request) => request.url.endsWith('/credentials'))).toBe(false)
  })

  it('withdraws the section rather than reporting an error when the server refuses', async () => {
    credentialsReply = () => ({ status: 403, body: errorBody('FORBIDDEN', 'Forbidden') })

    const wrapper = await render([...SYSTEM_ALL, 'integration:credential:read'])

    expect(has(wrapper, 'credentials-section')).toBe(false)
    expect(has(wrapper, 'credentials-error')).toBe(false)
    expect(wrapper.text()).not.toContain('Forbidden')
  })

  it('shows the reference, and only the reference, to a caller who may read it', async () => {
    const wrapper = await render([...SYSTEM_ALL, 'integration:credential:read'])

    const row = wrapper.get('[data-testid="credential-row-cr-1"]')

    expect(row.get('[data-testid="credential-secret-reference-value"]').text()).toBe(
      'vault://kelir/erp/api-key',
    )
    // Read alone: no write is offered.
    expect(has(wrapper, 'add-credential')).toBe(false)
    expect(has(wrapper, 'edit-credential-cr-1')).toBe(false)
    expect(has(wrapper, 'delete-credential-cr-1')).toBe(false)
  })

  it('offers each credential action with its own permission', async () => {
    const wrapper = await render([...SYSTEM_ALL, ...CREDENTIAL_ALL])

    expect(has(wrapper, 'add-credential')).toBe(true)
    expect(has(wrapper, 'edit-credential-cr-1')).toBe(true)
    expect(has(wrapper, 'delete-credential-cr-1')).toBe(true)
  })

  it('takes the reference in a text box that says it wants a reference, not a secret', async () => {
    const wrapper = await render([...SYSTEM_ALL, ...CREDENTIAL_ALL])

    await wrapper.get('[data-testid="add-credential"]').trigger('click')

    const input = wrapper.get('[data-testid="credential-secret-reference"]')

    expect(input.attributes('type')).toBe('text')
    expect(input.attributes('placeholder')).toContain('vault://')
    expect(wrapper.get('[data-testid="credential-secret-reference-hint"]').text()).toMatch(
      /not the secret itself/,
    )
    expect(wrapper.find('input[type="password"]').exists()).toBe(false)
  })

  it('tells the user what to enter and does not say a secret cannot be stored', async () => {
    // #552 (record 19, D-88): only a reference's shape is checked, and
    // `vault://sk_live_…` has the shape, so the section says what to do.
    const wrapper = await render([...SYSTEM_ALL, ...CREDENTIAL_ALL])
    const section = wrapper.get('[data-testid="credentials-section"]').text()

    expect(section).toMatch(/not the secret itself/)
    expect(section).not.toMatch(/never the secret/i)
  })

  it('shows a refused reference against its input', async () => {
    const wrapper = await render([...SYSTEM_ALL, ...CREDENTIAL_ALL])

    backend.restore()
    backend = installFakeBackend(() => ({
      status: 422,
      body: errorBody('VALIDATION_ERROR', 'Validation failed', [
        {
          path: 'secretReference',
          rule: 'reference',
          code: 'NOT_A_SECRET_REFERENCE',
          message: 'Must be a vault:// or env:// reference',
        },
      ]),
    }))

    await wrapper.get('[data-testid="add-credential"]').trigger('click')
    await wrapper.get('[data-testid="credential-type"]').setValue('API_KEY')
    await wrapper.get('[data-testid="credential-secret-reference"]').setValue('hunter2')
    await wrapper.get('[data-testid="credential-form"]').trigger('submit')
    await flushPromises()

    expect(wrapper.get('[data-testid="secretReference-error"]').text()).toContain('vault://')
  })

  // -- Activate and deactivate ---------------------------------------------

  it('deactivates after a confirmation and shows the status the server returned', async () => {
    const wrapper = await render(SYSTEM_ALL)

    await wrapper.get('[data-testid="deactivate-external-system"]').trigger('click')

    // Nothing is sent until the confirmation.
    expect(backend.requests.some((request) => request.url.endsWith('/deactivate'))).toBe(false)
    expect(wrapper.text()).toContain('Deactivate external system')

    await wrapper.get('[data-testid="confirm-action"]').trigger('click')
    await flushPromises()

    const post = backend.requests.find((request) => request.url === `${BASE}/deactivate`)

    expect(post?.method).toBe('post')
    expect(wrapper.get('[data-testid="external-system-status"]').text()).toBe('Inactive')
    expect(has(wrapper, 'activate-external-system')).toBe(true)
    expect(has(wrapper, 'deactivate-external-system')).toBe(false)
  })

  it('activates an inactive system through its own verb', async () => {
    current = system({ status: 'INACTIVE' })

    const wrapper = await render(SYSTEM_ALL)

    expect(has(wrapper, 'deactivate-external-system')).toBe(false)

    await wrapper.get('[data-testid="activate-external-system"]').trigger('click')
    await wrapper.get('[data-testid="confirm-action"]').trigger('click')
    await flushPromises()

    expect(backend.requests.some((request) => request.url === `${BASE}/activate`)).toBe(true)
    expect(backend.requests.some((request) => request.method === 'put')).toBe(false)
    expect(wrapper.get('[data-testid="external-system-status"]').text()).toBe('Active')
  })

  it('keeps the confirmation open with the refusal when the toggle fails', async () => {
    toggleReply = () => ({ status: 403, body: errorBody('FORBIDDEN', 'You may not do that') })

    const wrapper = await render(SYSTEM_ALL)

    await wrapper.get('[data-testid="deactivate-external-system"]').trigger('click')
    await wrapper.get('[data-testid="confirm-action"]').trigger('click')
    await flushPromises()

    expect(wrapper.text()).toContain('You may not do that')
    expect(wrapper.get('[data-testid="external-system-status"]').text()).toBe('Active')
  })

  // -- Edit -----------------------------------------------------------------

  it('never offers INACTIVE as an edit status', async () => {
    const wrapper = await render(SYSTEM_ALL)

    await wrapper.get('[data-testid="edit-external-system"]').trigger('click')

    const values = wrapper
      .get('[data-testid="system-status"]')
      .findAll('option')
      .map((option) => option.attributes('value'))

    expect(values).toEqual(['ACTIVE', 'MAINTENANCE'])
  })

  it('sends no status when editing an inactive system', async () => {
    current = system({ status: 'INACTIVE' })

    const wrapper = await render(SYSTEM_ALL)

    await wrapper.get('[data-testid="edit-external-system"]').trigger('click')

    expect(has(wrapper, 'system-status')).toBe(false)

    await wrapper.get('[data-testid="system-name"]').setValue('Renamed ERP')
    await wrapper.get('[data-testid="external-system-form"]').trigger('submit')
    await flushPromises()

    const put = backend.requests.find((request) => request.method === 'put')

    expect(put?.body).toMatchObject({ systemName: 'Renamed ERP' })
    expect(put?.body).not.toHaveProperty('status')
    expect(put?.body).not.toHaveProperty('systemCode')
  })

  it('shows a refused edit against the field the server named', async () => {
    putReply = () => ({
      status: 422,
      body: errorBody('VALIDATION_ERROR', 'Validation failed', [
        { path: 'timeoutSeconds', rule: 'range', code: 'OUT_OF_RANGE', message: '1 to 300' },
      ]),
    })

    const wrapper = await render(SYSTEM_ALL)

    await wrapper.get('[data-testid="edit-external-system"]').trigger('click')
    await wrapper.get('[data-testid="external-system-form"]').trigger('submit')
    await flushPromises()

    expect(wrapper.get('[data-testid="timeoutSeconds-error"]').text()).toBe('1 to 300')
  })

  // -- Endpoints ------------------------------------------------------------

  it('retires an endpoint by its status after a confirmation', async () => {
    const wrapper = await render(SYSTEM_ALL)

    await wrapper.get('[data-testid="retire-endpoint-CREATE_PO"]').trigger('click')
    await wrapper.get('[data-testid="confirm-action"]').trigger('click')
    await flushPromises()

    const put = backend.requests.find(
      (request) => request.method === 'put' && request.url === `${BASE}/endpoints/ep-1`,
    )

    expect(put?.body).toEqual({ status: 'INACTIVE' })
  })
})
