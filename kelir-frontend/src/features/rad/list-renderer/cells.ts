import type { RenderableColumn } from '@/types/rad'
import { DOCUMENT_PRIORITY_LABELS, DOCUMENT_STATUS_LABELS } from '@/types/document'

/**
 * Turning one cell of one row into the text a table shows (#340).
 *
 * **Pure, and separate from the component on purpose.** This is the whole of
 * the definition-to-props mapping: a `dataType`, a `format` and an arbitrary
 * JSON value in, a string out. Keeping it out of the SFC is what lets #340 AC6
 * enumerate the cases a browser flow is too slow to reach — every `format`,
 * every shape a `form_data.*` path can return, and the difference between a
 * value that is absent and one that is empty.
 *
 * **Nothing here decides *which* columns exist.** That is the server's answer
 * (`domain/render.rs`), already resolved by the time this runs. A client that
 * re-derived it would be a second copy of a rule that has one.
 */

/** What a cell shows when the document has no value at that path. */
export const ABSENT = '—'

/**
 * The text one column shows for one row's value.
 *
 * **An em dash for absent, not an empty cell.** A blank table cell is
 * indistinguishable from a rendering bug, and this list's whole reason for
 * existing is that a screen which says nothing while being wrong is worse than
 * one that refuses (#326, and #340 AC4 one layer down). `—` says *there is no
 * value here* in a way a reader can act on.
 *
 * **`false` and `0` are values.** They are falsy in JavaScript and would vanish
 * from a naive `value || ABSENT`, which would turn an unticked approval and a
 * zero total into the same dash as a missing field.
 */
export function cellText(column: RenderableColumn, value: unknown): string {
  if (value === null || value === undefined) {
    return ABSENT
  }

  if (typeof value === 'string' && value.trim() === '') {
    return ABSENT
  }

  return format(column, value)
}

function format(column: RenderableColumn, value: unknown): string {
  // A status and a priority are stored as their wire codes and read as labels
  // the rest of the product already uses. Taken from the same two maps
  // `DocumentListPage` reads, so a rendered list and the built-in list never
  // spell one status two ways.
  if (typeof value === 'string') {
    if (column.dataType === 'STATUS' || column.key.endsWith('status')) {
      const label = DOCUMENT_STATUS_LABELS[value as keyof typeof DOCUMENT_STATUS_LABELS]

      if (label) {
        return label
      }
    }

    if (column.dataType === 'PRIORITY' || column.key.endsWith('priority')) {
      const label = DOCUMENT_PRIORITY_LABELS[value as keyof typeof DOCUMENT_PRIORITY_LABELS]

      if (label) {
        return label
      }
    }
  }

  switch (column.format) {
    case 'date-short':
      return asDate(value, { dateStyle: 'medium' })
    case 'date-time':
      return asDate(value, { dateStyle: 'medium', timeStyle: 'short' })
    case 'currency':
      return asNumber(value, { minimumFractionDigits: 2, maximumFractionDigits: 2 })
    case 'number':
      return asNumber(value, {})
    default:
      return plain(value)
  }
}

/**
 * A timestamp as the viewer's locale writes it.
 *
 * **An unparseable value is shown rather than swallowed.** A `format` of
 * `date-short` on a column that turns out to hold `"pending"` is a definition
 * mistake, and printing `pending` is what lets somebody see it — where
 * `Invalid Date` or a dash would hide which of the two things went wrong.
 */
function asDate(value: unknown, options: Intl.DateTimeFormatOptions): string {
  if (typeof value !== 'string' && typeof value !== 'number') {
    return plain(value)
  }

  const at = new Date(value)

  return Number.isNaN(at.getTime()) ? plain(value) : at.toLocaleString(undefined, options)
}

/** A number as the viewer's locale writes it, or the raw value if it is not one. */
function asNumber(value: unknown, options: Intl.NumberFormatOptions): string {
  const numeric = typeof value === 'number' ? value : Number(value)

  if (typeof value === 'boolean' || !Number.isFinite(numeric)) {
    return plain(value)
  }

  return numeric.toLocaleString(undefined, options)
}

/**
 * Anything else, as text.
 *
 * **An object or an array is JSON rather than `[object Object]`.** A
 * `form_data.*` path can legitimately land on a repeater — `form_data.line_items`
 * is an array — and a definition that displays one has made a questionable
 * choice, not an impossible one. Showing the shape says which; `[object Object]`
 * says nothing at all.
 */
function plain(value: unknown): string {
  if (typeof value === 'boolean') {
    return value ? 'Yes' : 'No'
  }

  if (typeof value === 'object') {
    return JSON.stringify(value)
  }

  return String(value)
}

/**
 * The next sort state when a header is clicked.
 *
 * **Three states, not two.** Ascending, descending, then back to the list's own
 * default — because a definition's `defaultSort` is a decision its author made,
 * and a table that could only toggle between two orders would make that
 * decision unreachable once somebody clicked a header.
 */
export function nextSort(
  column: RenderableColumn,
  current: { key: string; descending: boolean } | null,
): { key: string; descending: boolean } | null {
  if (!column.sortable) {
    return current
  }

  if (current?.key !== column.key) {
    return { key: column.key, descending: false }
  }

  return current.descending ? null : { key: column.key, descending: true }
}
