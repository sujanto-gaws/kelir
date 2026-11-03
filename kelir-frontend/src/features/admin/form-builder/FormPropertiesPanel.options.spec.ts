import { mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, describe, expect, it } from 'vitest'
import { defineComponent, h, ref, type PropType } from 'vue'

import { jfssViolations } from '@/lib/testing/jfss-meta-schema'
import type { JfssComponent, JfssOption } from '@/types/jfss'

import FormPropertiesPanel from './FormPropertiesPanel.vue'

/**
 * **A select's options, a row each** (#790, AC1–AC9).
 *
 * The *Options* box was one `label=value` line per option, `v-model` on a
 * computed property. Every edit rewrote every value with `String(value)`,
 * split a label holding `=`, and redrew what was being typed. Each row now
 * replaces only the option it holds.
 *
 * The host replaces the node with what the panel emits and clones it on the
 * way, as the page's `replaceNode` does.
 *
 * # Seen to fail (coding standard §2.9)
 *
 * Against `main`'s box at `35681b1`, 2026-11-03, every case but AC8 failed:
 * there are no rows to drive. AC8 pins #732's AC4, which `main` already kept.
 * Each mutation below was run against this folder's specs and reverted after:
 *
 * | Mutation | Reddened |
 * |---|---|
 * | every other option's value written as `String(value)` | AC1, AC2, AC4, AC6 |
 * | a typed value never read by the value type | AC4, four AC5 cases, *keeps a half-typed value …* |
 * | `integer` takes a fraction | AC5's `1.5`, and `propertyEdits.spec.ts` |
 * | no draft, so the box shows the stored value | *keeps a half-typed value …* |
 * | a removal does not shift the drafts | the same |
 * | a draft never gives way to the stored value | the same |
 * | a new option is `{Option, option}` | AC3, AC6, AC7 |
 * | a row named from 0 | AC9 |
 * | no note on text that does not match the type | AC5's `abc`, `1.5` and `maybe` |
 * | a label trimmed | AC7 |
 * | an edit elsewhere writes options with `String(value)` | AC8 |
 * | no type hint under a value | AC2, *keeps a half-typed value …*, and `propertyEdits.spec.ts` |
 * | the group unlabelled | AC9 |
 */

let mounted: VueWrapper | undefined

afterEach(() => {
  mounted?.unmount()
  mounted = undefined
})

function harness(initial: Record<string, unknown>) {
  const history: Record<string, unknown>[] = []

  const Host = defineComponent({
    props: { start: { type: Object as PropType<Record<string, unknown>>, required: true } },
    setup(props) {
      const node = ref(JSON.parse(JSON.stringify(props.start)) as JfssComponent)

      return () =>
        h(FormPropertiesPanel, {
          component: node.value,
          path: 'components.0',
          disabled: false,
          details: [],
          lookupSource: '',
          scope: [],
          onUpdate: (next: JfssComponent) => {
            node.value = JSON.parse(JSON.stringify(next)) as JfssComponent
            history.push(node.value as unknown as Record<string, unknown>)
          },
        })
    },
  })

  const wrapper = mount(Host, { props: { start: initial } })

  mounted = wrapper

  const latest = () => history[history.length - 1] ?? initial

  return { wrapper, latest, options: () => latest().options as JfssOption[] }
}

function select(options: JfssOption[], type = 'string'): Record<string, unknown> {
  return {
    id: 'f',
    role: 'data',
    type: 'select',
    key: 'f',
    label: 'F',
    validation: { type },
    options,
  }
}

function row(wrapper: VueWrapper, index: number, part: 'label' | 'value' | 'remove' | 'note') {
  return wrapper.find(`[data-testid="options-f-${index}-${part}"]`)
}

function shown(wrapper: VueWrapper, index: number, part: 'label' | 'value'): string {
  return (row(wrapper, index, part).element as HTMLInputElement).value
}

async function add(wrapper: VueWrapper): Promise<void> {
  await wrapper.get('[data-testid="options-f-add"]').trigger('click')
}

const MIXED: JfssOption[] = [
  { label: 'One', value: 1 },
  { label: 'Yes', value: true },
  { label: 'None', value: null },
  { label: 'Object', value: { x: 1 } },
  { label: 'a=b', value: 'c' },
]

describe('a select’s options, a row each (#790)', () => {
  it('adds Two=two through the rows, and keeps the three loaded as they were (AC1)', async () => {
    const loaded = [
      { label: 'One', value: 1 },
      { label: 'Yes', value: true },
      { label: 'a=b', value: 'c' },
    ]
    const { wrapper, options, latest } = harness(select(loaded))

    await add(wrapper)
    await row(wrapper, 3, 'label').setValue('Two')
    await row(wrapper, 3, 'value').setValue('two')

    expect(options()).toStrictEqual([...loaded, { label: 'Two', value: 'two' }])
    expect(jfssViolations({ formId: 'f', version: '2.0.1', components: [latest()] })).toEqual([])
  })

  it('keeps 1, true, null and {x: 1} as loaded when another option’s label is edited, and shows each with its type (AC2)', async () => {
    const loaded = [{ label: 'Edit me', value: 'e' }, ...MIXED]
    const { wrapper, options } = harness(select(loaded))

    expect(
      [1, 2, 3, 4].map((index) => [
        shown(wrapper, index, 'value'),
        row(wrapper, index, 'note').text(),
      ]),
    ).toEqual([
      ['1', 'number'],
      ['true', 'boolean'],
      ['null', 'null'],
      ['{"x":1}', 'object'],
    ])

    await row(wrapper, 0, 'label').setValue('Edited')

    expect(options()[0]).toStrictEqual({ label: 'Edited', value: 'e' })
    expect(options().slice(1)).toStrictEqual(MIXED)
    expect(options()[1].value).toBe(1)
    expect(options()[2].value).toBe(true)
  })

  it('shows a label holding = as it is, keeps it on another row’s edit, and stores a new one as typed (AC3)', async () => {
    const { wrapper, options } = harness(
      select([
        { label: 'a=b', value: 'c' },
        { label: 'B', value: 'b' },
      ]),
    )

    expect([shown(wrapper, 0, 'label'), shown(wrapper, 0, 'value')]).toEqual(['a=b', 'c'])

    await row(wrapper, 1, 'value').setValue('b2')

    expect(options()[0]).toStrictEqual({ label: 'a=b', value: 'c' })

    await add(wrapper)
    await row(wrapper, 2, 'label').setValue('x=y')

    expect(options()[2]).toStrictEqual({ label: 'x=y', value: '' })
  })

  it('keeps the number 1 on a label edit, and stores 2 as a number on a number field (AC4)', async () => {
    const { wrapper, options } = harness(
      select(
        [
          { label: 'One', value: 1 },
          { label: 'Ten', value: 10 },
        ],
        'number',
      ),
    )

    await row(wrapper, 0, 'label').setValue('Uno')

    expect(options()[0]).toStrictEqual({ label: 'Uno', value: 1 })

    await row(wrapper, 0, 'value').setValue('2')

    expect(options()).toStrictEqual([
      { label: 'Uno', value: 2 },
      { label: 'Ten', value: 10 },
    ])
  })

  it.each([
    ['number', '42', 42, false],
    ['number', 'abc', 'abc', true],
    ['integer', '7', 7, false],
    ['integer', '1.5', '1.5', true],
    ['boolean', 'true', true, false],
    ['boolean', 'false', false, false],
    ['boolean', 'maybe', 'maybe', true],
    ['string', '42', '42', false],
    ['array', '[1]', '[1]', false],
    ['object', '{}', '{}', false],
  ] as const)(
    'on a %s field, %s is stored as %j, said to be text: %s (AC5)',
    async (type, typed, stored, asText) => {
      const { wrapper, options } = harness(select([{ label: 'A', value: 'a' }], type))

      await row(wrapper, 0, 'value').setValue(typed)

      expect(options()[0]).toStrictEqual({ label: 'A', value: stored })
      expect(shown(wrapper, 0, 'value')).toBe(typed)

      const note = row(wrapper, 0, 'note')

      expect(note.exists() && note.text().includes('Stored as text')).toBe(asText)
    },
  )

  it('keeps a half-typed value as typed while it is typed, and after a row above goes (AC5)', async () => {
    const { wrapper, options } = harness(
      select(
        [
          { label: 'A', value: 1 },
          { label: 'B', value: 2 },
        ],
        'number',
      ),
    )

    await row(wrapper, 1, 'value').setValue('-')

    expect([shown(wrapper, 1, 'value'), options()[1].value]).toEqual(['-', '-'])

    await row(wrapper, 1, 'value').setValue('1.')

    expect([shown(wrapper, 1, 'value'), options()[1].value]).toEqual(['1.', 1])

    await row(wrapper, 0, 'remove').trigger('click')

    expect(shown(wrapper, 0, 'value')).toBe('1.')
    expect(options()).toStrictEqual([{ label: 'B', value: 1 }])

    // A switch of value type converts nothing, so the box shows what is stored.
    await wrapper.get('[data-testid="vtype-f"]').setValue('string')

    expect([shown(wrapper, 0, 'value'), row(wrapper, 0, 'note').text()]).toEqual(['1', 'number'])
    expect(options()).toStrictEqual([{ label: 'B', value: 1 }])
  })

  it('removes a middle row, appends a new one, and keeps two options of one label apart (AC6)', async () => {
    const { wrapper, options } = harness(
      select([
        { label: 'Same', value: 1 },
        { label: 'Middle', value: 'm' },
        { label: 'Same', value: 2 },
      ]),
    )

    await row(wrapper, 1, 'remove').trigger('click')

    expect(options()).toStrictEqual([
      { label: 'Same', value: 1 },
      { label: 'Same', value: 2 },
    ])

    await row(wrapper, 1, 'label').setValue('Other')
    await add(wrapper)

    expect(options()).toStrictEqual([
      { label: 'Same', value: 1 },
      { label: 'Other', value: 2 },
      { label: '', value: '' },
    ])

    for (let left = 3; left > 0; left -= 1) {
      await row(wrapper, 0, 'remove').trigger('click')
    }

    expect(options()).toStrictEqual([])
  })

  it('shows a new row’s label as typed, a keystroke at a time, trailing space included (AC7)', async () => {
    const { wrapper, options } = harness(select([{ label: 'A', value: 'a' }]))

    await add(wrapper)

    for (const typed of ['D', 'Dr', 'Draft', 'Draft ', 'Draft x']) {
      await row(wrapper, 1, 'label').setValue(typed)

      expect(shown(wrapper, 1, 'label'), typed).toBe(typed)
      expect(shown(wrapper, 1, 'value'), typed).toBe('')
      expect(options(), typed).toStrictEqual([
        { label: 'A', value: 'a' },
        { label: typed, value: '' },
      ])
    }
  })

  it('emits options deep-equal to those loaded on an edit elsewhere in the panel (AC8, #732 AC4)', async () => {
    const { wrapper, latest } = harness(select(MIXED, 'number'))

    await wrapper.get('[data-testid="description-f"]').setValue('Pick one')

    expect(latest().description).toBe('Pick one')
    expect(latest().options).toStrictEqual(MIXED)
  })

  it('names each row’s inputs and button by the option’s number, and labels the group (AC9)', () => {
    const { wrapper } = harness(
      select([
        { label: 'A', value: 'a' },
        { label: 'B', value: 'b' },
      ]),
    )
    const group = wrapper.get('[data-testid="options-f"]')

    expect(group.attributes('role')).toBe('group')
    expect(wrapper.get(`#${group.attributes('aria-labelledby')}`).text()).toBe('Options')
    expect(
      [0, 1].flatMap((index) =>
        (['label', 'value', 'remove'] as const).map((part) =>
          row(wrapper, index, part).attributes('aria-label'),
        ),
      ),
    ).toEqual([
      'Option 1 label',
      'Option 1 value',
      'Remove option 1',
      'Option 2 label',
      'Option 2 value',
      'Remove option 2',
    ])
    expect(wrapper.get('[data-testid="options-f-add"]').text()).toBe('Add option')

    for (const control of group.findAll('input, button')) {
      expect(control.attributes('data-testid')).toMatch(/^options-f-/)
    }
  })
})
