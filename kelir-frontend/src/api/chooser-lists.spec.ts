import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import { listDocumentTypes } from './document-types'
import { listRoles, listUsers } from './identity'
import { listForms, listLists } from './rad'
import { listWorkflowDefinitions } from './workflow'
import { installFakeBackend, type FakeBackendHandle } from '@/lib/testing/fake-backend'

/**
 * The list reads a chooser searches (#525).
 *
 * What is asserted is the wire: each sends `search` and `status` as query
 * parameters beside the paging, and a blank one is left off rather than sent as
 * `?search=` — an empty search box means "no filter", and `?status=` is not a
 * status.
 */

const LIST = { success: true, data: [], meta: { page: 1, pageSize: 100, total: 0 } }

describe('chooser list reads', () => {
  let backend: FakeBackendHandle

  beforeEach(() => {
    backend = installFakeBackend(() => ({ status: 200, body: LIST }))
  })

  afterEach(() => backend.restore())

  function sent(): { url: string; params: Record<string, unknown> } {
    expect(backend.requests).toHaveLength(1)

    const [request] = backend.requests

    return { url: request.url, params: request.params }
  }

  it('sends the search and the status of a document type read', async () => {
    await listDocumentTypes({ search: 'req', status: 'ACTIVE', pageSize: 100 })

    expect(sent()).toEqual({
      url: '/document-types',
      params: { search: 'req', status: 'ACTIVE', pageSize: 100 },
    })
  })

  it('sends the bound revision of a document type read, and leaves a blank one off', async () => {
    // #713: the workflow editor's Deprecate warning lists the types bound to a
    // revision through this filter, one call rather than one read per type.
    await listDocumentTypes({ workflowDefinitionId: 'wf-1', pageSize: 100 })

    expect(sent()).toEqual({
      url: '/document-types',
      params: { workflowDefinitionId: 'wf-1', pageSize: 100 },
    })

    backend.requests.length = 0
    await listDocumentTypes({ workflowDefinitionId: '' })

    expect(sent().params).toEqual({})
  })

  it('sends the search and the status of a form read', async () => {
    await listForms({ search: 'po', status: 'PUBLISHED' })

    expect(sent()).toEqual({ url: '/rad/forms', params: { search: 'po', status: 'PUBLISHED' } })
  })

  it('sends the search and the status of a list read', async () => {
    await listLists({ search: 'po', status: 'ACTIVE' })

    expect(sent()).toEqual({ url: '/rad/lists', params: { search: 'po', status: 'ACTIVE' } })
  })

  it('sends the search and the status of a workflow definition read', async () => {
    await listWorkflowDefinitions({ search: 'approval', status: 'ACTIVE' })

    expect(sent()).toEqual({
      url: '/workflow/definitions',
      params: { search: 'approval', status: 'ACTIVE' },
    })
  })

  it('sends the search and the status of a user read', async () => {
    await listUsers({ search: 'budi', status: 'ACTIVE', page: 1 })

    expect(sent()).toEqual({
      url: '/identity/users',
      params: { search: 'budi', status: 'ACTIVE', page: 1 },
    })
  })

  it('sends the search of a role read', async () => {
    await listRoles({ search: 'clerk' })

    expect(sent()).toEqual({ url: '/identity/roles', params: { search: 'clerk' } })
  })

  it('leaves a blank search and status off the wire', async () => {
    await listDocumentTypes({ search: '', status: undefined, page: 2, pageSize: 20 })

    expect(sent().params).toEqual({ page: 2, pageSize: 20 })
  })
})
