import { mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, describe, expect, it } from 'vitest'
import { nextTick } from 'vue'

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

      await byLabel(wrapper, 'Expression, operand 3: number').setValue('-')
      expect(emitted(wrapper)).toEqual([])
      expect(wrapper.text()).toContain('Enter a number.')

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
