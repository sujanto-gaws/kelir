import type { SearchPageQuery } from './api'

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

/**
 * What the document-type list narrows on (`domain::DocumentTypeQuery`).
 *
 * `workflowDefinitionId` keeps the types with a **live** binding to that one
 * revision: not deleted and `ACTIVE`, whatever its validity window (#713). An id
 * that names nothing is an empty page. It is what the workflow editor's
 * *Deprecate* warning lists.
 */
export interface DocumentTypeListQuery extends SearchPageQuery<DocumentTypeStatus> {
  workflowDefinitionId?: string
}

/**
 * How far a document type's own configuration goes (`domain::DocumentType`).
 *
 * **Wider than [`DocumentTypeSummary`], and the two are kept apart.** A picker
 * needs a name and a status; the builder needs the bindings, the security level
 * and the workflows, which is a whole type on the wire and the reason the list
 * endpoint does not return them.
 */
export type SecurityLevel = 'PUBLIC' | 'INTERNAL' | 'CONFIDENTIAL' | 'RESTRICTED'

/**
 * One workflow a type routes to, and when (`domain::WorkflowBinding`), as the
 * server returns it.
 *
 * **These five fields and no others.** The endpoint denies an unknown field, so
 * a binding sent back must carry exactly these — which is why there is no
 * `condition` here: the wire calls it `conditionExpression`, and it is a string.
 */
export interface WorkflowBinding {
  workflowDefinitionId: string
  /** Whether this binding applies; `null` makes it the default binding. */
  conditionExpression: string | null
  /**
   * Which binding wins where two match. Lower runs first, as everywhere else in
   * this product.
   */
  priority: number
  /** The first day this binding applies (`YYYY-MM-DD`), or `null` for always. */
  validFrom: string | null
  /** The last day this binding applies (`YYYY-MM-DD`), or `null` for always. */
  validTo: string | null
}

/**
 * A binding as a write sends it. Only the revision is required: the server
 * defaults the rest to priority 1, no condition and no window.
 */
export type WorkflowBindingRequest = Pick<WorkflowBinding, 'workflowDefinitionId'> &
  Partial<Omit<WorkflowBinding, 'workflowDefinitionId'>>

export interface DocumentType {
  id: string
  typeCode: string
  name: string
  description: string | null
  category: string | null
  formId: string | null
  listId: string | null
  defaultSecurityLevel: SecurityLevel
  retentionPolicyId: string | null
  targetEntityType: string | null
  status: DocumentTypeStatus
  workflows: WorkflowBinding[]
  createdAt: string
  updatedAt: string
}

export interface CreateDocumentTypeRequest {
  typeCode: string
  name: string
  description?: string | null
  category?: string | null
  formId?: string | null
  listId?: string | null
  defaultSecurityLevel?: SecurityLevel
  retentionPolicyId?: string | null
  targetEntityType?: GovernedEntityType | null
  status?: DocumentTypeStatus
  /**
   * Replaces the stored set whole when present, `[]` included; absent leaves it alone.
   * With `expectedWorkflows`, the replacement is refused (409) if the stored set has
   * changed since it was read (#728).
   */
  workflows?: WorkflowBindingRequest[]
}

/**
 * The master-data record a type governs
 * (`master_data::domain::governance::GovernedEntity`).
 *
 * **The column is free text with no `CHECK`**, and the backend refuses a value
 * it does not know rather than guessing one — a type carrying an unknown value
 * governs nothing. The chooser offers only the two the backend implements, so
 * the screen cannot author the third state.
 */
export type GovernedEntityType = 'PARTY' | 'FACILITY'

/**
 * Editing a type. Every field is optional and absent means *leave alone*, which
 * is the backend's own convention — so a partial edit does not have to resend
 * what it is not changing.
 *
 * `typeCode` is absent because it may not change: a delegation scopes itself to
 * it and an integration names a type by it.
 */
export interface UpdateDocumentTypeRequest extends Partial<
  Omit<CreateDocumentTypeRequest, 'typeCode'>
> {
  /**
   * The bindings the caller read, sent beside `workflows` (#728, D-117). The
   * server compares them with the stored set, as a set on all five fields,
   * before it writes: a difference is a 409
   * {@link DOCUMENT_TYPE_WORKFLOWS_CHANGED} and nothing is written. Sent
   * without `workflows` it is a 422. Absent, a `workflows` replaces whatever
   * is stored, as before.
   */
  expectedWorkflows?: WorkflowBindingRequest[]
}

/**
 * The code an edit is refused with when the type's stored bindings are not
 * the `expectedWorkflows` it sent (#728, D-117): another administrator changed
 * them since they were read. This code, not the 409, is what says a reload
 * will help.
 */
export const DOCUMENT_TYPE_WORKFLOWS_CHANGED = 'DOCUMENT_TYPE_WORKFLOWS_CHANGED'

/** Where a numbering sequence restarts (`numbering::SequenceScope`). */
export type SequenceScope = 'GLOBAL' | 'YEAR' | 'MONTH' | 'DEPARTMENT_YEAR'

/** What happens to a number whose submission failed (`numbering::GapPolicy`). */
export type GapPolicy = 'GAPLESS' | 'ALLOW_GAPS'

export interface NumberingRule {
  id: string
  documentTypeId: string
  ruleTemplate: string
  sequenceScope: SequenceScope
  sequencePadding: number
  gapPolicy: GapPolicy
  /** The furthest bucket this type's sequence has reached. */
  sequenceKey: string
  /** The number the next document takes in that bucket. */
  nextSequence: number
  isActive: boolean
}

/**
 * Setting a rule. There is no create-versus-update on the wire: a type has a
 * numbering rule or it does not, which is why the endpoint is a `PUT` on a
 * sub-resource rather than a `POST` that would conflict the second time.
 */
export interface SetNumberingRuleRequest {
  ruleTemplate: string
  sequenceScope: SequenceScope
  sequencePadding?: number
  gapPolicy?: GapPolicy
  nextSequence?: number
}
