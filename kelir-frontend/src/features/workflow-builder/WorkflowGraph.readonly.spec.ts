import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type { JwssDefinition } from '@/types/workflow'

import WorkflowGraph from './WorkflowGraph.vue'
import { layoutWorkflowGraph } from './workflowGraphLayout'

/**
 * The read-only graph against everything Vue Flow can say back (#687,
 * **D-95** A): the test-engineer campaign over PR #719.
 *
 * `WorkflowGraph.spec.ts` shows the component declares no events and turns off
 * dragging, connecting and selecting. **Neither shows what happens when Vue
 * Flow emits anyway** — a `nodes-change` that removes a state, an
 * `update:nodes` with every node moved, a `connect`. So the stub here is
 * made to emit every event the real `VueFlow` and `Controls` declare (the
 * lists are read from the installed packages, so a new event in an upgrade is
 * emitted too), with the payload an edit would carry, and the definition,
 * deeply frozen, must come out as it went in.
 *
 * **And the reader's zoom.** The component fits the view again whenever its
 * nodes are recomputed, and they are recomputed whenever the definition is a
 * new object, which the draft makes on every edit.
 */

const flow = vi.hoisted(() => ({
  fitView: vi.fn(),
  zoomIn: vi.fn(),
  zoomOut: vi.fn(),
}))

const declared = vi.hoisted(() => ({ flow: [] as string[], controls: [] as string[] }))

vi.mock('./workflowGraphLayout', async (original) => {
  const actual = await original<typeof import('./workflowGraphLayout')>()

  return { ...actual, layoutWorkflowGraph: vi.fn(actual.layoutWorkflowGraph) }
})

/**
 * A stub that renders `tag` round its default slot and declares `emits`. The
 * stubs are built through this one helper so each is a line, as
 * `WorkflowGraph.spec.ts` builds its recorders.
 */
async function stub(
  name: string,
  tag: string,
  options: { emits?: string[]; props?: string[]; inheritAttrs?: boolean } = {},
) {
  const { defineComponent, h } = await import('vue')

  return defineComponent({
    name,
    inheritAttrs: options.inheritAttrs ?? true,
    props: options.props ?? [],
    emits: options.emits ?? [],
    setup:
      (_props, { slots }) =>
      () =>
        h(tag, tag === 'button' ? { type: 'button' } : {}, slots.default?.()),
  })
}

vi.mock('@vue-flow/core', async (original) => {
  const actual = await original<typeof import('@vue-flow/core')>()

  declared.flow = [...((actual.VueFlow as { emits?: string[] }).emits ?? [])]

  return {
    MarkerType: actual.MarkerType,
    Position: actual.Position,
    useVueFlow: () => flow,
    Handle: await stub('FlowHandle', 'span'),
    BaseEdge: await stub('BaseEdge', 'span'),
    VueFlow: await stub('VueFlow', 'div', {
      inheritAttrs: false,
      props: ['nodes', 'edges'],
      emits: declared.flow,
    }),
  }
})

vi.mock('@vue-flow/controls', async (original) => {
  const actual = await original<typeof import('@vue-flow/controls')>()

  declared.controls = [...((actual.Controls as { emits?: string[] }).emits ?? [])]

  return {
    Controls: await stub('FlowControls', 'div', { emits: declared.controls }),
    ControlButton: await stub('ControlButton', 'button'),
  }
})

interface FlowNode {
  id: string
  position: { x: number; y: number }
  data: { code: string }
}

function definition(): JwssDefinition {
  return {
    workflowKey: 'purchase_approval',
    version: '1.0.0',
    name: 'Purchase approval',
    initialState: 'MANAGER',
    states: [
      { code: 'MANAGER', name: 'Manager approval', mapsToDocumentStatus: 'PENDING_APPROVAL' },
      { code: 'DONE', name: 'Done', mapsToDocumentStatus: 'COMPLETED', isFinal: true },
    ],
    transitions: [
      { from: 'MANAGER', to: 'DONE', action: 'APPROVE' },
      { from: 'DONE', to: 'MANAGER', action: 'RETURN' },
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

/** What an edit would carry on each event; anything else gets an empty object. */
function payloadOf(name: string, nodes: FlowNode[]): unknown {
  const moved = nodes.map((node) => ({ ...node, position: { x: 999, y: 999 } }))

  switch (name) {
    case 'nodesChange':
      return [
        { type: 'remove', id: nodes[0].id },
        { type: 'position', id: nodes[1].id, position: { x: 999, y: 999 }, dragging: false },
        { type: 'select', id: nodes[1].id, selected: true },
      ]
    case 'edgesChange':
      return [{ type: 'remove', id: 'transition-0' }]
    case 'update:nodes':
      return moved
    case 'update:edges':
      return []
    case 'update:modelValue':
      return [...moved]
    case 'connect':
    case 'edgeUpdate':
      return { source: nodes[1].id, target: nodes[0].id, sourceHandle: null, targetHandle: null }
    case 'nodeDragStop':
    case 'nodeDrag':
    case 'nodeDragStart':
      return { node: moved[0], nodes: moved, event: new MouseEvent('mouseup') }
    case 'nodeClick':
    case 'nodeDoubleClick':
      return { node: nodes[0], event: new MouseEvent('click') }
    case 'move':
    case 'moveEnd':
    case 'viewportChange':
    case 'viewportChangeEnd':
      return { x: 40, y: 40, zoom: 1.8 }
    default:
      return {}
  }
}

describe('WorkflowGraph, whatever Vue Flow emits', () => {
  let wrapper: VueWrapper | null

  beforeEach(() => {
    wrapper = null
    flow.fitView.mockClear()
    flow.zoomIn.mockClear()
    flow.zoomOut.mockClear()
    vi.mocked(layoutWorkflowGraph).mockClear()
  })

  afterEach(() => {
    wrapper?.unmount()
  })

  function render(given: JwssDefinition): VueWrapper {
    wrapper = mount(WorkflowGraph, { props: { definition: given } })

    return wrapper
  }

  function nodes(page: VueWrapper): FlowNode[] {
    return page.findComponent({ name: 'VueFlow' }).props('nodes') as FlowNode[]
  }

  describe('it never writes', () => {
    it('reads the events to emit from the installed Vue Flow, so none is missed', () => {
      expect(declared.flow).toEqual(
        expect.arrayContaining(['nodesChange', 'edgesChange', 'connect', 'update:nodes']),
      )
      expect(declared.flow.length).toBeGreaterThan(40)
      expect(declared.controls).toEqual(
        expect.arrayContaining(['zoomIn', 'zoomOut', 'fitView', 'interactionChange']),
      )
    })

    it('keeps the definition, its nodes and their places through every event Vue Flow declares', async () => {
      const given = deepFreeze(definition())
      const page = render(given)
      const before = structuredClone(nodes(page))
      const graph = page.findComponent({ name: 'VueFlow' })
      const controls = page.findComponent({ name: 'FlowControls' })

      for (const name of declared.flow) {
        graph.vm.$emit(name, payloadOf(name, before))
      }

      for (const name of declared.controls) {
        controls.vm.$emit(name, false)
      }

      await flushPromises()

      expect(given).toEqual(definition())
      expect(page.emitted()).toEqual({})
      expect(nodes(page)).toEqual(before)
      expect(page.findComponent({ name: 'VueFlow' }).props('edges')).toHaveLength(2)
    })

    it('answers Vue Flow’s nodes-initialized by fitting, and no other event by anything', async () => {
      const page = render(definition())
      const graph = page.findComponent({ name: 'VueFlow' })

      await flushPromises()
      flow.fitView.mockClear()

      for (const name of declared.flow.filter((event) => event !== 'nodesInitialized')) {
        graph.vm.$emit(name, payloadOf(name, nodes(page)))
      }

      await flushPromises()

      expect(flow.fitView).not.toHaveBeenCalled()
      expect(flow.zoomIn).not.toHaveBeenCalled()
      expect(flow.zoomOut).not.toHaveBeenCalled()

      graph.vm.$emit('nodesInitialized', [])

      expect(flow.fitView).toHaveBeenCalledTimes(1)
    })
  })

  describe('the reader’s zoom', () => {
    it('fits the view again when a state is added, since the drawing changed', async () => {
      const page = render(definition())

      await flushPromises()
      flow.fitView.mockClear()

      const next = definition()

      next.states.push({ code: 'FINANCE', name: 'Finance', mapsToDocumentStatus: 'IN_REVIEW' })
      await page.setProps({ definition: next })
      await flushPromises()

      expect(flow.fitView).toHaveBeenCalledTimes(1)
    })

    // The draft builds a new definition on every edit, and the graph
    // recomputed its nodes for any new object, then refitted (the #719
    // campaign). The workflow's name, key and description sit above the tabs
    // and stay editable while the graph shows, so each keystroke there threw
    // away a reader's zoom and pan though nothing drawn had changed.
    it('keeps the reader’s zoom while a field the graph does not draw is edited', async () => {
      const page = render(definition())

      await flushPromises()
      await page.get('[aria-label="Zoom in"]').trigger('click')
      flow.fitView.mockClear()

      for (const name of ['P', 'Pu', 'Pur']) {
        await page.setProps({ definition: { ...definition(), name, description: name } })
        await flushPromises()
      }

      expect(flow.zoomIn).toHaveBeenCalledTimes(1)
      expect(flow.fitView).not.toHaveBeenCalled()
    })

    // The same cause at a cost: the layout was recomputed for a change it does
    // not draw. dagre's time grows faster than the definition (measured in
    // jsdom, one layout: 20 states and 55 transitions 0.2 s, 40 and 115 0.8 s,
    // 80 and 235 3.0 s, 150 and 441 5.1 s), and JWSS sets no maximum, so typing
    // a name beside a large graph stalled the page on every keystroke. One
    // layout of a large definition is still that slow: #721.
    it('does not lay the graph out again for a change it does not draw', async () => {
      const page = render(definition())

      await flushPromises()

      const once = vi.mocked(layoutWorkflowGraph).mock.calls.length
      const undrawn: JwssDefinition[] = [
        { ...definition(), name: 'Renamed' },
        { ...definition(), workflowKey: 'renamed', version: '1.0.1', description: 'Typed' },
        { ...definition(), settings: { anything: true }, variables: [] },
      ]
      const restated = definition()

      restated.states[0].mapsToDocumentStatus = 'IN_REVIEW'
      restated.transitions[0].allowedBy = 'role:MANAGER'
      undrawn.push(restated)

      for (const next of undrawn) {
        await page.setProps({ definition: next })
        await flushPromises()
      }

      expect(vi.mocked(layoutWorkflowGraph).mock.calls.length).toBe(once)
    })

    it('lays the graph out again when a condition is added, which the graph marks', async () => {
      const page = render(definition())

      await flushPromises()

      const once = vi.mocked(layoutWorkflowGraph).mock.calls.length
      const conditioned = definition()

      conditioned.transitions[0].condition = { '>': [{ var: 'formData.amount' }, 10] }
      await page.setProps({ definition: conditioned })
      await flushPromises()

      expect(vi.mocked(layoutWorkflowGraph).mock.calls.length).toBe(once + 1)

      const reworded = definition()

      reworded.transitions[0].condition = { '>': [{ var: 'formData.amount' }, 99] }
      await page.setProps({ definition: reworded })
      await flushPromises()

      expect(vi.mocked(layoutWorkflowGraph).mock.calls.length).toBe(once + 1)
    })
  })
})
