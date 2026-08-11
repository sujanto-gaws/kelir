/**
 * Wire types for the backend response envelope.
 *
 * These mirror `kelir-backend/src/response.rs` and are the only place the
 * envelope shape is described on the client (coding standard §3.3). Components
 * never see them — the API client unwraps to `data` before returning.
 */

/** A single field-level validation failure (JSON Form Schema S10.3). */
export interface ValidationDetail {
  path: string
  rule: string
  code: string
  message: string
}

/** Pagination metadata returned with every list response. */
export interface PageMeta {
  page: number
  pageSize: number
  total: number
}

export interface ItemEnvelope<T> {
  success: true
  data: T
}

export interface ListEnvelope<T> {
  success: true
  data: T[]
  meta: PageMeta
}

export interface ErrorEnvelope {
  success: false
  error: {
    code: string
    message: string
    details: ValidationDetail[]
  }
}

/** A list result after unwrapping: rows plus their pagination metadata. */
export interface Page<T> {
  items: T[]
  meta: PageMeta
}

/** Query parameters accepted by every list endpoint. */
export interface PageQuery {
  page?: number
  pageSize?: number
}
