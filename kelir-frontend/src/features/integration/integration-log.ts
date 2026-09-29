import type { BadgeVariant } from '@/components/ui/badge'
import type { IntegrationLogStatus } from '@/types/integration'

/**
 * What the integration log screens share (FR-INT-006, #548): how a status is
 * drawn, how a duration and a payload are written, and how a date-range input
 * and the query string's ISO instants turn into each other.
 */

/** A failure is red, a success is solid, and a call still in flight is an outline. */
export function logStatusVariant(status: IntegrationLogStatus): BadgeVariant {
  switch (status) {
    case 'SUCCESS':
      return 'default'
    case 'FAILED':
    case 'DEAD_LETTER':
      return 'destructive'
    default:
      return 'outline'
  }
}

export function formatDuration(ms: number): string {
  return ms < 1000 ? `${ms} ms` : `${(ms / 1000).toFixed(2)} s`
}

export function formatTimestamp(iso: string | null): string {
  if (!iso) {
    return '—'
  }

  const at = new Date(iso)

  return Number.isNaN(at.getTime()) ? iso : at.toLocaleString()
}

/**
 * A stored payload, pretty-printed as it is stored.
 *
 * **Nothing is unmasked, reordered or dropped**: the value is the server's and
 * is printed whole. A payload that is itself a string is shown as that string
 * rather than as a quoted JSON literal, and `null` says there was none.
 */
export function formatPayload(payload: unknown): string | null {
  if (payload === null || payload === undefined) {
    return null
  }

  if (typeof payload === 'string') {
    return payload
  }

  return JSON.stringify(payload, null, 2)
}

function pad(value: number): string {
  return String(value).padStart(2, '0')
}

/**
 * An ISO instant from the query string as a `datetime-local` value, in the
 * browser's own zone; blank for anything that is not an instant.
 */
export function toLocalInput(iso: string | undefined): string {
  if (!iso) {
    return ''
  }

  const at = new Date(iso)

  if (Number.isNaN(at.getTime())) {
    return ''
  }

  return `${at.getFullYear()}-${pad(at.getMonth() + 1)}-${pad(at.getDate())}T${pad(
    at.getHours(),
  )}:${pad(at.getMinutes())}`
}

/**
 * A `datetime-local` value as the ISO instant the API takes.
 *
 * `datetime-local` has no zone and the API takes an instant, so the value is
 * read in the browser's zone — the one the person was looking at when they
 * chose it. Blank or unparsable is blank, which drops the filter.
 */
export function fromLocalInput(value: string): string {
  if (!value) {
    return ''
  }

  const at = new Date(value)

  return Number.isNaN(at.getTime()) ? '' : at.toISOString()
}
