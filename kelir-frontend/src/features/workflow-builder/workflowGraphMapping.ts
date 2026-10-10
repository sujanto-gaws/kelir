import type { JwssDefinition, TransitionAction } from '@/types/workflow'

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
 * marked as not declared, so the edge still has both ends.
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

function hasCondition(transition: { condition?: unknown }): boolean {
  return transition.condition !== undefined && transition.condition !== null
}

function labelOf(action: TransitionAction): string {
  // A stored action outside the vocabulary still reads, as its own code.
  return TRANSITION_ACTIONS[action]?.label ?? action
}

/** The graph a definition draws. Pure: the definition is read, never changed. */
export function workflowGraphOf(definition: JwssDefinition): WorkflowGraph {
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
      label: marker ? `${action} · ${marker}` : action,
      conditional,
      fallback,
    }
  })

  return { nodes, edges }
}
