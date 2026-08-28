/**
 * What the document endpoints return (`/api/v1/documents/*`).
 *
 * The wire shape is the backend's `modules/document/domain`, serialized
 * `camelCase`. Only the parts a screen reads are modelled — a type mirroring a
 * whole module for the sake of completeness is a type that goes stale where
 * nothing looks, which is what `types/rad.ts` says for the same reason.
 */

/**
 * Where a document is in its own life (`domain/status.rs`).
 *
 * **Not `recordStatus`.** That is the governance lifecycle of a *master-data
 * record* — a supplier is drafted, approved, suspended — and it is a property
 * of a thing that keeps existing. A document is an event: it happens once, ends,
 * and is then history. The two never appear on the same row.
 */
export type DocumentStatus =
  | 'DRAFT'
  | 'SUBMITTED'
  | 'IN_REVIEW'
  | 'PENDING_APPROVAL'
  | 'APPROVED'
  | 'REJECTED'
  | 'RETURNED'
  | 'COMPLETED'
  | 'ARCHIVED'
  | 'CANCELLED'

export type DocumentPriority = 'LOW' | 'NORMAL' | 'HIGH' | 'URGENT'

/** A kind of master-data record a document may concern (`domain/link.rs`). */
export type EntityType = 'PARTY' | 'FACILITY'

export type MetadataType = 'STRING' | 'NUMBER' | 'BOOLEAN' | 'DATE'

export interface MetadataEntry {
  value: string
  dataType: MetadataType
}

/**
 * A document, whole.
 *
 * `entityType` and `entityId` are the link, **unresolved**. Reading a document
 * hands back the identifiers and nothing about the record they name; resolving
 * them goes through `GET /documents/{id}/linked-entity`, which requires the
 * entity's own read permission by calling the master-data service rather than
 * checking a string of its own. That is #161's answer to the same question, and
 * it is why this interface carries no supplier name.
 */
export interface Document {
  id: string
  /** The internal handle, `DOC-2026-000123`. What a draft is called before it has a number. */
  documentRef: string
  /** The business number. `null` until a submit assigns it. */
  documentNumber: string | null
  documentTypeId: string
  /** The exact form revision this document was created against, pinned at creation. */
  formId: string | null
  title: string
  status: DocumentStatus
  priority: DocumentPriority
  /** The server's answer, never the browser's (JFSS S8.1). */
  formData: Record<string, unknown>
  metadata: Record<string, MetadataEntry>
  entityType: EntityType | null
  entityId: string | null
  requestedForDepartmentId: string | null
  requestedForFacilityId: string | null
  requestedBy: string | null
  submittedAt: string | null
  createdBy: string | null
  createdAt: string
  updatedAt: string
}

/**
 * A document on a list screen: everything but the form data and the metadata.
 *
 * The payload is the reason this type exists. A page of twenty documents with
 * their form data inlined is twenty forms' worth of data on the wire to render
 * a table of titles and statuses.
 */
export interface DocumentSummary {
  id: string
  documentRef: string
  documentNumber: string | null
  documentTypeId: string
  /** Denormalized by the list's own join, so twenty rows are not twenty lookups. */
  documentTypeCode: string
  title: string
  status: DocumentStatus
  priority: DocumentPriority
  entityType: EntityType | null
  entityId: string | null
  submittedAt: string | null
  createdAt: string
  updatedAt: string
}

/** The linked record, resolved through its own permission. */
export interface ResolvedEntity {
  entityType: EntityType
  entityId: string
  code: string
  name: string
}

/** One row of a document's status history. */
export interface StatusHistoryEntry {
  /** `null` on the row that records the document's creation. */
  previousStatus: DocumentStatus | null
  status: DocumentStatus
  changedBy: string | null
  reason: string | null
  changedAt: string
}

/** What a transition answers with: both ends, so a concurrent change is visible. */
export interface TransitionResult {
  previousStatus: DocumentStatus
  status: DocumentStatus
}

export interface CreateDocumentRequest {
  documentTypeId: string
  title: string
  formData?: Record<string, unknown>
  metadata?: Record<string, MetadataEntry>
  priority?: DocumentPriority
  entityType?: EntityType
  entityId?: string
}

/**
 * Editing a draft.
 *
 * There is no `status` member and no `documentNumber` member, and the backend
 * refuses one: a transition is `PUT /status`'s and a number is the submit's.
 * Adding either here would not make it work — it would make every edit a 422.
 */
export interface UpdateDocumentRequest {
  title?: string
  formData?: Record<string, unknown>
  metadata?: Record<string, MetadataEntry>
  priority?: DocumentPriority
  entityType?: EntityType | null
  entityId?: string | null
}

/**
 * What a person calls each status.
 *
 * Here rather than in each component, for the reason `PARTY_STATUS_LABELS`
 * exists: a status spelled two ways on two screens is one screen that is wrong.
 */
export const DOCUMENT_STATUS_LABELS: Record<DocumentStatus, string> = {
  DRAFT: 'Draft',
  SUBMITTED: 'Submitted',
  IN_REVIEW: 'In review',
  PENDING_APPROVAL: 'Pending approval',
  APPROVED: 'Approved',
  REJECTED: 'Rejected',
  RETURNED: 'Returned',
  COMPLETED: 'Completed',
  ARCHIVED: 'Archived',
  CANCELLED: 'Cancelled',
}

export const DOCUMENT_PRIORITY_LABELS: Record<DocumentPriority, string> = {
  LOW: 'Low',
  NORMAL: 'Normal',
  HIGH: 'High',
  URGENT: 'Urgent',
}

export const ENTITY_TYPE_LABELS: Record<EntityType, string> = {
  PARTY: 'Party',
  FACILITY: 'Facility',
}

/**
 * Where a document in this status may go, as the backend's legality table says.
 *
 * **A copy of a state machine, and the honest name for that is a risk.** The
 * authority is `domain/status.rs`; this exists so the workspace can offer the
 * moves that will be accepted rather than every value in the enum, and a
 * screen offering a button that always 422s is worse than one offering none.
 *
 * It is kept safe by being *advisory*: the backend refuses anything this gets
 * wrong, so a copy that drifts costs a missing button or a refused click, never
 * an illegal transition. `documents.spec.ts` asserts the two agree on the paths
 * the workspace actually offers.
 *
 * `DRAFT` is empty on purpose — a draft is submitted through
 * `POST /{id}/submission`, which the workspace offers as its own button.
 *
 * **`PENDING_APPROVAL` is empty for a different reason, and Sprint 10 made it
 * the load-bearing one.** A document in that state is being decided by a
 * workflow, and the backend refuses a manual transition on any document with a
 * live process — the synchronization is one-way (**D-36**). The way on is the
 * task, not this table. That refusal is what the product enforces; this entry
 * is what stops the workspace offering a button for it in the first place.
 */
export const ALLOWED_TRANSITIONS: Record<DocumentStatus, DocumentStatus[]> = {
  DRAFT: [],
  SUBMITTED: ['IN_REVIEW', 'APPROVED', 'REJECTED', 'RETURNED', 'CANCELLED'],
  IN_REVIEW: ['APPROVED', 'REJECTED', 'RETURNED', 'CANCELLED'],
  RETURNED: ['SUBMITTED', 'CANCELLED'],
  APPROVED: ['COMPLETED', 'CANCELLED'],
  REJECTED: ['CANCELLED'],
  PENDING_APPROVAL: [],
  COMPLETED: [],
  ARCHIVED: [],
  CANCELLED: [],
}
