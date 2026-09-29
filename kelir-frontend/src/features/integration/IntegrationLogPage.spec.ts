import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'

import IntegrationLogPage from './IntegrationLogPage.vue'
import {
  errorBody,
  installFakeBackend,
  itemBody,
  pageBody,
  type FakeBackendHandle,
  type FakeReply,
  type RecordedRequest,
} from '@/lib/testing/fake-backend'
import { useAuthStore } from '@/stores/auth'
import type { CurrentUser } from '@/types/auth'

/**
 * The integration log page (FR-INT-006, #548).
 *
 * **The client's half.** Which rows a caller may read, the order and what a
 * filter matches are the backend's. What is asserted here: the page's three
 * states; the filters go on the wire, the date range as ISO instants, and the
 * open row never does; the system chooser and the links are offered only with
 * the permissions of what they open; and a row opens its detail at `?log=`.
 */

const SYSTEM_ID = '0199a1a0-0000-7000-8000-00000000e501'
const LOG_OK = '0199a1a0-0000-7000-8000-00000000f001'
const LOG_FAILED = '0199a1a0-0000-7000-8000-00000000f002'

function logRow(overrides: Record<string, unknown> = {}) {
  return {
    id: LOG_OK,
    externalSystemId: SYSTEM_ID,
    externalSystemCode: 'SAP_ERP',
    externalSystemName: 'Corporate ERP',
    direction: 'OUTBOUND',
    integrationType: 'REST',
    method: 'GET',
    endpoint: 'https://erp.example.com/api/purchase-orders',
    entityType: 'IntegrationEndpoint',
    entityId: '0199a1a0-0000-7000-8000-00000000d001',
    status: 'SUCCESS',
    statusCode: 200,
    errorMessage: null,
    correlationId: '0199a1a0-0000-7000-8000-00000000c001',
    startedAt: '2026-09-29T03:00:00Z',
    completedAt: '2026-09-29T03:00:00.142Z',
    durationMs: 142,
    ...overrides,
  }
}

function system() {
  return {
    id: SYSTEM_ID,
    systemCode: 'SAP_ERP',
    systemName: 'Corporate ERP',
    systemType: 'ERP',
    baseUrl: 'https://erp.example.com/api',
    authType: 'BEARER_TOKEN',
    timeoutSeconds: 30,
    retryPolicy: {},
    description: null,
    status: 'ACTIVE',
    createdAt: '2026-09-25T00:00:00Z',
    updatedAt: '2026-09-25T00:00:00Z',
  }
}

function principal(permissions: string[]): CurrentUser {
  return {
    id: 'u-1',
    username: 'admin',
    displayName: 'Administrator',
    email: 'admin@example.test',
    roles: [],
    permissions,
  }
}

const blank = { template: '<div />' }

const LOG_READER = ['integration:log:read']
const LOG_AND_SYSTEMS = ['integration:log:read', 'integration:external-system:read']

describe('IntegrationLogPage', () => {
  let backend: FakeBackendHandle
  let router: Router
  let logsReply: () => FakeReply
  let detailReply: () => FakeReply

  beforeEach(() => {
    setActivePinia(createPinia())
    window.localStorage.clear()

    logsReply = () => ({
      status: 200,
      body: pageBody([
        logRow(),
        logRow({
          id: LOG_FAILED,
          status: 'FAILED',
          statusCode: null,
          endpoint: null,
          errorMessage: 'NO_USABLE_CREDENTIAL: no credential is active and valid today',
          durationMs: 3,
        }),
      ]),
    })
    detailReply = () => ({
      status: 200,
      body: itemBody({
        ...logRow(),
        documentId: null,
        requestPayload: { method: 'GET', headers: { Authorization: 'Bearer ****' } },
        responsePayload: { statusCode: 200, bodyPreview: '{"ok":true}', bodyTruncated: false },
      }),
    })

    backend = installFakeBackend((request: RecordedRequest): FakeReply => {
      if (request.url === '/integration/logs') return logsReply()
      if (request.url.startsWith('/integration/logs/')) return detailReply()
      if (request.url === '/integration/external-systems') {
        return { status: 200, body: pageBody([system()], { pageSize: 100 }) }
      }

      return { status: 404, body: errorBody('NOT_FOUND', 'Not found') }
    })

    router = createRouter({
      history: createMemoryHistory(),
      routes: [
        { path: '/admin/integration-logs', name: 'admin-integration-logs', component: blank },
        { path: '/admin/external-systems/:id', name: 'admin-external-system', component: blank },
        { path: '/documents/:id', name: 'document', component: blank },
      ],
    })
  })

  afterEach(() => backend.restore())

  async function settle(): Promise<void> {
    for (let round = 0; round < 5; round += 1) {
      await flushPromises()
    }
  }

  async function render(permissions: string[] = LOG_AND_SYSTEMS, query = ''): Promise<VueWrapper> {
    useAuthStore().$patch({ user: principal(permissions) })
    await router.push(`/admin/integration-logs${query}`)
    await router.isReady()

    const wrapper = mount(IntegrationLogPage, { global: { plugins: [router] } })
    await settle()

    return wrapper
  }

  function logRequests(): RecordedRequest[] {
    return backend.requests.filter((request) => request.url === '/integration/logs')
  }

  function lastLogRequest(): RecordedRequest {
    const all = logRequests()

    return all[all.length - 1]
  }

  // -- States ---------------------------------------------------------------

  it('lists the rows the API returned, with their status, code and error', async () => {
    const wrapper = await render()

    const ok = wrapper.get(`[data-testid="integration-log-row-${LOG_OK}"]`)
    expect(ok.text()).toContain('SAP_ERP')
    expect(ok.text()).toContain('Corporate ERP')
    expect(ok.text()).toContain('GET https://erp.example.com/api/purchase-orders')
    expect(ok.get('[data-testid="integration-log-status"]').text()).toBe('Success')
    expect(ok.text()).toContain('HTTP 200')
    expect(ok.text()).toContain('142 ms')

    const failed = wrapper.get(`[data-testid="integration-log-row-${LOG_FAILED}"]`)
    expect(failed.get('[data-testid="integration-log-status"]').text()).toBe('Failed')
    expect(failed.get('[data-testid="integration-log-error-message"]').text()).toContain(
      'NO_USABLE_CREDENTIAL',
    )
    expect(failed.text()).not.toContain('HTTP')
  })

  it('says no call has been logged rather than drawing an empty table', async () => {
    logsReply = () => ({ status: 200, body: pageBody([]) })

    const wrapper = await render()

    expect(wrapper.get('[data-testid="integration-logs-empty"]').text()).toContain(
      'No calls have been logged yet',
    )
    expect(wrapper.find('[data-testid="integration-logs-table"]').exists()).toBe(false)
  })

  it('says nothing matches the filters when a filtered view is empty', async () => {
    logsReply = () => ({ status: 200, body: pageBody([]) })

    const wrapper = await render(undefined, '?status=DEAD_LETTER')

    expect(wrapper.get('[data-testid="integration-logs-empty"]').text()).toContain(
      'No calls match these filters',
    )
  })

  it('shows a failed load as a failure, not as an empty log, and tries again', async () => {
    logsReply = () => ({
      status: 422,
      body: errorBody('VALIDATION_ERROR', 'from must not be after to'),
    })

    const wrapper = await render()

    expect(wrapper.get('[data-testid="integration-logs-error"]').text()).toContain(
      'from must not be after to',
    )
    expect(wrapper.find('[data-testid="integration-logs-empty"]').exists()).toBe(false)

    logsReply = () => ({ status: 200, body: pageBody([logRow()]) })
    await wrapper.get('[data-testid="integration-logs-error"] button').trigger('click')
    await settle()

    expect(wrapper.find(`[data-testid="integration-log-row-${LOG_OK}"]`).exists()).toBe(true)
  })

  // -- Filters to the query -------------------------------------------------

  it('sends the URL filters to the server rather than narrowing a fetched page', async () => {
    await render(
      undefined,
      `?externalSystemId=${SYSTEM_ID}&status=FAILED&from=2026-09-01T00:00:00.000Z&to=2026-09-30T00:00:00.000Z&page=2`,
    )

    expect(lastLogRequest().params).toEqual({
      page: 2,
      pageSize: 20,
      externalSystemId: SYSTEM_ID,
      status: 'FAILED',
      from: '2026-09-01T00:00:00.000Z',
      to: '2026-09-30T00:00:00.000Z',
    })
  })

  it('writes a chosen system and status to the URL and returns to page 1', async () => {
    const wrapper = await render(undefined, '?page=3')

    await wrapper.get('[data-testid="integration-logs-system"]').setValue(SYSTEM_ID)
    await settle()

    expect(router.currentRoute.value.query).toEqual({ externalSystemId: SYSTEM_ID })
    expect(lastLogRequest().params).toMatchObject({ externalSystemId: SYSTEM_ID, page: 1 })

    await wrapper.get('[data-testid="integration-logs-status"]').setValue('FAILED')
    await settle()

    expect(router.currentRoute.value.query).toEqual({
      externalSystemId: SYSTEM_ID,
      status: 'FAILED',
    })
  })

  it('writes the date range as ISO instants, and a cleared date drops the filter', async () => {
    const wrapper = await render()

    const from = wrapper.get('[data-testid="integration-logs-from"]')
    await from.setValue('2026-09-01T08:30')
    await from.trigger('change')
    await settle()

    const sent = router.currentRoute.value.query.from as string
    expect(sent).toBe(new Date('2026-09-01T08:30').toISOString())
    expect(lastLogRequest().params).toMatchObject({ from: sent })
    // The input shows it back in the browser's zone.
    expect((from.element as HTMLInputElement).value).toBe('2026-09-01T08:30')

    await from.setValue('')
    await from.trigger('change')
    await settle()

    expect(router.currentRoute.value.query).toEqual({})
  })

  it('says which bound is wrong when the range ends before it starts', async () => {
    const wrapper = await render(
      undefined,
      '?from=2026-09-30T00:00:00.000Z&to=2026-09-01T00:00:00.000Z',
    )

    expect(wrapper.get('[data-testid="integration-logs-range-error"]').text()).toContain(
      'before Started from',
    )
    // Still the server's to refuse: the request went.
    expect(lastLogRequest().params).toMatchObject({ to: '2026-09-01T00:00:00.000Z' })
  })

  it('offers the systems the caller may read in the chooser', async () => {
    const wrapper = await render()

    const options = wrapper
      .get('[data-testid="integration-logs-system"]')
      .findAll('option')
      .map((option) => option.text())

    expect(options).toEqual(['Any', 'SAP_ERP · Corporate ERP'])
  })

  it('clears every filter at once', async () => {
    const wrapper = await render(undefined, `?status=FAILED&externalSystemId=${SYSTEM_ID}`)

    await wrapper.get('[data-testid="integration-logs-clear"]').trigger('click')
    await settle()

    expect(router.currentRoute.value.query).toEqual({})
  })

  it('turns pages through the URL', async () => {
    logsReply = () => ({
      status: 200,
      body: pageBody([logRow()], { page: 1, pageSize: 20, total: 45 }),
    })

    const wrapper = await render()

    await wrapper.get('[data-testid="next-page"]').trigger('click')
    await settle()

    expect(router.currentRoute.value.query).toEqual({ page: '2' })
    expect(lastLogRequest().params).toMatchObject({ page: 2 })
  })

  // -- Permissions ----------------------------------------------------------

  it('links a row to its system only with the registry read permission', async () => {
    const withSystems = await render(LOG_AND_SYSTEMS)

    expect(
      withSystems
        .get(
          `[data-testid="integration-log-row-${LOG_OK}"] [data-testid="integration-log-system-link"]`,
        )
        .attributes('href'),
    ).toBe(`/admin/external-systems/${SYSTEM_ID}`)

    const logOnly = await render(LOG_READER)

    expect(logOnly.find('[data-testid="integration-log-system-link"]').exists()).toBe(false)
    expect(logOnly.get(`[data-testid="integration-log-row-${LOG_OK}"]`).text()).toContain('SAP_ERP')
  })

  it('does not read the registry for a caller who may not, and says why the chooser is off', async () => {
    const wrapper = await render(LOG_READER)

    expect(
      backend.requests.some((request) => request.url === '/integration/external-systems'),
    ).toBe(false)
    expect(wrapper.get('[data-testid="integration-logs-system"]').attributes()).toHaveProperty(
      'disabled',
    )
    expect(wrapper.get('[data-testid="integration-logs-system-hint"]').text()).toContain(
      'integration:external-system:read',
    )
  })

  it('keeps a system filter from the URL choosable, so it can be cleared, without the registry', async () => {
    const wrapper = await render(LOG_READER, `?externalSystemId=${SYSTEM_ID}`)

    const chooser = wrapper.get('[data-testid="integration-logs-system"]')

    expect(chooser.attributes()).not.toHaveProperty('disabled')
    expect(chooser.findAll('option').map((option) => option.text())).toEqual(['Any', 'SAP_ERP'])

    await chooser.setValue('')
    await settle()

    expect(router.currentRoute.value.query).toEqual({})
  })

  // -- Detail ---------------------------------------------------------------

  it('opens a row at ?log= without reloading the list, and closes back to the filters', async () => {
    const wrapper = await render(undefined, '?status=SUCCESS')
    const before = logRequests().length

    await wrapper.get(`[data-testid="integration-log-open-${LOG_OK}"]`).trigger('click')
    await settle()

    expect(router.currentRoute.value.query).toEqual({ status: 'SUCCESS', log: LOG_OK })
    expect(backend.requests.some((request) => request.url === `/integration/logs/${LOG_OK}`)).toBe(
      true,
    )
    expect(wrapper.get('[data-testid="integration-log-detail-id"]').text()).toBe(LOG_OK)
    // `log` is the dialog's: the list was not read again, and never with it.
    expect(logRequests()).toHaveLength(before)
    expect(logRequests().every((request) => !('log' in request.params))).toBe(true)

    await wrapper.get('[data-testid="integration-log-detail-close"]').trigger('click')
    await settle()

    expect(router.currentRoute.value.query).toEqual({ status: 'SUCCESS' })
    expect(wrapper.find('[data-testid="integration-log-detail"]').exists()).toBe(false)
  })

  it('opens the row a link names, as the test call dialog links it', async () => {
    const wrapper = await render(undefined, `?log=${LOG_OK}`)

    expect(wrapper.get('[data-testid="integration-log-detail-id"]').text()).toBe(LOG_OK)
    expect(lastLogRequest().params).not.toHaveProperty('log')
  })
})
