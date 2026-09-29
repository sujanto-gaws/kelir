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

/**
 * A list query that also narrows on the server (#525).
 *
 * `search` is a case-insensitive substring match on the resource's key and
 * name; `status` is an exact match on its status. A chooser sends these rather
 * than fetching one page and filtering it, which left whatever sorted past the
 * hundredth row unreachable. A resource with no status filter leaves `Status`
 * at `never`. Blank values are not sent (`withoutBlanks`).
 */
export interface SearchPageQuery<Status extends string = never> extends PageQuery {
  search?: string
  status?: Status
}
