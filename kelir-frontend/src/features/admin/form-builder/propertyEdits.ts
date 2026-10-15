import type { ValidationDetail } from '@/types/api'
import type { JfssComponent, JfssValidation } from '@/types/jfss'

import { offeredEntry } from './builderRegistry'

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
 * draws nothing. Required properties (`key`, `label`, `content`, `title`) are
 * the exception, and keep what was typed, because removing them is a refusal
 * the server makes rather than one the panel hides.
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
 * The row template's **direct** data keys: what `sequenceKey` and `uniqueBy`
 * may name (AC3). A row's value is an object of exactly these keys, so a key
 * nested deeper is not one a row carries.
 */
export function rowKeys(grid: JfssComponent): string[] {
  const template = (grid as unknown as Node).components
  const keys: string[] = []

  if (!Array.isArray(template)) {
    return keys
  }

  for (const child of template as Node[]) {
    if (
      child?.role === 'data' &&
      typeof child.key === 'string' &&
      child.key !== '' &&
      !keys.includes(child.key)
    ) {
      keys.push(child.key)
    }
  }

  return keys
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
