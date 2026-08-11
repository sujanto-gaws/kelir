import axios, { AxiosError, type AxiosInstance, type AxiosRequestConfig } from 'axios'

import { ApiError, CLIENT_ERROR_CODES } from './error'
import type { ErrorEnvelope, ItemEnvelope, ListEnvelope, Page, PageQuery } from '@/types/api'

/**
 * The single HTTP entry point (coding standard §3.3): components and stores
 * call these helpers, never axios directly.
 *
 * Callers receive `data` — the envelope is unwrapped here, and every failure
 * becomes an {@link ApiError} so nothing downstream inspects axios internals.
 */
export const apiClient: AxiosInstance = axios.create({
  baseURL: import.meta.env.VITE_KELIR_API_BASE_URL ?? 'http://localhost:8080/api/v1',
  timeout: 30_000,
  headers: { Accept: 'application/json' },
})

function isErrorEnvelope(body: unknown): body is ErrorEnvelope {
  if (typeof body !== 'object' || body === null) {
    return false
  }

  const candidate = body as Partial<ErrorEnvelope>

  return candidate.success === false && typeof candidate.error?.code === 'string'
}

/** Turns anything axios throws into an ApiError with a stable code. */
export function toApiError(error: unknown): ApiError {
  if (error instanceof ApiError) {
    return error
  }

  if (error instanceof AxiosError) {
    const status = error.response?.status ?? 0
    const body: unknown = error.response?.data

    // The backend's error envelope is the best source when present.
    if (isErrorEnvelope(body)) {
      return new ApiError(body.error.code, body.error.message, status, body.error.details ?? [])
    }

    if (error.code === 'ECONNABORTED') {
      return new ApiError(CLIENT_ERROR_CODES.timeout, 'The request timed out', 0)
    }

    if (!error.response) {
      return new ApiError(
        CLIENT_ERROR_CODES.network,
        'Could not reach the server. Check your connection and try again.',
        0,
      )
    }

    // A response without an envelope — a proxy error page, say. Do not surface
    // its body: it is not ours and may be HTML.
    return new ApiError(
      CLIENT_ERROR_CODES.malformed,
      'The server returned an unexpected response',
      status,
    )
  }

  return new ApiError(
    CLIENT_ERROR_CODES.malformed,
    error instanceof Error ? error.message : 'An unexpected error occurred',
    0,
  )
}

function unwrapItem<T>(body: unknown): T {
  const envelope = body as Partial<ItemEnvelope<T>>

  if (envelope?.success !== true || !('data' in envelope)) {
    throw new ApiError(
      CLIENT_ERROR_CODES.malformed,
      'The server returned a response outside the standard envelope',
      200,
    )
  }

  return envelope.data as T
}

function unwrapList<T>(body: unknown): Page<T> {
  const envelope = body as Partial<ListEnvelope<T>>

  if (envelope?.success !== true || !Array.isArray(envelope.data) || !envelope.meta) {
    throw new ApiError(
      CLIENT_ERROR_CODES.malformed,
      'The server returned a list response outside the standard envelope',
      200,
    )
  }

  return { items: envelope.data, meta: envelope.meta }
}

/** GET an item resource, returning its `data`. */
export async function getItem<T>(url: string, config?: AxiosRequestConfig): Promise<T> {
  try {
    const response = await apiClient.get<unknown>(url, config)
    return unwrapItem<T>(response.data)
  } catch (error) {
    throw toApiError(error)
  }
}

/** GET a list resource, returning its rows and pagination metadata. */
export async function getPage<T>(
  url: string,
  query: PageQuery = {},
  config?: AxiosRequestConfig,
): Promise<Page<T>> {
  try {
    const response = await apiClient.get<unknown>(url, { ...config, params: { ...query } })
    return unwrapList<T>(response.data)
  } catch (error) {
    throw toApiError(error)
  }
}

/** POST a body, returning the created or updated `data`. */
export async function postItem<T>(
  url: string,
  body?: unknown,
  config?: AxiosRequestConfig,
): Promise<T> {
  try {
    const response = await apiClient.post<unknown>(url, body, config)
    return unwrapItem<T>(response.data)
  } catch (error) {
    throw toApiError(error)
  }
}

/** PUT a body, returning the updated `data`. */
export async function putItem<T>(
  url: string,
  body?: unknown,
  config?: AxiosRequestConfig,
): Promise<T> {
  try {
    const response = await apiClient.put<unknown>(url, body, config)
    return unwrapItem<T>(response.data)
  } catch (error) {
    throw toApiError(error)
  }
}

/** DELETE a resource. */
export async function deleteItem(url: string, config?: AxiosRequestConfig): Promise<void> {
  try {
    await apiClient.delete<unknown>(url, config)
  } catch (error) {
    throw toApiError(error)
  }
}

/**
 * Operational endpoints live at the root, outside `/api/v1`, and answer without
 * the envelope — so they need their own client rather than the helpers above.
 */
export async function getOperational<T>(path: string): Promise<T> {
  try {
    const base = (apiClient.defaults.baseURL ?? '').replace(/\/api\/v1\/?$/, '')
    const response = await axios.get<T>(`${base}${path}`, { timeout: 5_000 })
    return response.data
  } catch (error) {
    throw toApiError(error)
  }
}
