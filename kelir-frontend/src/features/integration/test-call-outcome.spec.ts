import { describe, expect, it } from 'vitest'

import { describeTestCallFailure, logIdIn, TEST_CALL_EXPLANATIONS } from './test-call-outcome'
import { ApiError } from '@/api/error'

/**
 * What a failed test call means (#547).
 *
 * **Every code the backend's `TestCallError::code` can answer with has an
 * explanation.** The list is copied from
 * `kelir-backend/src/modules/integration/domain/test_call.rs`; a code added
 * there and not here shows the server's words alone, which this test is the
 * reminder for.
 */
const BACKEND_CODES = [
  'EXTERNAL_SYSTEM_NOT_ACTIVE',
  'ENDPOINT_NOT_ACTIVE',
  'BASE_URL_MISSING',
  'TARGET_URL_INVALID',
  'NO_USABLE_CREDENTIAL',
  'AMBIGUOUS_CREDENTIAL',
  'CREDENTIAL_TYPE_NOT_SUPPORTED',
  'SECRET_REFERENCE_MALFORMED',
  'SECRET_NAME_NOT_PERMITTED',
  'SECRET_BACKEND_NOT_CONFIGURED',
  'SECRET_NOT_FOUND',
  'SECRET_MALFORMED',
  'HOST_NOT_RESOLVED',
  'EGRESS_REFUSED',
  'UPSTREAM_TIMEOUT',
  'UPSTREAM_UNREACHABLE',
]

const LOG_ID = '0199a1a0-0000-7000-8000-00000000f001'

describe('test call outcome', () => {
  it.each(BACKEND_CODES)('explains %s', (code) => {
    expect(TEST_CALL_EXPLANATIONS[code]?.length ?? 0).toBeGreaterThan(20)
  })

  it('finds the log id the server message names', () => {
    expect(logIdIn(`The endpoint is not ACTIVE (integration log ${LOG_ID})`)).toBe(LOG_ID)
    expect(logIdIn('No log here')).toBeNull()
  })

  it.each([
    [422, 'refused'],
    [502, 'unanswered'],
    [504, 'unanswered'],
    [403, 'failed'],
    [0, 'failed'],
  ])('classes a %i as %s', (status, kind) => {
    expect(describeTestCallFailure(new ApiError('X', 'm', status)).kind).toBe(kind)
  })

  it('collapses the whitespace in the server message and keeps its code', () => {
    const failure = describeTestCallFailure(
      new ApiError('SECRET_NOT_FOUND', '  The variable   is not set  ', 422),
    )

    expect(failure.message).toBe('The variable is not set')
    expect(failure.code).toBe('SECRET_NOT_FOUND')
    expect(failure.explanation).toContain('not set on the server')
  })
})
