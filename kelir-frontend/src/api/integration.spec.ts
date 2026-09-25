import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import {
  activateExternalSystem,
  createIntegrationCredential,
  createIntegrationEndpoint,
  deactivateExternalSystem,
  deleteIntegrationCredential,
  getExternalSystem,
  listExternalSystems,
  listIntegrationCredentials,
  listIntegrationEndpoints,
  registerExternalSystem,
  updateExternalSystem,
  updateIntegrationCredential,
  updateIntegrationEndpoint,
} from './integration'
import { ApiError } from './error'
import {
  errorBody,
  installFakeBackend,
  itemBody,
  type FakeBackendHandle,
  type FakeReply,
  type RecordedRequest,
} from '@/lib/testing/fake-backend'

/**
 * The integration registry client (#520).
 *
 * What is asserted is the wire: the path, the verb and the body each call
 * sends, and that a blank filter never reaches the server as `?status=` — which
 * the backend answers with a 422 because an empty string is not a status.
 */

const SYSTEM = 'sys-1'
const LIST = { success: true, data: [], meta: { page: 1, pageSize: 20, total: 0 } }

describe('integration api', () => {
  let backend: FakeBackendHandle
  let reply: (request: RecordedRequest) => FakeReply

  beforeEach(() => {
    reply = (request) =>
      request.method === 'get' && !request.url.endsWith('/sys-1')
        ? { status: 200, body: LIST }
        : { status: 200, body: itemBody({ id: SYSTEM }) }
    backend = installFakeBackend((request) => reply(request))
  })

  afterEach(() => backend.restore())

  function only(): RecordedRequest {
    expect(backend.requests).toHaveLength(1)

    return backend.requests[0]
  }

  it('sends the list filters it was given and drops the blank ones', async () => {
    await listExternalSystems({
      page: 2,
      pageSize: 20,
      search: 'erp',
      status: '',
      systemType: 'ERP',
    })

    const request = only()

    expect(request.url).toBe('/integration/external-systems')
    expect(request.params).toEqual({ page: 2, pageSize: 20, search: 'erp', systemType: 'ERP' })
  })

  it('reads, registers and edits a system at its own path', async () => {
    await getExternalSystem(SYSTEM)
    await registerExternalSystem({ systemCode: 'SAP_ERP', systemName: 'ERP' })
    await updateExternalSystem(SYSTEM, { systemName: 'ERP', status: 'MAINTENANCE' })

    expect(backend.requests.map((request) => [request.method, request.url])).toEqual([
      ['get', '/integration/external-systems/sys-1'],
      ['post', '/integration/external-systems'],
      ['put', '/integration/external-systems/sys-1'],
    ])
    expect(backend.requests[2].body).toEqual({ systemName: 'ERP', status: 'MAINTENANCE' })
  })

  it('turns a system off and on through its two verbs, never a PUT', async () => {
    await deactivateExternalSystem(SYSTEM)
    await activateExternalSystem(SYSTEM)

    expect(backend.requests.map((request) => [request.method, request.url])).toEqual([
      ['post', '/integration/external-systems/sys-1/deactivate'],
      ['post', '/integration/external-systems/sys-1/activate'],
    ])
  })

  it('manages endpoints under the system', async () => {
    await listIntegrationEndpoints(SYSTEM, { pageSize: 50 })
    await createIntegrationEndpoint(SYSTEM, {
      endpointCode: 'CREATE_PO',
      name: 'Create PO',
      method: 'POST',
      path: '/purchase-orders',
    })
    await updateIntegrationEndpoint(SYSTEM, 'ep-1', { status: 'INACTIVE' })

    expect(backend.requests.map((request) => [request.method, request.url])).toEqual([
      ['get', '/integration/external-systems/sys-1/endpoints'],
      ['post', '/integration/external-systems/sys-1/endpoints'],
      ['put', '/integration/external-systems/sys-1/endpoints/ep-1'],
    ])
    expect(backend.requests[0].params).toEqual({ pageSize: 50 })
    expect(backend.requests[2].body).toEqual({ status: 'INACTIVE' })
  })

  it('manages credential references under the system, deleting with a 204', async () => {
    reply = (request) =>
      request.method === 'delete'
        ? { status: 204 }
        : request.method === 'get'
          ? { status: 200, body: LIST }
          : { status: 200, body: itemBody({ id: 'cr-1' }) }

    await listIntegrationCredentials(SYSTEM)
    await createIntegrationCredential(SYSTEM, {
      credentialType: 'API_KEY',
      secretReference: 'vault://kelir/erp/api-key',
    })
    await updateIntegrationCredential(SYSTEM, 'cr-1', { validTo: null })
    await expect(deleteIntegrationCredential(SYSTEM, 'cr-1')).resolves.toBeUndefined()

    expect(backend.requests.map((request) => [request.method, request.url])).toEqual([
      ['get', '/integration/external-systems/sys-1/credentials'],
      ['post', '/integration/external-systems/sys-1/credentials'],
      ['put', '/integration/external-systems/sys-1/credentials/cr-1'],
      ['delete', '/integration/external-systems/sys-1/credentials/cr-1'],
    ])
    // `null` clears a bound, so it must survive serialization rather than be dropped.
    expect(backend.requests[2].body).toEqual({ validTo: null })
  })

  it('carries a 422 detail path through to the caller', async () => {
    reply = () => ({
      status: 422,
      body: errorBody('VALIDATION_ERROR', 'Invalid', [
        {
          path: 'retryPolicy.maxRetries',
          rule: 'range',
          code: 'OUT_OF_RANGE',
          message: 'At most 20',
        },
      ]),
    })

    const failure = await registerExternalSystem({ systemCode: 'X', systemName: 'X' }).catch(
      (error: unknown) => error,
    )

    expect(failure).toBeInstanceOf(ApiError)
    expect((failure as ApiError).fieldErrors()).toEqual({ 'retryPolicy.maxRetries': 'At most 20' })
  })
})
