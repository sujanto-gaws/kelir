import { getItem, getPage, postItem } from './client'
import type { ListFetchQuery } from '@/composables/useQueryBackedList'
import type { Page } from '@/types/api'
import type {
  DecisionAction,
  DecisionResult,
  DocumentWorkflow,
  InboxTask,
  TaskDetail,
  WorkflowHistoryEntry,
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
 * Records a decision and the reason for it (FR-TASK-006, #182).
 *
 * **The comment is omitted from the body when there is none**, rather than sent
 * as `null` or `""`. The server treats blank as absent anyway — a box somebody
 * tabbed past is not a reason — and sending nothing is what keeps an approval on
 * an unmarked edge a one-field request, which is what it is.
 *
 * **Whether a reason is required is not decided here.** The definition marks the
 * transition (JWSS §4.1) and `AvailableDecision.requiresComment` carries the
 * answer to the screen; the server checks again against the edge it actually
 * fires. A rule invented in this file would be a second one.
 */
export function decideTask(
  id: string,
  action: DecisionAction,
  comment?: string,
): Promise<DecisionResult> {
  const trimmed = comment?.trim()

  return postItem<DecisionResult>(`/workflow/tasks/${id}/decision`, {
    action,
    ...(trimmed ? { comment: trimmed } : {}),
  })
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

/**
 * How the document got here: one entry per transition, oldest first (#181).
 *
 * **Paginated at the API and read one page at a time here**, because a
 * long-running process is exactly where an unpaginated list stops working —
 * which is the reason the endpoint pages rather than an accident of it.
 *
 * Behind `workflow:instance:read`, deliberately not the audit permission: this
 * is the document's own account, shown to the approver deciding it.
 */
export function listWorkflowHistory(
  documentId: string,
  page = 1,
  pageSize = 20,
): Promise<Page<WorkflowHistoryEntry>> {
  return getPage<WorkflowHistoryEntry>(`/documents/${documentId}/workflow/history`, {
    page,
    pageSize,
  })
}
