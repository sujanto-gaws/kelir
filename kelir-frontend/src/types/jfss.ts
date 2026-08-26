/**
 * JFSS v2.0.1, as TypeScript.
 *
 * Transcribed from [`jfss-meta-v2.0.1.json`](../../../docs/schema/jfss-meta-v2.0.1.json),
 * which is normative — "where this document and the Meta-Schema disagree, the
 * Meta-Schema is normative" ([JFSS](../../../docs/schema/JSON%20Form%20Schema.md)
 * §1.3). Where the two differ, this file follows the meta-schema and says so.
 *
 * **These types describe a definition that has already been stored.** The
 * backend refuses a non-conforming document at save time (`modules/rad/domain/jfss.rs`),
 * so nothing here re-checks a shape — a renderer that validated would be a
 * second, weaker validator disagreeing with the first (#162 AC2).
 *
 * **`type` is deliberately `string` and not a union.** JFSS §4.4 says it in as
 * many words: *"`type` is an open vocabulary defined by each implementation's
 * component registry"*, and the meta-schema enumerates none. Kelir's vocabulary
 * lives in [`features/rad/renderer/registry.ts`](../features/rad/renderer/registry.ts)
 * and nowhere else; narrowing it here would put it in two places and make a
 * definition carrying an unknown type a compile error rather than the runtime
 * case the renderer has to handle anyway.
 */

/**
 * A JSON Logic expression.
 *
 * `unknown` rather than a recursive type: the meta-schema accepts any
 * single-key object and defers operator and arity checking to the runtime and
 * the Calculation Rule Registry, which the backend enforces at save. A
 * structural type here would be a third opinion about which operators exist.
 */
export type JsonLogic = unknown

/** JFSS §3. What a component is for, which decides how it is rendered. */
export type JfssRole = 'data' | 'layout' | 'display' | 'action'

/** JFSS §6.1. Where a rule runs. */
export type JfssScope = 'client' | 'server' | 'both'

/** JFSS §7.1. What a `conditional` does when its expression is true. */
export type JfssConditionalAction = 'show' | 'hide' | 'enable' | 'disable'

/**
 * JFSS §4.2.3. How `calculate` participates in value resolution.
 *
 * Absent means `derived` — the specification requires implementations to treat
 * a missing mode as `derived` rather than inferring one from the expression.
 */
export type JfssCalculateMode = 'derived' | 'generated'

/**
 * JFSS §5. The basic validation contract.
 *
 * **Exactly the keywords `jfss-meta-v2.0.1.json` allows, because it closes this
 * object with `additionalProperties: false`.** A definition carrying anything
 * else is refused at save, so a keyword declared here that the meta-schema does
 * not have describes a document that can never be stored — which is worse than
 * a missing one, because it reads as supported.
 *
 * Notably absent, and each was declared here once by assumption from JSON
 * Schema: `minItems`, `maxItems`, `exclusiveMinimum`, `exclusiveMaximum` and
 * `multipleOf`. An array's size is not constrained by this contract at all in
 * v2.0.1 — `uniqueItems` and `uniqueBy` are what it offers for arrays — and the
 * specification is frozen, so that is the shape rather than a gap to fill.
 */
export interface JfssValidation {
  type: 'string' | 'number' | 'integer' | 'boolean' | 'array' | 'object'
  required?: boolean
  minLength?: number
  maxLength?: number
  minimum?: number
  maximum?: number
  pattern?: string
  format?: 'email' | 'uri' | 'date' | 'time' | 'date-time' | 'uuid'
  enum?: unknown[]
  uniqueItems?: boolean
  /** The child `key` an array's rows must be unique by (Validation Rule Registry). */
  uniqueBy?: string
  /** Per-keyword message overrides, keyed by the keyword they replace. */
  messages?: Record<string, string>
}

/** JFSS §6.2. An advanced rule, which declares where it runs. */
export interface JfssAdvancedRule {
  rule: string
  scope: JfssScope
  params?: Record<string, unknown>
  message?: string
}

/** JFSS §7. Runtime visibility or enablement, driven by an expression. */
export interface JfssConditional {
  action: JfssConditionalAction
  logic: JsonLogic
}

/** JFSS §4.2. One choice offered by a `select` or `radio`. */
export interface JfssOption {
  label: string
  value: unknown
}

/** JFSS §4.1. What every component carries, whatever its role. */
export interface JfssBaseComponent {
  /** Unique per component *instance*, and what `settings.lookups` binds on. */
  id: string
  role: JfssRole
  type: string
  conditional?: JfssConditional
}

/** JFSS §4.2. A component that collects a value. */
export interface JfssDataComponent extends JfssBaseComponent {
  role: 'data'
  key: string
  label: string
  placeholder?: string
  description?: string
  defaultValue?: unknown
  validation: JfssValidation
  options?: JfssOption[]
  rules?: JfssAdvancedRule[]
  calculate?: JsonLogic
  calculateMode?: JfssCalculateMode
  readOnly?: boolean
  /**
   * Array/repeater types only: the row template.
   *
   * §4.3.1 — *"that array is a schema definition repeated per row, not a set of
   * sibling fields"*. It shares its property name with a layout container's
   * children and means something different, which is worth knowing before
   * writing a traversal.
   */
  components?: JfssComponent[]
  /** Array/repeater types only: a child `key` to fill with the 1-based row index. */
  sequenceKey?: string
  /** Array/repeater types only: empty rows to create on mount. */
  defaultItems?: number
}

/** One positional slot of a `columns` container. */
export interface JfssColumnSlot {
  components: JfssComponent[]
  /** Free-form per-column layout hints; the meta-schema leaves this open. */
  [key: string]: unknown
}

/** One named slot of a `tabs` container, or one step of a sequential type. */
export interface JfssTabSlot {
  title: string
  components: JfssComponent[]
  [key: string]: unknown
}

/**
 * JFSS §4.3. A container.
 *
 * **Exactly one of `components`, `columns` or `tabs` is present** — the
 * meta-schema enforces it with a `oneOf`, so all three are optional here and
 * the guards below are how the renderer decides which it got.
 */
export interface JfssLayoutComponent extends JfssBaseComponent {
  role: 'layout'
  title?: string
  grid?: { columns?: number; gap?: string; [key: string]: unknown }
  components?: JfssComponent[]
  columns?: JfssColumnSlot[]
  tabs?: JfssTabSlot[]
}

/** JFSS §4.4. Static or computed text, and the purely presentational types. */
export interface JfssDisplayComponent extends JfssBaseComponent {
  role: 'display'
  /** Absent on types that render no text — `divider`, `spacer` (§4.4). */
  content?: string
  variant?: string
  calculate?: JsonLogic
}

/** JFSS §4.5. Something to press. */
export interface JfssActionComponent extends JfssBaseComponent {
  role: 'action'
  label: string
  action: string
  theme?: string
}

export type JfssComponent =
  JfssDataComponent | JfssLayoutComponent | JfssDisplayComponent | JfssActionComponent

/** JFSS §2. The document. */
export interface JfssDefinition {
  formId: string
  /** The specification version, pinned to the 2.x line. Not the revision. */
  version: string
  title?: string
  /**
   * Global form configuration, and the one object JFSS declares open.
   *
   * Kelir puts its lookup bindings here under `lookups` (decision **D-23**),
   * because the specification is frozen at v2.0.1 and a component property is
   * not something an implementation may add.
   */
  settings?: JfssSettings
  components: JfssComponent[]
}

/** JFSS §2 `settings`, with the one key Kelir defines inside it. */
export interface JfssSettings {
  /**
   * Component `id` to master-data source key (**D-23**, FR-RAD-007).
   *
   * Keyed by `id` rather than `key`: §4.1 makes `id` unique per instance, while
   * two components in one document may legitimately share a `key` when one of
   * them lives in a datagrid row template.
   */
  lookups?: Record<string, string>
  [key: string]: unknown
}

// --- Guards -----------------------------------------------------------------
//
// Narrowing by `role` rather than by the presence of a property: the property
// sets overlap (a data component and a layout container both may carry
// `components`, meaning different things), so a shape test would sometimes be
// right by accident.

export function isDataComponent(component: JfssComponent): component is JfssDataComponent {
  return component.role === 'data'
}

export function isLayoutComponent(component: JfssComponent): component is JfssLayoutComponent {
  return component.role === 'layout'
}

export function isDisplayComponent(component: JfssComponent): component is JfssDisplayComponent {
  return component.role === 'display'
}

export function isActionComponent(component: JfssComponent): component is JfssActionComponent {
  return component.role === 'action'
}

/**
 * Every child of a component, whatever shape holds them.
 *
 * **JFSS §4.3.1 is a rule about traversals and this is the only one in the
 * frontend.** The three container shapes — `components`, `columns[].components`
 * and `tabs[].components` — are a closed set, and *"traversing only
 * `components` will silently ignore every child nested inside a `columns` or
 * `tabs` container"*. The backend states the same rule about its two walks
 * (`domain/jfss.rs`); one function here means the frontend cannot acquire a
 * second walk that forgets a shape.
 *
 * A `data` component's `components` is a row template rather than a set of
 * siblings, so it is **not** returned here: a caller walking the tree to render
 * it must not render a datagrid's row template as though it were inline. The
 * datagrid field asks for its template directly.
 */
export function childComponents(component: JfssComponent): JfssComponent[] {
  if (!isLayoutComponent(component)) {
    return []
  }

  if (component.components) {
    return component.components
  }

  if (component.columns) {
    return component.columns.flatMap((slot) => slot.components ?? [])
  }

  if (component.tabs) {
    return component.tabs.flatMap((slot) => slot.components ?? [])
  }

  return []
}

/**
 * Every `data` component **of one scope**, in document order.
 *
 * Descends through layout containers, and **stops at a `data` component**. That
 * asymmetry is the point: a repeater's `components` is a row template whose
 * `key`s address properties of a row object, not of the payload this scope
 * describes (§4.3.1). Descending would return `quantity` and `unit_price` as
 * siblings of `line_items`, and the form would build a payload carrying a
 * top-level `quantity` that belongs to no row and that the server would
 * validate against nothing.
 *
 * Call it again with a template's own children to get that row's fields —
 * which is what the datagrid does, and why one function serves both.
 *
 * **This is not the walk for finding every component in a document.** A lookup
 * inside a row template is still a lookup; it is resolved where it is rendered,
 * through the binding map, rather than by anything walking from the root.
 */
export function dataComponents(components: JfssComponent[]): JfssDataComponent[] {
  return components.flatMap((component) => {
    if (isDataComponent(component)) {
      return [component]
    }

    return dataComponents(childComponents(component))
  })
}
