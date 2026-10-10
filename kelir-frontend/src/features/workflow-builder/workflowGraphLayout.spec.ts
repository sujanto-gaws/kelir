import { describe, expect, it } from 'vitest'

import type { JwssDefinition } from '@/types/workflow'

import { starterDefinition } from './jwssRegistry'
import { workflowGraphOf } from './workflowGraphMapping'
import {
  layoutWorkflowGraph,
  NODE_HEIGHT,
  NODE_WIDTH,
  pathThrough,
  type Point,
} from './workflowGraphLayout'

/**
 * Where the graph's states and transitions are drawn (#687, **D-95** A).
 *
 * **What is asserted is what a reader relies on**: every state has a place,
 * no two overlap, the path reads top to bottom from the initial state, every
 * transition runs from its state to its target with its label on it, and the
 * same definition lays out the same way, which is what lets the layout be
 * computed on every load instead of stored. dagre's exact coordinates are
 * dagre's, and are not pinned.
 */

function chain(): JwssDefinition {
  const approve = { assigneeType: 'ROLE' as const, roleCode: 'APPROVER' }

  return {
    workflowKey: 'chain',
    version: '1.0.0',
    name: 'Chain',
    initialState: 'FIRST',
    states: [
      {
        code: 'FIRST',
        name: 'First',
        mapsToDocumentStatus: 'PENDING_APPROVAL',
        task: { taskDefinitionKey: 'first', taskName: 'First', assignment: approve },
      },
      {
        code: 'SECOND',
        name: 'Second',
        mapsToDocumentStatus: 'IN_REVIEW',
        task: { taskDefinitionKey: 'second', taskName: 'Second', assignment: approve },
      },
      { code: 'DONE', name: 'Done', mapsToDocumentStatus: 'COMPLETED', isFinal: true },
    ],
    transitions: [
      { from: 'FIRST', to: 'SECOND', action: 'APPROVE' },
      { from: 'SECOND', to: 'DONE', action: 'APPROVE' },
      // A cycle: the layout must still rank FIRST above SECOND.
      { from: 'SECOND', to: 'FIRST', action: 'RETURN' },
      { from: 'FIRST', to: 'FIRST', action: 'RETURN' },
      { from: 'SECOND', to: 'NOWHERE', action: 'AUTO' },
    ],
  }
}

function overlaps(a: Point, b: Point): boolean {
  return Math.abs(a.x - b.x) < NODE_WIDTH && Math.abs(a.y - b.y) < NODE_HEIGHT
}

/** Whether a point lies on a node's border or inside it, given its top-left corner. */
function onOrIn(point: Point, corner: Point): boolean {
  const slack = 0.5

  return (
    point.x >= corner.x - slack &&
    point.x <= corner.x + NODE_WIDTH + slack &&
    point.y >= corner.y - slack &&
    point.y <= corner.y + NODE_HEIGHT + slack
  )
}

/** Whether a point lies on the line through `points`, within half a pixel. */
function onRoute(point: Point, points: readonly Point[]): boolean {
  return points.slice(1).some((to, at) => {
    const from = points[at]
    const length = Math.hypot(to.x - from.x, to.y - from.y)

    if (length === 0) {
      return Math.hypot(point.x - from.x, point.y - from.y) < 0.5
    }

    const along =
      ((point.x - from.x) * (to.x - from.x) + (point.y - from.y) * (to.y - from.y)) / length ** 2
    const nearest = {
      x: from.x + (to.x - from.x) * Math.min(1, Math.max(0, along)),
      y: from.y + (to.y - from.y) * Math.min(1, Math.max(0, along)),
    }

    return Math.hypot(point.x - nearest.x, point.y - nearest.y) < 0.5
  })
}

describe('layoutWorkflowGraph', () => {
  describe('the states', () => {
    it('places every node, an undeclared one included, at a finite point', () => {
      const graph = workflowGraphOf(chain())
      const { nodes } = layoutWorkflowGraph(graph)

      expect([...nodes.keys()].sort()).toEqual(graph.nodes.map((node) => node.id).sort())

      for (const point of nodes.values()) {
        expect(Number.isFinite(point.x) && Number.isFinite(point.y)).toBe(true)
      }
    })

    it('places no two states over each other', () => {
      const points = [...layoutWorkflowGraph(workflowGraphOf(chain())).nodes.values()]

      for (let a = 0; a < points.length; a += 1) {
        for (let b = a + 1; b < points.length; b += 1) {
          expect(overlaps(points[a], points[b]), `nodes ${a} and ${b}`).toBe(false)
        }
      }
    })

    it('reads top to bottom along the path, a return edge notwithstanding', () => {
      const graph = workflowGraphOf(chain())
      const { nodes } = layoutWorkflowGraph(graph)
      const y = (code: string) => nodes.get(graph.nodes.find((node) => node.code === code)!.id)!.y

      expect(y('FIRST')).toBeLessThan(y('SECOND'))
      expect(y('SECOND')).toBeLessThan(y('DONE'))
    })

    it('answers top-left corners, not centres, inside the margin', () => {
      const { nodes } = layoutWorkflowGraph(workflowGraphOf(starterDefinition()))
      const xs = [...nodes.values()].map((point) => point.x)
      const ys = [...nodes.values()].map((point) => point.y)

      // dagre's margin is 16 and its centres are half a node in from the corner,
      // so a corner at the margin is the corner, and a centre would be 116 / 48.
      expect(Math.min(...xs)).toBe(16)
      expect(Math.min(...ys)).toBe(16)
    })

    it('lays out a definition with one state, and with none', () => {
      const one = workflowGraphOf({
        ...chain(),
        states: [chain().states[2]],
        initialState: 'DONE',
        transitions: [],
      })
      const none = workflowGraphOf({ ...chain(), initialState: '', states: [], transitions: [] })

      expect([...layoutWorkflowGraph(one).nodes.values()]).toEqual([{ x: 16, y: 16 }])
      expect(layoutWorkflowGraph(none).nodes.size).toBe(0)
      expect(layoutWorkflowGraph(none).edges.size).toBe(0)
    })
  })

  describe('the transitions', () => {
    it('routes every edge from its source state to its target state', () => {
      const graph = workflowGraphOf(chain())
      const { nodes, edges } = layoutWorkflowGraph(graph)

      expect([...edges.keys()]).toEqual(graph.edges.map((edge) => edge.id))

      for (const edge of graph.edges) {
        const { points } = edges.get(edge.id)!

        expect(points.length, edge.id).toBeGreaterThanOrEqual(2)
        expect(onOrIn(points[0], nodes.get(edge.source)!), `${edge.id} starts at its source`).toBe(
          true,
        )
        expect(
          onOrIn(points[points.length - 1], nodes.get(edge.target)!),
          `${edge.id} ends at its target`,
        ).toBe(true)
      }
    })

    it('puts each label on its own edge, clear of every state', () => {
      const graph = workflowGraphOf(chain())
      const { nodes, edges } = layoutWorkflowGraph(graph)

      for (const edge of graph.edges) {
        const { label, points } = edges.get(edge.id)!

        expect(onRoute(label, points), `${edge.id}'s label is on its line`).toBe(true)

        for (const corner of nodes.values()) {
          expect(onOrIn(label, corner), `${edge.id}'s label`).toBe(false)
        }
      }
    })

    it('loops a transition from a state to itself off its right side, label outside', () => {
      const graph = workflowGraphOf(chain())
      const { nodes, edges } = layoutWorkflowGraph(graph)
      const loop = graph.edges.find((edge) => edge.source === edge.target)!
      const corner = nodes.get(loop.source)!
      const { points, label } = edges.get(loop.id)!
      const right = corner.x + NODE_WIDTH

      expect(points[0].x).toBe(right)
      expect(points[points.length - 1].x).toBe(right)
      // Out from the upper half, back into the lower half: the arrow points in.
      expect(points[0].y).toBeLessThan(points[points.length - 1].y)
      expect(points.every((point) => point.x >= right)).toBe(true)
      expect(label.x).toBeGreaterThan(right)
      expect(label.y).toBe(corner.y + NODE_HEIGHT / 2)
    })

    it('gives two transitions between the same states two routes and two labels', () => {
      const definition = chain()

      definition.transitions = [
        { from: 'FIRST', to: 'SECOND', action: 'APPROVE', condition: { '==': [1, 1] } },
        { from: 'FIRST', to: 'SECOND', action: 'APPROVE' },
      ]

      const graph = workflowGraphOf(definition)
      const { edges } = layoutWorkflowGraph(graph)
      const [a, b] = graph.edges.map((edge) => edges.get(edge.id)!)

      expect(a.label).not.toEqual(b.label)
    })

    it('leaves room for each label when several transitions join the same two states', () => {
      // A multigraph keeps every parallel edge, each with its label's width, so
      // three labels between a pair push the pair aside; one edge does not.
      const between = (count: number) => {
        const definition = chain()

        definition.transitions = Array.from({ length: count }, (_, at) => ({
          from: 'FIRST',
          to: 'SECOND',
          action: 'APPROVE' as const,
          condition: { '==': [{ var: 'formData.route' }, at] },
        }))

        const graph = workflowGraphOf(definition)

        return layoutWorkflowGraph(graph).nodes.get(graph.nodes[0].id)!.x
      }

      expect(between(3)).toBeGreaterThan(between(1))
    })
  })

  it('lays out the same definition the same way every time, so nothing needs storing', () => {
    const first = layoutWorkflowGraph(workflowGraphOf(chain()))
    const second = layoutWorkflowGraph(workflowGraphOf(chain()))

    expect(second).toEqual(first)
  })

  it('leaves the graph it was given as it was', () => {
    const graph = workflowGraphOf(chain())
    const before = JSON.parse(JSON.stringify(graph))

    layoutWorkflowGraph(graph)

    expect(graph).toEqual(before)
  })
})

describe('pathThrough', () => {
  it('draws nothing for no points', () => {
    expect(pathThrough([])).toBe('')
  })

  it('runs straight between two points, ending on the last, where the arrowhead goes', () => {
    expect(
      pathThrough([
        { x: 0, y: 0 },
        { x: 0, y: 100 },
      ]),
    ).toBe('M 0 0 L 0 100')
  })

  it('rounds a bend off, turning a fixed distance either side of it', () => {
    expect(
      pathThrough([
        { x: 0, y: 0 },
        { x: 0, y: 100 },
        { x: 100, y: 100 },
      ]),
    ).toBe('M 0 0 L 0 88 Q 0 100 12 100 L 100 100')
  })

  it('turns no further out than half a short segment', () => {
    expect(
      pathThrough([
        { x: 0, y: 0 },
        { x: 0, y: 10 },
        { x: 10, y: 10 },
      ]),
    ).toBe('M 0 0 L 0 5 Q 0 10 5 10 L 10 10')
  })

  it('keeps a bend that doubles back on itself finite', () => {
    const path = pathThrough([
      { x: 5, y: 5 },
      { x: 5, y: 5 },
      { x: 5, y: 5 },
    ])

    expect(path).not.toMatch(/NaN|Infinity/)
  })
})
