import { mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, describe, expect, it } from 'vitest'
import { defineComponent, h, ref, type PropType } from 'vue'

import type { LogicVariable } from '@/components/logic-builder/logicTree'
import { jfssViolations } from '@/lib/testing/jfss-meta-schema'
import type { JfssComponent } from '@/types/jfss'

import FormPropertiesPanel from './FormPropertiesPanel.vue'

/**
 * **The panel never rewrites what it does not show** (#688 AC4).
 *
 * Each fixture carries every property the per-type table keeps as loaded and
 * does not offer — `defaultValue`, `readOnly`, `calculateMode`, `format`,
 * `enum`, `messages`, `uniqueItems`, `grid`, `theme`, a heading's `variant`, a
 * display node's `calculate` — plus a rule name outside the five, and a
 * `calculate` and a `conditional.logic` the builder cannot show (`min`, `if`).
 * Then **one offered property is edited, and the node's JSON is diffed**: it
 * must change at that path and nowhere else.
 *
 * The page-level half — a definition loaded and saved with nothing edited goes
 * out exactly as it came in, a `navigate` button included — is in
 * `FormBuilderPage.expressions.spec.ts`.
 *
 * # Seen to fail (coding standard §2.9)
 *
 * Run 2026-10-15 against this file alone, each mutation reverted after:
 *
 * | Mutation | Reddened |
 * |---|---|
 * | `DataFieldProperties` rebuilds `validation` as `{type, required}` on a required toggle (drops `messages`) | *textfield: required-f …* |
 * | `LayoutProperties` emits `{id, role, type, title, components}` (drops `grid`) | *panel: title-p …* |
 * | `ButtonProperties` emits `action: 'submit'` | *button: button-label-b …* |
 * | `switchType` deletes `calculateMode` | *textfield: type-f = textarea …* |
 */

/** The paths at which two JSON values differ: a leaf, or a key present on one side alone. */
function changedPaths(before: unknown, after: unknown, path = ''): string[] {
  if (JSON.stringify(before) === JSON.stringify(after)) {
    return []
  }

  const isRecord = (value: unknown): value is Record<string, unknown> =>
    typeof value === 'object' && value !== null

  if (isRecord(before) && isRecord(after) && Array.isArray(before) === Array.isArray(after)) {
    const keys = [...new Set([...Object.keys(before), ...Object.keys(after)])]

    return keys.flatMap((key) =>
      changedPaths(before[key], after[key], path === '' ? key : `${path}.${key}`),
    )
  }

  return [path]
}

const SCOPE: LogicVariable[] = [
  { path: 'qty', label: 'Quantity', type: 'number' },
  { path: 'code', label: 'Code', type: 'string' },
]

/** Every kept-as-loaded data property, a rule outside the five, and two expressions the builder cannot show. */
const FIELD = {
  id: 'f',
  role: 'data',
  type: 'textfield',
  key: 'code',
  label: 'Code',
  placeholder: 'ABC',
  description: 'The supplier’s code',
  defaultValue: 'ABC',
  readOnly: false,
  calculateMode: 'generated',
  validation: {
    type: 'string',
    required: true,
    minLength: 1,
    maxLength: 9,
    pattern: '^[A-Z]+$',
    format: 'email',
    enum: ['ABC', 'XYZ'],
    messages: { required: 'A code is needed.' },
  },
  rules: [
    { rule: 'regex', scope: 'both', params: { pattern: '^A' }, message: 'Starts with A.' },
    { rule: 'futureRule', scope: 'server', params: { depth: 2 }, message: 'Not yet.' },
  ],
  calculate: { min: [{ var: 'qty' }, 10] },
  conditional: { action: 'hide', logic: { if: [{ var: 'qty' }, true, false] } },
}

const GRID = {
  id: 'g',
  role: 'data',
  type: 'datagrid',
  key: 'lines',
  label: 'Lines',
  description: 'One per item',
  readOnly: false,
  sequenceKey: 'line_no',
  defaultItems: 1,
  validation: {
    type: 'array',
    required: true,
    uniqueBy: 'sku',
    uniqueItems: true,
    messages: { uniqueBy: 'A SKU appears twice.' },
  },
  components: [
    {
      id: 'line_no',
      role: 'data',
      type: 'number',
      key: 'line_no',
      label: 'No.',
      validation: { type: 'integer' },
    },
    {
      id: 'sku',
      role: 'data',
      type: 'textfield',
      key: 'sku',
      label: 'SKU',
      validation: { type: 'string' },
    },
  ],
  conditional: { action: 'show', logic: { if: [{ var: 'qty' }, true, false] } },
  rules: [{ rule: 'futureRule', scope: 'server', params: {}, message: 'Not yet.' }],
}

const PANEL = {
  id: 'p',
  role: 'layout',
  type: 'panel',
  title: 'Shipping',
  grid: { columns: 2, gap: 'md' },
  conditional: { action: 'show', logic: { var: 'qty' } },
  components: [],
}

const ALERT = {
  id: 'a',
  role: 'display',
  type: 'alert',
  content: 'Mind the cut-off',
  variant: 'info',
  calculate: { cat: ['Due ', { var: 'code' }] },
  conditional: { action: 'show', logic: { if: [{ var: 'qty' }, true, false] } },
}

const HEADING = {
  id: 'h',
  role: 'display',
  type: 'heading',
  content: 'Lines',
  variant: 'h2',
  calculate: { cat: ['Order ', { var: 'code' }] },
}

const BUTTON = {
  id: 'b',
  role: 'action',
  type: 'button',
  label: 'Go on',
  action: 'navigate',
  theme: 'secondary',
}

let mounted: VueWrapper | undefined

afterEach(() => {
  mounted?.unmount()
  mounted = undefined
})

function harness(initial: Record<string, unknown>) {
  const history: unknown[] = []

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
            history.push(node.value)
          },
        })
    },
  })

  const wrapper = mount(Host, { props: { start: initial } })

  mounted = wrapper

  return { wrapper, history }
}

type Edit = [testId: string, value: string | boolean, changed: string[]]

const CASES: [name: string, fixture: Record<string, unknown>, edits: Edit[]][] = [
  [
    'textfield',
    FIELD,
    [
      ['label-f', 'Supplier code', ['label']],
      ['key-f', 'supplier_code', ['key']],
      ['placeholder-f', 'XYZ', ['placeholder']],
      ['placeholder-f', '', ['placeholder']],
      ['description-f', 'Six letters', ['description']],
      ['required-f', false, ['validation.required']],
      ['min-length-f', '2', ['validation.minLength']],
      ['max-length-f', '', ['validation.maxLength']],
      ['pattern-f', '^[A-Z]{3}$', ['validation.pattern']],
      ['conditional-action-f', 'show', ['conditional.action']],
      ['rule-scope-f-0', 'client', ['rules.0.scope']],
      ['rule-params-f-0', '{"pattern": "^Z"}', ['rules.0.params.pattern']],
      ['rule-scope-f-1', 'both', ['rules.1.scope']],
      ['type-f', 'textarea', ['type']],
      ['type-f', 'select', ['type', 'options']],
      [
        'vtype-f',
        'number',
        ['validation.type', 'validation.minLength', 'validation.maxLength', 'validation.pattern'],
      ],
    ],
  ],
  [
    'datagrid',
    GRID,
    [
      ['label-g', 'Line items', ['label']],
      ['description-g', '', ['description']],
      ['required-g', false, ['validation.required']],
      ['sequence-key-g', 'sku', ['sequenceKey']],
      ['default-items-g', '3', ['defaultItems']],
      ['unique-by-g-line_no', true, ['validation.uniqueBy']],
      ['conditional-action-g', 'hide', ['conditional.action']],
      ['rule-scope-g-0', 'both', ['rules.0.scope']],
    ],
  ],
  ['panel', PANEL, [['title-p', 'Delivery', ['title']]]],
  [
    'alert',
    ALERT,
    [
      ['content-a', 'Ships Friday', ['content']],
      ['variant-a', 'warning', ['variant']],
      ['conditional-action-a', 'disable', ['conditional.action']],
      ['type-a', 'paragraph', ['type']],
    ],
  ],
  [
    'heading',
    HEADING,
    [
      ['content-h', 'Items', ['content']],
      ['type-h', 'paragraph', ['type']],
    ],
  ],
  ['button', BUTTON, [['button-label-b', 'Continue', ['label']]]],
]

describe('the properties panel’s round trip (AC4)', () => {
  const EDITS = CASES.flatMap(([name, fixture, edits]) =>
    edits.map(([testId, value, changed]) => ({ name, fixture, testId, value, changed })),
  )

  it.each(EDITS)(
    '$name: $testId = $value changes the definition at that path alone',
    async ({ fixture, testId, value, changed }) => {
      expect(jfssViolations({ formId: 'f', version: '2.0.1', components: [fixture] })).toEqual([])

      const { wrapper, history } = harness(fixture)

      // Mounting, the builders' raw boxes and the unknown rule included, writes nothing.
      expect(history).toEqual([])

      const control = wrapper.find(`[data-testid="${testId}"]`)

      expect(control.exists(), testId).toBe(true)
      await control.setValue(value)

      expect(history.length, testId).toBeGreaterThan(0)
      expect(changedPaths(fixture, history[history.length - 1])).toEqual(changed)
    },
  )

  it('changes nothing it does not show when an expression it can show is edited', async () => {
    const fixture = {
      ...FIELD,
      calculate: { '+': [{ max: [{ var: 'qty' }, 1] }, 2] },
    }
    const { wrapper, history } = harness(fixture)

    await wrapper.get('[aria-label="Calculate, operand 2: number"]').setValue('5')

    expect(changedPaths(fixture, history[history.length - 1])).toEqual(['calculate.+.1'])
  })
})
