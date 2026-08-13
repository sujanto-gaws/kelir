import {
  AxiosError,
  AxiosHeaders,
  type AxiosResponse,
  type InternalAxiosRequestConfig,
} from 'axios'

import { apiClient } from '@/api/client'

/**
 * Test support — never imported by application code.
 *
 * Replaces the axios adapter rather than stubbing the client's own functions,
 * so interceptors, retries and envelope handling all run for real and only the
 * network is fake. That is what makes it possible to assert things like "the
 * refresh endpoint was called exactly once".
 */

export interface RecordedRequest {
  url: string
  method: string
  /** The parsed request body, or undefined when there was none. */
  body: unknown
  authorization?: string
}

export interface FakeReply {
  status: number
  /** Response body, already in the envelope the endpoint would return. */
  body?: unknown
  /**
   * Fail the way an unreachable server fails: a rejection carrying no response
   * at all, which is what axios produces when offline, and what separates a
   * transport failure from the server's verdict. `status` is ignored.
   */
  networkError?: boolean
}

export type FakeHandler = (request: RecordedRequest) => FakeReply

export interface FakeBackendHandle {
  /** Every request that reached the adapter, in order, including retries. */
  requests: RecordedRequest[]
  /** How many requests hit a URL — the count that proves single-flight refresh. */
  countOf(url: string): number
  restore(): void
}

/** An item envelope, as every non-204 endpoint returns. */
export function itemBody(data: unknown): unknown {
  return { success: true, data }
}

/** An error envelope, matching `kelir-backend/src/response.rs`. */
export function errorBody(
  code: string,
  message: string,
  details: { path: string; rule: string; code: string; message: string }[] = [],
): unknown {
  return { success: false, error: { code, message, details } }
}

function parseBody(data: unknown): unknown {
  if (typeof data !== 'string') {
    return data
  }

  try {
    return JSON.parse(data)
  } catch {
    return data
  }
}

function toRecorded(config: InternalAxiosRequestConfig): RecordedRequest {
  const authorization = config.headers.get('Authorization')

  return {
    url: config.url ?? '',
    method: (config.method ?? 'get').toLowerCase(),
    body: parseBody(config.data),
    authorization: typeof authorization === 'string' ? authorization : undefined,
  }
}

/**
 * Point the shared client at `handler` until `restore()` is called.
 *
 * Statuses at or above 400 are delivered the way axios delivers them — a
 * rejected `AxiosError` carrying the response — so the client's error path is
 * the one under test.
 */
export function installFakeBackend(handler: FakeHandler): FakeBackendHandle {
  const original = apiClient.defaults.adapter
  const requests: RecordedRequest[] = []

  apiClient.defaults.adapter = (config: InternalAxiosRequestConfig) => {
    const recorded = toRecorded(config)
    requests.push(recorded)

    const reply = handler(recorded)

    if (reply.networkError) {
      // No `response` property: that absence is exactly what `toApiError` reads
      // to classify a failure as a network error rather than a status.
      return Promise.reject(new AxiosError('Network Error', AxiosError.ERR_NETWORK, config))
    }

    const response: AxiosResponse = {
      status: reply.status,
      statusText: '',
      data: reply.body ?? '',
      headers: new AxiosHeaders(),
      config,
    }

    if (reply.status >= 400) {
      const error = new AxiosError(`request failed with status ${reply.status}`, undefined, config)
      error.response = response
      return Promise.reject(error)
    }

    return Promise.resolve(response)
  }

  return {
    requests,
    countOf: (url: string) => requests.filter((request) => request.url === url).length,
    restore: () => {
      apiClient.defaults.adapter = original
    },
  }
}
