/**
 * What the integration registry endpoints return (`/api/v1/integration/*`,
 * FR-INT-001, #520).
 *
 * The wire shape is the backend's `modules/integration`, serialized
 * `camelCase`. **No field in it holds a resolved secret**: a credential carries
 * a `secretReference`, a reference such as `vault://kelir/erp/api-key` (AC-5).
 * The API checks the reference's shape only, so it is returned as it was
 * entered (#552). Nothing on the client resolves a reference either.
 */

/** Whether a system may be called. `INACTIVE` is reached only through the deactivate verb. */
export type ExternalSystemStatus = 'ACTIVE' | 'INACTIVE' | 'MAINTENANCE'

export type ExternalSystemType =
  | 'ERP'
  | 'CRM'
  | 'HRIS'
  | 'E_SIGNATURE'
  | 'EMAIL_PROVIDER'
  | 'SSO_PROVIDER'
  | 'PAYMENT_GATEWAY'
  | 'TAX_SYSTEM'
  | 'BANK_SYSTEM'
  | 'BI_SYSTEM'
  | 'DOCUMENT_ARCHIVE'

/** How a system authenticates — a system's `authType` and a credential's `credentialType`. */
export type AuthType =
  | 'API_KEY'
  | 'BASIC_AUTH'
  | 'BEARER_TOKEN'
  | 'OAUTH2_CLIENT_CREDENTIALS'
  | 'JWT'
  | 'HMAC_SECRET'
  | 'CERTIFICATE'
  | 'SFTP_PASSWORD'

export type HttpMethod = 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE'

/** An endpoint has no delete: it is retired by `INACTIVE` and reinstated by `ACTIVE`. */
export type EndpointStatus = 'ACTIVE' | 'INACTIVE'

/** Absent keys are omitted on the wire; `{}` when none is set. */
export interface RetryPolicy {
  maxRetries?: number
  initialDelaySeconds?: number
  backoffMultiplier?: number
  deadLetterAfterAttempts?: number
}

export interface ExternalSystem {
  id: string
  systemCode: string
  systemName: string
  systemType: ExternalSystemType | null
  baseUrl: string | null
  authType: AuthType | null
  timeoutSeconds: number
  retryPolicy: RetryPolicy
  description: string | null
  status: ExternalSystemStatus
  createdAt: string
  updatedAt: string
}

/** Always registered `ACTIVE`: there is no `status` on create. */
export interface RegisterExternalSystemRequest {
  systemCode: string
  systemName: string
  systemType?: ExternalSystemType
  baseUrl?: string
  authType?: AuthType
  timeoutSeconds?: number
  retryPolicy?: RetryPolicy
  description?: string
}

/**
 * Every field optional, an omitted one unchanged, `null` clears.
 *
 * `systemCode` is not here because it may not change. `status` may move only
 * between `ACTIVE` and `MAINTENANCE`: into or out of `INACTIVE` is the
 * deactivate and activate verbs, under their own permission.
 */
export interface UpdateExternalSystemRequest {
  systemName?: string
  systemType?: ExternalSystemType | null
  baseUrl?: string | null
  authType?: AuthType | null
  timeoutSeconds?: number
  retryPolicy?: RetryPolicy
  description?: string | null
  status?: Exclude<ExternalSystemStatus, 'INACTIVE'>
}

/** The system list's filters, beside paging. */
export interface ExternalSystemListQuery {
  page?: number
  pageSize?: number
  search?: string
  status?: ExternalSystemStatus
  systemType?: ExternalSystemType
}

export interface IntegrationEndpoint {
  id: string
  externalSystemId: string
  endpointCode: string
  name: string
  method: HttpMethod
  path: string
  description: string | null
  status: EndpointStatus
  createdAt: string
  updatedAt: string
}

export interface CreateIntegrationEndpointRequest {
  endpointCode: string
  name: string
  method: HttpMethod
  path: string
  description?: string
}

export interface UpdateIntegrationEndpointRequest {
  name?: string
  method?: HttpMethod
  path?: string
  description?: string | null
  status?: EndpointStatus
}

/** Exactly the nine keys the API returns, none of them a resolved secret. */
export interface IntegrationCredential {
  id: string
  externalSystemId: string
  credentialType: AuthType
  /** A reference to where the secret lives (`vault://…`, `env://NAME`), as entered; only its shape is checked. */
  secretReference: string
  /** ISO date, `YYYY-MM-DD`. */
  validFrom: string | null
  validTo: string | null
  isActive: boolean
  createdAt: string
  updatedAt: string
}

export interface CreateIntegrationCredentialRequest {
  credentialType: AuthType
  secretReference: string
  validFrom?: string | null
  validTo?: string | null
  isActive?: boolean
}

export interface UpdateIntegrationCredentialRequest {
  credentialType?: AuthType
  secretReference?: string
  validFrom?: string | null
  validTo?: string | null
  isActive?: boolean
}

/** `SUCCESS` for a `2xx` answer, `FAILED` for any other, a `3xx` included. */
export type TestCallStatus = 'SUCCESS' | 'FAILED'

/**
 * A test call the external system answered (FR-INT-002, #547).
 *
 * **An answer is not a success**: a `500` from the system is a `200` from
 * Kelir with `status: 'FAILED'`. A call that got no answer, or was refused
 * before anything was sent, is an error instead, whose message names the
 * integration log row. No header is returned. The preview has an echo of the
 * secret redacted in the spellings ADR-0043 §R lists, and no other (#665).
 */
export interface TestCallResponse {
  /** The `integration_logs` row this call wrote. */
  logId: string
  method: HttpMethod
  /** The system's `baseUrl` joined with the endpoint's `path`. */
  url: string
  status: TestCallStatus
  /** The system's HTTP status. Redirects are not followed, so a `3xx` is returned as it came. */
  statusCode: number
  durationMs: number
  /**
   * The start of the response body, at most 2048 characters. Sensitive keys are masked, and an
   * echo of the secret is redacted only in the spellings ADR-0043 §R lists.
   */
  bodyPreview: string
  /** Whether `bodyPreview` is shorter than the body. */
  bodyTruncated: boolean
}

/** Where a logged call went: out to a system, or in from one. */
export type IntegrationDirection = 'INBOUND' | 'OUTBOUND'

/**
 * A logged call's state (Database Schema §12.5). A test call writes only the
 * two terminal values; the other three belong to the retry work (FR-INT-007).
 */
export type IntegrationLogStatus = 'SUCCESS' | 'FAILED' | 'PENDING' | 'RETRYING' | 'DEAD_LETTER'

/**
 * One `integration_logs` row as the log list returns it (FR-INT-006, #548).
 *
 * **Everything but the payloads.** Most columns are nullable in the schema
 * because a call can be refused before it has a URL, a method or an answer —
 * a test call refused at the credential has no `statusCode`, for one.
 */
export interface IntegrationLogSummary {
  id: string
  externalSystemId: string | null
  /** The system's code and name as they are now, or `null` for a row with no system. */
  externalSystemCode: string | null
  externalSystemName: string | null
  direction: IntegrationDirection
  /** `REST`, `SOAP`, `FILE`, `WEBHOOK` or `QUEUE`. */
  integrationType: string | null
  method: string | null
  /** The URL called, or `null` when the call was refused before it had one. */
  endpoint: string | null
  /** What the call was about: a test call's is `IntegrationEndpoint` and the endpoint's id. */
  entityType: string | null
  entityId: string | null
  status: IntegrationLogStatus
  statusCode: number | null
  errorMessage: string | null
  correlationId: string | null
  startedAt: string
  completedAt: string | null
  durationMs: number | null
}

/**
 * One row in full: the summary, the document it was made for, and what was
 * sent and received. **The payloads are masked when written and shown as
 * stored**: nothing on the client, or behind the route, resolves them again.
 */
export interface IntegrationLog extends IntegrationLogSummary {
  documentId: string | null
  requestPayload: unknown
  responsePayload: unknown
}

export const INTEGRATION_LOG_STATUS_LABELS: Record<IntegrationLogStatus, string> = {
  SUCCESS: 'Success',
  FAILED: 'Failed',
  PENDING: 'Pending',
  RETRYING: 'Retrying',
  DEAD_LETTER: 'Dead letter',
}

export const INTEGRATION_DIRECTION_LABELS: Record<IntegrationDirection, string> = {
  INBOUND: 'Inbound',
  OUTBOUND: 'Outbound',
}

export const EXTERNAL_SYSTEM_STATUS_LABELS: Record<ExternalSystemStatus, string> = {
  ACTIVE: 'Active',
  INACTIVE: 'Inactive',
  MAINTENANCE: 'Maintenance',
}

export const EXTERNAL_SYSTEM_TYPE_LABELS: Record<ExternalSystemType, string> = {
  ERP: 'ERP',
  CRM: 'CRM',
  HRIS: 'HRIS',
  E_SIGNATURE: 'E-signature',
  EMAIL_PROVIDER: 'Email provider',
  SSO_PROVIDER: 'SSO provider',
  PAYMENT_GATEWAY: 'Payment gateway',
  TAX_SYSTEM: 'Tax system',
  BANK_SYSTEM: 'Bank system',
  BI_SYSTEM: 'BI system',
  DOCUMENT_ARCHIVE: 'Document archive',
}

export const AUTH_TYPE_LABELS: Record<AuthType, string> = {
  API_KEY: 'API key',
  BASIC_AUTH: 'Basic auth',
  BEARER_TOKEN: 'Bearer token',
  OAUTH2_CLIENT_CREDENTIALS: 'OAuth 2.0 client credentials',
  JWT: 'JWT',
  HMAC_SECRET: 'HMAC secret',
  CERTIFICATE: 'Certificate',
  SFTP_PASSWORD: 'SFTP password',
}

export const HTTP_METHODS: readonly HttpMethod[] = ['GET', 'POST', 'PUT', 'PATCH', 'DELETE']

export const ENDPOINT_STATUS_LABELS: Record<EndpointStatus, string> = {
  ACTIVE: 'Active',
  INACTIVE: 'Retired',
}

/** A `Record` of labels as the options a `Select` takes, in declaration order. */
export function optionsOf<K extends string>(
  labels: Record<K, string>,
): { value: K; label: string }[] {
  return (Object.keys(labels) as K[]).map((value) => ({ value, label: labels[value] }))
}
