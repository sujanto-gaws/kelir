/**
 * What the document-type list returns (`domain::DocumentTypeSummary`).
 *
 * A type's status decides whether a document may be created from it, which is
 * the only thing a Sprint 9 screen asks about one.
 */
export type DocumentTypeStatus = 'DRAFT' | 'ACTIVE' | 'DEPRECATED'

export interface DocumentTypeSummary {
  id: string
  typeCode: string
  name: string
  category: string | null
  /**
   * The published form revision this type binds, or `null`.
   *
   * A type with no form is legal — a type is configured before its form exists
   * as often as after — and a document created from one has nothing to render.
   * The chooser says so rather than offering it as though it were ready.
   */
  formId: string | null
  status: DocumentTypeStatus
  createdAt: string
  updatedAt: string
}
