import { describe, expect, it } from 'vitest'

import type { JwssDefinition } from '@/types/workflow'

import { HISTORY_LIMIT, useWorkflowDraft } from './useWorkflowDraft'

/**
 * The editor's draft and its history (#426; kopiflowvue's store conventions,
 * **D-95**).
 *
 * What is asserted is what the editor depends on: an edit leaves what it did
 * not touch **the same object** (the logic builder's echo, and cheap
 * snapshots), a row keeps its key through moves and removals (no remount under
 * the user), undo and redo are pointer moves, and a rename takes its
 * references with it only when that is unambiguous.
 *
 * Seen red, 2026-10-09: removing a state that keeps its outgoing transitions
 * (*removes a state with the transitions out of it…*); dropping the
 * `isRestoring` guard (*applies a write that lands while an undo settles…*);
 * a rename that leaves its transitions (*renames a state and takes…*).
 */

function definition(): JwssDefinition {
  return {
    workflowKey: 'purchase_approval',
    version: '1.0.0',
    name: 'Purchase approval',
    initialState: 'REVIEW',
    states: [
      { code: 'REVIEW', name: 'Review', mapsToDocumentStatus: 'PENDING_APPROVAL' },
      { code: 'DONE', name: 'Done', mapsToDocumentStatus: 'COMPLETED', isFinal: true },
      { code: 'NO', name: 'No', mapsToDocumentStatus: 'REJECTED', isFinal: true },
    ],
    transitions: [
      {
        from: 'REVIEW',
        to: 'DONE',
        action: 'APPROVE',
        allowedBy: 'OWNER',
        condition: { '==': [{ var: 'document.status' }, 'PENDING_APPROVAL'] },
      },
      { from: 'REVIEW', to: 'NO', action: 'REJECT', allowedBy: 'OWNER' },
      { from: 'DONE', to: 'NO', action: 'CANCEL', allowedBy: 'OWNER' },
      { from: 'REVIEW', to: 'DONE', action: 'APPROVE', allowedBy: 'OWNER' },
    ],
  }
}

describe('useWorkflowDraft', () => {
  it('leaves what an edit did not touch as the same object', () => {
    const start = definition()
    const draft = useWorkflowDraft(start)
    const condition = start.transitions[0].condition

    draft.replaceState(1, { ...start.states[1], name: 'Approved' })

    const after = draft.definition.value

    expect(after).not.toBe(start)
    expect(after.states).not.toBe(start.states)
    expect(after.states[0]).toBe(start.states[0])
    expect(after.transitions).toBe(start.transitions)
    expect(after.transitions[0].condition).toBe(condition)
    // The loaded definition is never written to.
    expect(start.states[1].name).toBe('Done')
  })

  it('is dirty after an edit, and clean again when undone to what was loaded', async () => {
    const draft = useWorkflowDraft(definition())

    expect(draft.isDirty.value).toBe(false)
    expect(draft.canUndo.value).toBe(false)

    draft.setRoot('name', 'Renamed')

    expect(draft.isDirty.value).toBe(true)

    await draft.undo()

    expect(draft.definition.value.name).toBe('Purchase approval')
    expect(draft.isDirty.value).toBe(false)
    expect(draft.canRedo.value).toBe(true)

    await draft.redo()

    expect(draft.definition.value.name).toBe('Renamed')
    expect(draft.canRedo.value).toBe(false)
  })

  it('makes consecutive edits of one field a single undo step, and another field a second', async () => {
    const draft = useWorkflowDraft(definition())

    draft.setRoot('name', 'P')
    draft.setRoot('name', 'Pu')
    draft.setRoot('name', 'Pur')
    draft.setRoot('description', 'Why')

    await draft.undo()
    expect(draft.definition.value.name).toBe('Pur')
    expect(draft.definition.value.description).toBeUndefined()

    await draft.undo()
    expect(draft.definition.value.name).toBe('Purchase approval')
    expect(draft.canUndo.value).toBe(false)
  })

  it('a new edit after an undo discards what could have been redone', async () => {
    const draft = useWorkflowDraft(definition())

    draft.setRoot('name', 'One')
    await draft.undo()
    draft.setRoot('description', 'Two')

    expect(draft.canRedo.value).toBe(false)
  })

  it('applies a write that lands while an undo settles without recording it', async () => {
    const draft = useWorkflowDraft(definition())

    draft.setRoot('name', 'One')

    const settling = draft.undo()

    // A child re-emitting the value it was just handed, in the same tick.
    draft.setRoot('description', 'echo')
    await settling

    expect(draft.canRedo.value).toBe(true)
    expect(draft.canUndo.value).toBe(false)
  })

  it(`reaches back ${HISTORY_LIMIT} steps and no further`, async () => {
    const draft = useWorkflowDraft(definition())

    for (let step = 0; step < HISTORY_LIMIT + 5; step += 1) {
      draft.replaceState(0, { ...draft.definition.value.states[0], name: `Step ${step}` })
    }

    let undone = 0

    while (draft.canUndo.value) {
      await draft.undo()
      undone += 1
    }

    expect(undone).toBe(HISTORY_LIMIT)
    expect(draft.definition.value.states[0].name).toBe('Step 4')
  })

  it('renames a state and takes the initial state and its transitions with it', () => {
    const start = definition()
    const draft = useWorkflowDraft(start)

    draft.renameState(0, 'MANAGER_REVIEW')

    const after = draft.definition.value

    expect(after.initialState).toBe('MANAGER_REVIEW')
    expect(after.states[0].code).toBe('MANAGER_REVIEW')
    expect(after.transitions.map((transition) => transition.from)).toEqual([
      'MANAGER_REVIEW',
      'MANAGER_REVIEW',
      'DONE',
      'MANAGER_REVIEW',
    ])
    // A transition the rename did not concern is the same object.
    expect(after.transitions[2]).toBe(start.transitions[2])
  })

  it('renames only the state when the new code is already another state’s', () => {
    const draft = useWorkflowDraft(definition())

    draft.renameState(0, 'DONE')

    const after = draft.definition.value

    expect(after.states[0].code).toBe('DONE')
    expect(after.initialState).toBe('REVIEW')
    expect(after.transitions[0].from).toBe('REVIEW')
  })

  it('removes a state with the transitions out of it, and keeps the others’ keys', () => {
    const draft = useWorkflowDraft(definition())
    const keys = draft.current.value.transitionKeys

    draft.removeState(1)

    expect(draft.definition.value.states.map((state) => state.code)).toEqual(['REVIEW', 'NO'])
    // The CANCEL edge out of DONE went with it; the edges into DONE stay, for S2 to name.
    expect(draft.definition.value.transitions.map((transition) => transition.action)).toEqual([
      'APPROVE',
      'REJECT',
      'APPROVE',
    ])
    expect(draft.current.value.transitionKeys).toEqual([keys[0], keys[1], keys[3]])
  })

  it('moves a transition past another state’s, among its own state’s only', () => {
    const draft = useWorkflowDraft(definition())
    const keys = draft.current.value.transitionKeys

    draft.moveTransition(3, -1)

    // Index 2 is out of DONE, so REVIEW's last edge swaps with REVIEW's second.
    expect(draft.definition.value.transitions.map((transition) => transition.action)).toEqual([
      'APPROVE',
      'APPROVE',
      'CANCEL',
      'REJECT',
    ])
    expect(draft.current.value.transitionKeys).toEqual([keys[0], keys[3], keys[2], keys[1]])

    // The first edge out of a state has nothing above it to swap with.
    draft.moveTransition(0, -1)
    expect(draft.current.value.transitionKeys).toEqual([keys[0], keys[3], keys[2], keys[1]])
  })

  it('adds a state with an unused code, and a transition out of a state', () => {
    const draft = useWorkflowDraft(definition())

    draft.addState('final')
    draft.addState('final')
    draft.addTransition('REVIEW')

    const after = draft.definition.value

    expect(after.states.slice(-2).map((state) => state.code)).toEqual(['DONE_2', 'DONE_3'])
    expect(after.transitions[after.transitions.length - 1]).toEqual({
      from: 'REVIEW',
      to: 'DONE',
      action: 'APPROVE',
      allowedBy: { assigneeType: 'ROLE', roleCode: '' },
    })
    expect(new Set(draft.current.value.stateKeys).size).toBe(5)
  })

  it('starts afresh on load, with nothing to undo', () => {
    const draft = useWorkflowDraft(definition())

    draft.setRoot('name', 'Edited')
    draft.load(definition())

    expect(draft.canUndo.value).toBe(false)
    expect(draft.isDirty.value).toBe(false)
  })

  // --- The test-engineer campaign, 2026-10-10 ----------------------------------

  it('renames only the state when another state already has its old code', () => {
    const start = definition()

    start.states[2] = { ...start.states[2], code: 'REVIEW' }

    const draft = useWorkflowDraft(start)

    draft.renameState(0, 'MANAGER_REVIEW')

    const after = draft.definition.value

    // Two states were REVIEW, so which one the edges and the initial state
    // meant is the author's call, and none of them moves.
    expect(after.states.map((state) => state.code)).toEqual(['MANAGER_REVIEW', 'DONE', 'REVIEW'])
    expect(after.initialState).toBe('REVIEW')
    expect(after.transitions).toEqual(start.transitions)
  })

  it('records an edit to the field an undo just restored as a step of its own', async () => {
    const draft = useWorkflowDraft(definition())

    draft.setRoot('name', 'P')
    draft.setRoot('name', 'Pu')
    await draft.undo()
    draft.setRoot('name', 'X')

    expect(draft.canUndo.value).toBe(true)

    await draft.undo()

    expect(draft.definition.value.name).toBe('Purchase approval')
  })
})
