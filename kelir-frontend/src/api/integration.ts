import { deleteItem, getItem, getPage, postItem, putItem } from './client'
import type { ListFetchQuery } from '@/composables/useQueryBackedList'
import type { Page, PageQuery } from '@/types/api'
import type {
  CreateIntegrationCredentialRequest,
  CreateIntegrationEndpointRequest,
  ExternalSystem,
  IntegrationCredential,
  IntegrationEndpoint,
  IntegrationLog,
  IntegrationLogSummary,
  RegisterExternalSystemRequest,
  TestCallResponse,
  UpdateExternalSystemRequest,
  UpdateIntegrationCredentialRequest,
  UpdateIntegrationEndpointRequest,
} from '@/types/integration'

/**
 * The external system registry (`/api/v1/integration/external-systems/*`,
 * FR-INT-001, #520).
 *
 * Three resources under one system. **Endpoints ride on the system's own
 * permissions** (`integration:external-system:read` and `:update`) and are
 * managed on its detail page. **Credentials have permissions of their own**
 * (`integration:credential:*`) — seeing a system does not mean seeing where
 * its secrets live — and their routes require only those.
 *
 * There is no `DELETE` for a system or an endpoint: a system is deactivated,
 * an endpoint is retired by its status.
 *
 * **A test call has a permission of its own** (`integration:endpoint:call`,
 * FR-INT-002, #547): reading an endpoint does not let you call it.
 */
const SYSTEMS = '/integration/external-systems'

/**
 * The system list, filtered on the server.
 *
 * A blank filter is an absent parameter rather than an empty one: `?status=`
 * does not parse as a status and the backend answers it with a 422.
 */
export function listExternalSystems(query: ListFetchQuery = {}): Promise<Page<ExternalSystem>> {
  return getPage<ExternalSystem>(SYSTEMS, withoutBlanks(query))
}

/** A list query with its blank values left out, so none reaches the wire as `?key=`. */
function withoutBlanks(query: ListFetchQuery): Record<string, string | number> {
  const params: Record<string, string | number> = {}

  for (const [key, value] of Object.entries(query)) {
    if (value !== undefined && value !== null && value !== '') {
      params[key] = value
    }
  }

  return params
}

export function getExternalSystem(id: string): Promise<ExternalSystem> {
  return getItem<ExternalSystem>(`${SYSTEMS}/${id}`)
}

export function registerExternalSystem(
  request: RegisterExternalSystemRequest,
): Promise<ExternalSystem> {
  return postItem<ExternalSystem>(SYSTEMS, request)
}

export function updateExternalSystem(
  id: string,
  request: UpdateExternalSystemRequest,
): Promise<ExternalSystem> {
  return putItem<ExternalSystem>(`${SYSTEMS}/${id}`, request)
}

/** Turns a system off. Idempotent: an inactive system comes back unchanged. */
export function deactivateExternalSystem(id: string): Promise<ExternalSystem> {
  return postItem<ExternalSystem>(`${SYSTEMS}/${id}/deactivate`)
}

/**
 * Turns an inactive system back on — under `:deactivate`, because turning a
 * system on or off is one permission. A `PUT` cannot do this.
 */
export function activateExternalSystem(id: string): Promise<ExternalSystem> {
  return postItem<ExternalSystem>(`${SYSTEMS}/${id}/activate`)
}

/** Every endpoint of a system, retired ones included, ordered by code. */
export function listIntegrationEndpoints(
  systemId: string,
  query: PageQuery = {},
): Promise<Page<IntegrationEndpoint>> {
  return getPage<IntegrationEndpoint>(`${SYSTEMS}/${systemId}/endpoints`, query)
}

export function createIntegrationEndpoint(
  systemId: string,
  request: CreateIntegrationEndpointRequest,
): Promise<IntegrationEndpoint> {
  return postItem<IntegrationEndpoint>(`${SYSTEMS}/${systemId}/endpoints`, request)
}

/** Edits an endpoint; retiring one is `{ status: 'INACTIVE' }`. */
export function updateIntegrationEndpoint(
  systemId: string,
  endpointId: string,
  request: UpdateIntegrationEndpointRequest,
): Promise<IntegrationEndpoint> {
  return putItem<IntegrationEndpoint>(`${SYSTEMS}/${systemId}/endpoints/${endpointId}`, request)
}

/**
 * A system's credential references, inactive ones included, oldest first.
 *
 * A caller without `integration:credential:read` is answered 403, and the page
 * treats that as *this section is not yours* rather than as a failure.
 */
export function listIntegrationCredentials(
  systemId: string,
  query: PageQuery = {},
): Promise<Page<IntegrationCredential>> {
  return getPage<IntegrationCredential>(`${SYSTEMS}/${systemId}/credentials`, query)
}

export function createIntegrationCredential(
  systemId: string,
  request: CreateIntegrationCredentialRequest,
): Promise<IntegrationCredential> {
  return postItem<IntegrationCredential>(`${SYSTEMS}/${systemId}/credentials`, request)
}

export function updateIntegrationCredential(
  systemId: string,
  credentialId: string,
  request: UpdateIntegrationCredentialRequest,
): Promise<IntegrationCredential> {
  return putItem<IntegrationCredential>(
    `${SYSTEMS}/${systemId}/credentials/${credentialId}`,
    request,
  )
}

/** A soft delete; the API answers 204. */
export function deleteIntegrationCredential(systemId: string, credentialId: string): Promise<void> {
  return deleteItem(`${SYSTEMS}/${systemId}/credentials/${credentialId}`)
}

/**
 * The margin the browser waits beyond the system's own timeout, for the
 * server's resolution, logging and reply. Without it the client's default of
 * 30 seconds equals the default `timeoutSeconds`, and the browser would give
 * up at the same moment the server reports `UPSTREAM_TIMEOUT` — with its log id.
 */
const TEST_CALL_MARGIN_SECONDS = 15

/**
 * Calls one endpoint once, with the system's one usable credential, and says
 * what came back (FR-INT-002, #547). There is no request body, here or to the
 * system.
 *
 * A `200` is an answer, `SUCCESS` or `FAILED`. A refusal before anything was
 * sent is a 422, an unreachable system a 502 and a timeout a 504; each is
 * logged, and its message names the log row. `timeoutSeconds` is the system's,
 * so the browser waits for the server's verdict rather than giving up first.
 */
export function testCallIntegrationEndpoint(
  systemId: string,
  endpointId: string,
  timeoutSeconds: number,
): Promise<TestCallResponse> {
  return postItem<TestCallResponse>(
    `${SYSTEMS}/${systemId}/endpoints/${endpointId}/test-call`,
    undefined,
    { timeout: (timeoutSeconds + TEST_CALL_MARGIN_SECONDS) * 1000 },
  )
}

const LOGS = '/integration/logs'

/**
 * The integration log, newest first (FR-INT-006, #548), under
 * `integration:log:read` — a permission of its own, apart from reading the
 * systems: a log reader need not manage them, and a system manager does not
 * see every call's payload by default.
 *
 * Filters: `externalSystemId`, `status`, and `from` and `to` as ISO instants
 * on `startedAt`. Blank ones are left out, as on the system list.
 */
export function listIntegrationLogs(
  query: ListFetchQuery = {},
): Promise<Page<IntegrationLogSummary>> {
  return getPage<IntegrationLogSummary>(LOGS, withoutBlanks(query))
}

/** One log row with its payloads, masked when written and returned as stored. */
export function getIntegrationLog(id: string): Promise<IntegrationLog> {
  return getItem<IntegrationLog>(`${LOGS}/${id}`)
}
