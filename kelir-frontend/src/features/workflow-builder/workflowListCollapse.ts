/**
 * The editor list's threshold (#800): **above 40 states or 112 transitions**,
 * either count alone and strictly above, the list draws each state's header
 * and draws its body (its fields, its task and its transitions) only once the
 * state is opened. At or under it, the list draws everything, as it always did.
 *
 * **Set on the numbers** (#800, AC-38-1, 2026-11-09): 40 states and 112
 * transitions is the largest size measured whose editor loaded in at most 1 s
 * before the change (665 ms; 80 states took 1,491 ms). Every real draft is far
 * under it: the largest any Kelir text or test draws is 31 states.
 *
 * **Counted on the definition as written**, states and transitions as they
 * are listed, unlike `WORKFLOW_GRAPH_CEILING`, which counts what the graph
 * would draw. The list draws every row, declared or not.
 */
export const WORKFLOW_LIST_THRESHOLD = { states: 40, transitions: 112 } as const

export function isAboveListThreshold(definition: {
  readonly states: readonly unknown[]
  readonly transitions: readonly unknown[]
}): boolean {
  return (
    definition.states.length > WORKFLOW_LIST_THRESHOLD.states ||
    definition.transitions.length > WORKFLOW_LIST_THRESHOLD.transitions
  )
}
