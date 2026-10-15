import type { ValidationDetail } from '@/types/api'
import type { JfssComponent, JfssValidation } from '@/types/jfss'

import { offeredEntry } from './builderRegistry'
import { dataNodes } from './expressionScope'

/**
 * **The properties panel's edits, as pure functions** (#688 part 2, **D-86** A,
 * **D-116**).
 *
 * Each takes what the node holds and returns what it holds next, and **each
 * leaves alone every property it was not asked to change** (AC4): a node is
 * spread, never rebuilt, so a `grid`, a `theme`, a `messages` or a
 * `calculateMode` the panel does not show survives every edit it makes.
 *
 * **A cleared field removes its property** (AC3, AC3b, AC7). The panel never
 * writes `null` or `''` for an optional property: the meta-schema refuses
 * `null` for every one of them, and an empty string is a value the author did
 * not type — an empty `pattern` matches everything, an empty `placeholder`
 * draws nothing. **The properties JFSS requires are the exception**: a data
 * node's `key` and `label`, and a button's `label`. Those keep what was typed,
 * even empty, because removing them is a refusal the server makes rather than
 * one the panel hides. Everything else the panel edits — a display node's
 * `content`, a layout's `title`, every limit and text — is optional, and goes
 * when cleared.
 */

type Node = Record<string, unknown>

/** A copy of `record` with `key` set, or removed when `value` is `undefined` or `''`. */
export function withProperty<T extends object>(record: T, key: string, value: unknown): T {
  const next = { ...record } as Node

  if (value === undefined || value === '') {
    delete next[key]
  } else {
    next[key] = value
  }

  return next as T
}

// --- Expressions (D-110) ----------------------------------------------------------

/**
 * Whether a value may be written as a `calculate` or a `conditional.logic`:
 * **a plain object with exactly one property**, the meta-schema's
 * `$defs/jsonLogic` (`type: object`, `minProperties: 1`, `maxProperties: 1`).
 *
 * The visual builder only ever emits one, but its raw box emits whatever the
 * text parses to — `null`, `5`, `[]`, `{}`, an object of two keys — and the
 * shared builder is right to: a workflow condition may be a literal. A form's
 * expression may not, so this host refuses it rather than writing a value the
 * server refuses at save.
 */
export function isExpression(value: unknown): boolean {
  return (
    typeof value === 'object' &&
    value !== null &&
    !Array.isArray(value) &&
    Object.keys(value).length === 1
  )
}

/** Whether a value may be written as a rule's `params`: a plain object (`advancedRule.params`). */
export function isParams(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

// --- A rule's required parameter (#722) ------------------------------------------

interface RequiredParam {
  param: string
  kind: 'text' | 'list'
  /** What the rule does at submit with the parameter empty. */
  withoutIt: 'refuses' | 'accepts'
}

/**
 * Each enforceable rule's required parameter, by the name the Validation Rule
 * Registry gives it (§3.1). **The one table** (AC15): *Add rule* and a change of
 * rule seed its empty shape, so the box shows the author what to fill in
 * (schema-guardian M1), and the flag reads the same entry.
 *
 * **The verdict is the server's** (`kelir-backend/src/modules/rad/domain/engine.rs`),
 * which refuses such a rule at save and at publish (`RULE_PARAM_MISSING`,
 * `RULE_PARAM_INVALID`); the flag only says so before the Save. Whether a
 * `target` names a field in the rule's scope is not checked here: that is the
 * server's `RULE_TARGET_NOT_FOUND`.
 */
const REQUIRED_PARAMS: Readonly<Record<string, RequiredParam>> = {
  matchesField: { param: 'target', kind: 'text', withoutIt: 'refuses' },
  notMatchesField: { param: 'target', kind: 'text', withoutIt: 'accepts' },
  regex: { param: 'pattern', kind: 'text', withoutIt: 'accepts' },
  oneOf: { param: 'values', kind: 'list', withoutIt: 'refuses' },
  notOneOf: { param: 'values', kind: 'list', withoutIt: 'accepts' },
}

function requiredParam(rule: string): RequiredParam | undefined {
  return Object.prototype.hasOwnProperty.call(REQUIRED_PARAMS, rule)
    ? REQUIRED_PARAMS[rule]
    : undefined
}

/** A rule's parameters, empty: `{target: ''}`, `{values: []}`, or `{}` for a rule outside the table. */
export function seededParams(rule: string): Record<string, unknown> {
  const required = requiredParam(rule)

  return required ? { [required.param]: required.kind === 'text' ? '' : [] } : {}
}

/** What the flag under a rule's parameters says: the parameter, and the rest of the sentence. */
export interface ParamsFlag {
  param: string
  text: string
}

/**
 * The flag for a rule whose required parameter is missing, empty (`''`, `[]`)
 * or of the wrong JSON type, or `undefined` when there is nothing to say.
 * **Flagged, not refused**, as `limitConflicts` is: what was typed is saved, and
 * the server refuses it.
 */
export function paramsFlag(rule: string, params: unknown): ParamsFlag | undefined {
  const required = requiredParam(rule)

  if (!required) {
    return undefined
  }

  const { param, kind, withoutIt } = required
  const value = isParams(params) ? params[param] : undefined
  const consequence = `a ${rule} rule with no ${param} ${withoutIt} every value. Saving is refused until it is filled in.`

  if (value === undefined) {
    return { param, text: `is missing: ${consequence}` }
  }

  const fits = kind === 'text' ? typeof value === 'string' : Array.isArray(value)

  if (!fits) {
    return {
      param,
      text: `must be ${kind === 'text' ? 'a text' : 'a list'}. Saving is refused until it is one.`,
    }
  }

  return (value as string | unknown[]).length === 0
    ? { param, text: `is empty: ${consequence}` }
    : undefined
}

// --- Validation limits (D-116) ------------------------------------------------

/** Every limit the panel edits, in the order it draws them. */
export const LIMITS = ['minLength', 'maxLength', 'pattern', 'minimum', 'maximum'] as const

export type Limit = (typeof LIMITS)[number]

/**
 * The limits a `validation.type` can carry (AC3b): lengths and a pattern for a
 * string, bounds for a number. **No other type is offered one**: the
 * meta-schema would accept `minimum` on a boolean, and the renderer would
 * never apply it, so offering it would be offering a setting that does
 * nothing.
 */
export function limitsFor(type: string): readonly Limit[] {
  switch (type) {
    case 'string':
      return ['minLength', 'maxLength', 'pattern']
    case 'number':
    case 'integer':
      return ['minimum', 'maximum']
    default:
      return []
  }
}

/**
 * The limits each component type is offered, before its `validation.type` is
 * consulted: **a string's limits on a text box alone, bounds on a number
 * alone**. A date, a select, a radio group and a lookup also carry
 * `validation.type: "string"`, but their value is a date or a chosen option,
 * not text an author's length or pattern is about, so offering them one would
 * be offering a setting that does nothing useful. A limit loaded on any other
 * type is kept as loaded (AC4), not offered.
 */
const LIMITS_BY_COMPONENT: Readonly<Record<string, readonly Limit[]>> = {
  textfield: ['minLength', 'maxLength', 'pattern'],
  textarea: ['minLength', 'maxLength', 'pattern'],
  number: ['minimum', 'maximum'],
}

/**
 * The limits the panel offers a node (AC3b, D-116): those its component type
 * is offered **and** its `validation.type` can carry. A text field whose value
 * type was switched to `number` is offered none, and so is a number field
 * switched to `string`.
 */
export function offeredLimits(componentType: string, valueType: string): readonly Limit[] {
  const byComponent = Object.prototype.hasOwnProperty.call(LIMITS_BY_COMPONENT, componentType)
    ? LIMITS_BY_COMPONENT[componentType]
    : []

  return limitsFor(valueType).filter((limit) => byComponent.includes(limit))
}

/**
 * Switches `validation.type`, **removing only the limits the new type cannot
 * carry** (AC3b). `number` to `integer` keeps both bounds; `string` to
 * `number` drops the lengths and the pattern. Everything else — `required`,
 * `format`, `enum`, `messages` — is kept as it was.
 */
export function switchValueType(
  validation: JfssValidation,
  next: JfssValidation['type'],
): JfssValidation {
  const carried = limitsFor(next)
  const result: Node = { ...validation, type: next }

  for (const limit of LIMITS) {
    if (!carried.includes(limit)) {
      delete result[limit]
    }
  }

  return result as unknown as JfssValidation
}

/**
 * Whether a lower bound exceeds its upper one (AC3b). **Flagged, not
 * refused**: what was typed is saved, and the server and the renderer stay the
 * verdict on it.
 */
export function limitConflicts(validation: JfssValidation): { length: boolean; range: boolean } {
  const exceeds = (low: unknown, high: unknown) =>
    typeof low === 'number' && typeof high === 'number' && low > high

  return {
    length: exceeds(validation.minLength, validation.maxLength),
    range: exceeds(validation.minimum, validation.maximum),
  }
}

// --- Placeholder (D-116) ---------------------------------------------------------

/**
 * The data types whose renderer field draws a `placeholder`: `TextField`,
 * `TextareaField` and `NumberField` on the input, `SelectField` as its empty
 * choice, and `LookupField` on its search box (`features/rad/fields/`).
 *
 * **Only these are offered one.** A date, a radio group and a checkbox draw
 * none, so a placeholder offered there would be saved and never seen. One
 * loaded on them is kept as loaded (AC4), not offered.
 * `FormPropertiesPanel.renders.spec.ts` renders every data type with one and
 * fails if this list and the renderer disagree.
 */
export const PLACEHOLDER_TYPES: readonly string[] = [
  'textfield',
  'textarea',
  'number',
  'select',
  'lookup',
]

// --- Type switches (part 1's rules, moved here unchanged) ------------------------

/** What a fresh node of a type carries: the builder's own defaults. */
function defaultsOf(type: string, id: string): Node {
  return (offeredEntry(type)?.create({ id, key: 'k' }) ?? {}) as Node
}

/**
 * Switches a node's type within its role, and the properties that belong to
 * it (#688 part 1, the schema-guardian's M2 and L). **Nothing else changes**
 * (AC4).
 *
 * - A **divider** draws no text (JFSS §4.4), so its `content` goes; leaving a
 *   divider for a type that draws text gives that type's default content.
 * - A **select** or **radio** takes its choices from `options`, so one with
 *   none gets the default option.
 * - A **lookup** takes its choices from master data, and the server refuses
 *   one that also carries `options` (`LOOKUP_HAS_STATIC_OPTIONS`), so they go.
 */
export function switchType(component: JfssComponent, next: string): JfssComponent {
  const node: Node = { ...(component as unknown as Node), type: next }

  if (component.role === 'display') {
    if (next === 'divider') {
      delete node.content
    } else if (typeof node.content !== 'string' || node.content === '') {
      node.content = defaultsOf(next, component.id).content
    }
  }

  if (component.role === 'data') {
    const options = node.options

    if (
      (next === 'select' || next === 'radio') &&
      !(Array.isArray(options) && options.length > 0)
    ) {
      node.options = defaultsOf(next, component.id).options
    }

    if (next === 'lookup') {
      delete node.options
    }
  }

  return node as unknown as JfssComponent
}

// --- Data grid (AC3) -------------------------------------------------------------

/**
 * The row template's data fields: what `sequenceKey` and `uniqueBy` may name
 * (AC3). **Through any layout container in the template**, as the server's
 * `container_children` walks it and as the renderer fills a row: a field in a
 * panel inside the row is still a key of the row's object. Not into a nested
 * grid's own template, whose keys are its rows', not this row's. The same walk
 * as an expression's variables (`expressionScope.dataNodes`).
 */
export function rowFields(grid: JfssComponent): JfssComponent[] {
  const template = (grid as unknown as Node).components

  return Array.isArray(template) ? dataNodes(template as JfssComponent[]) : []
}

/** The keys of {@link rowFields}, once each. */
export function rowKeys(grid: JfssComponent): string[] {
  return rowFields(grid).map((field) => (field as unknown as { key: string }).key)
}

/**
 * The row fields `sequenceKey` may name: those with no `calculate`. JFSS S12.7
 * — *never use `sequenceKey` in combination with a `calculate` property on the
 * same child field* — since the two would each overwrite the other's value.
 */
export function sequenceKeys(grid: JfssComponent): string[] {
  return rowFields(grid)
    .filter((field) => !('calculate' in (field as unknown as Node)))
    .map((field) => (field as unknown as { key: string }).key)
}

/** `uniqueBy` as a list, whichever of its two shapes it was loaded in. */
export function uniqueByKeys(value: unknown): string[] {
  if (typeof value === 'string') {
    return [value]
  }

  return Array.isArray(value) ? value.filter((key): key is string => typeof key === 'string') : []
}

/**
 * `uniqueBy` with `key` added or removed, **in the shape it was loaded in**
 * (AC3): a string stays a string until a second key is added, and a list stays
 * a list. Emptied, it is `undefined`, and the property goes.
 *
 * A loaded key that names no row field is kept until it is unchecked: it is
 * flagged, never dropped.
 */
export function toggleUniqueBy(
  current: string | string[] | undefined,
  key: string,
  on: boolean,
): string | string[] | undefined {
  const keys = uniqueByKeys(current)
  const next = on ? (keys.includes(key) ? keys : [...keys, key]) : keys.filter((k) => k !== key)

  if (next.length === 0) {
    return undefined
  }

  return Array.isArray(current) || next.length > 1 ? next : next[0]
}

// --- Where the server's details land (AC9) ---------------------------------------

/**
 * The rest of a detail's path below the node at `nodePath`, or `null` when the
 * detail is not about a property of this node. `definition.components.0.calculate`
 * is `calculate` for the node at `components.0`.
 */
export function restOfPath(detail: ValidationDetail, nodePath: string): string | null {
  const prefix = `definition.${nodePath}.`

  return detail.path.startsWith(prefix) ? detail.path.slice(prefix.length) : null
}

/**
 * The first message whose path is `property`, or below it, on this node: the
 * message shown at that property's field. `property` is dot-notation, as
 * S10.3 writes it: `calculate`, `conditional.logic`, `rules.2.params`,
 * `validation.pattern`.
 */
export function messageAt(
  details: readonly ValidationDetail[],
  nodePath: string,
  property: string,
): string | undefined {
  return details.find((detail) => {
    const rest = restOfPath(detail, nodePath)

    return rest !== null && (rest === property || rest.startsWith(`${property}.`))
  })?.message
}
