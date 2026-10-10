import { describe, expect, it } from 'vitest'

import type { JwssDefinition, JwssTransition } from '@/types/workflow'

import { starterDefinition } from './jwssRegistry'
import {
  CONDITION_MARKER,
  drawnPartsOf,
  FALLBACK_MARKER,
  workflowGraphOf,
  type WorkflowGraph,
  type WorkflowGraphNode,
} from './workflowGraphMapping'

/**
 * The definition-to-graph mapping (#687, **D-95** A): a node is a state, an
 * edge is a transition, and nothing else.
 *
 * **The mapping is the part of the graph that can be wrong without anybody
 * seeing it.** Vue Flow draws whatever it is given and dagre places whatever
 * it is given, so a transition dropped here, an edge hung on the wrong state,
 * or a branch drawn without its marker would render as a plausible picture of
 * a different workflow. These tests pin it to the definition, one rule at a
 * time.
 */

const approve = { assigneeType: 'ROLE' as const, roleCode: 'APPROVER' }

/** Every object and array in a value frozen, so a write anywhere in it throws. */
function deepFreeze<T>(value: T): T {
  if (value !== null && typeof value === 'object') {
    for (const field of Object.values(value)) {
      deepFreeze(field)
    }

    Object.freeze(value)
  }

  return value
}

/**
 * A branching purchase approval: the manager's approve goes to finance when
 * the amount is large and straight to done otherwise (S7), finance can return
 * to the manager (a cycle), and both can reject.
 */
function purchase(): JwssDefinition {
  return {
    workflowKey: 'purchase',
    version: '1.0.0',
    name: 'Purchase',
    initialState: 'MANAGER',
    states: [
      {
        code: 'MANAGER',
        name: 'Manager approval',
        mapsToDocumentStatus: 'PENDING_APPROVAL',
        task: { taskDefinitionKey: 'manager', taskName: 'Approve', assignment: approve },
      },
      {
        code: 'FINANCE',
        name: 'Finance review',
        mapsToDocumentStatus: 'IN_REVIEW',
        task: { taskDefinitionKey: 'finance', taskName: '', assignment: approve },
      },
      { code: 'DONE', name: 'Done', mapsToDocumentStatus: 'COMPLETED', isFinal: true },
      { code: 'REJECTED', name: 'Rejected', mapsToDocumentStatus: 'REJECTED', isFinal: true },
    ],
    transitions: [
      {
        from: 'MANAGER',
        to: 'FINANCE',
        action: 'APPROVE',
        condition: { '>': [{ var: 'formData.amount' }, 1000] },
      },
      { from: 'MANAGER', to: 'DONE', action: 'APPROVE' },
      { from: 'MANAGER', to: 'REJECTED', action: 'REJECT' },
      { from: 'FINANCE', to: 'DONE', action: 'APPROVE' },
      { from: 'FINANCE', to: 'MANAGER', action: 'RETURN' },
      { from: 'FINANCE', to: 'REJECTED', action: 'REJECT' },
    ],
  }
}

function nodeByCode(graph: WorkflowGraph, code: string): WorkflowGraphNode {
  const found = graph.nodes.filter((node) => node.code === code)

  expect(found, `one node for ${code}`).toHaveLength(1)

  return found[0]
}

/** An edge's ends as state codes, so an assertion reads like the definition. */
function ends(graph: WorkflowGraph): [string, string][] {
  const code = new Map(graph.nodes.map((node) => [node.id, node.code]))

  return graph.edges.map((edge) => [code.get(edge.source)!, code.get(edge.target)!])
}

describe('workflowGraphOf', () => {
  describe('a node is a state', () => {
    it('draws every declared state as one node, in declaration order, with its name', () => {
      const graph = workflowGraphOf(purchase())

      expect(graph.nodes.map((node) => [node.code, node.name, node.stateIndex])).toEqual([
        ['MANAGER', 'Manager approval', 0],
        ['FINANCE', 'Finance review', 1],
        ['DONE', 'Done', 2],
        ['REJECTED', 'Rejected', 3],
      ])
      expect(graph.nodes.every((node) => node.declared)).toBe(true)
    })

    it('adds no node the definition does not name: no gateway, no start or end node', () => {
      // JWSS §1.1: a finite state machine, not BPMN. The branch out of MANAGER
      // is two edges from one node, not a node of its own.
      const graph = workflowGraphOf(purchase())

      expect(graph.nodes).toHaveLength(4)
    })

    it('gives every node a distinct id', () => {
      const graph = workflowGraphOf(purchase())

      expect(new Set(graph.nodes.map((node) => node.id)).size).toBe(graph.nodes.length)
    })

    it('says which task a state generates, by name, or by key when the name is blank', () => {
      const graph = workflowGraphOf(purchase())

      expect(nodeByCode(graph, 'MANAGER').taskName).toBe('Approve')
      expect(nodeByCode(graph, 'FINANCE').taskName).toBe('finance')
      expect(nodeByCode(graph, 'DONE').taskName).toBeNull()
    })

    it('names a state by its code when its name is blank', () => {
      const definition = purchase()

      definition.states[1] = { ...definition.states[1], name: '' }

      expect(nodeByCode(workflowGraphOf(definition), 'FINANCE').name).toBe('FINANCE')
    })

    it('draws a definition with no states and no transitions as an empty graph', () => {
      const graph = workflowGraphOf({
        ...purchase(),
        initialState: '',
        states: [],
        transitions: [],
      })

      expect(graph).toEqual({ nodes: [], edges: [] })
    })
  })

  describe('the initial and final states are distinguished', () => {
    it('marks the initial state, and only it', () => {
      const graph = workflowGraphOf(purchase())

      expect(graph.nodes.filter((node) => node.initial).map((node) => node.code)).toEqual([
        'MANAGER',
      ])
    })

    it('follows initialState, not the first state', () => {
      const graph = workflowGraphOf({ ...purchase(), initialState: 'FINANCE' })

      expect(nodeByCode(graph, 'FINANCE').initial).toBe(true)
      expect(nodeByCode(graph, 'MANAGER').initial).toBe(false)
    })

    it('marks every state with isFinal, and no other', () => {
      const graph = workflowGraphOf(purchase())

      expect(graph.nodes.filter((node) => node.final).map((node) => node.code)).toEqual([
        'DONE',
        'REJECTED',
      ])
    })

    it('does not call a state final because nothing leaves it', () => {
      // A wait state with no outgoing transition is a dead end the server's
      // reachability check refuses (S6); drawing it as final would hide that.
      const definition = purchase()

      definition.states.push({ code: 'PARKED', name: 'Parked', mapsToDocumentStatus: 'RETURNED' })

      expect(nodeByCode(workflowGraphOf(definition), 'PARKED').final).toBe(false)
    })

    it('reads isFinal: false as not final', () => {
      const definition = purchase()

      definition.states[2] = { ...definition.states[2], isFinal: false }

      expect(nodeByCode(workflowGraphOf(definition), 'DONE').final).toBe(false)
    })

    it('marks one state both initial and final when one state is both', () => {
      const graph = workflowGraphOf({ ...purchase(), initialState: 'DONE' })
      const done = nodeByCode(graph, 'DONE')

      expect([done.initial, done.final]).toEqual([true, true])
    })

    it('draws the starter definition with its initial state and both final states', () => {
      const graph = workflowGraphOf(starterDefinition())

      expect(graph.nodes.map((node) => [node.code, node.initial, node.final])).toEqual([
        ['PENDING_APPROVAL', true, false],
        ['COMPLETED', false, true],
        ['REJECTED', false, true],
      ])
    })
  })

  describe('an edge is a transition', () => {
    it('draws every transition as one edge, in order, between the states it names', () => {
      const graph = workflowGraphOf(purchase())

      expect(graph.edges.map((edge) => edge.transitionIndex)).toEqual([0, 1, 2, 3, 4, 5])
      expect(ends(graph)).toEqual([
        ['MANAGER', 'FINANCE'],
        ['MANAGER', 'DONE'],
        ['MANAGER', 'REJECTED'],
        ['FINANCE', 'DONE'],
        ['FINANCE', 'MANAGER'],
        ['FINANCE', 'REJECTED'],
      ])
    })

    it('gives every edge a distinct id, even two between the same states with one action', () => {
      const definition = purchase()

      definition.transitions.push({ from: 'MANAGER', to: 'DONE', action: 'APPROVE' })

      const graph = workflowGraphOf(definition)

      expect(graph.edges).toHaveLength(7)
      expect(new Set(graph.edges.map((edge) => edge.id)).size).toBe(7)
    })

    it('labels an edge with its action, as the list labels it', () => {
      const graph = workflowGraphOf(purchase())

      expect(graph.edges.map((edge) => edge.action)).toEqual([
        'APPROVE',
        'APPROVE',
        'REJECT',
        'APPROVE',
        'RETURN',
        'REJECT',
      ])
      expect(graph.edges[2].label).toBe('Reject')
      expect(graph.edges[4].label).toBe('Return')
    })

    it('labels every action in the vocabulary by its registry label', () => {
      const definition = purchase()

      definition.transitions = [
        { from: 'MANAGER', to: 'DONE', action: 'AUTO' },
        { from: 'MANAGER', to: 'DONE', action: 'RESUBMIT' },
      ]

      expect(workflowGraphOf(definition).edges.map((edge) => edge.label)).toEqual([
        'Automatic',
        'Resubmit',
      ])
    })

    it('labels an action outside the vocabulary by its own code rather than dropping it', () => {
      const definition = purchase()

      definition.transitions = [
        { from: 'MANAGER', to: 'DONE', action: 'TELEPORT' as JwssTransition['action'] },
      ]

      const [edge] = workflowGraphOf(definition).edges

      expect(edge.label).toBe('TELEPORT')
    })

    it('draws a transition from a state to itself as an edge from the node to the same node', () => {
      const definition = purchase()

      definition.transitions = [{ from: 'FINANCE', to: 'FINANCE', action: 'RETURN' }]

      const graph = workflowGraphOf(definition)

      expect(graph.edges[0].source).toBe(graph.edges[0].target)
      expect(graph.nodes).toHaveLength(4)
    })
  })

  describe('conditions are marked (S7)', () => {
    it('marks a transition with a condition, and labels it with the marker', () => {
      const [toFinance] = workflowGraphOf(purchase()).edges

      expect(toFinance.conditional).toBe(true)
      expect(toFinance.fallback).toBe(false)
      expect(toFinance.label).toBe(`Approve · ${CONDITION_MARKER}`)
    })

    it('marks the sibling with no condition as the fallback', () => {
      const [, toDone] = workflowGraphOf(purchase()).edges

      expect(toDone.conditional).toBe(false)
      expect(toDone.fallback).toBe(true)
      expect(toDone.label).toBe(`Approve · ${FALLBACK_MARKER}`)
    })

    it('marks neither on a transition with no conditional sibling', () => {
      const unbranched = workflowGraphOf(purchase()).edges.slice(2)

      expect(unbranched.map((edge) => [edge.conditional, edge.fallback, edge.label])).toEqual([
        [false, false, 'Reject'],
        [false, false, 'Approve'],
        [false, false, 'Return'],
        [false, false, 'Reject'],
      ])
    })

    it('finds the fallback whatever its position among the siblings', () => {
      // S7: the fallback is evaluated last regardless of document order.
      const definition = purchase()

      definition.transitions = [definition.transitions[1], definition.transitions[0]]

      const edges = workflowGraphOf(definition).edges

      expect(edges.map((edge) => [edge.fallback, edge.conditional])).toEqual([
        [true, false],
        [false, true],
      ])
    })

    it('counts as siblings only transitions with the same state and the same action', () => {
      const definition = purchase()

      // The same action out of another state, and another action out of the
      // same state, are not the conditional edge's siblings.
      definition.transitions = [
        { ...definition.transitions[0] },
        { from: 'FINANCE', to: 'DONE', action: 'APPROVE' },
        { from: 'MANAGER', to: 'REJECTED', action: 'REJECT' },
      ]

      const edges = workflowGraphOf(definition).edges

      expect(edges.map((edge) => edge.fallback)).toEqual([false, false, false])
    })

    it('marks every one of several conditional siblings, and has no fallback when none omits it', () => {
      const definition = purchase()

      definition.transitions = [
        { ...definition.transitions[0] },
        {
          from: 'MANAGER',
          to: 'DONE',
          action: 'APPROVE',
          condition: { '<=': [{ var: 'formData.amount' }, 1000] },
        },
      ]

      const edges = workflowGraphOf(definition).edges

      expect(edges.map((edge) => [edge.conditional, edge.fallback])).toEqual([
        [true, false],
        [true, false],
      ])
    })

    it('reads a condition that is falsy JSON Logic, such as false or 0, as a condition', () => {
      const definition = purchase()

      definition.transitions = [
        { from: 'MANAGER', to: 'FINANCE', action: 'APPROVE', condition: false },
        { from: 'MANAGER', to: 'DONE', action: 'APPROVE', condition: 0 },
      ]

      expect(workflowGraphOf(definition).edges.map((edge) => edge.conditional)).toEqual([
        true,
        true,
      ])
    })

    it('reads a condition of null as none', () => {
      const definition = purchase()

      definition.transitions = [
        { from: 'MANAGER', to: 'FINANCE', action: 'APPROVE', condition: null },
      ]

      expect(workflowGraphOf(definition).edges[0].conditional).toBe(false)
    })
  })

  describe('unknown targets are handled', () => {
    it('draws a target no state declares as a node marked not declared, and keeps the edge', () => {
      const definition = purchase()

      definition.transitions.push({ from: 'FINANCE', to: 'ARCHIVE', action: 'APPROVE' })

      const graph = workflowGraphOf(definition)
      const ghost = nodeByCode(graph, 'ARCHIVE')

      expect(ghost).toMatchObject({
        declared: false,
        initial: false,
        final: false,
        taskName: null,
        stateIndex: null,
        name: 'ARCHIVE',
      })
      expect(graph.edges).toHaveLength(7)
      expect(ends(graph)[6]).toEqual(['FINANCE', 'ARCHIVE'])
    })

    it('draws a source no state declares the same way', () => {
      const definition = purchase()

      definition.transitions.push({ from: 'LIMBO', to: 'DONE', action: 'AUTO' })

      const graph = workflowGraphOf(definition)

      expect(nodeByCode(graph, 'LIMBO').declared).toBe(false)
      expect(ends(graph)[6]).toEqual(['LIMBO', 'DONE'])
    })

    it('draws one node for an undeclared code however many transitions name it', () => {
      const definition = purchase()

      definition.transitions.push(
        { from: 'FINANCE', to: 'ARCHIVE', action: 'APPROVE' },
        { from: 'MANAGER', to: 'ARCHIVE', action: 'AUTO' },
        { from: 'ARCHIVE', to: 'DONE', action: 'AUTO' },
      )

      const graph = workflowGraphOf(definition)

      expect(graph.nodes.filter((node) => !node.declared)).toHaveLength(1)
      expect(graph.nodes).toHaveLength(5)
    })

    it('every edge has both ends among the nodes', () => {
      const definition = purchase()

      definition.transitions.push(
        { from: 'GONE', to: 'ALSO_GONE', action: 'AUTO' },
        { from: 'MANAGER', to: '', action: 'APPROVE' },
      )

      const graph = workflowGraphOf(definition)
      const ids = new Set(graph.nodes.map((node) => node.id))

      for (const edge of graph.edges) {
        expect(ids.has(edge.source), `${edge.id} source`).toBe(true)
        expect(ids.has(edge.target), `${edge.id} target`).toBe(true)
      }
    })

    it('draws an initial state no state declares as a node marked initial and not declared', () => {
      const graph = workflowGraphOf({ ...purchase(), initialState: 'START' })
      const start = nodeByCode(graph, 'START')

      expect([start.declared, start.initial]).toEqual([false, true])
      expect(graph.nodes.filter((node) => node.initial)).toHaveLength(1)
    })

    it('draws no node for a blank initial state', () => {
      const graph = workflowGraphOf({ ...purchase(), initialState: '' })

      expect(graph.nodes).toHaveLength(4)
      expect(graph.nodes.some((node) => node.initial)).toBe(false)
    })

    it('hangs an edge on the first of two states sharing a code, and draws both', () => {
      // A draft can hold a duplicate code until the server refuses it; the
      // engine would find the first, so the edges go there.
      const definition = purchase()

      definition.states.push({ ...definition.states[1], name: 'Finance, again' })

      const graph = workflowGraphOf(definition)
      const finance = graph.nodes.filter((node) => node.code === 'FINANCE')

      expect(finance.map((node) => node.name)).toEqual(['Finance review', 'Finance, again'])
      expect(graph.edges.filter((edge) => edge.target === finance[1].id)).toHaveLength(0)
      expect(graph.edges.filter((edge) => edge.target === finance[0].id)).toHaveLength(1)
    })

    it('marks only the first of two states sharing the initial code as initial', () => {
      const definition = purchase()

      definition.states.push({ ...definition.states[0], name: 'Manager, again' })

      const initial = workflowGraphOf(definition).nodes.filter((node) => node.initial)

      expect(initial.map((node) => node.name)).toEqual(['Manager approval'])
    })
  })

  describe('it reads the definition and writes nothing', () => {
    it('maps a deeply frozen definition without writing to it', () => {
      const definition = deepFreeze(purchase())

      expect(() => workflowGraphOf(definition)).not.toThrow()
    })

    it('leaves the definition equal to what it was, with no position or layout added', () => {
      const definition = purchase()
      const before = JSON.parse(JSON.stringify(definition))

      workflowGraphOf(definition)

      expect(definition).toEqual(before)
    })

    it('carries no position: where a node goes is not part of the graph', () => {
      const graph = workflowGraphOf(purchase())

      for (const node of graph.nodes) {
        expect(Object.keys(node)).not.toContain('position')
        expect(Object.keys(node)).not.toContain('x')
      }
    })

    it('maps the same definition to the same graph every time', () => {
      expect(workflowGraphOf(purchase())).toEqual(workflowGraphOf(purchase()))
    })
  })
})

describe('drawnPartsOf', () => {
  /** What the graph is derived from, as `WorkflowGraph.vue` derives it: through JSON. */
  function throughKey(definition: JwssDefinition): WorkflowGraph {
    return workflowGraphOf(JSON.parse(JSON.stringify(drawnPartsOf(definition))))
  }

  it('keeps everything the graph draws: the graph of the parts is the graph of the whole', () => {
    const odd = purchase()

    odd.initialState = 'NOWHERE'
    odd.states.push({ ...odd.states[0], name: '' })
    odd.transitions.push(
      { from: 'GHOST', to: 'DONE', action: 'AUTO' },
      { from: 'DONE', to: 'DONE', action: 'APPROVE', condition: false },
    )

    for (const definition of [purchase(), starterDefinition(), odd]) {
      expect(throughKey(definition)).toEqual(workflowGraphOf(definition))
    }
  })

  it('keeps nothing the graph does not draw', () => {
    const before = JSON.stringify(drawnPartsOf(purchase()))
    const edited = purchase()

    edited.name = 'Renamed'
    edited.workflowKey = 'renamed'
    edited.description = 'Typed'
    edited.states[0].mapsToDocumentStatus = 'IN_REVIEW'
    edited.states[0].task!.dueInHours = 48
    edited.transitions[0].condition = { '<': [{ var: 'formData.amount' }, 5] }
    edited.transitions[1].requiresComment = true

    expect(JSON.stringify(drawnPartsOf(edited))).toBe(before)
  })
})
