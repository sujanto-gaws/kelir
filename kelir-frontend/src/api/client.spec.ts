import { AxiosError, AxiosHeaders } from 'axios'
import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from 'vitest'

import { apiClient, getItem, getPage, toApiError } from './client'
import { ApiError, CLIENT_ERROR_CODES } from './error'
import { registerSessionBridge } from './session'
import {
  errorBody,
  installFakeBackend,
  itemBody,
  type FakeBackendHandle,
} from '@/lib/testing/fake-backend'

function axiosErrorWith(status: number, data: unknown): AxiosError {
  const error = new AxiosError('request failed')
  error.response = {
    status,
    statusText: '',
    data,
    headers: new AxiosHeaders(),
    config: { headers: new AxiosHeaders() },
  }
  return error
}

describe('apiClient', () => {
  it('targets the versioned api base path', () => {
    expect(apiClient.defaults.baseURL).toContain('/api/v1')
  })

  it('applies a request timeout so a hung backend cannot block the ui', () => {
    expect(apiClient.defaults.timeout).toBeGreaterThan(0)
  })
})

describe('envelope unwrapping', () => {
  beforeEach(() => {
    vi.restoreAllMocks()
  })

  it('returns data from an item envelope', async () => {
    vi.spyOn(apiClient, 'get').mockResolvedValue({
      data: { success: true, data: { id: 'abc', name: 'Kelir' } },
    })

    await expect(getItem<{ id: string; name: string }>('/tenants/abc')).resolves.toEqual({
      id: 'abc',
      name: 'Kelir',
    })
  })

  it('returns rows and meta from a list envelope', async () => {
    vi.spyOn(apiClient, 'get').mockResolvedValue({
      data: {
        success: true,
        data: [{ id: '1' }, { id: '2' }],
        meta: { page: 2, pageSize: 20, total: 42 },
      },
    })

    const page = await getPage<{ id: string }>('/tenants', { page: 2 })

    expect(page.items).toHaveLength(2)
    expect(page.meta).toEqual({ page: 2, pageSize: 20, total: 42 })
  })

  it('rejects a 200 response that is not in the envelope', async () => {
    // A proxy returning its own JSON body must not be mistaken for data.
    vi.spyOn(apiClient, 'get').mockResolvedValue({ data: { id: 'abc' } })

    await expect(getItem('/tenants/abc')).rejects.toMatchObject({
      code: CLIENT_ERROR_CODES.malformed,
    })
  })

  it('rejects a list response missing its meta', async () => {
    vi.spyOn(apiClient, 'get').mockResolvedValue({ data: { success: true, data: [] } })

    await expect(getPage('/tenants')).rejects.toBeInstanceOf(ApiError)
  })
})

describe('error normalisation', () => {
  it('carries the code, message and details from the error envelope', () => {
    const error = toApiError(
      axiosErrorWith(422, {
        success: false,
        error: {
          code: 'VALIDATION_ERROR',
          message: 'Validation failed',
          details: [
            { path: 'name', rule: 'required', code: 'REQUIRED', message: 'Name is required' },
          ],
        },
      }),
    )

    expect(error.code).toBe('VALIDATION_ERROR')
    expect(error.status).toBe(422)
    expect(error.isValidation).toBe(true)
    expect(error.fieldErrors()).toEqual({ name: 'Name is required' })
  })

  it('keeps the first message per field so each input shows one reason', () => {
    const error = toApiError(
      axiosErrorWith(422, {
        success: false,
        error: {
          code: 'VALIDATION_ERROR',
          message: 'Validation failed',
          details: [
            { path: 'name', rule: 'required', code: 'REQUIRED', message: 'first' },
            { path: 'name', rule: 'maxLength', code: 'TOO_LONG', message: 'second' },
          ],
        },
      }),
    )

    expect(error.fieldErrors()).toEqual({ name: 'first' })
  })

  it('distinguishes unauthorized from forbidden', () => {
    const unauthorized = toApiError(
      axiosErrorWith(401, {
        success: false,
        error: { code: 'UNAUTHORIZED', message: 'Authentication required', details: [] },
      }),
    )
    const forbidden = toApiError(
      axiosErrorWith(403, {
        success: false,
        error: { code: 'FORBIDDEN', message: 'Not permitted', details: [] },
      }),
    )

    expect(unauthorized.isUnauthorized).toBe(true)
    expect(unauthorized.isForbidden).toBe(false)
    expect(forbidden.isForbidden).toBe(true)
    expect(forbidden.isUnauthorized).toBe(false)
  })

  it('reports a network failure when no response came back', () => {
    const error = toApiError(new AxiosError('Network Error'))

    expect(error.code).toBe(CLIENT_ERROR_CODES.network)
    expect(error.isNetwork).toBe(true)
  })

  it('reports a timeout distinctly from a network failure', () => {
    const timeout = new AxiosError('timeout of 30000ms exceeded')
    timeout.code = 'ECONNABORTED'

    expect(toApiError(timeout).code).toBe(CLIENT_ERROR_CODES.timeout)
  })

  it('does not surface a non-envelope error body', () => {
    // An HTML error page from a proxy must never reach the user as a message.
    const error = toApiError(axiosErrorWith(502, '<html><body>Bad Gateway</body></html>'))

    expect(error.code).toBe(CLIENT_ERROR_CODES.malformed)
    expect(error.message).not.toContain('html')
  })

  it('passes an ApiError through unchanged', () => {
    const original = new ApiError('CONFLICT', 'Already exists', 409)

    expect(toApiError(original)).toBe(original)
  })

  it('reads the wait out of a rate-limit response', () => {
    // The backend puts the wait in the message today; the header is honoured
    // first so a future backend change needs no client change.
    const error = toApiError(
      axiosErrorWith(429, {
        success: false,
        error: {
          code: 'TOO_MANY_REQUESTS',
          message: 'Too many attempts. Try again in 42 seconds.',
          details: [],
        },
      }),
    )

    expect(error.isRateLimited).toBe(true)
    expect(error.retryAfterSeconds).toBe(42)
  })
})

/**
 * The refresh-and-retry path. These tests drive the real interceptors through a
 * fake adapter, because the invariants they protect — one refresh per request,
 * one refresh in flight — are properties of the interceptor, not of any
 * function that could be stubbed.
 */
describe('refresh on 401', () => {
  const unauthorizedBody = errorBody('UNAUTHORIZED', 'Authentication required')

  let backend: FakeBackendHandle
  let refresh: Mock<() => Promise<boolean>>
  let token: string | null

  beforeEach(() => {
    // The unwrapping suite above spies on `apiClient.get`; these tests need the
    // real request path so the interceptors actually run.
    vi.restoreAllMocks()
    token = 'access-1'
    refresh = vi.fn<() => Promise<boolean>>().mockResolvedValue(true)
    registerSessionBridge({ accessToken: () => token, refresh })
  })

  afterEach(() => {
    backend.restore()
    registerSessionBridge(null)
  })

  it('attaches the bearer token and retries once with the refreshed one', async () => {
    backend = installFakeBackend((request) =>
      backend.countOf(request.url) === 1
        ? { status: 401, body: unauthorizedBody }
        : { status: 200, body: itemBody({ id: 'w1' }) },
    )
    refresh.mockImplementation(() => {
      token = 'access-2'
      return Promise.resolve(true)
    })

    await expect(getItem<{ id: string }>('/widgets')).resolves.toEqual({ id: 'w1' })

    expect(refresh).toHaveBeenCalledTimes(1)
    expect(backend.countOf('/widgets')).toBe(2)
    expect(backend.requests).toMatchObject([
      { authorization: 'Bearer access-1' },
      { authorization: 'Bearer access-2' },
    ])
  })

  it('spends exactly one refresh per request rather than looping', async () => {
    // The server keeps saying 401 even after a successful refresh. Without the
    // one-shot flag this retries forever.
    backend = installFakeBackend(() => ({ status: 401, body: unauthorizedBody }))

    await expect(getItem('/widgets')).rejects.toMatchObject({ status: 401 })

    expect(refresh).toHaveBeenCalledTimes(1)
    expect(backend.countOf('/widgets')).toBe(2)
  })

  it('gives up without retrying when the refresh fails', async () => {
    // A failed refresh means the session is gone: the store has cleared it and
    // retrying could only produce another 401.
    refresh.mockResolvedValue(false)
    backend = installFakeBackend(() => ({ status: 401, body: unauthorizedBody }))

    await expect(getItem('/widgets')).rejects.toMatchObject({ status: 401 })

    expect(refresh).toHaveBeenCalledTimes(1)
    expect(backend.countOf('/widgets')).toBe(1)
  })

  it('never refreshes for the endpoints that mint tokens', async () => {
    // A 401 from login is the answer, and a 401 from refresh means the token is
    // spent — presenting it again would revoke the whole session family.
    backend = installFakeBackend(() => ({ status: 401, body: unauthorizedBody }))

    await expect(getItem('/auth/login')).rejects.toMatchObject({ status: 401 })
    await expect(getItem('/auth/refresh')).rejects.toMatchObject({ status: 401 })
    await expect(getItem('/auth/logout')).rejects.toMatchObject({ status: 401 })

    expect(refresh).not.toHaveBeenCalled()
    expect(backend.countOf('/auth/login')).toBe(1)
  })

  it('leaves a 403 alone — refreshing cannot grant a permission', async () => {
    backend = installFakeBackend(() => ({
      status: 403,
      body: errorBody('FORBIDDEN', 'You do not have permission to perform this action'),
    }))

    await expect(getItem('/widgets')).rejects.toMatchObject({ status: 403 })

    expect(refresh).not.toHaveBeenCalled()
  })

  it('sends no authorization header when there is no session', async () => {
    token = null
    backend = installFakeBackend(() => ({ status: 200, body: itemBody({ id: 'w1' }) }))

    await getItem('/widgets')

    expect(backend.requests).toMatchObject([{ authorization: undefined }])
  })
})
