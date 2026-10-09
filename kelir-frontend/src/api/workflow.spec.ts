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
 * (`workflow/handlers.rs:55–64`), and no other (#426 AC1: no new route).
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

  it('has no deprecate call until #573’s route exists', () => {
    // #426 AC5, test-engineer campaign 2026-10-10. A client function for a
    // route that is not there is a 404 waiting for a caller. This flips when
    // #573 merges and `deprecateWorkflowDefinition` lands with it.
    expect(Object.keys(workflowApi).filter((name) => /deprecat|retire/i.test(name))).toEqual([])
  })
})
