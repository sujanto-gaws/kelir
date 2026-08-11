import { AxiosError, AxiosHeaders } from 'axios'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { apiClient, getItem, getPage, toApiError } from './client'
import { ApiError, CLIENT_ERROR_CODES } from './error'

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
})
