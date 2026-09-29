import { getPage, withoutBlanks } from './client'
import type { Page, SearchPageQuery } from '@/types/api'
import type { WorkflowDefinitionStatus, WorkflowDefinitionSummary } from '@/types/workflow'

/**
 * The workflow endpoints (`/api/v1/workflow/*`) this client reads.
 *
 * **Only the definition list, and only its summary.** What a screen needs from
 * a workflow is *which ones can a document type be bound to* — the definition
 * document itself is the designer's surface, which is FR-RAD-011 and has none.
 * Modelling it would be modelling something nothing reads, the rule
 * `types/rad.ts` states and `document-types.ts` used to.
 */
export function listWorkflowDefinitions(
  query: SearchPageQuery<WorkflowDefinitionStatus> = {},
): Promise<Page<WorkflowDefinitionSummary>> {
  // `search` matches the workflow key and the name (#525).
  return getPage<WorkflowDefinitionSummary>('/workflow/definitions', withoutBlanks(query))
}
