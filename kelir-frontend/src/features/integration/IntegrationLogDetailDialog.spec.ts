import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'

import IntegrationLogDetailDialog from './IntegrationLogDetailDialog.vue'
import {
  errorBody,
  installFakeBackend,
  itemBody,
  type FakeBackendHandle,
  type FakeReply,
} from '@/lib/testing/fake-backend'
import { useAuthStore } from '@/stores/auth'

/**
 * One integration log row in full (FR-INT-006, #548).
 *
 * Every field is shown, the payloads pretty-printed **exactly as stored** —
 * masked values stay masked (AC3) — and a payload the row lacks says so. The
 * system and document links are offered only with the permission of the
 * screen each opens.
 */

const LOG_ID = '0199a1a0-0000-7000-8000-00000000f001'
const SYSTEM_ID = '0199a1a0-0000-7000-8000-00000000e501'
const DOCUMENT_ID = '0199a1a0-0000-7000-8000-00000000a001'

function detail(overrides: Record<string, unknown> = {}) {
  return {
    id: LOG_ID,
    externalSystemId: SYSTEM_ID,
    externalSystemCode: 'SAP_ERP',
    externalSystemName: 'Corporate ERP',
    direction: 'OUTBOUND',
    integrationType: 'REST',
    method: 'POST',
    endpoint: 'https://erp.example.com/api/purchase-orders',
    entityType: 'IntegrationEndpoint',
    entityId: '0199a1a0-0000-7000-8000-00000000d001',
    status: 'FAILED',
    statusCode: 500,
    errorMessage: null,
    correlationId: 'corr-1',
    startedAt: '2026-09-29T03:00:00Z',
    completedAt: '2026-09-29T03:00:01.250Z',
    durationMs: 1250,
    documentId: DOCUMENT_ID,
    requestPayload: {
      method: 'POST',
      headers: { Authorization: '[REDACTED]' },
      correlationId: 'corr-1',
    },
    responsePayload: { statusCode: 500, bodyPreview: 'upstream exploded', bodyTruncated: false },
    ...overrides,
  }
}

const blank = { template: '<div />' }

describe('IntegrationLogDetailDialog', () => {
  let backend: FakeBackendHandle
  let router: Router
  let reply: () => FakeReply

  beforeEach(() => {
    setActivePinia(createPinia())
    reply = () => ({ status: 200, body: itemBody(detail()) })
    backend = installFakeBackend(() => reply())
    router = createRouter({
      history: createMemoryHistory(),
      routes: [
        { path: '/', component: blank },
        { path: '/admin/external-systems/:id', name: 'admin-external-system', component: blank },
        { path: '/documents/:id', name: 'document', component: blank },
      ],
    })
  })

  afterEach(() => backend.restore())

  async function render(
    permissions: string[] = ['integration:log:read'],
    logId: string | null = LOG_ID,
  ): Promise<VueWrapper> {
    useAuthStore().$patch({
      user: {
        id: 'u-1',
        username: 'admin',
        displayName: 'Administrator',
        email: 'admin@example.test',
        roles: [],
        permissions,
      },
    })

    const wrapper = mount(IntegrationLogDetailDialog, {
      props: { open: true, logId },
      global: { plugins: [router] },
    })

    for (let round = 0; round < 4; round += 1) {
      await flushPromises()
    }

    return wrapper
  }

  function text(wrapper: VueWrapper, testId: string): string {
    return wrapper.get(`[data-testid="${testId}"]`).text()
  }

  it('reads the row it was given', async () => {
    await render()

    expect(backend.requests.map((request) => [request.method, request.url])).toEqual([
      ['get', `/integration/logs/${LOG_ID}`],
    ])
  })

  it('reads nothing while closed or without a row', async () => {
    await render(undefined, null)

    expect(backend.requests).toHaveLength(0)
  })

  it('shows every field of the row', async () => {
    const wrapper = await render()
    const all = text(wrapper, 'integration-log-detail')

    expect(text(wrapper, 'integration-log-detail-status')).toBe('Failed')
    expect(text(wrapper, 'integration-log-detail-status-code')).toBe('HTTP 500')
    expect(all).toContain('in 1.25 s')
    expect(text(wrapper, 'integration-log-detail-id')).toBe(LOG_ID)
    expect(text(wrapper, 'integration-log-detail-system')).toBe('SAP_ERP · Corporate ERP')
    expect(all).toContain('Outbound')
    expect(all).toContain('REST')
    expect(text(wrapper, 'integration-log-detail-request')).toBe(
      'POST https://erp.example.com/api/purchase-orders',
    )
    expect(all).toContain('IntegrationEndpoint')
    expect(all).toContain('0199a1a0-0000-7000-8000-00000000d001')
    expect(all).toContain('corr-1')
    expect(all).toContain(new Date('2026-09-29T03:00:00Z').toLocaleString())
    expect(all).toContain(new Date('2026-09-29T03:00:01.250Z').toLocaleString())
  })

  it('pretty-prints both payloads as stored, masked values untouched', async () => {
    const wrapper = await render()

    const request = wrapper.get('[data-testid="integration-log-detail-request-payload"]')
    expect(request.element.tagName).toBe('PRE')
    expect(request.text()).toBe(JSON.stringify(detail().requestPayload, null, 2))
    expect(request.text()).toContain('"Authorization": "[REDACTED]"')

    expect(text(wrapper, 'integration-log-detail-response-payload')).toBe(
      JSON.stringify(detail().responsePayload, null, 2),
    )
  })

  it('says a payload was not recorded rather than drawing null, and shows the error', async () => {
    reply = () => ({
      status: 200,
      body: itemBody(
        detail({
          status: 'FAILED',
          statusCode: null,
          durationMs: null,
          endpoint: null,
          responsePayload: null,
          errorMessage: 'NO_USABLE_CREDENTIAL: no credential is active and valid today',
        }),
      ),
    })

    const wrapper = await render()

    expect(wrapper.find('[data-testid="integration-log-detail-response-payload"]').exists()).toBe(
      false,
    )
    expect(text(wrapper, 'integration-log-detail-no-response')).toContain('None was recorded')
    expect(text(wrapper, 'integration-log-detail-error-message')).toContain('NO_USABLE_CREDENTIAL')
    expect(wrapper.find('[data-testid="integration-log-detail-status-code"]').exists()).toBe(false)
    expect(text(wrapper, 'integration-log-detail-request')).toBe('POST —')
  })

  it('links the system and the document only with the permission of each', async () => {
    const neither = await render(['integration:log:read'])

    expect(neither.find('[data-testid="integration-log-detail-system-link"]').exists()).toBe(false)
    expect(neither.find('[data-testid="integration-log-detail-document-link"]').exists()).toBe(
      false,
    )
    // Still shown, as text.
    expect(text(neither, 'integration-log-detail-document')).toBe(DOCUMENT_ID)

    const both = await render([
      'integration:log:read',
      'integration:external-system:read',
      'document:read',
    ])

    expect(both.get('[data-testid="integration-log-detail-system-link"]').attributes('href')).toBe(
      `/admin/external-systems/${SYSTEM_ID}`,
    )
    expect(
      both.get('[data-testid="integration-log-detail-document-link"]').attributes('href'),
    ).toBe(`/documents/${DOCUMENT_ID}`)
  })

  it('has no document row for a call made for no document', async () => {
    reply = () => ({ status: 200, body: itemBody(detail({ documentId: null })) })

    const wrapper = await render(['integration:log:read', 'document:read'])

    expect(wrapper.find('[data-testid="integration-log-detail-document"]').exists()).toBe(false)
  })

  it('says a row that does not exist does not exist', async () => {
    reply = () => ({ status: 404, body: errorBody('NOT_FOUND', 'Integration log not found') })

    const wrapper = await render()

    expect(text(wrapper, 'integration-log-detail-error')).toContain(
      'There is no integration log with this id',
    )
  })

  it.each(['../external-systems', 'not-a-uuid', `${LOG_ID}/../../x`])(
    'treats %j, which is not a UUID, as not found without a request',
    async (logId) => {
      const wrapper = await render(undefined, logId)

      expect(backend.requests).toHaveLength(0)
      expect(text(wrapper, 'integration-log-detail-error')).toContain(
        'There is no integration log with this id',
      )
      expect(wrapper.find('[data-testid="integration-log-detail-error"] button').exists()).toBe(
        false,
      )
    },
  )

  it('shows a refusal as a failure, with a way to try again', async () => {
    reply = () => ({ status: 403, body: errorBody('FORBIDDEN', 'Missing integration:log:read') })

    const wrapper = await render()

    expect(text(wrapper, 'integration-log-detail-error')).toContain('Missing integration:log:read')

    reply = () => ({ status: 200, body: itemBody(detail()) })
    await wrapper.get('[data-testid="integration-log-detail-error"] button').trigger('click')
    await flushPromises()
    await flushPromises()

    expect(text(wrapper, 'integration-log-detail-id')).toBe(LOG_ID)
  })
})
