import { deleteItem, getItem, getPage, postItem, putItem } from './client'
import type { Page, PageQuery } from '@/types/api'
import type {
  CreateDocumentTypeRequest,
  DocumentType,
  DocumentTypeSummary,
  NumberingRule,
  SetNumberingRuleRequest,
  UpdateDocumentTypeRequest,
} from '@/types/document-type'

/**
 * The document-type endpoints (`/api/v1/document-types/*`).
 *
 * **The whole surface, since #341.** Until then only the list was here, and its
 * own comment said why: *the bindings and the numbering rule are an
 * administrator's surface and have no screen yet, so modelling them would be
 * modelling something nothing reads*. That was true and it was the gap **D-64**
 * accepted SRS §9 criterion 4 against — an administrator configured a document
 * type over the API because there was no screen. There is one now.
 */
export function listDocumentTypes(query: PageQuery = {}): Promise<Page<DocumentTypeSummary>> {
  return getPage<DocumentTypeSummary>('/document-types', query)
}

/** One type, with its bindings and workflows. */
export function getDocumentType(id: string): Promise<DocumentType> {
  return getItem<DocumentType>(`/document-types/${id}`)
}

export function createDocumentType(request: CreateDocumentTypeRequest): Promise<DocumentType> {
  return postItem<DocumentType>('/document-types', request)
}

/**
 * Edits a type. Only the fields the caller sends move; the rest are left alone,
 * which is what lets the dialog send an edit rather than a whole replacement.
 */
export function updateDocumentType(
  id: string,
  request: UpdateDocumentTypeRequest,
): Promise<DocumentType> {
  return putItem<DocumentType>(`/document-types/${id}`, request)
}

export function deleteDocumentType(id: string): Promise<void> {
  return deleteItem(`/document-types/${id}`)
}

/**
 * The type's numbering rule, or a 404 where it has none.
 *
 * **A type without one is ordinary**, not an error: a rule is configured when
 * somebody decides how the type's documents are numbered, which is often after
 * the type exists. The caller distinguishes the 404 from a failure.
 */
export function getNumberingRule(id: string): Promise<NumberingRule> {
  return getItem<NumberingRule>(`/document-types/${id}/numbering-rule`)
}

/**
 * Sets the rule. A `PUT` because a type has one rule or none — the backend's
 * own note says a `POST` would conflict the second time.
 */
export function setNumberingRule(
  id: string,
  request: SetNumberingRuleRequest,
): Promise<NumberingRule> {
  return putItem<NumberingRule>(`/document-types/${id}/numbering-rule`, request)
}

export function clearNumberingRule(id: string): Promise<void> {
  return deleteItem(`/document-types/${id}/numbering-rule`)
}
