import type { ValidationDetail } from '@/types/api'

/**
 * The single error type the API layer throws.
 *
 * Every failure — HTTP error envelope, network outage, timeout — arrives here
 * with a stable `code`, so callers branch on the code and never on the
 * human-readable message (coding standard §2.3, mirrored on the client).
 */
export class ApiError extends Error {
  readonly code: string
  readonly status: number
  readonly details: ValidationDetail[]

  constructor(code: string, message: string, status: number, details: ValidationDetail[] = []) {
    super(message)
    this.name = 'ApiError'
    this.code = code
    this.status = status
    this.details = details
  }

  /** True when the failure is field-level and can be shown against inputs. */
  get isValidation(): boolean {
    return this.code === 'VALIDATION_ERROR' && this.details.length > 0
  }

  /** The session is gone or was never established. */
  get isUnauthorized(): boolean {
    return this.status === 401
  }

  /** Authenticated, but not permitted — a different remedy from 401. */
  get isForbidden(): boolean {
    return this.status === 403
  }

  /** Nothing reached the server, so retrying may genuinely help. */
  get isNetwork(): boolean {
    return this.status === 0
  }

  /** Field errors keyed by path, for binding to form inputs. */
  fieldErrors(): Record<string, string> {
    const result: Record<string, string> = {}

    for (const detail of this.details) {
      // First message per field wins: showing one reason per input is clearer
      // than concatenating several.
      if (!(detail.path in result)) {
        result[detail.path] = detail.message
      }
    }

    return result
  }
}

/** Codes assigned client-side when no envelope came back. */
export const CLIENT_ERROR_CODES = {
  network: 'NETWORK_ERROR',
  timeout: 'TIMEOUT',
  malformed: 'MALFORMED_RESPONSE',
} as const
