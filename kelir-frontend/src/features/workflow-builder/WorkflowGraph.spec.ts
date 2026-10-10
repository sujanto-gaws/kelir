import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { press, tabTo } from '@/lib/testing/keyboard'
import type { JwssDefinition } from '@/types/workflow'

import WorkflowGraph from './WorkflowGraph.vue'
import { pathThrough } from './workflowGraphLayout'

/**
 * The read-only graph (#687, **D-95** A).
 *
 * **What this file can see is what the component hands Vue Flow, not the
 * drawing.** Vue Flow measures its nodes with a `ResizeObserver` and pans with
 * d3, and jsdom lays out nothing, so `@vue-flow/core` and `@vue-flow/controls`
 * are replaced by components that record their props and render their slots,
 * as `DocumentStatusChart.spec.ts` replaces Unovis. These tests are about what
 * *this* component decides: that every state is a node at a laid-out place,
 * that the view cannot edit, that it lays out again when the definition
 * changes, and that zoom and fit are reachable by keyboard.
 *
 * Whether Vue Flow stays off first load is `scripts/check-bundle-split.mjs`'s,
 * against the build.
 */

const flow = vi.hoisted(() => ({
  fitView: vi.fn(),
  zoomIn: vi.fn(),
  zoomOut: vi.fn(),
}))

/**
 * A stub that records its props and renders `render`'s tree. Its `name` is what
 * a test finds it by; the stubs are built through this one helper so each is a
 * line rather than a component definition of its own.
 */
async function recorder(
  name: string,
  props: string[],
  render: (props: Record<string, unknown>, slots: import('vue').Slots) => import('vue').VNode,
  inheritAttrs = true,
) {
  const { defineComponent } = await import('vue')

  return defineComponent({
    name,
    inheritAttrs,
    props,
    setup:
      (given, { slots }) =>
      () =>
        render(given as Record<string, unknown>, slots),
  })
}

vi.mock('@vue-flow/core', async () => {
  const { h } = await import('vue')

  return {
    MarkerType: { ArrowClosed: 'arrowclosed' },
    Position: { Top: 'top', Bottom: 'bottom' },
    useVueFlow: () => flow,
    Handle: await recorder('FlowHandle', ['type', 'position', 'connectable'], () => h('span')),
    BaseEdge: await recorder(
      'BaseEdge',
      ['id', 'path', 'markerEnd', 'label', 'labelX', 'labelY'],
      (given) => h('span', { 'data-edge': given.id }),
    ),
    // Renders each node's and each edge's slot, as Vue Flow does, with the
    // props it hands them: an edge's marker arrives as a `url(#…)` string.
    VueFlow: await recorder(
      'VueFlow',
      ['nodes', 'edges'],
      (given, slots) =>
        h('div', { 'data-stub': 'VueFlow' }, [
          ...(given.nodes as { id: string; data: unknown }[]).map((node) =>
            h('div', { key: node.id }, slots['node-state']?.({ data: node.data })),
          ),
          ...(given.edges as { id: string; type: string; data: unknown }[]).map((edge) =>
            h(
              'div',
              { key: edge.id },
              slots[`edge-${edge.type}`]?.({
                id: edge.id,
                data: edge.data,
                markerEnd: `url(#marker-${edge.id})`,
              }),
            ),
          ),
          slots.default?.(),
        ]),
      false,
    ),
  }
})

vi.mock('@vue-flow/controls', async () => {
  const { h } = await import('vue')

  return {
    Controls: await recorder(
      'FlowControls',
      ['showZoom', 'showFitView', 'showInteractive'],
      (_given, slots) => h('div', slots.default?.()),
    ),
    ControlButton: await recorder('ControlButton', [], (_given, slots) =>
      h('button', { type: 'button', class: 'vue-flow__controls-button' }, slots.default?.()),
    ),
  }
})

interface FlowNode {
  id: string
  type: string
  position: { x: number; y: number }
  ariaLabel: string
  data: { code: string; initial: boolean; final: boolean; declared: boolean }
}

interface FlowEdge {
  id: string
  type: string
  source: string
  target: string
  markerEnd: string
  class: string
  data: {
    label: string
    route: { points: { x: number; y: number }[]; label: { x: number; y: number } }
  }
}

function definition(): JwssDefinition {
  const approve = { assigneeType: 'ROLE' as const, roleCode: 'APPROVER' }

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
        task: { taskDefinitionKey: 'manager', taskName: 'Approve it', assignment: approve },
      },
      { code: 'DONE', name: 'Done', mapsToDocumentStatus: 'COMPLETED', isFinal: true },
      { code: 'REJECTED', name: 'Rejected', mapsToDocumentStatus: 'REJECTED', isFinal: true },
    ],
    transitions: [
      {
        from: 'MANAGER',
        to: 'DONE',
        action: 'APPROVE',
        condition: { '>': [{ var: 'formData.amount' }, 10] },
      },
      { from: 'MANAGER', to: 'REJECTED', action: 'APPROVE' },
      { from: 'MANAGER', to: 'ARCHIVE', action: 'REJECT' },
    ],
  }
}

/** Every object and array in a value frozen, so a write anywhere in it throws. */
function deepFreeze<T>(value: T): T {
  if (value !== null && typeof value === 'object') {
    Object.values(value).forEach(deepFreeze)
    Object.freeze(value)
  }

  return value
}

describe('WorkflowGraph', () => {
  let wrapper: VueWrapper | null

  beforeEach(() => {
    wrapper = null
    flow.fitView.mockClear()
    flow.zoomIn.mockClear()
    flow.zoomOut.mockClear()
  })

  afterEach(() => {
    wrapper?.unmount()
  })

  function render(given: JwssDefinition = definition(), attach = false): VueWrapper {
    wrapper = mount(WorkflowGraph, {
      props: { definition: given },
      attachTo: attach ? document.body : undefined,
    })

    return wrapper
  }

  function flowAttrs(page: VueWrapper): Record<string, unknown> {
    return page.findComponent({ name: 'VueFlow' }).vm.$attrs as Record<string, unknown>
  }

  function nodes(page: VueWrapper): FlowNode[] {
    return page.findComponent({ name: 'VueFlow' }).props('nodes') as FlowNode[]
  }

  function edges(page: VueWrapper): FlowEdge[] {
    return page.findComponent({ name: 'VueFlow' }).props('edges') as FlowEdge[]
  }

  function spread(given: FlowNode[]): number {
    return new Set(given.map((node) => `${node.position.x},${node.position.y}`)).size
  }

  describe('what it hands Vue Flow', () => {
    it('gives every state, and the undeclared code, a node of its own at a laid-out place', () => {
      const given = nodes(render())

      expect(given.map((node) => node.data.code)).toEqual([
        'MANAGER',
        'DONE',
        'REJECTED',
        'ARCHIVE',
      ])
      expect(given.every((node) => node.type === 'state')).toBe(true)
      // Not all at the origin: the layout ran, and placed them apart.
      expect(spread(given)).toBe(4)
    })

    it('gives every transition an edge, with its label, an arrowhead and its condition dashed', () => {
      const given = edges(render())
      const code = new Map(nodes(wrapper!).map((node) => [node.id, node.data.code]))

      expect(
        given.map((edge) => [code.get(edge.source), code.get(edge.target), edge.data.label]),
      ).toEqual([
        ['MANAGER', 'DONE', 'Approve · if…'],
        ['MANAGER', 'REJECTED', 'Approve · otherwise'],
        ['MANAGER', 'ARCHIVE', 'Reject'],
      ])
      expect(given.every((edge) => edge.type === 'transition')).toBe(true)
      expect(given.every((edge) => edge.markerEnd === 'arrowclosed')).toBe(true)
      expect(
        given.map((edge) => edge.class.split(' ').includes('workflow-graph-edge--conditional')),
      ).toEqual([true, false, false])
    })

    it('draws each edge along its laid-out route, with its label where the layout put it', () => {
      const page = render()
      const drawn = page.findAllComponents({ name: 'BaseEdge' })

      expect(drawn).toHaveLength(3)

      for (const [at, edge] of edges(page).entries()) {
        const base = drawn[at]

        expect(edge.data.route.points.length).toBeGreaterThanOrEqual(2)
        expect(base.props('id')).toBe(edge.id)
        expect(base.props('path')).toBe(pathThrough(edge.data.route.points))
        expect(base.props('label')).toBe(edge.data.label)
        expect([base.props('labelX'), base.props('labelY')]).toEqual([
          edge.data.route.label.x,
          edge.data.route.label.y,
        ])
        // The arrowhead Vue Flow resolved for the edge, not one of its own.
        expect(base.props('markerEnd')).toBe(`url(#marker-${edge.id})`)
      }
    })

    it('turns off everything that would edit: dragging, connecting, selecting, deleting', () => {
      expect(flowAttrs(render())).toMatchObject({
        'nodes-draggable': false,
        'nodes-connectable': false,
        'elements-selectable': false,
        'edges-updatable': false,
        'delete-key-code': null,
      })
    })

    it('declares no events, so nothing it does can reach the draft', () => {
      const page = render()

      expect(page.vm.$options.emits ?? []).toEqual([])
    })

    it('draws a deeply frozen definition, so it writes no layout into it', () => {
      const frozen = deepFreeze(definition())

      expect(() => render(frozen)).not.toThrow()
      expect(nodes(wrapper!)).toHaveLength(4)
    })
  })

  describe('what a reader sees on each node', () => {
    it('marks the initial state Start and the final states End, in words', () => {
      const page = render()
      const text = (code: string) => page.get(`[data-testid="graph-node-${code}"]`).text()

      expect(text('MANAGER')).toContain('Start')
      expect(text('MANAGER')).not.toContain('End')
      expect(text('DONE')).toContain('End')
      expect(text('DONE')).not.toContain('Start')
      expect(text('REJECTED')).toContain('End')
      expect(text('ARCHIVE')).not.toMatch(/Start|End/)
    })

    it('draws the initial state, a final state and an undeclared code each with its own border', () => {
      const page = render()
      const classes = (code: string) => page.get(`[data-testid="graph-node-${code}"]`).classes()

      expect(classes('MANAGER')).toContain('border-primary')
      expect(classes('MANAGER')).not.toContain('border-double')
      expect(classes('DONE')).toContain('border-double')
      expect(classes('DONE')).not.toContain('border-primary')
      expect(classes('ARCHIVE')).toContain('border-dashed')
      expect(classes('DONE')).not.toContain('border-dashed')
    })

    it('says a code no state declares is not declared', () => {
      expect(render().get('[data-testid="graph-node-ARCHIVE"]').text()).toContain(
        'ARCHIVE · not declared',
      )
    })

    it('names the task a state generates', () => {
      expect(render().get('[data-testid="graph-node-MANAGER"]').text()).toContain(
        'Task: Approve it',
      )
    })

    it('labels each node for assistive technology with its marks', () => {
      expect(nodes(render()).map((node) => node.ariaLabel)).toEqual([
        'Manager approval (MANAGER), initial state',
        'Done (DONE), final state',
        'Rejected (REJECTED), final state',
        'ARCHIVE (ARCHIVE), not declared by any state',
      ])
    })

    it('sums the graph up in its caption, and says where to edit', () => {
      expect(render().get('[data-testid="workflow-graph-summary"]').text()).toBe(
        '3 states, 3 transitions, 1 not declared. Read-only: change the workflow in the list.',
      )
    })

    it('says one state and one transition in the singular', () => {
      const one = definition()

      one.states = [one.states[0]]
      one.transitions = [{ from: 'MANAGER', to: 'MANAGER', action: 'RETURN' }]

      expect(render(one).get('[data-testid="workflow-graph-summary"]').text()).toBe(
        '1 state, 1 transition. Read-only: change the workflow in the list.',
      )
    })
  })

  describe('laid out again when the definition changes', () => {
    it('draws a state added to the definition, at a place of its own, and fits the view again', async () => {
      const page = render()

      await flushPromises()
      flow.fitView.mockClear()

      const next = definition()

      next.states.push({ code: 'FINANCE', name: 'Finance', mapsToDocumentStatus: 'IN_REVIEW' })
      next.transitions.push({ from: 'MANAGER', to: 'FINANCE', action: 'RETURN' })
      await page.setProps({ definition: next })
      await flushPromises()

      const given = nodes(page)

      expect(given.map((node) => node.data.code)).toContain('FINANCE')
      expect(spread(given)).toBe(5)
      expect(edges(page)).toHaveLength(4)
      expect(flow.fitView).toHaveBeenCalled()
    })

    it('draws a state removed from the definition as undeclared while a transition names it', async () => {
      const page = render()
      const next = definition()

      next.states = next.states.filter((state) => state.code !== 'REJECTED')
      await page.setProps({ definition: next })

      const rejected = nodes(page).find((node) => node.data.code === 'REJECTED')!

      expect(rejected.data.declared).toBe(false)
    })
  })

  describe('zoom and fit, by keyboard', () => {
    it('names each control, and Tab and Enter reach and press them', () => {
      render(definition(), true)

      tabTo((element) => element.getAttribute('aria-label') === 'Zoom in')
      press('Enter')
      expect(flow.zoomIn).toHaveBeenCalledTimes(1)

      tabTo((element) => element.getAttribute('aria-label') === 'Zoom out')
      press('Enter')
      expect(flow.zoomOut).toHaveBeenCalledTimes(1)

      flow.fitView.mockClear()
      tabTo((element) => element.getAttribute('aria-label') === 'Fit the workflow to the view')
      press('Enter')
      expect(flow.fitView).toHaveBeenCalledTimes(1)
    })

    it('offers no lock toggle: there is nothing to unlock in a read-only view', () => {
      const controls = render().findComponent({ name: 'FlowControls' })

      expect(controls.props('showInteractive')).toBe(false)
    })
  })
})
