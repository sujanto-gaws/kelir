import dagre from '@dagrejs/dagre'

import type { WorkflowGraph } from './workflowGraphMapping'

/**
 * Where each state of a workflow graph is drawn (#687, **D-95** A).
 *
 * **Computed on every load and never stored.** D-95 chose a graph laid out
 * automatically over one whose positions an author drags and saves: the JWSS
 * has no place for a position (`jwss-meta-v1.0.0.json` is
 * `additionalProperties: false`), and a layout kept beside it would be a second
 * document to keep in step. So the same definition always lays out the same
 * way, and nothing here is written anywhere.
 *
 * **dagre's layered layout, top to bottom** ([ADR-0046] §3.3 A). A definition
 * is a directed graph with cycles — a `RETURN` goes back to an earlier state —
 * and a layered layout ranks states by their transitions, reversing a cycle's
 * edges internally to rank it, which reads as an approval path reads.
 *
 * **Imported only by `WorkflowGraph.vue`**, which the editor reaches with
 * `defineAsyncComponent`, so dagre never ships on the first-load path
 * (`npm run check:bundle`).
 *
 * [ADR-0046]: ../../../../docs/architectures/adr/0046.%20The%20Builders%20Drag%20with%20Vue%20Draggable%20Plus%20and%20Draw%20with%20Vue%20Flow%20and%20Dagre,%20Off%20the%20First-Load%20Path.md
 */

/** A node's drawn size, which the layout spaces by. The node component is drawn to it. */
export const NODE_WIDTH = 200
export const NODE_HEIGHT = 64

/** Roughly a label character's width at the edge label's size, to leave room for it. */
const LABEL_CHAR_WIDTH = 7
const LABEL_HEIGHT = 18

export interface Point {
  x: number
  y: number
}

/** Where one transition is drawn: the line through `points`, and its label at `label`. */
export interface EdgeRoute {
  points: Point[]
  label: Point
}

export interface WorkflowGraphLayout {
  /**
   * The top-left corner of every node, by its id: what Vue Flow positions by.
   * dagre answers centres, so each is moved up and left by half the node.
   */
  nodes: Map<string, Point>
  /**
   * Every edge's route, by its id, in the same coordinates as the nodes.
   *
   * **The edges are drawn along dagre's routes, not Vue Flow's own curves.**
   * Vue Flow would join each pair of handles and put the label at the curve's
   * middle, so a return edge loops round its target and two labels between
   * the same ranks land on each other. dagre has already ranked the labels as
   * nodes of their own, and spaced the graph to fit them.
   */
  edges: Map<string, EdgeRoute>
}

export function layoutWorkflowGraph(graph: WorkflowGraph): WorkflowGraphLayout {
  // A multigraph, so two transitions between the same pair of states are two
  // edges, each with its label's room, rather than one overwriting the other.
  const layout = new dagre.graphlib.Graph({ multigraph: true })

  layout.setGraph({
    rankdir: 'TB',
    nodesep: 48,
    ranksep: 72,
    edgesep: 24,
    marginx: 16,
    marginy: 16,
  })
  layout.setDefaultEdgeLabel(() => ({}))

  for (const node of graph.nodes) {
    layout.setNode(node.id, { width: NODE_WIDTH, height: NODE_HEIGHT })
  }

  for (const edge of graph.edges) {
    layout.setEdge(
      edge.source,
      edge.target,
      { width: edge.label.length * LABEL_CHAR_WIDTH, height: LABEL_HEIGHT, labelpos: 'c' },
      edge.id,
    )
  }

  dagre.layout(layout)

  return {
    nodes: new Map(
      graph.nodes.map((node) => {
        const { x, y } = layout.node(node.id)

        return [node.id, { x: x - NODE_WIDTH / 2, y: y - NODE_HEIGHT / 2 }]
      }),
    ),
    edges: new Map(
      graph.edges.map((edge) => {
        const route = layout.edge({ v: edge.source, w: edge.target, name: edge.id })
        const label = { x: route.x ?? 0, y: route.y ?? 0 }

        return [
          edge.id,
          edge.source === edge.target
            ? loopBeside(layout.node(edge.source), label)
            : {
                points: (route.points ?? []).map((point: Point) => ({ x: point.x, y: point.y })),
                label,
              },
        ]
      }),
    ),
  }
}

/** Half the height of a transition from a state to itself. */
const LOOP = 14

/**
 * A transition from a state to itself, as a loop off the state's right side.
 *
 * **dagre's own points for a self-edge are not on its node** (3.1.1 answers a
 * route that starts beyond it and doubles back), so only the room it kept for
 * the label is used: the loop runs out to the label and back, and the label
 * sits on its far side.
 */
function loopBeside(centre: Point, label: Point): EdgeRoute {
  const side = centre.x + NODE_WIDTH / 2
  const far = Math.max(label.x, side + 2 * LOOP)

  return {
    points: [
      { x: side, y: centre.y - LOOP },
      { x: far, y: centre.y - LOOP },
      { x: far, y: centre.y + LOOP },
      { x: side, y: centre.y + LOOP },
    ],
    label: { x: far, y: centre.y },
  }
}

/** How far before and after a bend the line starts to turn. */
const CORNER = 12

/** The point `distance` from `from` towards `to`, never past the middle of the segment. */
function towards(from: Point, to: Point, distance: number): Point {
  const length = Math.hypot(to.x - from.x, to.y - from.y)
  const step = length === 0 ? 0 : Math.min(distance, length / 2) / length

  return { x: from.x + (to.x - from.x) * step, y: from.y + (to.y - from.y) * step }
}

/**
 * An SVG path along a route's points: straight between them, with each bend
 * rounded off, and straight into the last point, where the arrowhead goes.
 * The line passes beside every bend, so a label dagre placed on one sits on
 * its line.
 */
export function pathThrough(points: readonly Point[]): string {
  if (points.length === 0) {
    return ''
  }

  const steps = [`M ${points[0].x} ${points[0].y}`]

  for (let at = 1; at < points.length - 1; at += 1) {
    const bend = points[at]
    const into = towards(bend, points[at - 1], CORNER)
    const out = towards(bend, points[at + 1], CORNER)

    steps.push(`L ${into.x} ${into.y}`, `Q ${bend.x} ${bend.y} ${out.x} ${out.y}`)
  }

  const last = points[points.length - 1]

  steps.push(`L ${last.x} ${last.y}`)

  return steps.join(' ')
}
