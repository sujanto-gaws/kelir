import { mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, describe, expect, it } from 'vitest'
import { defineComponent, h, nextTick, reactive } from 'vue'

import { press, tabToLabel, type } from '@/lib/testing/keyboard'

import JsonLogicBuilder from './JsonLogicBuilder.vue'
import type { LogicTier, LogicVariable } from './logicTree'

/**
 * The visual JSON Logic builder, by what it shows and what it emits (#686).
 *
 * No production screen mounts it yet — the form builder is row 11 and the
 * workflow editor row 6 — so **the two hosts are these tests**: one mounts it
 * with a form's field keys, the other with JWSS §6.1's condition context.
 */

const FORM_FIELDS: LogicVariable[] = [
  { path: 'unit_price', label: 'Unit price', type: 'number' },
  { path: 'quantity', label: 'Quantity', type: 'number' },
  { path: 'type', label: 'Type', type: 'string' },
  { path: 'total', label: 'Total', type: 'number' },
]

/** JWSS §6.1's context, as row 6 will build it; `formData.*` is free, so paths may be typed. */
const JWSS_CONTEXT: LogicVariable[] = [
  { path: 'document.status', label: 'Document status', type: 'string' },
  { path: 'document.amount', label: 'Document amount', type: 'number' },
  { path: 'document.documentTypeKey', label: 'Document type', type: 'string' },
  { path: 'actor.userId', label: 'Actor', type: 'string' },
  { path: 'actor.roles', label: 'Actor roles' },
  { path: 'actor.departmentId', label: 'Actor department', type: 'string' },
]

let mounted: VueWrapper | undefined

afterEach(() => {
  mounted?.unmount()
  mounted = undefined
})

function mountBuilder(
  modelValue: unknown,
  options: {
    tier?: LogicTier
    variables?: LogicVariable[]
    allowFreePaths?: boolean
    attach?: boolean
  } = {},
) {
  const wrapper = mount(JsonLogicBuilder, {
    props: {
      modelValue,
      tier: options.tier ?? 'conditional',
      variables: options.variables ?? FORM_FIELDS,
      allowFreePaths: options.allowFreePaths ?? false,
      // A host binds v-model; without this the builder's own emission would
      // never come back to it as a prop, which is not how it is used.
      'onUpdate:modelValue': (value: unknown) => wrapper.setProps({ modelValue: value }),
    },
    attachTo: options.attach ? document.body : undefined,
  })

  mounted = wrapper

  return wrapper
}

function byLabel(wrapper: VueWrapper, name: string) {
  return wrapper.get(`[aria-label="${name}"]`)
}

function emitted(wrapper: VueWrapper): unknown[] {
  return (wrapper.emitted('update:modelValue') ?? []).map(([value]) => value)
}

function lastEmitted(wrapper: VueWrapper): unknown {
  const values = emitted(wrapper)

  return values[values.length - 1]
}

function optionLabels(wrapper: VueWrapper, name: string): string[] {
  return byLabel(wrapper, name)
    .findAll('option')
    .map((option) => option.text())
}

describe('JsonLogicBuilder', () => {
  describe('nothing is emitted until the user edits (C3)', () => {
    const expression = {
      and: [{ '===': [{ var: 'type' }, 'invoice'] }, { cat: ['a', 'b'] }, { '!': { var: 'x' } }],
    }

    it('emits nothing on mount, focus, blur or expanding a branch', async () => {
      const wrapper = mountBuilder(expression, { attach: true })

      for (const control of wrapper.findAll('select, input, textarea, button')) {
        await control.trigger('focus')
        await control.trigger('blur')
      }

      const toggle = byLabel(wrapper, 'Collapse Expression')

      await toggle.trigger('click')
      expect(byLabel(wrapper, 'Expand Expression').attributes('aria-expanded')).toBe('false')
      await byLabel(wrapper, 'Expand Expression').trigger('click')

      expect(wrapper.emitted()).not.toHaveProperty('update:modelValue')
      expect(wrapper.emitted()).not.toHaveProperty('validityChanged')
    })

    it('emits nothing on mounting an expression it opens raw', async () => {
      const wrapper = mountBuilder({ min: [1, 2] })

      await wrapper.get('textarea').trigger('focus')
      await wrapper.get('textarea').trigger('blur')

      expect(wrapper.emitted()).not.toHaveProperty('update:modelValue')
    })

    it('emits only once the user edits', async () => {
      const wrapper = mountBuilder(expression)

      await byLabel(wrapper, 'Expression, operand 1, operand 2: text').setValue('credit')

      expect(emitted(wrapper)).toHaveLength(1)
    })
  })

  describe('a mixed expression (C5)', () => {
    it('changes only the edited child and keeps the cat subtree as the same value', async () => {
      const cat = { cat: ['INV-', { var: 'n' }] }
      const sibling = { '>': [{ var: 'total' }, 0] }
      const expression = { and: [{ '===': [{ var: 'type' }, 'invoice'] }, cat, sibling] }

      const wrapper = mountBuilder(expression)

      // The cat subtree is shown, raw and marked advanced, and nothing else about it.
      const advanced = byLabel(wrapper, 'Expression, operand 2')

      expect(advanced.text()).toContain('Advanced')
      expect((advanced.get('textarea').element as HTMLTextAreaElement).value).toBe(
        JSON.stringify(cat, null, 2),
      )

      await byLabel(wrapper, 'Expression, operand 1, operand 2: text').setValue('credit')

      const value = lastEmitted(wrapper) as { and: unknown[] }

      expect(value.and[1]).toBe(cat)
      expect(value.and[2]).toBe(sibling)
      expect(JSON.stringify(value)).toBe(
        JSON.stringify({ and: [{ '===': [{ var: 'type' }, 'credit'] }, cat, sibling] }),
      )
    })
  })

  describe('the tier (C6)', () => {
    it('offers no comparison, and, or or not in a calculation, and shows one as advanced', () => {
      const comparison = { '>': [{ var: 'total' }, 1] }
      const wrapper = mountBuilder({ '*': [comparison, 2] }, { tier: 'calculate' })

      const kinds = optionLabels(wrapper, 'Expression, operand 2: kind')

      expect(kinds).toContain('Arithmetic')
      expect(kinds).not.toContain('Comparison')
      expect(kinds).not.toContain('All of / any of')
      expect(kinds).not.toContain('Not')
      expect(byLabel(wrapper, 'Expression, operand 1').text()).toContain('Advanced')
    })

    it('offers every section A operator in a conditional', () => {
      const wrapper = mountBuilder({ '===': [{ var: 'type' }, 'invoice'] })

      expect(optionLabels(wrapper, 'Expression: kind')).toEqual([
        'Variable',
        'Comparison',
        'Arithmetic',
        'All of / any of',
        'Not',
      ])
      expect(optionLabels(wrapper, 'Expression: operator')).toEqual([
        'is (===)',
        'is not (!==)',
        'equals (loose) (==)',
        'not equal (loose) (!=)',
        'greater than (>)',
        'at least (>=)',
        'less than (<)',
        'at most (<=)',
      ])
    })

    it('starts a new comparison as "is"', async () => {
      const wrapper = mountBuilder(undefined)

      await byLabel(wrapper, 'Expression: kind').setValue('comparison')

      expect((byLabel(wrapper, 'Expression: operator').element as HTMLSelectElement).value).toBe(
        '===',
      )
    })
  })

  describe('incomplete expressions (C7)', () => {
    it('shows an unfilled operand as invalid, emits nothing for it, and reports validity', async () => {
      const wrapper = mountBuilder({ '+': [{ var: 'unit_price' }, 1] }, { tier: 'calculate' })

      await byLabel(wrapper, 'Add operand to Expression').trigger('click')

      expect(emitted(wrapper)).toEqual([])
      expect(wrapper.emitted('validityChanged')).toEqual([[false]])
      expect(byLabel(wrapper, 'Expression, operand 3').text()).toContain(
        'Choose what this operand is.',
      )
      expect(byLabel(wrapper, 'Expression, operand 3: kind').attributes('aria-invalid')).toBe(
        'true',
      )

      await byLabel(wrapper, 'Expression, operand 3: kind').setValue('number')

      expect(emitted(wrapper)).toEqual([])

      for (const unfinished of ['-', '1e400', '0x10', '12abc']) {
        await byLabel(wrapper, 'Expression, operand 3: number').setValue(unfinished)
        expect(emitted(wrapper)).toEqual([])
        expect(wrapper.text()).toContain('Enter a number.')
      }

      await byLabel(wrapper, 'Expression, operand 3: number').setValue('2.5')

      expect(lastEmitted(wrapper)).toEqual({ '+': [{ var: 'unit_price' }, 1, 2.5] })
      expect(wrapper.emitted('validityChanged')).toEqual([[false], [true]])
      expect((wrapper.vm as unknown as { valid: boolean }).valid).toBe(true)
    })

    it('reports an empty expression as valid and clears to undefined', async () => {
      const wrapper = mountBuilder({ var: 'total' })

      await byLabel(wrapper, 'Clear Expression').trigger('click')

      expect(emitted(wrapper)).toEqual([undefined])
      expect(byLabel(wrapper, 'Expression: kind').attributes('aria-invalid')).toBe('false')
    })
  })

  describe('literals keep their type (B5)', () => {
    it('shows "5" as text and 5 as a number, and emits each as its own type', async () => {
      const wrapper = mountBuilder({ '==': ['5', 5] })

      expect(
        (byLabel(wrapper, 'Expression, operand 1: kind').element as HTMLSelectElement).value,
      ).toBe('string')
      expect(
        (byLabel(wrapper, 'Expression, operand 2: kind').element as HTMLSelectElement).value,
      ).toBe('number')

      await byLabel(wrapper, 'Expression, operand 2: number').setValue('6')

      expect(JSON.stringify(lastEmitted(wrapper))).toBe('{"==":["5",6]}')
    })

    it('keeps not in the form it was written in', async () => {
      const wrapper = mountBuilder({ '!': { var: 'type' } })

      await byLabel(wrapper, 'Expression, operand 1: variable').setValue('total')

      expect(JSON.stringify(lastEmitted(wrapper))).toBe('{"!":{"var":"total"}}')
    })
  })

  describe('variables are an input (C8)', () => {
    it('hard-codes none: given no variables it offers none', () => {
      const wrapper = mountBuilder({ var: 'total' }, { variables: [] })

      expect(optionLabels(wrapper, 'Expression: variable')).toEqual(['total (not offered)'])
    })

    it('keeps a path it does not offer verbatim, with a visible warning', () => {
      const wrapper = mountBuilder({ '>': [{ var: 'items.0.unit_price' }, 1] })

      expect(byLabel(wrapper, 'Expression, operand 1').text()).toContain(
        'Not in the offered variables',
      )
      expect(
        (byLabel(wrapper, 'Expression, operand 1: variable').element as HTMLSelectElement).value,
      ).toBe('items.0.unit_price')
      expect(wrapper.emitted()).not.toHaveProperty('update:modelValue')
    })
  })

  describe('the form builder input (C9)', () => {
    it('offers the field keys it is given, and nothing else', async () => {
      const wrapper = mountBuilder(
        { '*': [{ var: 'unit_price' }, { var: 'quantity' }] },
        { tier: 'calculate', variables: FORM_FIELDS },
      )

      expect(optionLabels(wrapper, 'Expression, operand 1: variable')).toEqual([
        'Unit price — unit_price (number)',
        'Quantity — quantity (number)',
        'Type — type (string)',
        'Total — total (number)',
      ])
      expect(wrapper.find('[aria-label="Expression, operand 1: variable path"]').exists()).toBe(
        false,
      )

      await byLabel(wrapper, 'Expression, operand 2: variable').setValue('total')

      expect(lastEmitted(wrapper)).toEqual({ '*': [{ var: 'unit_price' }, { var: 'total' }] })
    })
  })

  describe('the workflow builder input (C10)', () => {
    it('offers the JWSS §6.1 context, and takes a typed formData path', async () => {
      const condition = { '<=': [{ var: 'document.amount' }, 10000000] }
      const wrapper = mountBuilder(condition, {
        variables: JWSS_CONTEXT,
        allowFreePaths: true,
      })

      expect(optionLabels(wrapper, 'Expression, operand 1: variable')).toEqual([
        'Document status — document.status (string)',
        'Document amount — document.amount (number)',
        'Document type — document.documentTypeKey (string)',
        'Actor — actor.userId (string)',
        'Actor roles — actor.roles',
        'Actor department — actor.departmentId (string)',
      ])

      await byLabel(wrapper, 'Expression, operand 1: variable path').setValue('formData.total')

      expect(lastEmitted(wrapper)).toEqual({ '<=': [{ var: 'formData.total' }, 10000000] })
      expect(byLabel(wrapper, 'Expression, operand 1').text()).toContain(
        'Not in the offered variables',
      )
    })
  })

  describe('the raw-JSON box (C11, C12)', () => {
    it('opens an unrepresentable root whole, marked advanced, pretty-printed', () => {
      const value = { if: [{ var: 'a' }, 1, 2] }
      const wrapper = mountBuilder(value)

      expect(wrapper.text()).toContain('Advanced')
      expect(wrapper.text()).toContain('cannot show this expression')
      expect((wrapper.get('textarea').element as HTMLTextAreaElement).value).toBe(
        JSON.stringify(value, null, 2),
      )
      expect(byLabel(wrapper, 'Edit Expression visually').attributes('disabled')).toBeDefined()
    })

    it.each([[5], ['x'], [[1, 2]], [null], [{}], [{ '+': [1, 2], '-': [3, 4] }]])(
      'opens %j at the root raw (B2)',
      (value) => {
        const wrapper = mountBuilder(value)

        expect(wrapper.find('textarea').exists()).toBe(true)
        expect(wrapper.text()).toContain('Advanced')
      },
    )

    it('leaves the held value alone while the text is not JSON, and never rewrites one that is', async () => {
      const wrapper = mountBuilder({ min: [1, 2] })
      const box = wrapper.get('textarea')

      await box.setValue('{"min": [1, ')

      expect(emitted(wrapper)).toEqual([])
      expect(wrapper.emitted('validityChanged')).toEqual([[false]])

      await box.setValue('{"max":[3,1,2],  "z": 0}')

      expect(JSON.stringify(lastEmitted(wrapper))).toBe('{"max":[3,1,2],"z":0}')
      // The text is what was typed, not a re-indentation of the value.
      expect((box.element as HTMLTextAreaElement).value).toBe('{"max":[3,1,2],  "z": 0}')
      expect(wrapper.emitted('validityChanged')).toEqual([[false], [true]])
    })

    it('marks an opaque subtree advanced and keeps its value while its text is not JSON', async () => {
      const wrapper = mountBuilder({ and: [{ var: 'type' }, { in: ['a', ['a', 'b']] }] })
      const block = byLabel(wrapper, 'Expression, operand 2')

      expect(block.text()).toContain('Advanced')

      await block.get('textarea').setValue('{"in": ["a"')

      expect(emitted(wrapper)).toEqual([])
      expect(wrapper.emitted('validityChanged')).toEqual([[false]])

      await block.get('textarea').setValue('{"in":["c",["a","b"]]}')

      expect(lastEmitted(wrapper)).toEqual({ and: [{ var: 'type' }, { in: ['c', ['a', 'b']] }] })
    })

    it('switches between JSON and the visual view without emitting', async () => {
      const wrapper = mountBuilder({ '===': [{ var: 'type' }, 'invoice'] })

      await byLabel(wrapper, 'Edit Expression as JSON').trigger('click')

      expect(wrapper.find('textarea').exists()).toBe(true)

      await byLabel(wrapper, 'Edit Expression visually').trigger('click')

      expect(wrapper.find('[aria-label="Expression: operator"]').exists()).toBe(true)
      expect(wrapper.find('textarea').exists()).toBe(false)
      expect(wrapper.emitted()).not.toHaveProperty('update:modelValue')
    })
  })

  describe('a value the host changes is loaded afresh', () => {
    it('re-reads a new value, and does not re-read its own', async () => {
      const wrapper = mountBuilder({ var: 'total' })
      const control = byLabel(wrapper, 'Expression: variable').element

      await byLabel(wrapper, 'Expression: variable').setValue('quantity')
      // The host now holds the emitted value, as reactive state. Re-reading it
      // would rebuild the tree and remount every control under the user.
      expect(wrapper.props('modelValue')).toEqual({ var: 'quantity' })
      expect(byLabel(wrapper, 'Expression: variable').element).toBe(control)

      await wrapper.setProps({ modelValue: { var: 'type' } })

      expect((byLabel(wrapper, 'Expression: variable').element as HTMLSelectElement).value).toBe(
        'type',
      )
      expect(emitted(wrapper)).toHaveLength(1)
    })
  })

  describe('by keyboard alone (C15)', () => {
    it('adds, edits and removes a node with Tab, Enter and the arrow keys', async () => {
      const wrapper = mountBuilder(
        { and: [{ var: 'type' }, { var: 'quantity' }] },
        { attach: true },
      )

      // Add: an operand, focused for the user to choose what it is.
      tabToLabel('Add operand to Expression')
      press('Enter')
      await nextTick()
      await nextTick()

      expect(document.activeElement?.getAttribute('aria-label')).toBe('Expression, operand 3: kind')

      press('ArrowDown') // Variable
      await nextTick()
      tabToLabel('Expression, operand 3: variable')
      press('ArrowDown') // Unit price
      press('ArrowDown') // Quantity
      press('ArrowDown') // Type
      press('ArrowDown') // Total
      await nextTick()

      expect(lastEmitted(wrapper)).toEqual({
        and: [{ var: 'type' }, { var: 'quantity' }, { var: 'total' }],
      })

      // Edit: the operator, and a number typed into a new operand's field.
      tabToLabel('Expression: operator')
      press('ArrowDown') // any of
      await nextTick()

      expect(lastEmitted(wrapper)).toEqual({
        or: [{ var: 'type' }, { var: 'quantity' }, { var: 'total' }],
      })

      tabToLabel('Expression, operand 2: kind')
      press('ArrowDown') // Number
      await nextTick()
      tabToLabel('Expression, operand 2: number')
      type('42')
      await nextTick()

      expect(lastEmitted(wrapper)).toEqual({ or: [{ var: 'type' }, 42, { var: 'total' }] })

      // Remove: the first operand, with focus kept in the tree.
      tabToLabel('Remove Expression, operand 1')
      press(' ')
      await nextTick()
      await nextTick()

      expect(lastEmitted(wrapper)).toEqual({ or: [42, { var: 'total' }] })
      expect(document.activeElement?.getAttribute('aria-label')).toBe('Expression, operand 1: kind')
    })

    it('gives every control an accessible name', () => {
      const wrapper = mountBuilder({
        and: [
          { '===': [{ var: 'type' }, 'invoice'] },
          { '!': [true] },
          { '>': [{ '+': [1, null] }, 2] },
          { cat: ['a'] },
        ],
      })

      for (const control of wrapper.findAll('select, input, button')) {
        expect(control.attributes('aria-label') ?? '').not.toBe('')
      }

      for (const box of wrapper.findAll('textarea')) {
        expect(wrapper.find(`label[for="${box.attributes('id')}"]`).exists()).toBe(true)
      }
    })
  })
})

// --- The test-engineer campaign, 2026-10-09 ------------------------------------

function valueOf(wrapper: VueWrapper, name: string): string {
  return (byLabel(wrapper, name).element as HTMLInputElement | HTMLSelectElement).value
}

function isValid(wrapper: VueWrapper): boolean {
  return (wrapper.vm as unknown as { valid: boolean }).valid
}

/**
 * What the builder's own suite did not reach, found by the independent
 * campaign on #686, and the three gaps `requirements-analyst`'s trace named.
 */
describe('JsonLogicBuilder at its edges (campaign, 2026-10-09)', () => {
  describe('a host’s reactive v-model', () => {
    it('reads through the proxy, so a held subtree is the host’s own object', async () => {
      const cat = { cat: ['INV-', { var: 'n' }] }
      const state = reactive<{ expr: unknown }>({ expr: { and: [{ var: 'type' }, cat] } })
      const values: unknown[] = []
      const Host = defineComponent({
        setup: () => () =>
          h(JsonLogicBuilder, {
            modelValue: state.expr,
            tier: 'conditional',
            variables: FORM_FIELDS,
            'onUpdate:modelValue': (value: unknown) => {
              values.push(value)
              state.expr = value
            },
          }),
      })
      const wrapper = mount(Host)

      mounted = wrapper

      const control = byLabel(wrapper, 'Expression, operand 1: variable').element

      await byLabel(wrapper, 'Expression, operand 1: variable').setValue('total')

      expect((values[0] as { and: unknown[] }).and[1]).toBe(cat)
      // The host's echo, a proxy of what was emitted, is known as the builder's own.
      expect(byLabel(wrapper, 'Expression, operand 1: variable').element).toBe(control)

      await byLabel(wrapper, 'Expression, operand 1: variable').setValue('quantity')

      expect(values).toHaveLength(2)
      expect((values[1] as { and: unknown[] }).and[1]).toBe(cat)
    })
  })

  describe('a value the host changes', () => {
    it(// Was `it.fails`, the campaign's defect: `lastEmitted` outlived a load, so
    // a host that undid and then redid handed back the very value the builder
    // last emitted, the watcher took it for its own echo, and the builder kept
    // showing the undone expression. Fixed by forgetting it on every load.
    'is re-read when the host hands back what the builder last emitted (undo, then redo)', async () => {
      const wrapper = mountBuilder({ var: 'unit_price' })

      await byLabel(wrapper, 'Expression: variable').setValue('quantity')

      const redo = wrapper.props('modelValue')

      await wrapper.setProps({ modelValue: { var: 'unit_price' } })
      expect(valueOf(wrapper, 'Expression: variable')).toBe('unit_price')

      await wrapper.setProps({ modelValue: redo })
      expect(valueOf(wrapper, 'Expression: variable')).toBe('quantity')
    })

    it('is re-read on a redo when the host never echoed the emission', async () => {
      // No v-model echo: the emission is still pending when the host undoes.
      // Only forgetting it on load tells the redo apart from an echo.
      const wrapper = mount(JsonLogicBuilder, {
        props: { modelValue: { var: 'unit_price' }, tier: 'conditional', variables: FORM_FIELDS },
      })

      mounted = wrapper

      await byLabel(wrapper, 'Expression: variable').setValue('quantity')
      await wrapper.setProps({ modelValue: { var: 'unit_price' } })

      expect(valueOf(wrapper, 'Expression: variable')).toBe('unit_price')

      await wrapper.setProps({ modelValue: { var: 'quantity' } })

      expect(valueOf(wrapper, 'Expression: variable')).toBe('quantity')
    })

    it('is not re-read when a host that clones on write hands back an equal copy', async () => {
      // An undo stack or a copying store echoes an equal value that is not the
      // same object. Re-reading it would remount every control and drop the
      // operand the user has not filled in yet.
      const wrapper = mount(JsonLogicBuilder, {
        props: {
          modelValue: { and: [{ var: 'type' }, { var: 'quantity' }] },
          tier: 'conditional' as const,
          variables: FORM_FIELDS,
          'onUpdate:modelValue': (value: unknown) =>
            wrapper.setProps({ modelValue: JSON.parse(JSON.stringify(value)) }),
        },
      })

      mounted = wrapper

      await byLabel(wrapper, 'Add operand to Expression').trigger('click')

      const control = byLabel(wrapper, 'Expression, operand 1: variable').element

      await byLabel(wrapper, 'Expression, operand 1: variable').setValue('total')

      // Unfilled, so nothing was emitted: the host still holds its first value.
      expect(emitted(wrapper)).toEqual([])

      await byLabel(wrapper, 'Expression, operand 3: kind').setValue('null')
      await byLabel(wrapper, 'Expression, operand 2: variable').setValue('type')

      expect(emitted(wrapper)).toHaveLength(2)
      expect(lastEmitted(wrapper)).toEqual({ and: [{ var: 'total' }, { var: 'type' }, null] })
      // The echoes were copies, and the tree was kept: the same control is there.
      expect(byLabel(wrapper, 'Expression, operand 1: variable').element).toBe(control)

      // An undo to a different value is still read.
      await wrapper.setProps({ modelValue: { var: 'quantity' } })

      expect(valueOf(wrapper, 'Expression: variable')).toBe('quantity')
    })
  })

  describe('an expression nested past the depth cap', () => {
    // Rendering 256 nested levels takes about a second alone and several under
    // the full suite's load, so it gets a timeout of its own.
    it(
      'mounts, shows it visual to the cap, and holds the rest as an advanced block',
      { timeout: 30_000 },
      () => {
        let expr: unknown = { var: 'a' }

        for (let depth = 0; depth < 100_000; depth += 1) {
          expr = { '!': expr }
        }

        // Through a host, not as a mount prop: Vue Test Utils walks mount props
        // recursively, which is the harness overflowing and not the builder.
        const wrapper = mount(
          defineComponent({
            setup: () => () =>
              h(JsonLogicBuilder, { modelValue: expr, tier: 'conditional', variables: [] }),
          }),
        )

        mounted = wrapper

        expect(wrapper.text()).toContain('Advanced')
        expect(wrapper.text()).toContain('Nested too deeply to show as text')
        expect(wrapper.get('textarea').attributes('disabled')).toBeDefined()
        expect(wrapper.findComponent(JsonLogicBuilder).emitted()).not.toHaveProperty(
          'update:modelValue',
        )
      },
    )
  })

  describe('the tier switching (C6)', () => {
    it('re-reads the expression in the new tier, and emits nothing', async () => {
      const wrapper = mountBuilder(
        { '*': [{ '>': [{ var: 'total' }, 1] }, 2] },
        { tier: 'calculate' },
      )

      expect(byLabel(wrapper, 'Expression, operand 1').find('textarea').exists()).toBe(true)

      await wrapper.setProps({ tier: 'conditional' })

      expect(valueOf(wrapper, 'Expression, operand 1: operator')).toBe('>')
      expect(byLabel(wrapper, 'Expression, operand 1').find('textarea').exists()).toBe(false)

      await wrapper.setProps({ tier: 'calculate' })

      expect(byLabel(wrapper, 'Expression, operand 1').find('textarea').exists()).toBe(true)
      expect(wrapper.emitted()).not.toHaveProperty('update:modelValue')
    })

    it('opens a root the new tier does not offer in the raw box, and back', async () => {
      const wrapper = mountBuilder({ and: [{ var: 'type' }, true] })

      await wrapper.setProps({ tier: 'calculate' })

      expect(wrapper.text()).toContain('cannot show this expression')

      await wrapper.setProps({ tier: 'conditional' })

      expect(valueOf(wrapper, 'Expression: operator')).toBe('and')
      expect(wrapper.emitted()).not.toHaveProperty('update:modelValue')
    })

    it('drops an unfinished edit and reloads the host’s value, which is valid', async () => {
      const wrapper = mountBuilder({ '+': [{ var: 'unit_price' }, 1] }, { tier: 'calculate' })

      await byLabel(wrapper, 'Add operand to Expression').trigger('click')
      expect(isValid(wrapper)).toBe(false)

      await wrapper.setProps({ tier: 'conditional' })

      expect(wrapper.find('[aria-label="Expression, operand 3"]').exists()).toBe(false)
      expect(isValid(wrapper)).toBe(true)
      expect(wrapper.emitted('validityChanged')).toEqual([[false], [true]])
      expect(wrapper.emitted()).not.toHaveProperty('update:modelValue')

      // The tree is the host's value again: an edit changes only what it edits.
      await byLabel(wrapper, 'Expression, operand 2: number').setValue('2')

      expect(lastEmitted(wrapper)).toEqual({ '+': [{ var: 'unit_price' }, 2] })
    })
  })

  describe('an advanced block (C11, C12)', () => {
    it('stays raw when its text becomes an expression the builder could draw', async () => {
      const wrapper = mountBuilder({ and: [{ var: 'type' }, { in: ['a', ['a', 'b']] }] })
      const box = byLabel(wrapper, 'Expression, operand 2').get('textarea')

      await box.setValue('{"var":  "total"}')

      expect(byLabel(wrapper, 'Expression, operand 2').find('textarea').exists()).toBe(true)
      expect((box.element as HTMLTextAreaElement).value).toBe('{"var":  "total"}')
      expect(lastEmitted(wrapper)).toEqual({ and: [{ var: 'type' }, { var: 'total' }] })

      // Opened afresh, the same value is drawn.
      await wrapper.setProps({ modelValue: { and: [{ var: 'type' }, { var: 'total' }] } })

      expect(valueOf(wrapper, 'Expression, operand 2: variable')).toBe('total')
    })

    it.each([
      ['"5"', '"5"'],
      ['5', '5'],
      ['true', 'true'],
      ['null', 'null'],
      ['{"b": 1, "a": 2}', '{"b":1,"a":2}'],
    ])('emits %s typed in it as JSON parses it, type and key order kept', async (typed, json) => {
      const wrapper = mountBuilder({ and: [{ var: 'type' }, { in: ['a', ['a']] }] })

      await byLabel(wrapper, 'Expression, operand 2').get('textarea').setValue(typed)

      expect(JSON.stringify(lastEmitted(wrapper))).toBe(`{"and":[{"var":"type"},${json}]}`)
    })

    it('is unfilled, not emitted, when its text is emptied', async () => {
      const wrapper = mountBuilder({ and: [{ var: 'type' }, { in: ['a', ['a']] }] })

      await byLabel(wrapper, 'Expression, operand 2').get('textarea').setValue('  ')

      expect(emitted(wrapper)).toEqual([])
      expect(isValid(wrapper)).toBe(false)
      expect(byLabel(wrapper, 'Expression, operand 2').text()).toContain('Enter valid JSON.')
    })

    it('keeps an opaque subtree three deep as the same object through an edit', async () => {
      const cat = { cat: ['INV-', { var: 'n' }] }
      const not = { '!': [cat] }
      const sibling = { var: 'total' }
      const wrapper = mountBuilder({ and: [{ or: [not, { var: 'type' }] }, sibling] })

      await byLabel(wrapper, 'Expression, operand 1, operand 2: variable').setValue('quantity')

      const value = lastEmitted(wrapper) as { and: [{ or: [{ '!': unknown[] }] }, unknown] }

      expect(value.and[0].or[0]).toBe(not)
      expect(value.and[0].or[0]['!'][0]).toBe(cat)
      expect(value.and[1]).toBe(sibling)
    })
  })

  describe('the root raw box (C11)', () => {
    it('emptied, emits no expression, and can then be edited visually', async () => {
      const wrapper = mountBuilder({ min: [1, 2] })

      await wrapper.get('textarea').setValue(' ')

      expect(emitted(wrapper)).toEqual([undefined])
      expect(isValid(wrapper)).toBe(true)

      await byLabel(wrapper, 'Edit Expression visually').trigger('click')

      expect(optionLabels(wrapper, 'Expression: kind')).toContain('Variable')
    })

    it('cannot return to the visual view while its text is not JSON', async () => {
      const wrapper = mountBuilder({ '===': [{ var: 'type' }, 'invoice'] })

      await byLabel(wrapper, 'Edit Expression as JSON').trigger('click')
      await wrapper.get('textarea').setValue('{"===": [')

      expect(byLabel(wrapper, 'Edit Expression visually').attributes('disabled')).toBeDefined()

      await wrapper.get('textarea').setValue('{"!==": [{"var": "type"}, "invoice"]}')

      expect(byLabel(wrapper, 'Edit Expression visually').attributes('disabled')).toBeUndefined()
    })

    it('is not offered while an operand is unfilled', async () => {
      const wrapper = mountBuilder({ '+': [1, 2] }, { tier: 'calculate' })

      expect(byLabel(wrapper, 'Edit Expression as JSON').attributes('disabled')).toBeUndefined()

      await byLabel(wrapper, 'Add operand to Expression').trigger('click')

      expect(byLabel(wrapper, 'Edit Expression as JSON').attributes('disabled')).toBeDefined()
    })
  })

  describe('literals keep their type (B5)', () => {
    it('emits text that reads as a number or a boolean as text', async () => {
      const wrapper = mountBuilder({ '===': [{ var: 'type' }, 'invoice'] })

      for (const text of ['5', '-0', 'true', 'null']) {
        await byLabel(wrapper, 'Expression, operand 2: text').setValue(text)

        expect(lastEmitted(wrapper)).toEqual({ '===': [{ var: 'type' }, text] })
      }
    })

    it('emits false as false, and true as true', async () => {
      const wrapper = mountBuilder({ '===': [{ var: 'type' }, 'invoice'] })

      await byLabel(wrapper, 'Expression, operand 2: kind').setValue('boolean')
      expect(emitted(wrapper)).toEqual([])

      await byLabel(wrapper, 'Expression, operand 2: true or false').setValue('false')
      expect(JSON.stringify(lastEmitted(wrapper))).toBe('{"===":[{"var":"type"},false]}')

      await byLabel(wrapper, 'Expression, operand 2: true or false').setValue('true')
      expect(JSON.stringify(lastEmitted(wrapper))).toBe('{"===":[{"var":"type"},true]}')
    })

    it('shows and keeps numbers at the edges of what JavaScript writes', async () => {
      const wrapper = mountBuilder(
        { '+': [1e21, Number.MIN_VALUE, -0, { var: 'unit_price' }] },
        { tier: 'calculate' },
      )

      expect(valueOf(wrapper, 'Expression, operand 1: number')).toBe('1e+21')
      expect(valueOf(wrapper, 'Expression, operand 2: number')).toBe('5e-324')

      await byLabel(wrapper, 'Expression, operand 4: variable').setValue('quantity')

      const operands = (lastEmitted(wrapper) as { '+': unknown[] })['+']

      expect(operands[0]).toBe(1e21)
      expect(operands[1]).toBe(Number.MIN_VALUE)
      expect(Object.is(operands[2], -0)).toBe(true)

      await byLabel(wrapper, 'Expression, operand 3: number').setValue('-1.5E-3')

      expect((lastEmitted(wrapper) as { '+': unknown[] })['+'][2]).toBe(-0.0015)
    })

    it('leaves a number unfilled until it is one, whatever Number() would make of it', async () => {
      const wrapper = mountBuilder({ '+': [{ var: 'unit_price' }, 1] }, { tier: 'calculate' })

      for (const unfinished of ['', '  ', 'Infinity', 'NaN', '+1', '1,5', '1e', '.', '1_000']) {
        await byLabel(wrapper, 'Expression, operand 2: number').setValue(unfinished)

        expect(emitted(wrapper)).toEqual([])
        expect(isValid(wrapper)).toBe(false)
      }
    })

    it('starts a number afresh when the kind is changed away and back', async () => {
      const wrapper = mountBuilder({ '+': [{ var: 'unit_price' }, 5] }, { tier: 'calculate' })

      await byLabel(wrapper, 'Expression, operand 2: kind').setValue('string')
      await byLabel(wrapper, 'Expression, operand 2: kind').setValue('number')

      expect(valueOf(wrapper, 'Expression, operand 2: number')).toBe('')
      expect(byLabel(wrapper, 'Expression, operand 2').text()).toContain('Enter a number.')
      expect(emitted(wrapper)).toEqual([])
    })
  })

  describe('choosing what an operand is', () => {
    it('re-choosing the kind a node already is changes nothing and emits nothing', async () => {
      const wrapper = mountBuilder({ '===': [{ var: 'type' }, 'invoice'] })

      await byLabel(wrapper, 'Expression: kind').setValue('comparison')
      await byLabel(wrapper, 'Expression, operand 1: kind').setValue('var')

      expect(valueOf(wrapper, 'Expression, operand 1: variable')).toBe('type')
      expect(wrapper.emitted()).not.toHaveProperty('update:modelValue')
      expect(wrapper.emitted()).not.toHaveProperty('validityChanged')
    })
  })

  describe('the unknown-variable warning (B4)', () => {
    it('shows only on a path not offered, and goes when an offered one is chosen', async () => {
      const wrapper = mountBuilder(
        { '*': [{ var: 'unit_price' }, { var: 'items.0.price' }] },
        {
          tier: 'calculate',
        },
      )

      expect(byLabel(wrapper, 'Expression, operand 1').text()).not.toContain('Not in the offered')
      expect(byLabel(wrapper, 'Expression, operand 2').text()).toContain('Not in the offered')

      await byLabel(wrapper, 'Expression, operand 2: variable').setValue('quantity')

      expect(byLabel(wrapper, 'Expression, operand 2').text()).not.toContain('Not in the offered')
    })

    it('leaves a variable unfilled, not emitted, when its free path is blanked', async () => {
      const wrapper = mountBuilder(
        { '<=': [{ var: 'document.amount' }, 10] },
        { variables: JWSS_CONTEXT, allowFreePaths: true },
      )

      await byLabel(wrapper, 'Expression, operand 1: variable path').setValue('')

      expect(emitted(wrapper)).toEqual([])
      expect(isValid(wrapper)).toBe(false)
      expect(byLabel(wrapper, 'Expression, operand 1').text()).toContain('Choose a variable.')
      expect(
        byLabel(wrapper, 'Expression, operand 1: variable path').attributes('aria-invalid'),
      ).toBe('true')

      await byLabel(wrapper, 'Expression, operand 1: variable path').setValue('formData.x')

      expect(lastEmitted(wrapper)).toEqual({ '<=': [{ var: 'formData.x' }, 10] })
    })
  })

  describe('arity in the view (section A)', () => {
    it('offers no add or remove on a two-operand operator, or on not', () => {
      const wrapper = mountBuilder({ and: [{ '-': [{ var: 'total' }, 1] }, { '!': true }] })

      expect(wrapper.find('[aria-label="Add operand to Expression, operand 1"]').exists()).toBe(
        false,
      )
      expect(wrapper.find('[aria-label="Remove Expression, operand 1, operand 1"]').exists()).toBe(
        false,
      )
      expect(wrapper.find('[aria-label="Add operand to Expression, operand 2"]').exists()).toBe(
        false,
      )
      expect(wrapper.find('[aria-label="Remove Expression, operand 2, operand 1"]').exists()).toBe(
        false,
      )
      expect(wrapper.find('[aria-label="Expression, operand 2: operator"]').exists()).toBe(false)
    })

    it('offers remove on a variadic operand only above two', async () => {
      const wrapper = mountBuilder({ '+': [1, 2] }, { tier: 'calculate' })

      expect(wrapper.find('[aria-label="Remove Expression, operand 1"]').exists()).toBe(false)

      await wrapper.setProps({ modelValue: { '+': [1, 2, 3] } })

      expect(wrapper.find('[aria-label="Remove Expression, operand 1"]').exists()).toBe(true)
    })

    it('greys out an operator that cannot take the operands there are', () => {
      const wrapper = mountBuilder({ '+': [1, 2, 3] }, { tier: 'calculate' })
      const options = byLabel(wrapper, 'Expression: operator').findAll('option')
      const disabled = Object.fromEntries(
        options.map((option) => [
          option.attributes('value'),
          option.attributes('disabled') !== undefined,
        ]),
      )

      expect(disabled).toEqual({ '+': false, '-': true, '*': false, '/': true, '%': true })
    })
  })

  describe('disabled', () => {
    it('disables every control that edits, and leaves expanding alone', () => {
      const wrapper = mount(JsonLogicBuilder, {
        props: {
          modelValue: { and: [{ '+': [1, 2, { var: 'x' }] }, { cat: ['a'] }, { var: 'type' }] },
          tier: 'conditional',
          variables: FORM_FIELDS,
          allowFreePaths: true,
          disabled: true,
        },
      })

      mounted = wrapper

      for (const control of wrapper.findAll('select, input, textarea, button')) {
        const name = control.attributes('aria-label') ?? control.attributes('id') ?? ''
        const toggles = /^(Collapse|Expand) /.test(name)

        expect({ name, disabled: control.attributes('disabled') !== undefined }).toEqual({
          name,
          disabled: !toggles,
        })
      }
    })

    it('disables the root raw box too', () => {
      const wrapper = mount(JsonLogicBuilder, {
        props: { modelValue: { min: [1, 2] }, tier: 'conditional', variables: [], disabled: true },
      })

      mounted = wrapper

      expect(wrapper.get('textarea').attributes('disabled')).toBeDefined()
    })
  })

  describe('clearing', () => {
    it('starts over with the kinds the tier offers, and emits nothing until filled', async () => {
      const wrapper = mountBuilder({ var: 'total' }, { tier: 'calculate' })

      await byLabel(wrapper, 'Clear Expression').trigger('click')

      expect(optionLabels(wrapper, 'Expression: kind')).toEqual([
        'No expression',
        'Variable',
        'Arithmetic',
      ])

      await byLabel(wrapper, 'Expression: kind').setValue('var')

      expect(emitted(wrapper)).toEqual([undefined])
      expect(isValid(wrapper)).toBe(false)
    })
  })
})
