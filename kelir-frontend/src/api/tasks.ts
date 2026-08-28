import { getItem, getPage, postItem } from './client'
import type { ListFetchQuery } from '@/composables/useQueryBackedList'
import type { Page } from '@/types/api'
import type {
  DecisionAction,
  DecisionResult,
  DocumentWorkflow,
  InboxTask,
  TaskDetail,
  WorkflowTask,
} from '@/types/workflow'

/**
 * The task inbox and the workflow surface behind it (`/api/v1/tasks/*`,
 * `/api/v1/workflow/*`).
 *
 * Thin by design, like `documents.ts`: one call each through the shared client,
 * so envelope unwrapping and error normalisation happen in exactly one place
 * (coding standard §3.3).
 *
 * **The server decides what the caller may see.** The inbox's visibility rule —
 * *mine, or offered to a role I hold* — is a predicate in the backend's own
 * query, and nothing here narrows a wider result. A client-side filter would be
 * a second rule, and the two would drift; on this surface that would mean
 * showing somebody a task the API will then refuse them, which reads as a broken
 * product while being a leak.
 */

/** One page of the caller's own tasks. */
export function listTasks(query: ListFetchQuery): Promise<Page<InboxTask>> {
  const params: Record<string, string | number> = {}

  for (const [key, value] of Object.entries(query)) {
    if (value !== undefined && value !== null && value !== '') {
      params[key] = value as string | number
    }
  }

  return getPage<InboxTask>('/tasks', params)
}

/**
 * One task, with the document it is about and the decision being asked.
 *
 * A task another user holds answers **404**, not 403: the visibility rule is
 * what the read is filtered by, and a 403 would confirm the task exists.
 */
export function getTask(id: string): Promise<TaskDetail> {
  return getItem<TaskDetail>(`/tasks/${id}`)
}

/**
 * Takes an unclaimed role task.
 *
 * A compare-and-swap on the server: two people claiming at once produce one
 * winner and one 409, so the loser is told rather than quietly sharing an
 * approval.
 */
export function claimTask(id: string): Promise<WorkflowTask> {
  return postItem<WorkflowTask>(`/workflow/tasks/${id}/claim`, {})
}

/**
 * Records a decision, which moves the process and the document's status with it.
 *
 * **There is no comment.** FR-TASK-006 is Sprint 11's #182; the columns exist
 * and nothing writes them, which means a rejection recorded by this release has
 * no reason on it. Said here as well as in the backend, because this is where a
 * screen would otherwise be tempted to invent a field the API does not accept.
 */
export function decideTask(id: string, action: DecisionAction): Promise<DecisionResult> {
  return postItem<DecisionResult>(`/workflow/tasks/${id}/decision`, { action })
}

/**
 * The process deciding a document, if one is.
 *
 * **404 when nothing is deciding it**, which is a true statement rather than an
 * error: a document of a type that binds no workflow has no process, and that is
 * a valid configuration. The Workflow tab reads the absence as "no approval",
 * not as a failure.
 */
export function getDocumentWorkflow(documentId: string): Promise<DocumentWorkflow> {
  return getItem<DocumentWorkflow>(`/documents/${documentId}/workflow`)
}
