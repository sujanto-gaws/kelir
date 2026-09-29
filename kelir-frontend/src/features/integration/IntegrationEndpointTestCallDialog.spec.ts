import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import IntegrationEndpointTestCallDialog from './IntegrationEndpointTestCallDialog.vue'
import { ApiError } from '@/api/error'
import { testCallIntegrationEndpoint } from '@/api/integration'
import type { IntegrationEndpoint, TestCallResponse } from '@/types/integration'

/**
 * The test call dialog (FR-INT-002, #547).
 *
 * 1. **Nothing is sent until it is confirmed**, and a write method says it is one.
 * 2. **The wait is visible and cannot be sent twice.**
 * 3. **An answer is shown as the system gave it** — a 500 is `Failed`, not an
 *    error — with its code, duration, request, log id and body, and the
 *    truncation said aloud.
 * 4. **A refusal or an unanswered call is explained**, with its code, the
 *    server's words and the log id its message names.
 * 5. **A call still running when the dialog closes never lands on the next one.**
 */

vi.mock('@/api/integration', () => ({
  testCallIntegrationEndpoint: vi.fn(),
}))

const call = vi.mocked(testCallIntegrationEndpoint)

const LOG_ID = '0199a1a0-0000-7000-8000-00000000f001'

function endpoint(overrides: Partial<IntegrationEndpoint> = {}): IntegrationEndpoint {
  return {
    id: 'ep-1',
    externalSystemId: 'sys-1',
    endpointCode: 'GET_PO',
    name: 'Read purchase order',
    method: 'GET',
    path: '/purchase-orders/1',
    description: null,
    status: 'ACTIVE',
    createdAt: '2026-09-29T00:00:00Z',
    updatedAt: '2026-09-29T00:00:00Z',
    ...overrides,
  }
}

function answer(overrides: Partial<TestCallResponse> = {}): TestCallResponse {
  return {
    logId: LOG_ID,
    method: 'GET',
    url: 'https://erp.example.com/api/purchase-orders/1',
    status: 'SUCCESS',
    statusCode: 200,
    durationMs: 142,
    bodyPreview: '{"id":1,"token":"****"}',
    bodyTruncated: false,
    ...overrides,
  }
}

interface Deferred<T> {
  promise: Promise<T>
  resolve: (value: T) => void
  reject: (reason: unknown) => void
}

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void
  let reject!: (reason: unknown) => void
  const promise = new Promise<T>((yes, no) => {
    resolve = yes
    reject = no
  })

  return { promise, resolve, reject }
}

describe('IntegrationEndpointTestCallDialog', () => {
  beforeEach(() => {
    call.mockReset()
  })

  afterEach(() => {
    vi.clearAllMocks()
  })

  function render(target: IntegrationEndpoint = endpoint()): VueWrapper {
    return mount(IntegrationEndpointTestCallDialog, {
      props: { open: true, systemId: 'sys-1', endpoint: target, timeoutSeconds: 45 },
    })
  }

  function phase(wrapper: VueWrapper): string | undefined {
    return wrapper.get('[data-testid="test-call-dialog"]').attributes('data-phase')
  }

  function text(wrapper: VueWrapper, testId: string): string {
    return wrapper.get(`[data-testid="${testId}"]`).text()
  }

  async function run(wrapper: VueWrapper): Promise<void> {
    await wrapper.get('[data-testid="test-call-run"]').trigger('click')
    await flushPromises()
  }

  // -- Confirm --------------------------------------------------------------

  it('says what it will send and sends nothing until confirmed', () => {
    const wrapper = render()

    expect(phase(wrapper)).toBe('confirm')
    expect(wrapper.text()).toContain('GET')
    expect(wrapper.text()).toContain('/purchase-orders/1')
    expect(wrapper.find('[data-testid="test-call-write-warning"]').exists()).toBe(false)
    expect(call).not.toHaveBeenCalled()
  })

  it('warns that a write method is a real request the system may act on', () => {
    const wrapper = render(endpoint({ method: 'POST' }))

    expect(text(wrapper, 'test-call-write-warning')).toContain('real POST')
  })

  it('calls the endpoint with the system timeout once confirmed', async () => {
    call.mockResolvedValue(answer())

    await run(render())

    expect(call).toHaveBeenCalledTimes(1)
    expect(call).toHaveBeenCalledWith('sys-1', 'ep-1', 45)
  })

  // -- Pending --------------------------------------------------------------

  it('shows the wait, and cannot be closed or run twice while it lasts', async () => {
    const pending = deferred<TestCallResponse>()
    call.mockReturnValue(pending.promise)

    const wrapper = render()

    await wrapper.get('[data-testid="test-call-run"]').trigger('click')

    expect(phase(wrapper)).toBe('pending')
    expect(text(wrapper, 'test-call-pending')).toContain('up to 45 seconds')
    expect(wrapper.get('[data-testid="test-call-run"]').attributes('disabled')).toBeDefined()
    expect(wrapper.get('[data-testid="test-call-close"]').attributes('disabled')).toBeDefined()

    await wrapper.get('[data-testid="test-call-run"]').trigger('click')
    expect(call).toHaveBeenCalledTimes(1)

    pending.resolve(answer())
    await flushPromises()

    expect(phase(wrapper)).toBe('answered')
  })

  // -- Answered -------------------------------------------------------------

  it('shows a 2xx answer as a success with everything the server returned', async () => {
    call.mockResolvedValue(answer())

    const wrapper = render()
    await run(wrapper)

    expect(phase(wrapper)).toBe('answered')
    expect(text(wrapper, 'test-call-status')).toBe('Success')
    expect(text(wrapper, 'test-call-status-code')).toBe('HTTP 200')
    expect(text(wrapper, 'test-call-duration')).toBe('in 142 ms')
    expect(text(wrapper, 'test-call-request')).toBe(
      'GET https://erp.example.com/api/purchase-orders/1',
    )
    expect(text(wrapper, 'test-call-log-id')).toBe(LOG_ID)

    const body = wrapper.get('[data-testid="test-call-body"]')
    expect(body.element.tagName).toBe('PRE')
    expect(body.classes()).toEqual(expect.arrayContaining(['font-mono', 'overflow-auto']))
    expect(body.text()).toBe('{"id":1,"token":"****"}')
    expect(wrapper.find('[data-testid="test-call-truncated"]').exists()).toBe(false)
  })

  it('shows a non-2xx answer as Failed, not as an error', async () => {
    call.mockResolvedValue(answer({ status: 'FAILED', statusCode: 500, durationMs: 2500 }))

    const wrapper = render()
    await run(wrapper)

    expect(phase(wrapper)).toBe('answered')
    expect(text(wrapper, 'test-call-status')).toBe('Failed')
    expect(text(wrapper, 'test-call-status-code')).toBe('HTTP 500')
    expect(text(wrapper, 'test-call-duration')).toBe('in 2.50 s')
    expect(wrapper.find('[data-testid="test-call-failure"]').exists()).toBe(false)
  })

  it('says a redirect was not followed', async () => {
    call.mockResolvedValue(answer({ status: 'FAILED', statusCode: 302, bodyPreview: '' }))

    const wrapper = render()
    await run(wrapper)

    expect(wrapper.text()).toContain('Redirects are not followed')
    expect(text(wrapper, 'test-call-body')).toBe('(empty)')
  })

  it('says when the body was cut short', async () => {
    call.mockResolvedValue(answer({ bodyTruncated: true, bodyPreview: 'x'.repeat(2048) }))

    const wrapper = render()
    await run(wrapper)

    expect(text(wrapper, 'test-call-truncated')).toContain('first 2,048 characters')
  })

  // -- Failed ---------------------------------------------------------------

  it('explains a refusal, and shows its code, the server words and the log id', async () => {
    call.mockRejectedValue(
      new ApiError(
        'SECRET_NAME_NOT_PERMITTED',
        `The credential's env:// reference names a variable outside KELIR_INTEGRATION_SECRET_*, which is the                  only part of the environment a test call reads (integration log ${LOG_ID})`,
        422,
      ),
    )

    const wrapper = render()
    await run(wrapper)

    expect(phase(wrapper)).toBe('failed')
    expect(wrapper.get('[data-testid="test-call-failure"]').attributes('data-kind')).toBe('refused')
    expect(text(wrapper, 'test-call-failure-title')).toBe('Refused before anything was sent')
    expect(text(wrapper, 'test-call-explanation')).toContain('KELIR_INTEGRATION_SECRET_')
    expect(text(wrapper, 'test-call-failure-code')).toBe('SECRET_NAME_NOT_PERMITTED')
    // The server's words, its stray run of spaces collapsed.
    expect(text(wrapper, 'test-call-failure-message')).toContain('which is the only part')
    expect(text(wrapper, 'test-call-log-id')).toBe(LOG_ID)
  })

  it.each([
    [504, 'UPSTREAM_TIMEOUT', 'within the system'],
    [502, 'UPSTREAM_UNREACHABLE', 'could not be reached'],
  ])('explains a %i as a system that did not answer', async (status, code, explanation) => {
    call.mockRejectedValue(new ApiError(code, `No answer (integration log ${LOG_ID})`, status))

    const wrapper = render()
    await run(wrapper)

    expect(wrapper.get('[data-testid="test-call-failure"]').attributes('data-kind')).toBe(
      'unanswered',
    )
    expect(text(wrapper, 'test-call-failure-title')).toBe('The system did not answer')
    expect(text(wrapper, 'test-call-explanation')).toContain(explanation)
    expect(text(wrapper, 'test-call-log-id')).toBe(LOG_ID)
  })

  it('shows a failure with no log id and an unknown code without inventing either', async () => {
    call.mockRejectedValue(new ApiError('INTERNAL_ERROR', 'Something broke', 500))

    const wrapper = render()
    await run(wrapper)

    expect(text(wrapper, 'test-call-failure-title')).toBe('The test call could not be run')
    expect(wrapper.find('[data-testid="test-call-explanation"]').exists()).toBe(false)
    expect(wrapper.find('[data-testid="test-call-log-id"]').exists()).toBe(false)
    expect(text(wrapper, 'test-call-failure-message')).toBe('Something broke')
  })

  it('runs again from a result', async () => {
    call.mockRejectedValueOnce(new ApiError('HOST_NOT_RESOLVED', 'No host', 422))
    call.mockResolvedValueOnce(answer())

    const wrapper = render()
    await run(wrapper)
    expect(phase(wrapper)).toBe('failed')
    expect(text(wrapper, 'test-call-run')).toBe('Run again')

    await run(wrapper)
    expect(phase(wrapper)).toBe('answered')
    expect(call).toHaveBeenCalledTimes(2)
  })

  // -- Stale runs -----------------------------------------------------------

  it('starts over when reopened, and drops the answer of a call from before', async () => {
    const pending = deferred<TestCallResponse>()
    call.mockReturnValue(pending.promise)

    const wrapper = render()
    await wrapper.get('[data-testid="test-call-run"]').trigger('click')
    expect(phase(wrapper)).toBe('pending')

    await wrapper.setProps({ open: false })
    await wrapper.setProps({ open: true })
    expect(phase(wrapper)).toBe('confirm')

    pending.resolve(answer())
    await flushPromises()

    expect(phase(wrapper)).toBe('confirm')
    expect(wrapper.find('[data-testid="test-call-status"]').exists()).toBe(false)
  })
})
