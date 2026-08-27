import { deleteItem, getItem, getPage, postItem, putItem } from './client'
import type { ListFetchQuery } from '@/composables/useQueryBackedList'
import type { Page } from '@/types/api'
import type {
  CreateDocumentRequest,
  Document,
  DocumentStatus,
  DocumentSummary,
  ResolvedEntity,
  StatusHistoryEntry,
  TransitionResult,
  UpdateDocumentRequest,
} from '@/types/document'

/**
 * The document endpoints (`/api/v1/documents/*`).
 *
 * Thin by design, like `rad.ts` and `master-data.ts`: one call each through the
 * shared client, so envelope unwrapping and error normalisation happen in
 * exactly one place (coding standard §3.3).
 *
 * **The server paginates, searches and filters.** Every parameter below goes on
 * the wire; nothing fetches a population and narrows it here, which is the
 * failure FR-DOC-013 and NFR-PERF-002 exist to prevent — and on this surface it
 * would be worse than slow, because the list's visibility rule is enforced in
 * the query and a client-side narrowing would be a second rule.
 */

/**
 * One page of documents, searched and filtered.
 *
 * Blank values are dropped rather than sent, as `master-data.ts` does it:
 * `?search=` means *match everything* to the backend and is harmless, but
 * `?status=` is not in the vocabulary and is a 422 — so an empty select is an
 * absent parameter, which is what "no filter" means.
 */
export function listDocuments(query: ListFetchQuery): Promise<Page<DocumentSummary>> {
  const params: Record<string, string | number> = {}

  for (const [key, value] of Object.entries(query)) {
    if (value !== undefined && value !== null && value !== '') {
      params[key] = value as string | number
    }
  }

  return getPage<DocumentSummary>('/documents', params)
}

/** One document, with its form data, its metadata and its link identifiers. */
export function getDocument(id: string): Promise<Document> {
  return getItem<Document>(`/documents/${id}`)
}

export function createDocument(request: CreateDocumentRequest): Promise<Document> {
  return postItem<Document>('/documents', request)
}

/**
 * Edits a draft.
 *
 * **The backend re-evaluates the form data on this write too**, not only on the
 * submit (JFSS S8.1), so what comes back is the server's answer and the caller
 * shows *that* rather than what it sent. A screen that kept its own copy would
 * be showing a number the stored document does not hold.
 */
export function updateDocument(id: string, request: UpdateDocumentRequest): Promise<Document> {
  return putItem<Document>(`/documents/${id}`, request)
}

/** Discards a draft. A submitted document is cancelled through `transition`. */
export function deleteDocument(id: string): Promise<void> {
  return deleteItem(`/documents/${id}`)
}

/**
 * Submits a draft, which assigns its number.
 *
 * A verb sub-resource and not a status change: the backend takes the number, the
 * status and the re-evaluated payload in one transaction, and there is no route
 * that would do half of it.
 */
export function submitDocument(id: string): Promise<Document> {
  return postItem<Document>(`/documents/${id}/submission`)
}

/**
 * Moves a document to another status.
 *
 * Answers with **both ends**. A caller that sent `SUBMITTED -> APPROVED`
 * already knows the target; what it cannot know without being told is what the
 * document was when the transition ran, which is what makes a concurrent change
 * visible on the screen.
 */
export function transitionDocument(
  id: string,
  status: DocumentStatus,
  reason?: string,
): Promise<TransitionResult> {
  return putItem<TransitionResult>(`/documents/${id}/status`, { status, reason })
}

/** How the document got where it is, oldest first. */
export function getStatusHistory(id: string): Promise<StatusHistoryEntry[]> {
  return getItem<StatusHistoryEntry[]>(`/documents/${id}/status-history`)
}

/**
 * Resolves the master-data record a document is linked to.
 *
 * **A separate call, and that is the design.** Reading a document hands back
 * `entityType` and `entityId` and nothing about the record they name; this
 * endpoint requires the entity's own read permission, by calling the
 * master-data service rather than checking a string of its own. A caller who may
 * read documents and not suppliers gets the document and a 403 here — which is
 * what #161 decided for the same question, so the workspace shows the identifier
 * and says the name could not be loaded rather than pretending there is none.
 */
export function getLinkedEntity(id: string): Promise<ResolvedEntity> {
  return getItem<ResolvedEntity>(`/documents/${id}/linked-entity`)
}
