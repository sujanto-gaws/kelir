import { computed, nextTick, ref, shallowRef, type ComputedRef, type ShallowRef } from 'vue'

import type { JwssDefinition, JwssState, JwssTransition } from '@/types/workflow'

import { STATE_KINDS, type StateKind } from './jwssRegistry'

/**
 * The definition being edited, with its undo history (#426).
 *
 * **Borrowed from kopiflowvue's store (D-95), and kept page-local**: one editor
 * owns one draft, so it is a composable rather than a Pinia store (coding
 * standard §3.3). Its conventions:
 *
 * - **Nothing is mutated.** Every edit builds a new definition, replacing the
 *   arrays and objects on the path it changed and sharing the rest. So a
 *   snapshot is the object itself, never a clone; undo is a pointer move; and a
 *   transition's `condition` the user did not touch is the same object after an
 *   edit elsewhere — which is what keeps the logic builder from reloading and
 *   dropping the operands somebody has not filled in yet.
 * - **History lives outside reactivity.** The stacks are plain arrays; only
 *   their lengths are reactive, so an edit does not walk fifty definitions.
 * - **An `isRestoring` guard** makes a write that lands while an undo or redo
 *   is settling — a child re-emitting what it was just handed — apply without
 *   being recorded, so it cannot wipe the redo stack.
 * - **Rows have stable keys that are not in the JWSS.** A state or transition
 *   keeps its key across edits, removals and moves, so its component is not
 *   remounted under the user; the keys never reach the wire.
 */

export interface DraftSnapshot {
  definition: JwssDefinition
  stateKeys: readonly string[]
  transitionKeys: readonly string[]
}

export interface WorkflowDraft {
  current: ShallowRef<DraftSnapshot>
  definition: ComputedRef<JwssDefinition>
  /** Whether the definition differs from the one last loaded or saved. */
  isDirty: ComputedRef<boolean>
  canUndo: ComputedRef<boolean>
  canRedo: ComputedRef<boolean>
  /** Starts afresh from a definition: what the server holds, or a new workflow's starter. */
  load(definition: JwssDefinition): void
  undo(): Promise<void>
  redo(): Promise<void>
  /** Sets a root field. `coalesce` merges consecutive edits of one field into one undo step. */
  setRoot<Key extends 'name' | 'description' | 'workflowKey' | 'initialState'>(
    key: Key,
    value: JwssDefinition[Key],
  ): void
  replaceState(index: number, state: JwssState, coalesce?: string): void
  /**
   * Renames a state, and moves the initial state and every transition that
   * named the old code to the new one — unless another state already has the
   * new code, or another had the old one, where whichever was meant is the
   * author's call and nothing else is touched.
   */
  renameState(index: number, code: string): void
  addState(kind: StateKind): void
  /** Removes a state and the transitions out of it, which are drawn under it. */
  removeState(index: number): void
  moveState(index: number, direction: -1 | 1): void
  replaceTransition(index: number, transition: JwssTransition, coalesce?: string): void
  addTransition(from: string): void
  removeTransition(index: number): void
  /** Swaps a transition with the previous or next one out of the same state: S7's order. */
  moveTransition(index: number, direction: -1 | 1): void
}

/** How many steps undo reaches back. */
export const HISTORY_LIMIT = 50

let keySeed = 0

function nextKey(): string {
  keySeed += 1

  return `row-${keySeed}`
}

/** Without `key`: an optional property set to `undefined` is left out, not written as absent-but-present. */
export function withoutUndefined<T extends object>(value: T): T {
  return Object.fromEntries(Object.entries(value).filter(([, field]) => field !== undefined)) as T
}

function unusedCode(definition: JwssDefinition, stem: string): string {
  const taken = new Set(definition.states.map((state) => state.code))

  for (let n = 1; ; n += 1) {
    const candidate = n === 1 ? stem : `${stem}_${n}`

    if (!taken.has(candidate)) {
      return candidate
    }
  }
}

const STEMS: Record<StateKind, string> = { task: 'NEW_APPROVAL', wait: 'WAITING', final: 'DONE' }

function swap<T>(items: readonly T[], a: number, b: number): T[] {
  const next = items.slice()

  next[a] = items[b]
  next[b] = items[a]

  return next
}

export function useWorkflowDraft(initial: JwssDefinition): WorkflowDraft {
  const current = shallowRef<DraftSnapshot>(snapshotOf(initial))
  const baseline = shallowRef<JwssDefinition>(initial)

  let past: DraftSnapshot[] = []
  let future: DraftSnapshot[] = []
  let coalescing: string | null = null
  let isRestoring = false

  const pastLength = ref(0)
  const futureLength = ref(0)

  function snapshotOf(definition: JwssDefinition): DraftSnapshot {
    return {
      definition,
      stateKeys: definition.states.map(() => nextKey()),
      transitionKeys: definition.transitions.map(() => nextKey()),
    }
  }

  function syncLengths(): void {
    pastLength.value = past.length
    futureLength.value = future.length
  }

  function commit(next: DraftSnapshot, coalesce?: string): void {
    if (isRestoring) {
      current.value = next

      return
    }

    if (coalesce === undefined || coalesce !== coalescing) {
      past.push(current.value)

      if (past.length > HISTORY_LIMIT) {
        past = past.slice(past.length - HISTORY_LIMIT)
      }
    }

    coalescing = coalesce ?? null
    future = []
    current.value = next
    syncLengths()
  }

  function withDefinition(
    definition: JwssDefinition,
    keys: Partial<Pick<DraftSnapshot, 'stateKeys' | 'transitionKeys'>> = {},
  ): DraftSnapshot {
    return { ...current.value, ...keys, definition }
  }

  function load(definition: JwssDefinition): void {
    past = []
    future = []
    coalescing = null
    baseline.value = definition
    current.value = snapshotOf(definition)
    syncLengths()
  }

  async function restore(from: DraftSnapshot[], to: DraftSnapshot[]): Promise<void> {
    const target = from.pop()

    if (!target) {
      return
    }

    to.push(current.value)
    coalescing = null
    isRestoring = true
    current.value = target
    syncLengths()

    try {
      await nextTick()
    } finally {
      isRestoring = false
    }
  }

  const definition = computed(() => current.value.definition)

  function setRoot<Key extends 'name' | 'description' | 'workflowKey' | 'initialState'>(
    key: Key,
    value: JwssDefinition[Key],
  ): void {
    commit(withDefinition(withoutUndefined({ ...definition.value, [key]: value })), `root:${key}`)
  }

  function replaceState(index: number, state: JwssState, coalesce?: string): void {
    commit(
      withDefinition({
        ...definition.value,
        states: definition.value.states.map((existing, at) => (at === index ? state : existing)),
      }),
      coalesce,
    )
  }

  function renameState(index: number, code: string): void {
    const states = definition.value.states
    const old = states[index]?.code

    if (old === undefined || old === code) {
      return
    }

    const unambiguous = !states.some(
      (state, at) => at !== index && (state.code === code || state.code === old),
    )

    const follow = (reference: string): string =>
      unambiguous && reference === old ? code : reference

    commit(
      withDefinition({
        ...definition.value,
        initialState: follow(definition.value.initialState),
        states: states.map((state, at) => (at === index ? { ...state, code } : state)),
        transitions: definition.value.transitions.map((transition) =>
          unambiguous && (transition.from === old || transition.to === old)
            ? { ...transition, from: follow(transition.from), to: follow(transition.to) }
            : transition,
        ),
      }),
    )
  }

  function addState(kind: StateKind): void {
    const code = unusedCode(definition.value, STEMS[kind])

    commit(
      withDefinition(
        {
          ...definition.value,
          states: [...definition.value.states, STATE_KINDS[kind].create(code)],
        },
        { stateKeys: [...current.value.stateKeys, nextKey()] },
      ),
    )
  }

  function removeState(index: number): void {
    const removed = definition.value.states[index]

    if (!removed) {
      return
    }

    const keep = definition.value.transitions.map((transition) => transition.from !== removed.code)

    commit(
      withDefinition(
        {
          ...definition.value,
          states: definition.value.states.filter((_, at) => at !== index),
          transitions: definition.value.transitions.filter((_, at) => keep[at]),
        },
        {
          stateKeys: current.value.stateKeys.filter((_, at) => at !== index),
          transitionKeys: current.value.transitionKeys.filter((_, at) => keep[at]),
        },
      ),
    )
  }

  function moveState(index: number, direction: -1 | 1): void {
    const target = index + direction

    if (target < 0 || target >= definition.value.states.length) {
      return
    }

    commit(
      withDefinition(
        { ...definition.value, states: swap(definition.value.states, index, target) },
        { stateKeys: swap(current.value.stateKeys, index, target) },
      ),
    )
  }

  function replaceTransition(index: number, transition: JwssTransition, coalesce?: string): void {
    commit(
      withDefinition({
        ...definition.value,
        transitions: definition.value.transitions.map((existing, at) =>
          at === index ? transition : existing,
        ),
      }),
      coalesce,
    )
  }

  function addTransition(from: string): void {
    const to = definition.value.states.find((state) => state.code !== from)?.code ?? from

    commit(
      withDefinition(
        {
          ...definition.value,
          transitions: [
            ...definition.value.transitions,
            { from, to, action: 'APPROVE', allowedBy: { assigneeType: 'ROLE', roleCode: '' } },
          ],
        },
        { transitionKeys: [...current.value.transitionKeys, nextKey()] },
      ),
    )
  }

  function removeTransition(index: number): void {
    commit(
      withDefinition(
        {
          ...definition.value,
          transitions: definition.value.transitions.filter((_, at) => at !== index),
        },
        { transitionKeys: current.value.transitionKeys.filter((_, at) => at !== index) },
      ),
    )
  }

  function moveTransition(index: number, direction: -1 | 1): void {
    const transitions = definition.value.transitions
    const from = transitions[index]?.from

    // The neighbour out of the same state: order between two states' edges
    // means nothing, and order among one state's is what S7 evaluates by.
    let target = index + direction

    while (target >= 0 && target < transitions.length && transitions[target].from !== from) {
      target += direction
    }

    if (from === undefined || target < 0 || target >= transitions.length) {
      return
    }

    commit(
      withDefinition(
        { ...definition.value, transitions: swap(transitions, index, target) },
        { transitionKeys: swap(current.value.transitionKeys, index, target) },
      ),
    )
  }

  return {
    current,
    definition,
    isDirty: computed(() => definition.value !== baseline.value),
    canUndo: computed(() => pastLength.value > 0),
    canRedo: computed(() => futureLength.value > 0),
    load,
    undo: () => restore(past, future),
    redo: () => restore(future, past),
    setRoot,
    replaceState,
    renameState,
    addState,
    removeState,
    moveState,
    replaceTransition,
    addTransition,
    removeTransition,
    moveTransition,
  }
}
