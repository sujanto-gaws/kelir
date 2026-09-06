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
 * How far a document type's own configuration goes (`domain::DocumentType`).
 *
 * **Wider than [`DocumentTypeSummary`], and the two are kept apart.** A picker
 * needs a name and a status; the builder needs the bindings, the security level
 * and the workflows, which is a whole type on the wire and the reason the list
 * endpoint does not return them.
 */
export type SecurityLevel = 'PUBLIC' | 'INTERNAL' | 'CONFIDENTIAL' | 'RESTRICTED'

/** One workflow a type routes to, and when (`domain::WorkflowBinding`). */
export interface WorkflowBinding {
  workflowDefinitionId: string
  /**
   * Which binding wins where two match. Lower runs first, as everywhere else in
   * this product.
   */
  priority?: number | null
  /** A JSON Logic expression deciding whether this binding applies. */
  condition?: unknown
}

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
  workflows?: WorkflowBinding[]
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
export type UpdateDocumentTypeRequest = Partial<Omit<CreateDocumentTypeRequest, 'typeCode'>>

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
