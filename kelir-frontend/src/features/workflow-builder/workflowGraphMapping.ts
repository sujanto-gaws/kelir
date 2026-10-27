import type { TransitionAction } from '@/types/workflow'

import { TRANSITION_ACTIONS } from './jwssRegistry'

/**
 * A workflow definition as a graph (#687, **D-95** A): a node is a state, and
 * an edge is a transition.
 *
 * **This is a reading of the definition, never a second model of it.** It takes
 * the JWSS document as it is and returns plain data; nothing here is written
 * back, and no position, size or layout is part of it. Where the nodes are
 * drawn is `workflowGraphLayout`'s answer, computed again on every load, so a
 * layout has nowhere to be stored and `jwss-meta-v1.0.0.json` stays unchanged.
 *
 * **Branching is sibling transitions, not a gateway** (JWSS §1.1, S7). Two
 * transitions out of one state with one action and disjoint conditions are two
 * edges from the same node, each marked conditional; the one among them with no
 * condition is the fallback, taken last whatever its position. No node is added
 * for the decision itself, because the definition has none.
 *
 * **A definition being edited can be wrong**, and the graph still draws it:
 * the server's verdict is the only verdict (#426 AC3), and a graph that
 * refused to draw a draft would hide the mistake it could show. A state code a
 * transition or the initial state names, and no state declares, becomes a node
 * marked as not declared, so the edge still has both ends. **The graph declines
 * on size alone**, above `WORKFLOW_GRAPH_CEILING` (#721), never on validity.
 */

/** One node: a declared state, or a code something names that no state declares. */
export interface WorkflowGraphNode {
  /** Unique in the graph. A declared state's is its position, since codes may repeat in a draft. */
  id: string
  code: string
  /** The state's display name; for an undeclared code, the code itself. */
  name: string
  /** Whether a state in the definition declares this code. */
  declared: boolean
  /** Whether a submitted document enters the workflow here (`initialState`). */
  initial: boolean
  /** Whether the state ends the workflow (`isFinal`). An undeclared code is never final. */
  final: boolean
  /** The task a state generates, by its name, or `null` for a state with none. */
  taskName: string | null
  /** The state's position in `states`, or `null` for an undeclared code. */
  stateIndex: number | null
}

/** One edge: one transition, by its position in `transitions`. */
export interface WorkflowGraphEdge {
  /** Unique in the graph, from the transition's position. */
  id: string
  transitionIndex: number
  source: string
  target: string
  action: TransitionAction
  /** The action's label alone, as the list names it. */
  actionLabel: string
  /** What the edge says: the action's label, and the condition marker when there is one. */
  label: string
  /** Whether the transition carries a `condition` (JWSS §6). */
  conditional: boolean
  /**
   * Whether this is S7's fallback: no condition, while a sibling with the same
   * `from` and `action` has one. It is evaluated last, whatever its order.
   */
  fallback: boolean
}

export interface WorkflowGraph {
  nodes: WorkflowGraphNode[]
  edges: WorkflowGraphEdge[]
}

/** The marker a conditional edge's label carries. */
export const CONDITION_MARKER = 'if…'

/** The marker S7's fallback edge carries. */
export const FALLBACK_MARKER = 'otherwise'

/**
 * Whether a transition carries a `condition`: whether the key is there, as
 * the server reads it. **`null` is a condition**, not its absence: S7's
 * fallback is the sibling that *omits* `condition`, `workflow/domain/jwss.rs`
 * counts a fallback only where the key is missing, the engine keeps `null` as
 * a condition, and the meta-schema refuses it as not an object. A draft
 * holding one is wrong, and the graph draws it as the server reads it.
 * `undefined` is absent: JSON has no such value, so it never reaches the
 * server.
 */
function hasCondition(transition: { condition?: unknown }): boolean {
  return transition.condition !== undefined
}

function labelOf(action: TransitionAction): string {
  // A stored action outside the vocabulary still reads, as its own code.
  return TRANSITION_ACTIONS[action]?.label ?? action
}

/**
 * The parts of a definition the graph reads, and nothing else: a full
 * `JwssDefinition` is one, and so is what `drawnPartsOf` keeps of it.
 */
export interface WorkflowGraphSource {
  initialState?: string
  states?: {
    code: string
    name?: string
    isFinal?: boolean
    task?: { taskName: string; taskDefinitionKey: string }
  }[]
  transitions?: { from: string; to: string; action: TransitionAction; condition?: unknown }[]
}

/**
 * What of a definition the graph draws, copied out of it: the initial state,
 * each state's code, name, finality and task name, and each transition's ends,
 * action and whether it has a condition (not the condition itself, which is
 * drawn only as a marker).
 *
 * **The graph is laid out again only when this changes** (#719). The draft is
 * a new object on every edit, the name's and the description's included, and
 * a layout grows faster than the definition; `JSON.stringify` of this is the
 * key the graph is derived from, so an edit the graph does not draw neither
 * lays it out again nor fits the view.
 */
export function drawnPartsOf(definition: WorkflowGraphSource): WorkflowGraphSource {
  return {
    initialState: definition.initialState,
    states: (definition.states ?? []).map((state) => ({
      code: state.code,
      name: state.name,
      isFinal: state.isFinal,
      task: state.task
        ? { taskName: state.task.taskName, taskDefinitionKey: state.task.taskDefinitionKey }
        : undefined,
    })),
    transitions: (definition.transitions ?? []).map((transition) => ({
      from: transition.from,
      to: transition.to,
      action: transition.action,
      condition: hasCondition(transition) ? true : undefined,
    })),
  }
}

/** The graph a definition draws. Pure: the definition is read, never changed. */
export function workflowGraphOf(definition: WorkflowGraphSource): WorkflowGraph {
  const states = definition.states ?? []
  const transitions = definition.transitions ?? []

  const nodes: WorkflowGraphNode[] = []
  /** A code's node: the first state declaring it, as the engine would find it. */
  const nodeOf = new Map<string, string>()

  states.forEach((state, index) => {
    const id = `state-${index}`

    nodes.push({
      id,
      code: state.code,
      name: state.name || state.code,
      declared: true,
      initial: state.code === definition.initialState,
      final: state.isFinal === true,
      taskName: state.task ? state.task.taskName || state.task.taskDefinitionKey : null,
      stateIndex: index,
    })

    if (!nodeOf.has(state.code)) {
      nodeOf.set(state.code, id)
    }
  })

  // Only the first state with a code is where a document enters; a second
  // with the same code is drawn, and is not the initial one.
  for (const node of nodes) {
    node.initial = node.initial && nodeOf.get(node.code) === node.id
  }

  function nodeFor(code: string, initial = false): string {
    const known = nodeOf.get(code)

    if (known !== undefined) {
      return known
    }

    const id = `undeclared-${code}`

    nodes.push({
      id,
      code,
      name: code,
      declared: false,
      initial,
      final: false,
      taskName: null,
      stateIndex: null,
    })
    nodeOf.set(code, id)

    return id
  }

  if (definition.initialState && !nodeOf.has(definition.initialState)) {
    nodeFor(definition.initialState, true)
  }

  /** Whether a sibling — same `from`, same `action` — carries a condition. */
  const branched = new Set(
    transitions
      .filter((transition) => hasCondition(transition))
      .map((transition) => `${transition.from}\u0000${transition.action}`),
  )

  const edges = transitions.map((transition, index): WorkflowGraphEdge => {
    const conditional = hasCondition(transition)
    const fallback = !conditional && branched.has(`${transition.from}\u0000${transition.action}`)
    const marker = conditional ? CONDITION_MARKER : fallback ? FALLBACK_MARKER : null
    const action = labelOf(transition.action)

    return {
      id: `transition-${index}`,
      transitionIndex: index,
      source: nodeFor(transition.from),
      target: nodeFor(transition.to),
      action: transition.action,
      actionLabel: action,
      label: marker ? `${action} · ${marker}` : action,
      conditional,
      fallback,
    }
  })

  return { nodes, edges }
}

/**
 * The largest workflow the graph is drawn for (#721): above **either** number
 * the *Graph* tab draws nothing and says so, and the list is where the workflow
 * is read. The product owner's choice, 2026-10-26: whichever is reached first.
 *
 * **Why there is a ceiling at all.** dagre lays out on the main thread
 * ([ADR-0046] §3.3), and its time grows faster than the definition. In
 * Chromium on the laptop (#772) 80 states took 1.9 s, and the larger sizes
 * measured held the tab far longer; above this the picture is not worth the
 * wait, and the list says everything it would.
 *
 * **Checked before the graph's chunk is fetched**: the editor reads it, not
 * `WorkflowGraph.vue`, so a declined draft imports neither dagre nor Vue Flow.
 * That is why it lives here, in a module that imports neither.
 *
 * [ADR-0046]: ../../../../docs/architectures/adr/0046.%20The%20Builders%20Drag%20with%20Vue%20Draggable%20Plus%20and%20Draw%20with%20Vue%20Flow%20and%20Dagre,%20Off%20the%20First-Load%20Path.md
 */
export const WORKFLOW_GRAPH_CEILING = { states: 80, transitions: 240 } as const

/** How big a definition's graph is: what the caption says, and what the ceiling is checked on. */
export interface WorkflowGraphCount {
  /** Every node the graph draws: the declared states and the codes not declared. */
  states: number
  /** The states the definition declares, two sharing a code being two. */
  declared: number
  /** The codes the initial state or a transition names and no state declares. */
  undeclared: number
  transitions: number
}

/**
 * The size of the graph `workflowGraphOf` would draw, without drawing it: one
 * node per declared state, one per code not declared, one edge per transition.
 *
 * **The caption and the decline both read this**, so they cannot count
 * differently. It reads the same codes `workflowGraphOf` does: a blank initial
 * state names nothing, and every transition's two ends name a code, whatever
 * they hold.
 */
export function workflowGraphCountOf(definition: WorkflowGraphSource): WorkflowGraphCount {
  const states = definition.states ?? []
  const transitions = definition.transitions ?? []
  const declared = new Set(states.map((state) => state.code))
  const undeclared = new Set<string>()

  function names(code: string): void {
    if (!declared.has(code)) {
      undeclared.add(code)
    }
  }

  if (definition.initialState) {
    names(definition.initialState)
  }

  for (const transition of transitions) {
    names(transition.from)
    names(transition.to)
  }

  return {
    states: states.length + undeclared.size,
    declared: states.length,
    undeclared: undeclared.size,
    transitions: transitions.length,
  }
}

/** Whether a graph of this size is declined: **above** the ceiling on either count, each alone. */
export function isAboveGraphCeiling(count: WorkflowGraphCount): boolean {
  return (
    count.states > WORKFLOW_GRAPH_CEILING.states ||
    count.transitions > WORKFLOW_GRAPH_CEILING.transitions
  )
}
