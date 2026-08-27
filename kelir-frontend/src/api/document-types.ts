import { getPage } from './client'
import type { Page, PageQuery } from '@/types/api'
import type { DocumentTypeSummary } from '@/types/document-type'

/**
 * The document-type endpoints (`/api/v1/document-types/*`).
 *
 * Only the list is here, and only its summary shape. What a screen needs from a
 * type in Sprint 9 is *which types can a document be created from* — the
 * bindings and the numbering rule are an administrator's surface and have no
 * screen yet, so modelling them would be modelling something nothing reads
 * (`types/rad.ts` states the same rule).
 */
export function listDocumentTypes(query: PageQuery = {}): Promise<Page<DocumentTypeSummary>> {
  return getPage<DocumentTypeSummary>('/document-types', query)
}
