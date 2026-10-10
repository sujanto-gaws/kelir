import { describe, expect, it } from 'vitest'

import type { JwssDefinition, JwssTransition, TransitionAction } from '@/types/workflow'

import { layoutWorkflowGraph, NODE_HEIGHT, NODE_WIDTH, type Point } from './workflowGraphLayout'
import {
  CONDITION_MARKER,
  FALLBACK_MARKER,
  workflowGraphOf,
  type WorkflowGraph,
} from './workflowGraphMapping'

/**
 * The graph of a definition at its edges (#687, **D-95** A): the test-engineer
 * campaign over PR #719's mapping and layout, taken together.
 *
 * The builders' specs hold each module to its own contract. **This file feeds
 * both the definitions a draft can actually hold and a well-formed spec
 * rarely writes**: none of a state, one final state alone, a return to the
 * initial state, two identical transitions, `AUTO`, conditions of unusual
 * JSON, codes in any script or shaped like the graph's own ids, and a
 * definition of thirty states. Each test reads the graph *and* where it is
 * drawn, because a mapping that is right can still lay out wrong.
 *
 * A draft is drawn whatever it holds (#426 AC3: the server's verdict is the
 * only verdict), so nothing here is refused for breaking `^[A-Z][A-Z0-9_]*$`.
 */

function definition(
  codes: string[],
  initialState: string,
  transitions: [from: string, to: string, action: TransitionAction][],
): JwssDefinition {
  return {
    workflowKey: 'purchase_approval',
    version: '1.0.0',
    name: 'Purchase approval',
    initialState,
    states: codes.map((code) => ({
      code,
      name: `State ${code}`,
      mapsToDocumentStatus: 'PENDING_APPROVAL',
    })),
    transitions: transitions.map(([from, to, action]) => ({ from, to, action })),
  }
}

/** The graph, and where each node and edge is drawn. */
function drawn(given: JwssDefinition) {
  const graph = workflowGraphOf(given)

  return { graph, layout: layoutWorkflowGraph(graph) }
}

/** A node's id by its code; the first node with that code. */
function idOf(graph: WorkflowGraph, code: string): string {
  const node = graph.nodes.find((candidate) => candidate.code === code)

  if (!node) {
    throw new Error(`no node has code ${JSON.stringify(code)}`)
  }

  return node.id
}

/** Whether a point is on or inside a node's box, a pixel either way. */
function touches(point: Point, corner: Point): boolean {
  return (
    point.x >= corner.x - 1 &&
    point.x <= corner.x + NODE_WIDTH + 1 &&
    point.y >= corner.y - 1 &&
    point.y <= corner.y + NODE_HEIGHT + 1
  )
}

function apart(a: Point, b: Point): boolean {
  return Math.abs(a.x - b.x) >= NODE_WIDTH || Math.abs(a.y - b.y) >= NODE_HEIGHT
}

/** Every edge leaves its source's box and arrives at its target's. */
function expectEveryEdgeJoinsItsStates(given: ReturnType<typeof drawn>): void {
  for (const edge of given.graph.edges) {
    const route = given.layout.edges.get(edge.id)!
    const first = route.points[0]
    const last = route.points[route.points.length - 1]

    expect(route.points.length, edge.id).toBeGreaterThanOrEqual(2)
    expect(
      touches(first, given.layout.nodes.get(edge.source)!),
      `${edge.id} leaves its source`,
    ).toBe(true)
    expect(
      touches(last, given.layout.nodes.get(edge.target)!),
      `${edge.id} reaches its target`,
    ).toBe(true)
  }
}

function expectNoTwoNodesOverlap(given: ReturnType<typeof drawn>): void {
  const corners = [...given.layout.nodes.values()]

  corners.forEach((a, i) =>
    corners.slice(i + 1).forEach((b) => expect(apart(a, b), JSON.stringify([a, b])).toBe(true)),
  )
}

/** Every object and array in a value frozen, so a write anywhere in it throws. */
function deepFreeze<T>(value: T): T {
  if (value !== null && typeof value === 'object') {
    Object.values(value).forEach(deepFreeze)
    Object.freeze(value)
  }

  return value
}

describe('a workflow graph of an unusual definition', () => {
  describe('few states', () => {
    it('draws a definition with no states as its initial code alone, undeclared and laid out', () => {
      const { graph, layout } = drawn(definition([], 'DRAFT', []))

      expect(graph.nodes).toEqual([
        expect.objectContaining({ code: 'DRAFT', declared: false, initial: true, final: false }),
      ])
      expect(graph.edges).toEqual([])
      expect(Number.isFinite(layout.nodes.get(graph.nodes[0].id)!.x)).toBe(true)
    })

    it('draws a definition of one final state as one node marked Start and End, with no edge', () => {
      const only = definition(['DONE'], 'DONE', [])

      only.states[0].isFinal = true

      const { graph, layout } = drawn(only)

      expect(graph.nodes).toEqual([
        expect.objectContaining({ code: 'DONE', declared: true, initial: true, final: true }),
      ])
      expect(graph.edges).toEqual([])
      expect(layout.nodes.size).toBe(1)
      expect(layout.edges.size).toBe(0)
    })

    it('draws a lone final state that is not the initial one as End only', () => {
      const only = definition(['DONE'], 'START', [])

      only.states[0].isFinal = true

      const { graph } = drawn(only)
      const done = graph.nodes.find((node) => node.code === 'DONE')!

      expect(done).toMatchObject({ initial: false, final: true, declared: true })
      expect(graph.nodes.filter((node) => node.initial).map((node) => node.code)).toEqual(['START'])
    })
  })

  describe('a cycle back to the initial state', () => {
    const cycle: [string, string, TransitionAction][] = [
      ['START', 'REVIEW', 'SUBMIT'],
      ['REVIEW', 'DONE', 'APPROVE'],
      ['REVIEW', 'START', 'RETURN'],
    ]

    it('routes the return and the way forward as two edges, each joining its own two states', () => {
      const given = drawn(definition(['START', 'REVIEW', 'DONE'], 'START', cycle))

      expect(given.graph.edges.map((edge) => [edge.source, edge.target])).toEqual([
        [idOf(given.graph, 'START'), idOf(given.graph, 'REVIEW')],
        [idOf(given.graph, 'REVIEW'), idOf(given.graph, 'DONE')],
        [idOf(given.graph, 'REVIEW'), idOf(given.graph, 'START')],
      ])
      expectEveryEdgeJoinsItsStates(given)
      expectNoTwoNodesOverlap(given)
    })

    it('draws the initial state at the top when it is declared first', () => {
      const { graph, layout } = drawn(definition(['START', 'REVIEW', 'DONE'], 'START', cycle))
      const y = (code: string) => layout.nodes.get(idOf(graph, code))!.y

      expect(y('START')).toBeLessThan(y('REVIEW'))
      expect(y('REVIEW')).toBeLessThan(y('DONE'))
    })

    // dagre breaks a cycle by a depth-first walk from the first node it was
    // given. Given the states as declared, the initial one declared last, it
    // kept the return and reversed the way forward, drawing START on DONE's
    // rank below REVIEW (the #719 campaign measured DONE@170 REVIEW@16
    // START@170). The list's order is the author's, so a reorder made this.
    it('draws the initial state at the top when a cycle returns to it, wherever it is declared', () => {
      const { graph, layout } = drawn(definition(['DONE', 'REVIEW', 'START'], 'START', cycle))
      const y = (code: string) => layout.nodes.get(idOf(graph, code))!.y

      expect(y('START')).toBeLessThan(y('REVIEW'))
      expect(y('REVIEW')).toBeLessThan(y('DONE'))
    })

    it('lays the same definition out the same way every time, the initial state declared last', () => {
      const given = definition(['DONE', 'REVIEW', 'START'], 'START', cycle)

      expect(drawn(given).layout).toEqual(drawn(structuredClone(given)).layout)
    })
  })

  describe('two transitions with one action between the same two states', () => {
    it('draws two edges with two ids, two routes and two labels apart, neither a fallback', () => {
      const given = drawn(
        definition(['START', 'DONE'], 'START', [
          ['START', 'DONE', 'APPROVE'],
          ['START', 'DONE', 'APPROVE'],
        ]),
      )
      const [first, second] = given.graph.edges

      expect(first.id).not.toBe(second.id)
      expect([first.label, second.label]).toEqual(['Approve', 'Approve'])
      expect([first.conditional, first.fallback, second.conditional, second.fallback]).toEqual([
        false,
        false,
        false,
        false,
      ])

      const routes = [given.layout.edges.get(first.id)!, given.layout.edges.get(second.id)!]

      expect(routes[0].label).not.toEqual(routes[1].label)
      expectEveryEdgeJoinsItsStates(given)
    })

    it('marks the one with a condition and the one without as S7’s branch and fallback', () => {
      const given = definition(['START', 'DONE'], 'START', [
        ['START', 'DONE', 'APPROVE'],
        ['START', 'DONE', 'APPROVE'],
      ])

      given.transitions[1].condition = { '>': [{ var: 'formData.amount' }, 1000] }

      expect(workflowGraphOf(given).edges.map((edge) => edge.label)).toEqual([
        `Approve · ${FALLBACK_MARKER}`,
        `Approve · ${CONDITION_MARKER}`,
      ])
    })
  })

  describe('an AUTO transition', () => {
    it('is labelled Automatic and branches by S7 as a decision does', () => {
      const given = definition(['CHECK', 'SMALL', 'LARGE'], 'CHECK', [
        ['CHECK', 'LARGE', 'AUTO'],
        ['CHECK', 'SMALL', 'AUTO'],
        ['CHECK', 'CHECK', 'AUTO'],
      ])

      given.transitions[0].condition = { '>=': [{ var: 'formData.amount' }, 10000] }

      const { graph, layout } = drawn(given)

      expect(graph.edges.map((edge) => [edge.action, edge.label])).toEqual([
        ['AUTO', `Automatic · ${CONDITION_MARKER}`],
        ['AUTO', `Automatic · ${FALLBACK_MARKER}`],
        // A second unconditional AUTO is a sibling of the branch too, so the
        // server would refuse two fallbacks; the graph draws what is there.
        ['AUTO', `Automatic · ${FALLBACK_MARKER}`],
      ])
      // Adds no node for the decision itself: three states, three nodes.
      expect(graph.nodes).toHaveLength(3)
      expect(layout.edges.get(graph.edges[2].id)!.points).toHaveLength(4)
    })
  })

  describe('conditions of unusual JSON', () => {
    const shapes: [string, unknown][] = [
      ['an empty array', []],
      ['an empty object', {}],
      ['an empty string', ''],
      ['true', true],
      ['an array of rules', [{ '==': [1, 1] }, { '!': [false] }]],
      [
        'rules nested five deep, arrays in arrays',
        {
          and: [
            {
              or: [
                { '==': [{ var: 'document.status' }, 'DRAFT'] },
                { in: ['x', ['x', ['y', ['z']]]] },
              ],
            },
            { '!': { '!!': [{ var: ['formData.items', []] }] } },
          ],
        },
      ],
    ]

    it.each(shapes)(
      'marks %s as a condition, beside a fallback, and leaves it as it was',
      (_, shape) => {
        const given = definition(['START', 'YES', 'NO'], 'START', [
          ['START', 'YES', 'APPROVE'],
          ['START', 'NO', 'APPROVE'],
        ])

        given.transitions[0].condition = shape

        const frozen = deepFreeze(structuredClone(given))
        const graph = workflowGraphOf(frozen)

        expect(graph.edges.map((edge) => [edge.conditional, edge.fallback])).toEqual([
          [true, false],
          [false, true],
        ])
        expect(frozen).toEqual(given)
      },
    )

    it('reads a condition key present but undefined as none', () => {
      const given = definition(['START', 'DONE'], 'START', [['START', 'DONE', 'APPROVE']])

      ;(given.transitions[0] as JwssTransition).condition = undefined

      expect(workflowGraphOf(given).edges[0]).toMatchObject({
        conditional: false,
        fallback: false,
        label: 'Approve',
      })
    })
  })

  describe('codes in any script, and shaped like the graph’s own ids', () => {
    const odd = [
      'WITH SPACE',
      'QUOTE"D',
      "APOS'TROPHE",
      '<b>BOLD</b>',
      'ÉTAT_VALIDÉ',
      'موافقة',
      '审批',
      '✅_DONE',
      '__proto__',
      'constructor',
      'toString',
      'state-1',
      'undeclared-X',
      ' ',
    ]

    it('draws every one as a node of its own, joined to its edges, laid out apart', () => {
      const transitions = odd
        .slice(1)
        .map((code, at): [string, string, TransitionAction] => [odd[at], code, 'APPROVE'])
      const given = drawn(definition(odd, odd[0], transitions))

      expect(given.graph.nodes.map((node) => node.code)).toEqual(odd)
      expect(given.graph.nodes.every((node) => node.declared)).toBe(true)
      expect(new Set(given.graph.nodes.map((node) => node.id)).size).toBe(odd.length)
      expect(given.graph.edges.map((edge) => [edge.source, edge.target])).toEqual(
        transitions.map(([from, to]) => [idOf(given.graph, from), idOf(given.graph, to)]),
      )
      expectEveryEdgeJoinsItsStates(given)
      expectNoTwoNodesOverlap(given)
    })

    it('joins an edge to the state whose code is `state-1`, not to the node whose id is', () => {
      // The second state's node id is `state-1`; the first state's code is.
      const { graph } = drawn(
        definition(['state-1', 'OTHER'], 'OTHER', [['OTHER', 'state-1', 'APPROVE']]),
      )
      const edge = graph.edges[0]

      expect(graph.nodes.find((node) => node.id === edge.target)!.code).toBe('state-1')
      expect(graph.nodes.find((node) => node.id === edge.source)!.code).toBe('OTHER')
    })

    it('keeps an undeclared code shaped like a declared node’s id apart from that node', () => {
      const { graph } = drawn(definition(['A', 'B'], 'A', [['A', 'state-0', 'APPROVE']]))
      const target = graph.nodes.find((node) => node.id === graph.edges[0].target)!

      expect(target).toMatchObject({ code: 'state-0', declared: false })
      expect(graph.nodes).toHaveLength(3)
    })

    it('does not merge codes that differ only by Unicode form, case or a trailing space', () => {
      const composed = 'VALIDÉ'
      const decomposed = 'VALIDÉ'
      const { graph } = drawn(
        definition([composed, 'Draft', 'A'], 'A', [
          ['A', decomposed, 'APPROVE'],
          ['A', 'DRAFT', 'RETURN'],
          ['A', 'A ', 'REJECT'],
        ]),
      )

      // The engine compares codes as stored, so the graph does too: each
      // look-alike is a code no state declares, drawn as such.
      expect(
        graph.edges.map((edge) => {
          const target = graph.nodes.find((node) => node.id === edge.target)!

          return [target.code, target.declared]
        }),
      ).toEqual([
        [decomposed, false],
        ['DRAFT', false],
        ['A ', false],
      ])
      expect(graph.nodes).toHaveLength(6)
    })

    it('labels an action named like an Object member by its own code', () => {
      const given = definition(['A', 'B'], 'A', [['A', 'B', 'APPROVE']])

      given.transitions.push({
        from: 'A',
        to: 'B',
        action: 'constructor' as TransitionAction,
      })

      expect(workflowGraphOf(given).edges.map((edge) => edge.label)).toEqual([
        'Approve',
        'constructor',
      ])
    })
  })

  describe('a large definition', () => {
    /**
     * Thirty approval steps, each approving forward, returning to the start
     * and rejecting to one final state: 31 states and 87 transitions, the long
     * return edges being what makes a layered layout work.
     */
    function large(): JwssDefinition {
      const steps = Array.from({ length: 30 }, (_, at) => `STEP_${at}`)
      const transitions: [string, string, TransitionAction][] = []

      steps.forEach((code, at) => {
        if (at + 1 < steps.length) {
          transitions.push([code, steps[at + 1], 'APPROVE'])
        }

        if (at > 0) {
          transitions.push([code, steps[0], 'RETURN'], [code, 'REJECTED', 'REJECT'])
        }
      })

      const given = definition([...steps, 'REJECTED'], steps[0], transitions)

      given.states[given.states.length - 1].isFinal = true

      return given
    }

    it('lays out every state apart and joins every edge to its states', () => {
      const given = drawn(large())

      expect(given.graph.nodes).toHaveLength(31)
      expect(given.graph.edges).toHaveLength(87)
      expectNoTwoNodesOverlap(given)
      expectEveryEdgeJoinsItsStates(given)
    })

    it('lays out the same way from a fresh copy, so nothing about it needs storing', () => {
      const once = drawn(large()).layout
      const again = drawn(JSON.parse(JSON.stringify(large())) as JwssDefinition).layout

      expect([...again.nodes]).toEqual([...once.nodes])
      expect([...again.edges]).toEqual([...once.edges])
    })

    it('keeps the start on top and the steps in order down the page', () => {
      const { graph, layout } = drawn(large())
      const ys = Array.from(
        { length: 30 },
        (_, at) => layout.nodes.get(idOf(graph, `STEP_${at}`))!.y,
      )

      ys.slice(1).forEach((y, at) => expect(y).toBeGreaterThan(ys[at]))
    })
  })
})
