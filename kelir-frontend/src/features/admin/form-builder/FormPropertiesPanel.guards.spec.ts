import { mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, describe, expect, it } from 'vitest'
import { defineComponent, h, nextTick, ref, type PropType } from 'vue'

import type { LogicVariable } from '@/components/logic-builder/logicTree'
import { jfssViolations } from '@/lib/testing/jfss-meta-schema'
import type { JfssComponent } from '@/types/jfss'

import FormPropertiesPanel from './FormPropertiesPanel.vue'

/**
 * **What the properties panel refuses to write, and what it keeps as typed**
 * (PR #732's gate findings: `schema-guardian` F1, F2, F3, F5, F7, and the
 * test-engineer campaign's defect 2).
 *
 * # Seen to fail (coding standard §2.9)
 *
 * Each case below was run, 2026-10-15, against the sources as they were at
 * `f1fdd33` (before these fixes), and failed there:
 *
 * | Case | How it failed before |
 * |---|---|
 * | *refuses a raw calculate / conditional that is not one operator …* (F1) | `null`, `5`, `[]`, `{}` and two keys were written |
 * | *writes only a plain object as a rule's params* (F2a) | `[]`, `null`, `5` and `"x"` were written |
 * | *keeps the params box as typed, key by key …* (F2b, defect 2) | the emptied box was rewritten to `{}` |
 * | *does not offer a calculated row field as the row number …* (F3) | `line_total` was offered and not flagged |
 * | *removes a cleared title and content* (F5) | `title: ''` and `content: ''` were written |
 * | *offers a row field inside a layout in the row template* (F7) | `in_panel` was not offered |
 */

const SCOPE: LogicVariable[] = [{ path: 'quantity', label: 'Quantity', type: 'number' }]

let mounted: VueWrapper | undefined

afterEach(() => {
  mounted?.unmount()
  mounted = undefined
  document.body.innerHTML = ''
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
          scope: SCOPE,
          onUpdate: (next: JfssComponent) => {
            node.value = JSON.parse(JSON.stringify(next)) as JfssComponent
            history.push(node.value as unknown as Record<string, unknown>)
          },
        })
    },
  })

  const wrapper = mount(Host, { props: { start: initial }, attachTo: document.body })

  mounted = wrapper

  return { wrapper, history, latest: () => history[history.length - 1] ?? initial }
}

function field(key: string, extra: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    id: key,
    role: 'data',
    type: 'textfield',
    key,
    label: key,
    validation: { type: 'string' },
    ...extra,
  }
}

const NOT_AN_EXPRESSION = ['null', '5', '[]', '{}', '"text"', '{"+": [1, 2], "-": [3, 1]}']

describe('the expressions’ raw box (F1)', () => {
  it.each([
    {
      name: 'calculate',
      start: { calculate: { var: 'quantity' } },
      label: 'Calculate',
      section: 'calculate-f',
      refused: 'calculate-refused-f',
      read: (node: Record<string, unknown>) => node.calculate,
    },
    {
      name: 'conditional.logic',
      start: { conditional: { action: 'hide', logic: { var: 'quantity' } } },
      label: 'Conditional logic',
      section: 'conditional-f',
      refused: 'conditional-refused-f',
      read: (node: Record<string, unknown>) => (node.conditional as { logic: unknown }).logic,
    },
  ])(
    'refuses a raw $name that is not one operator, and keeps the last good one',
    async ({ start, label, section, refused, read }) => {
      const { wrapper, history, latest } = harness(field('f', start))

      await wrapper.get(`[aria-label="Edit ${label} as JSON"]`).trigger('click')

      const box = () => wrapper.get(`[data-testid="${section}"] textarea`)

      for (const text of NOT_AN_EXPRESSION) {
        await box().setValue(text)

        expect(history, text).toEqual([])
        expect(wrapper.find(`[data-testid="${refused}"]`).exists(), text).toBe(true)
        expect(
          wrapper.get(`[data-testid="${section}"] [role="group"]`).attributes('aria-invalid'),
          text,
        ).toBe('true')
      }

      await box().setValue('{"+": [1, 2]}')

      expect(read(latest())).toEqual({ '+': [1, 2] })
      expect(wrapper.find(`[data-testid="${refused}"]`).exists()).toBe(false)
      expect(jfssViolations({ formId: 'f', version: '2.0.1', components: [latest()] })).toEqual([])
    },
  )
})

describe('a rule’s params box (F2, defect 2)', () => {
  const rule = { rule: 'regex', scope: 'both', params: { pattern: '^A' }, message: 'm' }

  it('writes only a plain object as a rule’s params', async () => {
    const { wrapper, history, latest } = harness(field('f', { rules: [rule] }))
    const box = () => wrapper.get('[data-testid="rule-params-f-0"]')

    for (const text of ['[]', 'null', '5', '"x"', '[{"a": 1}]', '{"half": ']) {
      await box().setValue(text)

      expect(history, text).toEqual([])
      expect(wrapper.find('[data-testid="rule-params-f-0-refused"]').exists(), text).toBe(true)
      expect(box().attributes('aria-invalid'), text).toBe('true')
    }

    await box().setValue('{"pattern": "^Z"}')

    expect((latest().rules as { params: unknown }[])[0].params).toEqual({ pattern: '^Z' })
    expect(wrapper.find('[data-testid="rule-params-f-0-refused"]').exists()).toBe(false)
  })

  it('keeps the params box as typed, key by key, and writes what was typed', async () => {
    const { wrapper, latest } = harness(field('f', { rules: [rule] }))
    const box = () => wrapper.get<HTMLTextAreaElement>('[data-testid="rule-params-f-0"]')

    await box().setValue('')

    // Emptied: no parameters are `{}`, and the box is not rewritten to say so.
    expect((latest().rules as { params: unknown }[])[0].params).toEqual({})
    expect(box().element.value).toBe('')

    const typed = '{"min":1,"max":5}'

    for (let at = 1; at <= typed.length; at += 1) {
      await box().setValue(typed.slice(0, at))
      await nextTick()

      expect(box().element.value, `after ${at} keys`).toBe(typed.slice(0, at))
    }

    expect(box().element.value).toBe(typed)
    expect((latest().rules as { params: unknown }[])[0].params).toEqual({ min: 1, max: 5 })
  })

  it('follows a change of rule, and gives way on blur to the params the rule holds', async () => {
    const { wrapper } = harness(field('f', { rules: [rule] }))
    const box = () => wrapper.get<HTMLTextAreaElement>('[data-testid="rule-params-f-0"]')

    await wrapper.get('[data-testid="rule-f-0"]').setValue('oneOf')

    expect(JSON.parse(box().element.value)).toEqual({ values: [] })

    await box().setValue('{"values": [')
    await box().trigger('blur')

    expect(JSON.parse(box().element.value)).toEqual({ values: [] })
    expect(wrapper.find('[data-testid="rule-params-f-0-refused"]').exists()).toBe(false)
  })
})

describe('a data grid’s row fields (F3, F7)', () => {
  function grid(extra: Record<string, unknown> = {}): Record<string, unknown> {
    return {
      id: 'g',
      role: 'data',
      type: 'datagrid',
      key: 'lines',
      label: 'Lines',
      validation: { type: 'array' },
      components: [
        field('line_no', { type: 'number', validation: { type: 'integer' } }),
        field('line_total', {
          type: 'number',
          validation: { type: 'number' },
          calculate: { '*': [{ var: 'line_no' }, 2] },
        }),
        {
          id: 'panel_1',
          role: 'layout',
          type: 'panel',
          title: 'More',
          components: [field('in_panel')],
        },
      ],
      ...extra,
    }
  }

  function options(wrapper: VueWrapper): string[] {
    return wrapper
      .get('[data-testid="sequence-key-g"]')
      .findAll('option')
      .filter((option) => option.attributes('disabled') === undefined)
      .map((option) => option.attributes('value') ?? '')
  }

  it('does not offer a calculated row field as the row number, and flags a loaded one (S12.7)', async () => {
    const offered = harness(grid())

    expect(options(offered.wrapper)).not.toContain('line_total')
    offered.wrapper.unmount()

    const loaded = harness(grid({ sequenceKey: 'line_total' }))

    expect(loaded.wrapper.find('[data-testid="sequence-key-calculated-g"]').text()).toContain(
      'line_total',
    )

    await loaded.wrapper.get('[data-testid="label-g"]').setValue('Line items')

    expect(loaded.latest()).toMatchObject({ sequenceKey: 'line_total', label: 'Line items' })
  })

  it('offers a row field inside a layout in the row template, as the server reads the row', () => {
    const { wrapper } = harness(grid())

    expect(options(wrapper)).toEqual(['', 'line_no', 'in_panel'])
    expect(wrapper.find('[data-testid="unique-by-g-in_panel"]').exists()).toBe(true)
    expect(wrapper.find('[data-testid="unique-by-g-line_total"]').exists()).toBe(true)
  })
})

describe('optional text (F5)', () => {
  it('removes a cleared title and content, never writing an empty one', async () => {
    const panel = harness({ id: 'p', role: 'layout', type: 'panel', title: 'P', components: [] })

    await panel.wrapper.get('[data-testid="title-p"]').setValue('')

    expect(panel.latest()).toEqual({ id: 'p', role: 'layout', type: 'panel', components: [] })
    panel.wrapper.unmount()

    const heading = harness({ id: 'h', role: 'display', type: 'heading', content: 'H' })

    await heading.wrapper.get('[data-testid="content-h"]').setValue('')

    expect(heading.latest()).toEqual({ id: 'h', role: 'display', type: 'heading' })
  })
})
