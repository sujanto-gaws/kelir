import { describe, expect, it } from 'vitest'

import { jwssViolations } from '@/lib/testing/jwss-meta-schema'
import type { JwssDefinition } from '@/types/workflow'

import {
  ASSIGNEE_TYPES,
  conditionVariables,
  STATE_KINDS,
  starterDefinition,
  TRANSITION_ACTIONS,
} from './jwssRegistry'

/**
 * The editor's vocabularies, and what it offers a condition (#426).
 *
 * **The variables are pinned to what the engine builds.** `service/engine.rs`
 * gives JSON Logic `document` as `{status, documentTypeId, documentNumber}`,
 * `actor` as `{userId}`, `formData` and `variables`. JWSS §6.1 also sketches
 * `document.amount`, `document.documentTypeKey`, `actor.roles` and
 * `actor.departmentId`; none is built, so a condition on one reads `null` and
 * silently takes the fallback. Offering one is the defect this test names.
 * Seen red, 2026-10-09, with `document.amount` added to the offered list.
 */

describe('conditionVariables', () => {
  it('offers the context the engine evaluates, and the declared variables', () => {
    const definition = {
      ...starterDefinition(),
      variables: [{ key: 'threshold', dataType: 'NUMBER' }],
    } as JwssDefinition

    expect(conditionVariables(definition).map((variable) => variable.path)).toEqual([
      'document.status',
      'document.documentTypeId',
      'document.documentNumber',
      'actor.userId',
      'variables.threshold',
    ])
  })

  it('offers none of the §6.1 paths the engine does not build', () => {
    const offered = conditionVariables(starterDefinition()).map((variable) => variable.path)

    for (const unbuilt of [
      'document.amount',
      'document.documentTypeKey',
      'actor.roles',
      'actor.departmentId',
    ]) {
      expect(offered).not.toContain(unbuilt)
    }
  })
})

describe('the registries', () => {
  it('cover JWSS §4’s ten actions and §5.1’s six assignee types', () => {
    expect(Object.keys(TRANSITION_ACTIONS).sort()).toEqual(
      [
        'APPROVE',
        'AUTO',
        'CANCEL',
        'COMPLETE',
        'DELEGATE',
        'ESCALATE',
        'REJECT',
        'RESUBMIT',
        'RETURN',
        'SUBMIT',
      ].sort(),
    )
    // JWSS §5.3: Kelir resolves four, and refuses the other two at save.
    expect(
      Object.entries(ASSIGNEE_TYPES)
        .filter(([, entry]) => entry.resolved)
        .map(([type]) => type)
        .sort(),
    ).toEqual(['DEPARTMENT_ROLE', 'OWNER', 'ROLE', 'USER'])
  })
})

describe('what the editor starts from (AC2)', () => {
  /** The starter, with what only an author knows filled in. */
  function filled(): JwssDefinition {
    const definition = starterDefinition()

    return {
      ...definition,
      workflowKey: 'purchase_approval',
      name: 'Purchase approval',
    }
  }

  it('is a JWSS v1.0.0 document once its key and name are given', () => {
    expect(jwssViolations(filled())).toEqual([])
  })

  it('stays one with every kind of state added', () => {
    const definition = filled()

    definition.states = [
      ...definition.states,
      ...(['task', 'wait', 'final'] as const).map((kind) =>
        STATE_KINDS[kind].create(`EXTRA_${kind.toUpperCase()}`),
      ),
    ]

    expect(jwssViolations(definition)).toEqual([])
  })

  it('is checked by a meta-schema that does refuse: S12 on an AUTO edge', () => {
    // Seen to fail as a guard on this suite's own instrument: a validator that
    // accepted everything would make every AC2 assertion green.
    const definition = filled()

    definition.transitions = [
      { from: 'PENDING_APPROVAL', to: 'COMPLETED', action: 'AUTO', requiresComment: true },
    ]

    expect(jwssViolations(definition).join('\n')).toContain('.transitions[0].requiresComment')
  })
})
