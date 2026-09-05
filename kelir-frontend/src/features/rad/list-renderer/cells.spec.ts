import { describe, expect, it } from 'vitest'

import type { RenderableColumn } from '@/types/rad'

import { ABSENT, cellText, nextSort } from './cells'

/**
 * The definition-to-cell mapping (#340 AC6).
 *
 * **These are the cases the browser flow is too slow to enumerate.** One
 * Playwright run proves a configured list renders; it cannot afford one run per
 * `format`, per absent value, per shape a `form_data.*` path can return. The
 * mapping is pure so that it can be asked all of them here.
 */

function column(overrides: Partial<RenderableColumn> = {}): RenderableColumn {
  return {
    key: 'title',
    label: 'Title',
    dataType: null,
    format: null,
    width: null,
    sortable: true,
    ...overrides,
  }
}

describe('cellText — what a cell shows', () => {
  it('shows a plain string as itself', () => {
    expect(cellText(column(), 'Quarterly refresh')).toBe('Quarterly refresh')
  })

  /**
   * **An em dash, not an empty cell.** A blank cell is indistinguishable from a
   * rendering bug, and this list exists because a screen that says nothing
   * while being wrong is worse than one that refuses.
   */
  it.each([
    ['null', null],
    ['undefined', undefined],
    ['an empty string', ''],
    ['a string of spaces', '   '],
  ])('shows a dash for %s', (_name, value) => {
    expect(cellText(column(), value)).toBe(ABSENT)
  })

  /**
   * **`false` and `0` are values.** Both are falsy, and a naive
   * `value || ABSENT` would turn an unticked approval and a zero total into the
   * same dash as a field the document does not have.
   */
  it('shows false rather than treating it as absent', () => {
    expect(cellText(column(), false)).toBe('No')
  })

  it('shows zero rather than treating it as absent', () => {
    expect(cellText(column({ format: 'number' }), 0)).toBe('0')
  })

  it('shows true as a word rather than as a literal', () => {
    expect(cellText(column(), true)).toBe('Yes')
  })
})

describe('cellText — the formats §5.7 names', () => {
  it('formats a currency value to two places', () => {
    expect(cellText(column({ format: 'currency' }), 1234.5)).toContain('1,234.50')
  })

  it('formats a numeric string, because a form payload stores what was typed', () => {
    expect(cellText(column({ format: 'currency' }), '42')).toContain('42.00')
  })

  it('formats a date without inventing a time', () => {
    const shown = cellText(column({ format: 'date-short' }), '2026-09-05T01:59:49Z')

    expect(shown).toMatch(/2026/)
    expect(shown).not.toMatch(/:/)
  })

  it('formats a date and time when the column asks for both', () => {
    expect(cellText(column({ format: 'date-time' }), '2026-09-05T01:59:49Z')).toMatch(/:/)
  })

  /**
   * A `date-short` on a column that turns out to hold `pending` is a definition
   * mistake. Printing the value is what lets somebody see which of the two
   * things went wrong; `Invalid Date` or a dash would hide it.
   */
  it('shows an unparseable date as itself rather than as Invalid Date', () => {
    expect(cellText(column({ format: 'date-short' }), 'pending')).toBe('pending')
  })

  it('shows a non-numeric value under a numeric format as itself', () => {
    expect(cellText(column({ format: 'currency' }), 'to be agreed')).toBe('to be agreed')
  })

  it('leaves a value alone when the column declares no format', () => {
    expect(cellText(column(), 42)).toBe('42')
  })
})

describe('cellText — the vocabularies the rest of the product uses', () => {
  it('shows a status as the label the built-in list shows', () => {
    expect(cellText(column({ key: 'status', dataType: 'STATUS' }), 'PENDING_APPROVAL')).toBe(
      'Pending approval',
    )
  })

  it('shows a priority as its label', () => {
    expect(cellText(column({ key: 'priority', dataType: 'PRIORITY' }), 'HIGH')).toBe('High')
  })

  /**
   * A status the client has never heard of is shown rather than blanked. The
   * backend's own `from_db` is lenient for the same reason: a value already in
   * the database has a `CHECK` vouching for it, and hiding it would make a
   * migrated row invisible.
   */
  it('shows an unrecognised status code rather than nothing', () => {
    expect(cellText(column({ key: 'status', dataType: 'STATUS' }), 'ESCALATED')).toBe('ESCALATED')
  })
})

describe('cellText — what a form_data path can return', () => {
  /**
   * `form_data.line_items` is a legitimate path onto a repeater. Displaying one
   * is a questionable choice by the definition's author, not an impossible one
   * — and `[object Object]` would say nothing about which.
   */
  it('shows an object as JSON rather than as [object Object]', () => {
    expect(cellText(column(), { amount: 42 })).toBe('{"amount":42}')
  })

  it('shows an array as JSON', () => {
    expect(cellText(column(), [1, 2])).toBe('[1,2]')
  })
})

describe('nextSort — three states, not two', () => {
  /**
   * **The third state is the definition's own default.** A table that only
   * toggled between ascending and descending would make the author's
   * `defaultSort` unreachable the moment somebody clicked a header.
   */
  it('cycles ascending, descending, then back to the default', () => {
    const target = column({ key: 'createdAt' })

    const first = nextSort(target, null)
    expect(first).toEqual({ key: 'createdAt', descending: false })

    const second = nextSort(target, first)
    expect(second).toEqual({ key: 'createdAt', descending: true })

    expect(nextSort(target, second)).toBeNull()
  })

  it('starts a different column ascending rather than inheriting a direction', () => {
    const current = { key: 'title', descending: true }

    expect(nextSort(column({ key: 'createdAt' }), current)).toEqual({
      key: 'createdAt',
      descending: false,
    })
  })

  /**
   * `sortable` is the *server's* answer — the definition's `isSortable`
   * resolved against what the query can order by. A `form_data.*` column
   * arrives false however the definition marked it, and clicking it must do
   * nothing rather than send a sort the backend will refuse.
   */
  it('leaves the sort alone for a column the server did not mark sortable', () => {
    const current = { key: 'title', descending: false }

    expect(nextSort(column({ key: 'form_data.amount', sortable: false }), current)).toBe(current)
  })
})
