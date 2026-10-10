import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { afterAll, afterEach, beforeAll, describe, expect, it } from 'vitest'

import { tabStops } from '@/lib/testing/keyboard'
import type { JwssDefinition } from '@/types/workflow'

import WorkflowGraph from './WorkflowGraph.vue'

/**
 * The read-only graph with **the real Vue Flow** (#687, **D-95** A): the
 * test-engineer campaign over PR #719.
 *
 * `WorkflowGraph.spec.ts` replaces `@vue-flow/*` with recorders, so it can say
 * what the component hands Vue Flow and nothing about what Vue Flow 1.48.2
 * then puts in the document. **What a keyboard and a screen reader meet is
 * the second**, so this file mounts the library itself. jsdom lays nothing
 * out, so three browser APIs Vue Flow measures with are given fixed answers:
 * `ResizeObserver` observes nothing, `getBBox` answers a label's box, and
 * every element is a node's size. Nothing here is about where things are
 * drawn; that is `workflowGraphLayout.spec.ts`'s, and the real browser's.
 */

const restore: (() => void)[] = []

function replace<T extends object>(target: T, key: string, descriptor: PropertyDescriptor): void {
  const before = Object.getOwnPropertyDescriptor(target, key)

  Object.defineProperty(target, key, { configurable: true, ...descriptor })
  restore.push(() =>
    before ? Object.defineProperty(target, key, before) : Reflect.deleteProperty(target, key),
  )
}

beforeAll(() => {
  replace(globalThis, 'ResizeObserver', {
    writable: true,
    value: class {
      observe(): void {}
      unobserve(): void {}
      disconnect(): void {}
    },
  })
  replace(SVGElement.prototype, 'getBBox', {
    writable: true,
    value: () => ({ x: 0, y: 0, width: 48, height: 14 }),
  })
  replace(HTMLElement.prototype, 'offsetWidth', { get: () => 200 })
  replace(HTMLElement.prototype, 'offsetHeight', { get: () => 64 })
})

afterAll(() => {
  restore.reverse().forEach((undo) => undo())
})

function definition(): JwssDefinition {
  return {
    workflowKey: 'purchase_approval',
    version: '1.0.0',
    name: 'Purchase approval',
    initialState: 'MANAGER',
    states: [
      { code: 'MANAGER', name: 'Manager approval', mapsToDocumentStatus: 'PENDING_APPROVAL' },
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
      { from: 'MANAGER', to: 'REJECTED', action: 'REJECT' },
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

describe('WorkflowGraph, drawn by Vue Flow itself', () => {
  let wrapper: VueWrapper | null = null

  afterEach(() => {
    wrapper?.unmount()
    wrapper = null
    document.body.innerHTML = ''
  })

  async function render(given: JwssDefinition = definition()): Promise<VueWrapper> {
    const before = document.createElement('button')

    before.textContent = 'Before the graph'
    document.body.append(before)

    wrapper = mount(WorkflowGraph, { props: { definition: given }, attachTo: document.body })
    await flushPromises()
    await flushPromises()

    return wrapper
  }

  function figure(): HTMLElement {
    return document.querySelector<HTMLElement>('[data-testid="workflow-graph"]')!
  }

  describe('keyboard focus order', () => {
    it('stops Tab on zoom in, zoom out and fit, in that order, and on no state or transition', async () => {
      await render()

      expect(tabStops(figure()).map((stop) => stop.getAttribute('aria-label'))).toEqual([
        'Zoom in',
        'Zoom out',
        'Fit the workflow to the view',
      ])
      expect(figure().querySelectorAll('.vue-flow__node')).toHaveLength(3)
      expect(figure().querySelectorAll('.vue-flow__edge')).toHaveLength(3)
    })

    it('reaches zoom in first from the control before the graph', async () => {
      await render()

      const stops = tabStops()

      expect(stops[stops.findIndex((stop) => stop.textContent === 'Before the graph') + 1]).toBe(
        figure().querySelector('[aria-label="Zoom in"]'),
      )
    })
  })

  describe('what a screen reader is told', () => {
    it('names every state with its marks', async () => {
      await render()

      expect(
        [...figure().querySelectorAll('.vue-flow__node')].map((node) =>
          node.getAttribute('aria-label'),
        ),
      ).toEqual([
        'Manager approval (MANAGER), initial state',
        'Done (DONE), final state',
        'Rejected (REJECTED), final state',
      ])
    })

    // With no `ariaLabel`, Vue Flow 1.48.2's EdgeWrapper falls back to "Edge
    // from ${source} to ${target}" with the graph's internal ids, and gives the
    // non-focusable edge `role="img"`, whose children are presentational: the
    // drawn label ("Approve · if…") was hidden and "Edge from state-0 to
    // state-1" read instead (the #719 campaign). The name says what is drawn,
    // in words: the marker "if…" read aloud is punctuation.
    it('names every transition by its label and its states, not by Vue Flow’s ids', async () => {
      await render()

      const names = [...figure().querySelectorAll('.vue-flow__edge')].map((edge) =>
        edge.getAttribute('aria-label'),
      )

      names.forEach((name) => expect(name).not.toMatch(/state-\d|transition-\d/))
      expect(names).toEqual([
        'Approve, from Manager approval to Done, if a condition holds',
        'Approve, from Manager approval to Rejected, otherwise, when no condition holds',
        'Reject, from Manager approval to Rejected',
      ])
    })

    // Vue Flow described each node, by `aria-describedby`, as something to
    // "select", "move around" with the arrows and "remove" with Delete, until
    // `disable-keyboard-a11y` was set (the #719 campaign). None of it is true
    // of a read-only graph.
    it('describes no node as something to move or delete', async () => {
      await render()

      const descriptions = [...figure().querySelectorAll('.vue-flow__node')].map((node) => {
        const id = node.getAttribute('aria-describedby')

        return id ? (document.getElementById(id)?.textContent ?? '') : ''
      })

      descriptions.forEach((text) => expect(text).not.toMatch(/move|delete|remove|select/i))
    })
  })

  describe('a draft whatever it holds', () => {
    it('draws codes with quotes, markup and other scripts, each in a node of its own', async () => {
      const codes = ['QUOTE"D', "APOS'TROPHE", '<b>BOLD</b>', '审批', 'موافقة', '✅ DONE', '']
      const given: JwssDefinition = {
        ...definition(),
        initialState: codes[0],
        states: codes.map((code) => ({
          code,
          name: code,
          mapsToDocumentStatus: 'IN_REVIEW',
        })),
        transitions: codes
          .slice(1)
          .map((code, at) => ({ from: codes[at], to: code, action: 'APPROVE' as const })),
      }

      await render(given)

      expect(figure().querySelectorAll('.vue-flow__node')).toHaveLength(codes.length)
      expect(figure().querySelectorAll('.vue-flow__edge')).toHaveLength(codes.length - 1)
      expect(figure().querySelector('b')).toBeNull()
      expect(
        [...figure().querySelectorAll('[data-testid^="graph-node-"]')].map((node) =>
          node.getAttribute('data-testid'),
        ),
      ).toEqual(codes.map((code) => `graph-node-${code}`))
    })
  })

  describe('it never writes', () => {
    it('lets no key, click or double click on a state, a transition or the pane change anything', async () => {
      const given = deepFreeze(definition())
      const page = await render(given)
      const before = figure().querySelector('.vue-flow__nodes')!.innerHTML
      const targets = [
        ...figure().querySelectorAll<HTMLElement>('.vue-flow__node'),
        ...figure().querySelectorAll<SVGElement>('.vue-flow__edge'),
        figure().querySelector<HTMLElement>('.vue-flow__pane')!,
        figure().querySelector<HTMLElement>('.vue-flow')!,
      ]

      for (const target of targets) {
        for (const key of [
          'Delete',
          'Backspace',
          'Enter',
          ' ',
          'ArrowUp',
          'ArrowRight',
          'Escape',
        ]) {
          target.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true }))
          document.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true }))
        }

        target.dispatchEvent(new MouseEvent('click', { bubbles: true }))
        target.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }))
      }

      await flushPromises()

      expect(given).toEqual(definition())
      expect(page.emitted()).not.toHaveProperty('update:definition')
      expect(
        Object.keys(page.emitted()).filter((name) => !/^(click|dblclick|keydown)$/.test(name)),
      ).toEqual([])
      expect(figure().querySelectorAll('.vue-flow__node')).toHaveLength(3)
      expect(figure().querySelectorAll('.vue-flow__edge')).toHaveLength(3)
      expect(figure().querySelector('.vue-flow__nodes')!.innerHTML).toBe(before)
      expect(
        figure().querySelectorAll('.vue-flow__node.selected, .vue-flow__edge.selected'),
      ).toHaveLength(0)
    })
  })
})
