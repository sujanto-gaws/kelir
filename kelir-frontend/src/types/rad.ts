import type { JfssDefinition } from './jfss'

/**
 * What the RAD endpoints return.
 *
 * The wire shape is the backend's `modules/rad/domain`, serialized
 * `camelCase`. Only the parts the renderer reads are modelled — a type that
 * mirrors a whole module for the sake of completeness is a type that goes stale
 * where nothing looks.
 */

/** A form's place in its own lifecycle (`domain/form.rs`). */
export type FormStatus = 'DRAFT' | 'PUBLISHED' | 'DEPRECATED'

/**
 * A form definition and the row that holds it.
 *
 * `revision` and `jfssVersion` are different numbers and were conflated once
 * already: `revision` counts this form's own editions, `jfssVersion` is the
 * specification the `definition` conforms to.
 */
export interface Form {
  id: string
  formKey: string
  title: string
  revision: number
  jfssVersion: string
  status: FormStatus
  entityId: string | null
  definition: JfssDefinition
  publishedAt: string | null
  publishedBy: string | null
  createdAt: string
  updatedAt: string
}

/**
 * The master-data sources a lookup field may name (FR-RAD-007, #161).
 *
 * The same four the backend's `LookupSource` allows. A definition naming
 * anything else is refused at save, so this is a convenience for callers rather
 * than a check the renderer performs.
 */
export type LookupSource = 'supplier' | 'customer' | 'employee' | 'facility'

/** One choice a lookup offers (`domain/lookup.rs`). */
export interface LookupOption {
  /** The record's id — what a document stores to reference it. */
  value: string
  /** What a person calls the record. */
  label: string
  /** The business identifier, where the record carries one. */
  description: string | null
}

/** What a lookup may be asked for: paging and a search, and nothing else. */
export interface LookupQuery {
  page?: number
  pageSize?: number
  search?: string
}
