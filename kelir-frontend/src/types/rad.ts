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

/**
 * A filled-in form, as the **server** re-evaluated it (`domain/submission.rs`).
 *
 * `payload` is the backend's own answer and never what was posted: JFSS S8.1
 * makes it re-evaluate every `calculate` expression and overwrite the submitted
 * value, and S10.2 makes it discard the values of components that resolve to
 * hidden. It comes back so a caller can *see* that — a form that changes your
 * number without saying so is its own defect (#164 AC5).
 */
export interface FormSubmission {
  id: string
  formId: string
  /** The revision the form was filled in against. */
  formRevision: number
  /** The server's re-evaluated payload. */
  payload: Record<string, unknown>
  submittedAt: string
  submittedBy: string | null
  createdAt: string
  updatedAt: string
}

/**
 * A configured action (`domain/action.rs`, §5.10).
 *
 * **Every action here is one the caller may invoke.** The server filters by
 * `required_permission` and does not send that column back, so nothing on this
 * side re-decides it — a client that filtered again would be a second copy of a
 * rule that already has one answer.
 */
export interface RadAction {
  id: string
  actionKey: string
  label: string
  context: 'LIST' | 'DETAIL' | 'DOCUMENT' | 'TASK'
  actionType: 'NAVIGATE' | 'API_CALL' | 'WORKFLOW_ACTION' | 'PLUGIN'
  config: Record<string, unknown>
}

/** What a filter control is (`domain/list.rs`, §5.8's `CHECK`). */
export type ListFilterType = 'TEXT' | 'ENUM' | 'LOOKUP' | 'DATE_RANGE' | 'NUMBER_RANGE' | 'BOOLEAN'

/**
 * A column of a rendered list (`domain/render.rs`).
 *
 * `sortable` is not the definition's `isSortable`. It is that intent resolved
 * against what the query can order by, which is the server's answer and not a
 * question this side re-asks: a `form_data.*` column arrives `sortable: false`
 * however the definition marked it.
 */
export interface RenderableColumn {
  key: string
  label: string
  dataType: string | null
  format: string | null
  width: string | null
  sortable: boolean
}

/**
 * A filter control of a rendered list.
 *
 * `parameter` is what the rows request calls this filter; `key` is what the
 * definition calls it. They are usually the same and are not required to be,
 * which is why both are on the wire — the renderer sends `key` and the server
 * maps it.
 */
export interface RenderableFilter {
  key: string
  label: string
  filterType: ListFilterType
  options: unknown
  isDefault: boolean
  parameter: string
}

/**
 * Everything needed to draw one list, and nothing that needs a second request
 * to interpret.
 *
 * `defaultSortKey` and `defaultSortDescending` arrive resolved: the stored
 * `default_sort_json` is parsed on the server, so this side never reads that
 * shape and cannot disagree with the query about what the list opens on.
 */
export interface RenderableList {
  id: string
  listKey: string
  title: string
  pageSize: number
  columns: RenderableColumn[]
  filters: RenderableFilter[]
  defaultSortKey: string
  defaultSortDescending: boolean
}

/**
 * One row: the document's identity, and a cell per declared column.
 *
 * Keyed by the definition's own `columnKey`, so a renderer walks
 * `list.columns` and reads `row.cells[column.key]` — there is no second
 * mapping, and a column the definition drops cannot leave an orphan cell.
 */
export interface ListRow {
  id: string
  cells: Record<string, unknown>
}

/**
 * A form on a list screen: everything but the definition
 * (`domain::FormSummary`).
 *
 * The definition is the reason this type exists beside [`Form`] — a page of
 * twenty forms with their JFSS trees inlined is twenty documents on the wire to
 * render a table of titles.
 */
export interface FormSummary {
  id: string
  formKey: string
  title: string
  revision: number
  jfssVersion: string
  status: FormStatus
  entityId: string | null
  createdAt: string
  updatedAt: string
}

/** Where a menu entry came from (`domain::menu::MenuSource`, §5.9's `CHECK`). */
export type MenuSource = 'CORE' | 'CONFIG' | 'PLUGIN'

/**
 * One entry of the configured navigation.
 *
 * **`requiredPermission` hides; it does not guard.** §5.9's column comment is
 * *hide when the user lacks it*, and that is all it does — the screen the entry
 * points at enforces its own permission, through the router's `meta.permission`
 * and the endpoint behind it. An entry naming a permission nobody holds removes
 * a link and opens nothing.
 */
export interface MenuEntry {
  id: string
  menuKey: string
  label: string
  /** A Lucide icon name. An unknown one renders without an icon. */
  icon: string | null
  parentMenuId: string | null
  /** Relative to the application; the API refuses anything else. */
  routePath: string | null
  requiredPermission: string | null
  source: MenuSource
  sortOrder: number
  isEnabled: boolean
  createdAt: string
  updatedAt: string
}

export interface CreateMenuRequest {
  menuKey: string
  label: string
  icon?: string | null
  parentMenuId?: string | null
  routePath?: string | null
  requiredPermission?: string | null
  sortOrder?: number
  isEnabled?: boolean
}

/** Editing an entry. Absent means *leave alone*; `null` clears. */
export type UpdateMenuRequest = Partial<Omit<CreateMenuRequest, 'menuKey'>>

/**
 * Creating a form definition (`domain::form::CreateFormRequest`).
 *
 * `definition` is the whole JFSS document. The backend validates it against the
 * meta-schema, both rule registries and the lookup allow-list before it is
 * stored, and again at publish ([ADR-0035]) — so a client that pre-checks is
 * being helpful, never authoritative.
 */
export interface CreateFormRequest {
  formKey: string
  title: string
  entityId?: string | null
  definition: JfssDefinition
}

/**
 * Editing a draft revision, or seeding the next one.
 *
 * **`formKey` is absent because it may not change**: it is the identity a
 * document pins and what `document_types.form_id` is chosen by. Every field is
 * optional and absent means *leave alone*.
 */
export interface UpdateFormRequest {
  title?: string
  entityId?: string | null
  definition?: JfssDefinition
}

/** A list definition's status (`domain::list::ListStatus`). */
export type ListStatus = 'ACTIVE' | 'DEPRECATED'

/**
 * A list definition on a list-of-lists screen: without its columns and filters
 * (`domain::list::ListSummary`).
 *
 * Read by the document type builder to fill its list chooser — the same reason
 * [`FormSummary`] exists, and the endpoint returns the summary for it.
 */
export interface ListSummary {
  id: string
  listKey: string
  title: string
  entityId: string | null
  pageSize: number
  status: ListStatus
  createdAt: string
  updatedAt: string
}
