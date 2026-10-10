import { describe, expect, it } from 'vitest'

import { ApiError } from '@/api/error'
import type { ValidationDetail } from '@/types/api'
import type { JwssDefinition } from '@/types/workflow'

import { addressOf, placeDetails, placeVerdict, rowErrors } from './workflowVerdict'

/**
 * Where the server's verdict is drawn (#426 AC3).
 *
 * The messages below are the backend's own wording from `workflow/domain/jwss.rs`
 * — S6, S7 and S9 report at the list, and name the state in backticks — so a
 * change to that wording that stops naming the state is what would move these
 * messages off the state card.
 *
 * Seen red, 2026-10-09: list details left at the list (*moves an S6 detail…*,
 * *moves an S7 detail…*); same-path messages kept first-wins (*joins the
 * messages…*).
 */

const DEFINITION = {
  workflowKey: 'w',
  version: '1.0.0',
  name: 'W',
  initialState: 'REVIEW',
  states: [
    { code: 'REVIEW', name: 'Review', mapsToDocumentStatus: 'PENDING_APPROVAL' },
    { code: 'ORPHAN', name: 'Orphan', mapsToDocumentStatus: 'IN_REVIEW' },
    { code: 'DONE', name: 'Done', mapsToDocumentStatus: 'COMPLETED', isFinal: true },
  ],
  transitions: [],
} as unknown as JwssDefinition

function detail(path: string, message: string, code = 'INVALID_DEFINITION'): ValidationDetail {
  return { path, rule: 'S6', code, message }
}

describe('addressOf', () => {
  it('reads the state or transition a path addresses, and the field under it', () => {
    expect(addressOf('definition.states.2.task.assignment.roleCode')).toEqual({
      kind: 'state',
      index: 2,
      field: 'task.assignment.roleCode',
    })
    expect(addressOf('definition.transitions.10')).toEqual({
      kind: 'transition',
      index: 10,
      field: '',
    })
    expect(addressOf('definition.states')).toBeNull()
    expect(addressOf('definition.variables.0.source')).toBeNull()
    expect(addressOf('name')).toBeNull()
  })
})

describe('placeDetails', () => {
  it('moves an S6 detail about the whole list to the state it names', () => {
    const placed = placeDetails(
      [
        detail(
          'definition.states',
          '`ORPHAN` cannot be reached from the initial state',
          'UNREACHABLE_STATE',
        ),
      ],
      DEFINITION,
    )

    expect(placed.map((item) => item.path)).toEqual(['definition.states.1'])
  })

  it('moves an S7 detail to the state its transitions leave', () => {
    const placed = placeDetails(
      [
        detail(
          'definition.transitions',
          '2 transitions leave `REVIEW` on APPROVE with no condition; at most one may be the fallback',
          'AMBIGUOUS_FALLBACK',
        ),
      ],
      DEFINITION,
    )

    expect(placed.map((item) => item.path)).toEqual(['definition.states.0'])
  })

  it('leaves a list detail naming no declared state at the list', () => {
    const placed = placeDetails(
      [
        detail(
          'definition.states',
          'no state maps to COMPLETED or CANCELLED, so …',
          'NO_TERMINAL_STATUS',
        ),
        detail('definition.states', '`GHOST` cannot be reached from the initial state'),
      ],
      DEFINITION,
    )

    expect(placed).toHaveLength(1)
    expect(placed[0].path).toBe('definition.states')
    expect(placed[0].message).toContain('no state maps to COMPLETED')
    expect(placed[0].message).toContain('`GHOST`')
  })

  it('joins the messages of details that land on one path, rather than keeping the first', () => {
    const placed = placeDetails(
      [
        detail('definition.states', '`ORPHAN` cannot be reached from the initial state'),
        detail('definition.states', 'no final state is reachable from `ORPHAN`, so …'),
      ],
      DEFINITION,
    )

    expect(placed).toHaveLength(1)
    expect(placed[0].message).toBe(
      '`ORPHAN` cannot be reached from the initial state no final state is reachable from `ORPHAN`, so …',
    )
  })

  it('leaves a detail that already addresses a field where it is', () => {
    const details = [detail('definition.transitions.1.requiresComment', 'false was expected')]

    expect(placeDetails(details, DEFINITION)).toEqual(details)
  })
})

describe('placeVerdict', () => {
  it('rebuilds a refusal with its details placed, keeping its code, message and status', () => {
    const refused = new ApiError('VALIDATION_ERROR', 'Invalid', 422, [
      detail('definition.states', '`ORPHAN` cannot be reached'),
    ])

    const placed = placeVerdict(refused, DEFINITION) as ApiError

    expect(placed).toBeInstanceOf(ApiError)
    expect(placed.code).toBe('VALIDATION_ERROR')
    expect(placed.status).toBe(422)
    expect(placed.fieldErrors()).toEqual({ 'definition.states.1': '`ORPHAN` cannot be reached' })
  })

  it('passes through anything that is not a refusal with details', () => {
    const conflict = new ApiError('CONFLICT', 'Already exists', 409)
    const crash = new Error('boom')

    expect(placeVerdict(conflict, DEFINITION)).toBe(conflict)
    expect(placeVerdict(crash, DEFINITION)).toBe(crash)
  })
})

describe('rowErrors', () => {
  it('keys the messages under one row by their field relative to it, and only that row’s', () => {
    expect(
      rowErrors(
        {
          'definition.transitions.1': 'whole',
          'definition.transitions.1.allowedBy.roleCode': 'role',
          'definition.transitions.10.to': 'another row',
          'definition.states.1.name': 'a state',
        },
        'transition',
        1,
      ),
    ).toEqual({ '': 'whole', 'allowedBy.roleCode': 'role' })
  })
})
