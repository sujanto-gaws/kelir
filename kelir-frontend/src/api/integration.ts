import { deleteItem, getItem, getPage, postItem, putItem } from './client'
import type { ListFetchQuery } from '@/composables/useQueryBackedList'
import type { Page, PageQuery } from '@/types/api'
import type {
  CreateIntegrationCredentialRequest,
  CreateIntegrationEndpointRequest,
  ExternalSystem,
  IntegrationCredential,
  IntegrationEndpoint,
  RegisterExternalSystemRequest,
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
 */
const SYSTEMS = '/integration/external-systems'

/**
 * The system list, filtered on the server.
 *
 * A blank filter is an absent parameter rather than an empty one: `?status=`
 * does not parse as a status and the backend answers it with a 422.
 */
export function listExternalSystems(query: ListFetchQuery = {}): Promise<Page<ExternalSystem>> {
  const params: Record<string, string | number> = {}

  for (const [key, value] of Object.entries(query)) {
    if (value !== undefined && value !== null && value !== '') {
      params[key] = value
    }
  }

  return getPage<ExternalSystem>(SYSTEMS, params)
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
