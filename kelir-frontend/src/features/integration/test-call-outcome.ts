import { CLIENT_ERROR_CODES, type ApiError } from '@/api/error'

/**
 * What a failed test call means, in words an administrator can act on
 * (FR-INT-002, #547).
 *
 * The server's message says *what* happened and names the integration log
 * row. The explanation here says *what to do about it*, keyed on the code —
 * never on the message, which is prose and may change (coding standard §2.3).
 */

/** The three kinds of failure, which differ in whether the system was reached. */
export type TestCallFailureKind = 'refused' | 'unanswered' | 'failed'

export interface TestCallFailure {
  kind: TestCallFailureKind
  title: string
  /** What the code means for the person looking at it; empty for a code this client does not know. */
  explanation: string
  /** The server's own words, with its whitespace collapsed. */
  message: string
  code: string
  /** The `integration_logs` row the server wrote, when its message named one. */
  logId: string | null
}

/**
 * One sentence per code the test call can answer with. **A 422 means nothing
 * was sent**, so each of these is a setting to fix before trying again.
 */
export const TEST_CALL_EXPLANATIONS: Record<string, string> = {
  EXTERNAL_SYSTEM_NOT_ACTIVE:
    'The system is inactive or in maintenance. Activate it, or set it back to Active, and try again.',
  ENDPOINT_NOT_ACTIVE: 'This endpoint is retired. Reinstate it to call it.',
  BASE_URL_MISSING: 'The system has no base URL. Edit the system and add one.',
  TARGET_URL_INVALID:
    "The system's base URL and this endpoint's path do not make a URL on that system. Check both.",
  NO_USABLE_CREDENTIAL:
    'The system has no credential reference that is active and valid today. A test call needs exactly one.',
  AMBIGUOUS_CREDENTIAL:
    'More than one credential reference is active and valid today. Deactivate or date all but one.',
  CREDENTIAL_TYPE_NOT_SUPPORTED:
    'A test call can send a Bearer token or Basic auth credential only. Other types are not built yet.',
  SECRET_REFERENCE_MALFORMED:
    'The credential reference is not one Kelir can follow. Use env://KELIR_INTEGRATION_SECRET_<NAME>.',
  SECRET_BACKEND_NOT_CONFIGURED:
    'vault:// references cannot be read in this release. Use an env:// reference instead.',
  SECRET_NAME_NOT_PERMITTED:
    'A test call reads only environment variables whose names start with KELIR_INTEGRATION_SECRET_. Rename the variable and the reference.',
  SECRET_NOT_FOUND:
    'The environment variable the credential names is not set on the server, or is empty. Ask whoever runs Kelir to set it.',
  SECRET_MALFORMED:
    'The secret was found but cannot be sent as it is. A Basic auth secret must be user:password.',
  HOST_NOT_RESOLVED: "The system's host name did not resolve. Check the base URL.",
  EGRESS_REFUSED:
    "Kelir will not connect to the address the system's host resolves to. A private network range must be allowed by whoever runs Kelir; loopback and link-local addresses never are.",
  UPSTREAM_UNREACHABLE:
    'The system could not be reached: the connection failed or was closed. Check that it is running and reachable from the Kelir server.',
  UPSTREAM_TIMEOUT:
    "The system did not answer within the system's timeout. Raise the timeout, or check the system.",
  [CLIENT_ERROR_CODES.timeout]:
    'Your browser stopped waiting before the server answered. The call may still have run and been logged.',
  [CLIENT_ERROR_CODES.network]:
    'The Kelir server could not be reached, so the test call was not run.',
  FORBIDDEN: 'You do not hold integration:endpoint:call.',
  NOT_FOUND: 'The system or the endpoint no longer exists.',
}

const LOG_ID = /integration log ([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})/i

/** The log row a failure's message names, e.g. `… (integration log 0199…)`. */
export function logIdIn(message: string): string | null {
  return LOG_ID.exec(message)?.[1] ?? null
}

function kindOf(error: ApiError): TestCallFailureKind {
  if (error.status === 422) {
    return 'refused'
  }

  return error.status === 502 || error.status === 504 ? 'unanswered' : 'failed'
}

const TITLES: Record<TestCallFailureKind, string> = {
  refused: 'Refused before anything was sent',
  unanswered: 'The system did not answer',
  failed: 'The test call could not be run',
}

export function describeTestCallFailure(error: ApiError): TestCallFailure {
  const kind = kindOf(error)
  const message = error.message.replace(/\s+/g, ' ').trim()

  return {
    kind,
    title: TITLES[kind],
    explanation: TEST_CALL_EXPLANATIONS[error.code] ?? '',
    message,
    code: error.code,
    logId: logIdIn(message),
  }
}
