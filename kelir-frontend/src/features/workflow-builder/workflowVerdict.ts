import { ApiError } from '@/api/error'
import type { ValidationDetail } from '@/types/api'
import type { JwssDefinition } from '@/types/workflow'

/**
 * Where the server's verdict on a definition is shown (#426 AC3, AC4).
 *
 * **There is no second validator here.** The backend checks a definition at
 * save and at publish (ADR-0019, `workflow/domain/jwss.rs`), and every problem
 * comes back as a detail whose `path` addresses it by position in the request
 * body: `definition.states.1.task.assignment.roleCode`,
 * `definition.transitions.3.requiresComment`. This module only decides *where*
 * each one is drawn. `useFormErrors` then holds them by path, as every
 * hand-built form does (coding standard §3.4), and `FormUnplacedErrors` lists
 * whatever this screen has no place for.
 */

/** A detail's path, relative to the state or transition it is under. */
export interface Addressed {
  kind: 'state' | 'transition'
  index: number
  /** `''` for the state or transition itself; `task.assignment.roleCode` for a field in it. */
  field: string
}

const ROW = /^definition\.(states|transitions)\.(\d+)(?:\.(.+))?$/

/** The state or transition a path addresses, or `null` for one that addresses neither. */
export function addressOf(path: string): Addressed | null {
  const match = ROW.exec(path)

  if (!match) {
    return null
  }

  return {
    kind: match[1] === 'states' ? 'state' : 'transition',
    index: Number(match[2]),
    field: match[3] ?? '',
  }
}

/** The first `` `CODE` `` a message names, which is how S6 and S7 name their state. */
function namedCode(message: string): string | undefined {
  return /`([^`]+)`/.exec(message)?.[1]
}

/**
 * The details, with each one about a whole list moved to the state it names,
 * and the messages of details that share a path joined.
 *
 * **Two rules address the list, and name the state in the message.** S6
 * (`UNREACHABLE_STATE`, `DEAD_END_STATE`) and S7 (`AMBIGUOUS_FALLBACK`) report
 * at `definition.states` and `definition.transitions`, because what they find
 * is a property of the graph, and say which state in backticks. AC3 asks for
 * the verdict *at the state it names*, so a detail whose first backticked
 * token is a declared state code is placed at that state. One whose message
 * names no declared state — S9's *no state maps to COMPLETED* — stays at the
 * list.
 *
 * **Joined, not first-wins**, because one state can be both unreachable and a
 * dead end, and `ApiError.fieldErrors()` keeps only the first message per path.
 */
export function placeDetails(
  details: readonly ValidationDetail[],
  definition: JwssDefinition,
): ValidationDetail[] {
  const byPath = new Map<string, ValidationDetail>()

  for (const detail of details) {
    let path = detail.path

    if (path === 'definition.states' || path === 'definition.transitions') {
      const code = namedCode(detail.message)
      const index = definition.states.findIndex((state) => state.code === code)

      if (index >= 0) {
        path = `definition.states.${index}`
      }
    }

    const existing = byPath.get(path)

    byPath.set(
      path,
      existing
        ? { ...existing, message: `${existing.message} ${detail.message}` }
        : { ...detail, path },
    )
  }

  return [...byPath.values()]
}

/**
 * A refusal with its details placed, for `useFormErrors().report`. Anything
 * that is not an `ApiError` with details passes through as it came.
 */
export function placeVerdict(error: unknown, definition: JwssDefinition): unknown {
  if (!(error instanceof ApiError) || error.details.length === 0) {
    return error
  }

  return new ApiError(
    error.code,
    error.message,
    error.status,
    placeDetails(error.details, definition),
    error.retryAfterSeconds,
  )
}

/**
 * The messages addressed to one state or transition, keyed by field relative
 * to it. The row draws each under the input it has for that field, and lists
 * the rest at its top, so everything under a drawn row is placed on it.
 */
export function rowErrors(
  fieldErrors: Record<string, string>,
  kind: Addressed['kind'],
  index: number,
): Record<string, string> {
  const found: Record<string, string> = {}

  for (const [path, message] of Object.entries(fieldErrors)) {
    const address = addressOf(path)

    if (address && address.kind === kind && address.index === index) {
      found[address.field] = message
    }
  }

  return found
}
