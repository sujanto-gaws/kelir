import axios, {
  AxiosError,
  type AxiosInstance,
  type AxiosRequestConfig,
  type InternalAxiosRequestConfig,
} from 'axios'

import { ApiError, CLIENT_ERROR_CODES } from './error'
import { currentSessionBridge } from './session'
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

/**
 * Marks a request that has already spent its one refresh attempt.
 *
 * A string key, not a symbol: axios rebuilds the config through `mergeConfig`
 * on the retry, and that walks `Object.keys`, which omits symbols. A symbol
 * flag would silently vanish and the request would refresh forever. Unknown
 * string keys survive the merge and are never put on the wire.
 */
const REFRESH_ATTEMPTED = 'kelirRefreshAttempted'

interface RetryableConfig extends InternalAxiosRequestConfig {
  [REFRESH_ATTEMPTED]?: boolean
}

/**
 * Endpoints that must never trigger a refresh-and-retry.
 *
 * A 401 from any of these *is* the answer — bad credentials, a dead refresh
 * token — so retrying would either loop or, worse, present an already-rotated
 * refresh token and take the whole session family down with it.
 */
const NO_REFRESH_PATHS = ['/auth/login', '/auth/refresh', '/auth/logout'] as const

function isRefreshableRequest(url: string | undefined): boolean {
  const path = (url ?? '').split('?')[0]

  return !NO_REFRESH_PATHS.some((excluded) => path === excluded || path.endsWith(excluded))
}

// Attach the bearer token on every request, including retries: a retry re-enters
// this interceptor, so it picks up the token the refresh just installed rather
// than replaying the expired one.
apiClient.interceptors.request.use((config) => {
  const token = currentSessionBridge()?.accessToken() ?? null

  if (token) {
    config.headers.set('Authorization', `Bearer ${token}`)
  } else {
    config.headers.delete('Authorization')
  }

  return config
})

/**
 * Refresh once on 401, then retry the original request.
 *
 * Two invariants matter here and are covered by tests:
 *
 * 1. **At most one refresh per request.** The request is flagged before the
 *    retry, so a second 401 propagates instead of refreshing again.
 * 2. **At most one refresh in flight.** Concurrent 401s all await the same
 *    promise, which the bridge (the auth store) owns. The backend revokes a
 *    refresh token as it is used and treats a replay as theft by killing the
 *    session family — so a parallel second refresh would sign the user out.
 */
apiClient.interceptors.response.use(undefined, async (error: unknown) => {
  if (!(error instanceof AxiosError) || error.response?.status !== 401) {
    throw error
  }

  const config = error.config as RetryableConfig | undefined
  const bridge = currentSessionBridge()

  if (!config || config[REFRESH_ATTEMPTED] || !bridge || !isRefreshableRequest(config.url)) {
    throw error
  }

  config[REFRESH_ATTEMPTED] = true

  // A failed refresh has already cleared the session inside the bridge; the
  // original 401 travels on so the caller sees why the request failed.
  if (!(await bridge.refresh())) {
    throw error
  }

  return apiClient.request(config)
})

function isErrorEnvelope(body: unknown): body is ErrorEnvelope {
  if (typeof body !== 'object' || body === null) {
    return false
  }

  const candidate = body as Partial<ErrorEnvelope>

  return candidate.success === false && typeof candidate.error?.code === 'string'
}

/**
 * How long to wait after a 429.
 *
 * `Retry-After` is preferred and is where a future backend change would put it.
 * Today the wait is only in the message, so it is read from there as a
 * fallback — and read defensively: an unparsed message simply yields no
 * countdown, and the message itself is what the user is shown either way.
 */
function retryAfterFrom(error: AxiosError, status: number, message: string): number | undefined {
  if (status !== 429) {
    return undefined
  }

  const header = error.response?.headers?.['retry-after']
  const fromHeader = typeof header === 'string' || typeof header === 'number' ? Number(header) : NaN

  if (Number.isFinite(fromHeader) && fromHeader >= 0) {
    return fromHeader
  }

  const match = /(\d+)\s*second/i.exec(message)

  return match ? Number(match[1]) : undefined
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
      return new ApiError(
        body.error.code,
        body.error.message,
        status,
        body.error.details ?? [],
        retryAfterFrom(error, status, body.error.message),
      )
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

/**
 * POST a body to an endpoint that answers 204 with no envelope — sign-out and
 * change-password, where there is nothing to return.
 */
export async function postVoid(
  url: string,
  body?: unknown,
  config?: AxiosRequestConfig,
): Promise<void> {
  try {
    await apiClient.post<unknown>(url, body, config)
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
