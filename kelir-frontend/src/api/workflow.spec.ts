import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import * as workflowApi from './workflow'
import {
  createWorkflowDefinition,
  createWorkflowRevision,
  getWorkflowDefinition,
  publishWorkflowDefinition,
  updateWorkflowDefinition,
} from './workflow'
import { installFakeBackend, type FakeBackendHandle } from '@/lib/testing/fake-backend'
import type { JwssDefinition } from '@/types/workflow'

/**
 * The editor's calls land on the routes the backend already has
 * (`workflow/handlers.rs:55–64`), and no other (#426 AC1: no new route but the
 * deprecation D-101 B adds, #711).
 */

const DEFINITION = { workflowKey: 'w', name: 'W' } as JwssDefinition

describe('workflow definition client', () => {
  let backend: FakeBackendHandle

  beforeEach(() => {
    backend = installFakeBackend(() => ({ status: 200, body: { success: true, data: {} } }))
  })

  afterEach(() => backend.restore())

  it('reads, creates, edits, publishes and revises at the existing routes', async () => {
    await getWorkflowDefinition('d1')
    await createWorkflowDefinition({ workflowKey: 'w', name: 'W', definition: DEFINITION })
    await updateWorkflowDefinition('d1', { name: 'W2', definition: DEFINITION })
    await publishWorkflowDefinition('d1')
    await createWorkflowRevision('d1')

    expect(backend.requests.map((request) => `${request.method} ${request.url}`)).toEqual([
      'get /workflow/definitions/d1',
      'post /workflow/definitions',
      'put /workflow/definitions/d1',
      'post /workflow/definitions/d1/publication',
      'post /workflow/definitions/d1/revisions',
    ])
    expect(backend.requests[1].body).toEqual({
      workflowKey: 'w',
      name: 'W',
      definition: DEFINITION,
    })
    // A revision seeded from its source sends nothing to change.
    expect(backend.requests[4].body).toEqual({})
  })

  it('deprecates at #711’s route, and at no other', async () => {
    // #426 AC5, #713. Flipped 2026-10-10: this asserted that no deprecate call
    // existed until #573’s route did. The route merged with #711, and the call
    // lands with the editor’s action.
    await workflowApi.deprecateWorkflowDefinition('d1')

    expect(Object.keys(workflowApi).filter((name) => /deprecat|retire/i.test(name))).toEqual([
      'deprecateWorkflowDefinition',
    ])
    expect(backend.requests.map((request) => `${request.method} ${request.url}`)).toEqual([
      'post /workflow/definitions/d1/deprecation',
    ])
  })
})
