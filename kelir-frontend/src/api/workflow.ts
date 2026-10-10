import { getItem, getPage, postItem, putItem, withoutBlanks } from './client'
import type { Page, SearchPageQuery } from '@/types/api'
import type {
  CreateWorkflowRequest,
  UpdateWorkflowRequest,
  WorkflowDefinition,
  WorkflowDefinitionStatus,
  WorkflowDefinitionSummary,
} from '@/types/workflow'

/**
 * The workflow definition endpoints (`/api/v1/workflow/definitions*`).
 *
 * The list feeds the document type dialog's chooser and the workflow list at
 * `/admin/workflows`; the rest is the editor's (FR-WF-018, #426), against the
 * routes the backend has had since Sprint 10 (`workflow/handlers.rs:55–64`).
 *
 * **Deprecating a revision is here since 2026-10-10** (corrected in place: this
 * said it was *not here yet*, waiting for #573's route). The route merged with
 * #711 (**D-101** B, under `workflow:definition:deprecate` by **D-108**), and
 * {@link deprecateWorkflowDefinition} is the editor's call to it (#713, plan 19
 * row 6b).
 */
export function listWorkflowDefinitions(
  query: SearchPageQuery<WorkflowDefinitionStatus> = {},
): Promise<Page<WorkflowDefinitionSummary>> {
  // `search` matches the workflow key and the name (#525).
  return getPage<WorkflowDefinitionSummary>('/workflow/definitions', withoutBlanks(query))
}

/** One revision with its JWSS document (`workflow:definition:read`). */
export function getWorkflowDefinition(id: string): Promise<WorkflowDefinition> {
  return getItem<WorkflowDefinition>(`/workflow/definitions/${id}`)
}

/**
 * Creates a workflow as revision 1, in `DRAFT` (`workflow:definition:create`).
 *
 * **The whole definition is validated at save** (ADR-0019): the meta-schema,
 * the conditional operator tier and JWSS §8's structural rules. A 422 carries a
 * detail per problem, addressed by its position in this body
 * (`definition.transitions.2.requiresComment`), which is what the editor shows
 * at the state or transition it names.
 */
export function createWorkflowDefinition(
  request: CreateWorkflowRequest,
): Promise<WorkflowDefinition> {
  return postItem<WorkflowDefinition>('/workflow/definitions', request)
}

/**
 * Edits a **draft** revision in place (`workflow:definition:update`).
 *
 * A published revision refuses this as `NOT_A_DRAFT`: a running approval
 * executes the revision it started against, so editing one would change the
 * rules mid-approval. {@link createWorkflowRevision} is the way forward.
 */
export function updateWorkflowDefinition(
  id: string,
  request: UpdateWorkflowRequest,
): Promise<WorkflowDefinition> {
  return putItem<WorkflowDefinition>(`/workflow/definitions/${id}`, request)
}

/**
 * Publishes a draft, making it `ACTIVE` (`workflow:definition:publish`).
 *
 * **What is published is what is stored**, re-validated, and every role it
 * names is checked live in the tenant (**D-111**): a dead one is refused as
 * `ROLE_NOT_LIVE` at its path. Unsaved edits on the screen are not part of it.
 */
export function publishWorkflowDefinition(id: string): Promise<WorkflowDefinition> {
  return postItem<WorkflowDefinition>(`/workflow/definitions/${id}/publication`, {})
}

/**
 * Opens the next revision as a `DRAFT`, seeded from this one
 * (`workflow:definition:create`, since it makes a new row).
 */
export function createWorkflowRevision(
  id: string,
  request: UpdateWorkflowRequest = {},
): Promise<WorkflowDefinition> {
  return postItem<WorkflowDefinition>(`/workflow/definitions/${id}/revisions`, request)
}

/**
 * Deprecates an `ACTIVE` revision (`workflow:definition:deprecate`, **D-101** B,
 * **D-108**). New documents stop routing to it; approvals already running on it
 * carry on, since an instance pins its revision.
 *
 * **A document type still bound to it stays bound**, and that type's next
 * submission is refused with `WORKFLOW_NOT_PUBLISHED` until it is rebound. The
 * route does not warn, so the screen does before it calls this.
 *
 * A 409 means the revision is not `ACTIVE`: a draft, or already deprecated.
 * Nothing writes `ACTIVE` back.
 */
export function deprecateWorkflowDefinition(id: string): Promise<WorkflowDefinition> {
  return postItem<WorkflowDefinition>(`/workflow/definitions/${id}/deprecation`, {})
}
