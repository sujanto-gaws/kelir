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
