import { mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, describe, expect, it } from 'vitest'
import { defineComponent, h, ref, type PropType } from 'vue'

import JsonLogicBuilder from '@/components/logic-builder/JsonLogicBuilder.vue'
import type { LogicVariable } from '@/components/logic-builder/logicTree'
import { jfssViolations } from '@/lib/testing/jfss-meta-schema'
import type { ValidationDetail } from '@/types/api'
import type { JfssAdvancedRule, JfssComponent } from '@/types/jfss'

import { offeredEntries } from './builderRegistry'
import FormPropertiesPanel from './FormPropertiesPanel.vue'
import ValidationRulesSection from './ValidationRulesSection.vue'

/**
 * The properties panel, mounted for one selected node (#688 part 2: AC1–AC7,
 * AC9, AC10; part 1's A3, A4, E3, E7, F2 carried over from
 * `FormComponentEditor.spec.ts`).
 *
 * **The host replaces the node with what the panel emits**, as the page does,
 * and **clones it on the way**, as the page's `replaceNode` does — so nothing
 * here passes because the panel was handed back its own object. The page's
 * real write path is driven in `FormBuilderPage.expressions.spec.ts`.
 *
 * # Seen to fail (coding standard §2.9)
 *
 * Run 2026-10-15 against this file alone, each mutation reverted after:
 *
 * | Mutation | Reddened |
 * |---|---|
 * | `placeholder` written as `placeHolder` (a stray key) | *every edit of a …* for each data type but `datagrid` (AC2) |
 * | the two tiers swapped in `ExpressionProperties` | *mounts calculate in the calculate tier …* (AC5) |
 * | `calculate` offered its own key | *offers calculate the scope without its own key …* (AC6) |
 * | a cleared expression writes `null` | *removes calculate when it is cleared …*, *every edit of a …* (AC2, AC7) |
 * | a cleared `conditional.logic` leaves `{action}` | *removes the whole conditional …* (AC7) |
 * | `disabled` not passed to the conditional builder | *disables every control …* (AC7) |
 * | `PLACEHOLDER_TYPES` gains `date` | *has a body for all 18 …*, *offers description on data nodes, placeholder where …* (AC1, AC3b) |
 * | `limitsFor('boolean')` offers `minimum` | *offers the limits that fit validation.type* (AC3b) |
 * | `LIMITS_BY_COMPONENT` offers `minLength` on `date` | *has a body for all 18 …*, *offers string limits on a text box alone …* (AC1, AC3b) |
 * | the length note shown on `conflicts.range` | *flags a lower limit above its upper one …* (AC3b) |
 * | `success` offered as an alert variant | *offers an alert info and warning alone …* (AC3b) |
 * | `withProperty` writes `''` for a cleared value | *clears a limit, a placeholder and a description …* (AC3b) |
 * | `switchValueType` keeps every limit | *removes only the limits the new value type cannot carry* (AC3b) |
 * | `toggleUniqueBy` always writes a list | *keeps uniqueBy in its loaded shape …* (AC3) |
 * | `defaultItems` accepts any number | *refuses a negative or non-integer defaultItems* (AC3) |
 * | the params message dropped from the rules section | *shows a detail at …rules.N.params …* (AC9) |
 * | `rule-scope-` renamed | *keeps the rules section's test ids …* (AC10) |
 */

const ENFORCEABLE = ['matchesField', 'notMatchesField', 'regex', 'oneOf', 'notOneOf']

const SCOPE: LogicVariable[] = [
  { path: 'node_1', label: 'Itself', type: 'string' },
  { path: 'quantity', label: 'Quantity', type: 'number' },
  { path: 'unit_price', label: 'Unit price', type: 'number' },
]

let mounted: VueWrapper | undefined

afterEach(() => {
  mounted?.unmount()
  mounted = undefined
  document.body.innerHTML = ''
})

interface HarnessOptions {
  details?: ValidationDetail[]
  scope?: LogicVariable[]
  disabled?: boolean
}

/** A JSON copy, as `formTree.replaceNode` makes on every write. */
function clone<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T
}

function harness(initial: JfssComponent, options: HarnessOptions = {}) {
  const history: JfssComponent[] = []

  const Host = defineComponent({
    props: { start: { type: Object as PropType<JfssComponent>, required: true } },
    setup(props) {
      const node = ref<JfssComponent>(clone(props.start))
      const source = ref('')

      return () =>
        h(FormPropertiesPanel, {
          component: node.value,
          path: 'components.0',
          disabled: options.disabled ?? false,
          details: options.details ?? [],
          lookupSource: source.value,
          scope: options.scope ?? SCOPE,
          onUpdate: (next: JfssComponent) => {
            node.value = clone(next)
            history.push(clone(next))
          },
          'onUpdate:lookupSource': (next: string) => {
            source.value = next
          },
        })
    },
  })

  const wrapper = mount(Host, { props: { start: initial } })

  mounted = wrapper

  return {
    wrapper,
    latest: () => history[history.length - 1] ?? initial,
    count: () => history.length,
  }
}

function violations(node: JfssComponent): string[] {
  return jfssViolations({ formId: 'f', version: '2.0.1', components: [node] })
}

/** The enabled controls with a test id, in document order. */
function controls(wrapper: VueWrapper, selector: string): string[] {
  return wrapper
    .findAll(selector)
    .filter((element) => element.attributes('disabled') === undefined)
    .map((element) => element.attributes('data-testid') ?? '')
    .filter((id) => id !== '')
}

function byLabel(wrapper: VueWrapper, name: string) {
  return wrapper.get(`[aria-label="${name}"]`)
}

function textField(extra: Record<string, unknown> = {}): JfssComponent {
  return {
    id: 'f',
    role: 'data',
    type: 'textfield',
    key: 'f',
    label: 'F',
    validation: { type: 'string' },
    ...extra,
  } as JfssComponent
}

function rowField(key: string): Record<string, unknown> {
  return {
    id: `row_${key}`,
    role: 'data',
    type: 'textfield',
    key,
    label: key,
    validation: { type: 'string' },
  }
}

function grid(extra: Record<string, unknown> = {}): JfssComponent {
  return {
    id: 'g',
    role: 'data',
    type: 'datagrid',
    key: 'lines',
    label: 'Lines',
    validation: { type: 'array' },
    components: [rowField('line_no'), rowField('sku')],
    ...extra,
  } as unknown as JfssComponent
}

/** What each value box is given, by its test id's prefix; the rest get `Edited_value`. */
const VALUE_FOR: Record<string, string> = {
  'min-length': '3',
  'max-length': '5',
  'default-items': '2',
  minimum: '-2.5',
  maximum: '10',
  pattern: '^[A-Z]+$',
}

type Read = (node: Record<string, unknown>) => unknown

const validationOf = (node: Record<string, unknown>) => node.validation as Record<string, unknown>

/** Optional properties: each is also cleared, and must then be absent. */
const OPTIONAL: Record<string, Read> = {
  placeholder: (node) => node.placeholder,
  description: (node) => node.description,
  'min-length': (node) => validationOf(node).minLength,
  'max-length': (node) => validationOf(node).maxLength,
  minimum: (node) => validationOf(node).minimum,
  maximum: (node) => validationOf(node).maximum,
  pattern: (node) => validationOf(node).pattern,
  'default-items': (node) => node.defaultItems,
}

const TEXT_FOR: Record<string, string> = {
  options: 'First=first\nSecond',
  'rule-params': '{"pattern": "^[A-Z]+$"}',
}

function prefixOf(id: string, table: Record<string, unknown>): string | undefined {
  return Object.keys(table).find((prefix) => id.startsWith(`${prefix}-`))
}

/** Starts an expression in a builder, from nothing, and checks the node after. */
async function buildExpression(
  wrapper: VueWrapper,
  label: string,
  check: (what: string) => void,
): Promise<boolean> {
  const kind = wrapper.find(`[aria-label="${label}: kind"]`)

  if (!kind.exists() || kind.attributes('disabled') !== undefined) {
    return false
  }

  await kind.setValue('var')
  await byLabel(wrapper, `${label}: variable`).setValue('quantity')
  check(`${label} built`)

  return true
}

/**
 * Drives every enabled control the panel draws, both builders included, and
 * validates the node against the 2.0.1 meta-schema after each change (AC2).
 */
async function editEverything(node: JfssComponent): Promise<number> {
  const { wrapper, latest, count } = harness(node)
  const check = (what: string) => expect(violations(latest()), what).toEqual([])
  const read = () => latest() as unknown as Record<string, unknown>

  const addRule = wrapper.find(`[data-testid="add-rule-${node.id}"]`)

  if (addRule.exists()) {
    await addRule.trigger('click')
    check('add rule')
  }

  for (const id of controls(wrapper, 'input:not([type="checkbox"])')) {
    const kind = prefixOf(id, VALUE_FOR)

    await wrapper.find(`[data-testid="${id}"]`).setValue(kind ? VALUE_FOR[kind] : 'Edited_value')
    check(id)

    const optional = prefixOf(id, OPTIONAL)

    if (optional) {
      await wrapper.find(`[data-testid="${id}"]`).setValue('')
      check(`${id} cleared`)
      expect(OPTIONAL[optional](read()), `${id} cleared`).toBeUndefined()
      // And back, so the rest of the drive runs with it set.
      await wrapper.find(`[data-testid="${id}"]`).setValue(VALUE_FOR[optional] ?? 'Edited_value')
      check(`${id} again`)
    }
  }

  for (const id of controls(wrapper, 'input[type="checkbox"]')) {
    await wrapper.find(`[data-testid="${id}"]`).setValue(true)
    check(`${id} on`)
    await wrapper.find(`[data-testid="${id}"]`).setValue(false)
    check(`${id} off`)
  }

  for (const id of controls(wrapper, 'textarea')) {
    const kind = prefixOf(id, TEXT_FOR)

    await wrapper.find(`[data-testid="${id}"]`).setValue(kind ? TEXT_FOR[kind] : '{}')
    check(id)
    await wrapper.find(`[data-testid="${id}"]`).setValue('')
    check(`${id} cleared`)
  }

  // The two builders: build, check, and (for conditional) drive its action.
  if (await buildExpression(wrapper, 'Calculate', check)) {
    await byLabel(wrapper, 'Clear Calculate').trigger('click')
    check('calculate cleared')
    expect(latest()).not.toHaveProperty('calculate')
  }

  if (await buildExpression(wrapper, 'Conditional logic', check)) {
    for (const action of ['hide', 'enable', 'disable', 'show']) {
      await wrapper.find(`[data-testid="conditional-action-${node.id}"]`).setValue(action)
      check(`conditional action ${action}`)
    }

    await byLabel(wrapper, 'Clear Conditional logic').trigger('click')
    check('conditional cleared')
    expect(latest()).not.toHaveProperty('conditional')
  }

  // Selects last, and the type select last of all: it changes which controls exist.
  const allSelects = controls(wrapper, 'select')
  const selects = [
    ...allSelects.filter((id) => !id.startsWith('type-')),
    ...allSelects.filter((id) => id.startsWith('type-')),
  ]

  for (const id of selects) {
    const select = wrapper.find(`[data-testid="${id}"]`)

    if (!select.exists() || select.attributes('disabled') !== undefined) {
      continue
    }

    const values = select
      .findAll('option')
      .filter((option) => option.attributes('disabled') === undefined)
      .map((option) => option.attributes('value') ?? '')

    // Every value, and the empty one (a cleared choice) last.
    for (const value of [...values.filter((v) => v !== ''), ...values.filter((v) => v === '')]) {
      await wrapper.find(`[data-testid="${id}"]`).setValue(value)
      check(`${id} = ${value}`)
    }
  }

  return count()
}

/** A node, as the palette creates it; a data grid with a row template, so its keys are offered. */
function fresh(type: string): JfssComponent {
  const entry = offeredEntries().find((candidate) => candidate.type === type)!
  const node = entry.create({ id: 'node_1', key: 'node_1' })

  return type === 'datagrid'
    ? ({ ...node, components: [rowField('line_no'), rowField('sku')] } as unknown as JfssComponent)
    : node
}

describe('FormPropertiesPanel', () => {
  // --- AC1, AC2: a body for every type, and every edit validates ----------------

  it.each(offeredEntries().map((entry) => [entry.type] as const))(
    'every edit of a %s validates against the meta-schema (AC2, part 1 E3)',
    async (type) => {
      const node = fresh(type)

      expect(violations(node)).toEqual([])

      const edits = await editEverything(node)

      // Columns and tabs have nothing to edit; every other type has something.
      if (type === 'columns' || type === 'tabs') {
        expect(edits).toBe(0)
      } else {
        expect(edits).toBeGreaterThan(0)
      }
    },
  )

  /** A data body; `placeholder` only where the renderer draws one (D-116). */
  const DATA = ['type', 'key', 'label', 'placeholder', 'description', 'vtype', 'required']
  const DATA_NO_PLACEHOLDER = DATA.filter((id) => id !== 'placeholder')
  const STRING_LIMITS = ['min-length', 'max-length', 'pattern']

  /** What each type's body offers, by test id prefix, in the table's order (AC1). */
  const BODIES: Record<string, string[]> = {
    textfield: [...DATA, ...STRING_LIMITS],
    textarea: [...DATA, ...STRING_LIMITS],
    // A string's limits on a text box alone, though these carry `string` too.
    date: DATA_NO_PLACEHOLDER,
    select: [...DATA, 'options'],
    radio: [...DATA_NO_PLACEHOLDER, 'options'],
    lookup: [...DATA, 'lookup'],
    number: [...DATA, 'minimum', 'maximum'],
    checkbox: DATA_NO_PLACEHOLDER,
    datagrid: [
      'key',
      'label',
      'description',
      'required',
      'sequence-key',
      'default-items',
      'unique-by-node_1-line_no',
      'unique-by-node_1-sku',
    ],
    panel: ['title'],
    fieldset: ['title'],
    columns: [],
    tabs: [],
    heading: ['type', 'content'],
    paragraph: ['type', 'content'],
    divider: ['type'],
    alert: ['type', 'content', 'variant'],
    button: ['button-label'],
  }

  /** Where each kind of node is offered expressions (AC1, the table). */
  const EXPRESSIONS: Record<string, { calculate: boolean; conditional: boolean }> = {
    data: { calculate: true, conditional: true },
    datagrid: { calculate: false, conditional: true },
    layout: { calculate: false, conditional: false },
    display: { calculate: false, conditional: true },
    action: { calculate: false, conditional: false },
  }

  it('has a body for all 18 SUPPORTED types, as the table says (AC1)', () => {
    expect(Object.keys(BODIES).sort()).toEqual(
      offeredEntries()
        .map((entry) => entry.type)
        .sort(),
    )

    for (const entry of offeredEntries()) {
      const { wrapper } = harness(fresh(entry.type))
      const offered = controls(wrapper, 'input, select, textarea').map((id) =>
        id.startsWith('unique-by-') ? id : id.slice(0, id.length - '-node_1'.length),
      )

      expect(wrapper.find('[data-testid="editor-node_1"]').exists(), entry.type).toBe(true)
      expect(offered, entry.type).toEqual(BODIES[entry.type])

      const where = EXPRESSIONS[entry.type === 'datagrid' ? 'datagrid' : entry.role]

      expect(wrapper.find('[data-testid="calculate-node_1"]').exists(), entry.type).toBe(
        where.calculate,
      )
      expect(wrapper.find('[data-testid="conditional-node_1"]').exists(), entry.type).toBe(
        where.conditional,
      )
      expect(wrapper.find('[data-testid="add-rule-node_1"]').exists(), entry.type).toBe(
        entry.role === 'data',
      )

      wrapper.unmount()
    }
  })

  // --- Part 1's behaviours, kept (AC1) -------------------------------------------

  it('offers a panel and a fieldset their title and nothing else (A3)', () => {
    for (const type of ['panel', 'fieldset']) {
      const { wrapper } = harness({
        id: 'p',
        role: 'layout',
        type,
        title: 'P',
        components: [],
      } as JfssComponent)

      expect(controls(wrapper, 'input, select, textarea')).toEqual(['title-p'])
      wrapper.unmount()
    }
  })

  it('offers columns and tabs no property body, and shows their type read-only (A3)', () => {
    for (const node of [
      { id: 'c', role: 'layout', type: 'columns', columns: [{ components: [] }] },
      { id: 'c', role: 'layout', type: 'tabs', tabs: [{ title: 'T', components: [] }] },
    ]) {
      const { wrapper } = harness(node as JfssComponent)

      expect(controls(wrapper, 'input, select, textarea')).toEqual([])
      expect(wrapper.find('[data-testid="type-c"]').attributes('disabled')).toBeDefined()
      wrapper.unmount()
    }
  })

  it('offers a button its label, and keeps a loaded action it does not offer (A3)', async () => {
    const { wrapper, latest } = harness({
      id: 'b',
      role: 'action',
      type: 'button',
      label: 'Go',
      action: 'reset',
    } as JfssComponent)

    expect(controls(wrapper, 'input, select, textarea')).toEqual(['button-label-b'])
    expect(
      (wrapper.find('[data-testid="button-action-b"]').element as HTMLInputElement).value,
    ).toBe('reset')

    await wrapper.find('[data-testid="button-label-b"]').setValue('Send')

    expect(latest()).toMatchObject({ label: 'Send', action: 'reset' })
  })

  it('switches a data type among the data types, datagrid excluded, and display among display (A3)', () => {
    const options = (wrapper: VueWrapper, id: string) =>
      wrapper
        .find(`[data-testid="type-${id}"]`)
        .findAll('option')
        .map((option) => option.attributes('value'))
    const data = harness(textField())

    expect(options(data.wrapper, 'f')).toEqual([
      'textfield',
      'textarea',
      'number',
      'date',
      'select',
      'radio',
      'checkbox',
      'lookup',
    ])
    data.wrapper.unmount()

    const display = harness({
      id: 'd',
      role: 'display',
      type: 'heading',
      content: 'H',
    } as JfssComponent)

    expect(options(display.wrapper, 'd')).toEqual(['heading', 'paragraph', 'divider', 'alert'])
  })

  it('shows a data grid’s type read-only and does not offer its value type (A3)', () => {
    const { wrapper } = harness(grid())

    expect(wrapper.find('[data-testid="type-g"]').attributes('disabled')).toBeDefined()
    expect(wrapper.find('[data-testid="vtype-g"]').exists()).toBe(false)
  })

  it('has no Up, Down or Remove of its own (A4)', () => {
    const { wrapper } = harness(textField())

    for (const id of ['up-f', 'down-f', 'remove-f']) {
      expect(wrapper.find(`[data-testid="${id}"]`).exists()).toBe(false)
    }
  })

  it('adds a rule the meta-schema accepts, and an emptied params box is {} (E7)', async () => {
    const { wrapper, latest } = harness(textField())

    await wrapper.find('[data-testid="add-rule-f"]').trigger('click')

    expect((latest() as { rules: unknown[] }).rules[0]).toEqual({
      rule: 'matchesField',
      scope: 'both',
      params: { target: '' },
      message: 'This value is not valid.',
    })
    expect(violations(latest())).toEqual([])

    await wrapper.find('[data-testid="rule-params-f-0"]').setValue('{"target": "other"}')
    await wrapper.find('[data-testid="rule-params-f-0"]').setValue('')

    expect((latest() as { rules: { params: unknown }[] }).rules[0].params).toEqual({})
  })

  it('offers the five enforceable rules and no others (F2, AC11)', async () => {
    const { wrapper } = harness(textField())

    await wrapper.find('[data-testid="add-rule-f"]').trigger('click')

    expect(
      wrapper
        .find('[data-testid="rule-f-0"]')
        .findAll('option')
        .map((option) => option.attributes('value')),
    ).toEqual(ENFORCEABLE)
  })

  it.each([
    ['matchesField', { target: '' }],
    ['notMatchesField', { target: '' }],
    ['regex', { pattern: '' }],
    ['oneOf', { values: [] }],
    ['notOneOf', { values: [] }],
  ])(
    'seeds %s with its own empty parameters when the rule is chosen (M1)',
    async (rule, params) => {
      const { wrapper, latest } = harness(textField())

      await wrapper.find('[data-testid="add-rule-f"]').trigger('click')
      // Chosen from another rule, with parameters typed for that one: a change
      // of rule replaces them.
      await wrapper.find('[data-testid="rule-f-0"]').setValue(rule === 'regex' ? 'oneOf' : 'regex')
      await wrapper.find('[data-testid="rule-params-f-0"]').setValue('{"other": "x"}')
      await wrapper.find('[data-testid="rule-f-0"]').setValue(rule)

      expect((latest() as { rules: { rule: string; params: unknown }[] }).rules[0]).toMatchObject({
        rule,
        params,
      })
      expect(violations(latest())).toEqual([])
    },
  )

  it('drops content on a switch to divider, and gives it back on a switch away (M2)', async () => {
    const { wrapper, latest } = harness({
      id: 'd',
      role: 'display',
      type: 'heading',
      content: 'Shipping',
    } as JfssComponent)

    await wrapper.find('[data-testid="type-d"]').setValue('divider')

    expect(latest()).toEqual({ id: 'd', role: 'display', type: 'divider' })

    await wrapper.find('[data-testid="type-d"]').setValue('alert')

    expect(latest()).toEqual({ id: 'd', role: 'display', type: 'alert', content: 'Alert' })

    // Text an author wrote is kept between types that draw text.
    await wrapper.find('[data-testid="content-d"]').setValue('Read this')
    await wrapper.find('[data-testid="type-d"]').setValue('paragraph')

    expect(latest()).toMatchObject({ type: 'paragraph', content: 'Read this' })
  })

  it('gives a select or radio its default option, and keeps options it has (L)', async () => {
    const { wrapper, latest } = harness(textField())

    await wrapper.find('[data-testid="type-f"]').setValue('select')

    expect((latest() as { options: unknown[] }).options).toEqual([
      { label: 'Option 1', value: 'option_1' },
    ])

    await wrapper.find('[data-testid="options-f"]').setValue('Air=air\nSea=sea')
    await wrapper.find('[data-testid="type-f"]').setValue('radio')

    expect((latest() as { options: unknown[] }).options).toEqual([
      { label: 'Air', value: 'air' },
      { label: 'Sea', value: 'sea' },
    ])
    expect(violations(latest())).toEqual([])
  })

  it('drops static options on a switch to lookup, and says the lookup needs a source (L)', async () => {
    const { wrapper, latest } = harness(
      textField({ type: 'select', options: [{ label: 'A', value: 'a' }] }),
    )

    await wrapper.find('[data-testid="type-f"]').setValue('lookup')

    expect(latest()).not.toHaveProperty('options')
    expect(wrapper.find('[data-testid="lookup-unbound-f"]').exists()).toBe(true)

    await wrapper.find('[data-testid="lookup-f"]').setValue('supplier')

    expect(wrapper.find('[data-testid="lookup-unbound-f"]').exists()).toBe(false)
  })

  // --- AC3: the data grid ---------------------------------------------------------

  describe('a data grid (AC3)', () => {
    function optionValues(wrapper: VueWrapper, id: string): string[] {
      return wrapper
        .find(`[data-testid="${id}"]`)
        .findAll('option')
        .map((option) => option.attributes('value') ?? '')
    }

    it('offers sequenceKey and uniqueBy only the row template’s direct data keys', () => {
      const { wrapper } = harness(
        grid({
          components: [
            rowField('line_no'),
            { id: 'note', role: 'display', type: 'paragraph', content: 'Each line' },
            rowField('sku'),
            {
              id: 'sub',
              role: 'data',
              type: 'datagrid',
              key: 'parts',
              label: 'Parts',
              validation: { type: 'array' },
              components: [rowField('deep')],
            },
          ],
        }),
      )

      expect(optionValues(wrapper, 'sequence-key-g')).toEqual(['', 'line_no', 'sku', 'parts'])
      expect(
        wrapper
          .findAll('input[data-testid^="unique-by-g-"]')
          .map((element) => element.attributes('data-testid')),
      ).toEqual(['unique-by-g-line_no', 'unique-by-g-sku', 'unique-by-g-parts'])
    })

    it('keeps and flags a loaded sequenceKey or uniqueBy key that names no row field', async () => {
      const { wrapper, latest } = harness(
        grid({ sequenceKey: 'ghost', validation: { type: 'array', uniqueBy: ['sku', 'gone'] } }),
      )

      expect(wrapper.find('[data-testid="sequence-key-unknown-g"]').text()).toContain('ghost')
      expect(
        (wrapper.find('[data-testid="sequence-key-g"]').element as HTMLSelectElement).value,
      ).toBe('ghost')
      expect(wrapper.find('[data-testid="unique-by-unknown-g-gone"]').exists()).toBe(true)

      await wrapper.find('[data-testid="label-g"]').setValue('Line items')
      await wrapper.find('[data-testid="unique-by-g-line_no"]').setValue(true)

      expect(latest()).toMatchObject({
        sequenceKey: 'ghost',
        validation: { uniqueBy: ['sku', 'gone', 'line_no'] },
      })

      // Unchecked, it goes, as any key does.
      await wrapper.find('[data-testid="unique-by-g-gone"]').setValue(false)

      expect((latest() as { validation: { uniqueBy: unknown } }).validation.uniqueBy).toEqual([
        'sku',
        'line_no',
      ])
    })

    it('refuses a negative or non-integer defaultItems, and writes a whole number', async () => {
      const { wrapper, latest, count } = harness(grid({ defaultItems: 1 }))

      for (const typed of ['-1', '2.5', 'two', '1e1']) {
        await wrapper.find('[data-testid="default-items-g"]').setValue(typed)

        expect(count(), typed).toBe(0)
        expect(wrapper.find('[data-testid="default-items-g-refused"]').exists(), typed).toBe(true)
      }

      await wrapper.find('[data-testid="default-items-g"]').setValue('2')

      expect(latest()).toMatchObject({ defaultItems: 2 })
      expect(wrapper.find('[data-testid="default-items-g-refused"]').exists()).toBe(false)

      await wrapper.find('[data-testid="default-items-g"]').setValue('0')

      expect(latest()).toMatchObject({ defaultItems: 0 })
      expect(violations(latest())).toEqual([])
    })

    it('keeps uniqueBy in its loaded shape: a string until a second key, a list as a list', async () => {
      const asString = harness(grid({ validation: { type: 'array', uniqueBy: 'sku' } }))

      await asString.wrapper.find('[data-testid="label-g"]').setValue('Edited')
      expect(asString.latest()).toMatchObject({ validation: { uniqueBy: 'sku' } })

      await asString.wrapper.find('[data-testid="unique-by-g-line_no"]').setValue(true)
      expect(asString.latest()).toMatchObject({ validation: { uniqueBy: ['sku', 'line_no'] } })
      asString.wrapper.unmount()

      const asList = harness(grid({ validation: { type: 'array', uniqueBy: ['sku', 'line_no'] } }))

      await asList.wrapper.find('[data-testid="unique-by-g-line_no"]').setValue(false)
      expect(asList.latest()).toMatchObject({ validation: { uniqueBy: ['sku'] } })
      asList.wrapper.unmount()

      const none = harness(grid())

      await none.wrapper.find('[data-testid="unique-by-g-sku"]').setValue(true)
      expect(none.latest()).toMatchObject({ validation: { uniqueBy: 'sku' } })
    })

    it('removes sequenceKey, defaultItems and uniqueBy when cleared, never writing null or empty', async () => {
      const { wrapper, latest } = harness(
        grid({
          sequenceKey: 'line_no',
          defaultItems: 2,
          description: 'Lines',
          validation: { type: 'array', uniqueBy: 'sku' },
        }),
      )

      await wrapper.find('[data-testid="sequence-key-g"]').setValue('')
      await wrapper.find('[data-testid="default-items-g"]').setValue('')
      await wrapper.find('[data-testid="unique-by-g-sku"]').setValue(false)
      await wrapper.find('[data-testid="description-g"]').setValue('')

      expect(latest()).toEqual(grid())
      expect(violations(latest())).toEqual([])
    })
  })

  // --- AC3b: limits and texts (D-116) ----------------------------------------------

  describe('the limits and text properties (AC3b, D-116)', () => {
    it.each([
      ['string', ['min-length-f', 'max-length-f', 'pattern-f']],
      ['number', ['minimum-f', 'maximum-f']],
      ['integer', ['minimum-f', 'maximum-f']],
      ['boolean', []],
      ['array', []],
      ['object', []],
    ])('offers the limits that fit validation.type %s', (type, expected) => {
      const limits = ['min-length-f', 'max-length-f', 'pattern-f', 'minimum-f', 'maximum-f']
      const offered = (node: JfssComponent) => {
        const { wrapper } = harness(node)
        const found = limits.filter((id) => wrapper.find(`[data-testid="${id}"]`).exists())

        wrapper.unmount()

        return found
      }

      // Each limit where its component type uses it: lengths and a pattern on a
      // text box, bounds on a number.
      const onText = offered(textField({ validation: { type } }))
      const onNumber = offered(textField({ type: 'number', validation: { type } }))

      expect([...onText, ...onNumber]).toEqual(expected)
    })

    it.each(['date', 'select', 'radio', 'lookup', 'checkbox'])(
      'offers string limits on a text box alone: none on a %s, and keeps one it was loaded with',
      async (type) => {
        const validation = { type: 'string', minLength: 2, maxLength: 9, pattern: '^[A-Z]' }
        const extra =
          type === 'select' || type === 'radio' ? { options: [{ label: 'A', value: 'A' }] } : {}
        const { wrapper, latest } = harness(textField({ type, validation, ...extra }))

        expect(wrapper.find('[data-testid="limits-f"]').exists()).toBe(false)

        await wrapper.find('[data-testid="label-f"]').setValue('Edited')

        expect(latest()).toMatchObject({ validation })
      },
    )

    it('offers description on data nodes, placeholder where the renderer draws one, and variant on an alert alone', () => {
      for (const entry of offeredEntries()) {
        const { wrapper } = harness(fresh(entry.type))
        const has = (prefix: string) => wrapper.find(`[data-testid="${prefix}-node_1"]`).exists()

        // Where the renderer draws one: `FormPropertiesPanel.renders.spec.ts`
        // holds this list against the renderer's fields.
        expect(has('placeholder'), entry.type).toBe(
          ['textfield', 'textarea', 'number', 'select', 'lookup'].includes(entry.type),
        )
        expect(has('description'), entry.type).toBe(entry.role === 'data')
        expect(has('variant'), entry.type).toBe(entry.type === 'alert')
        wrapper.unmount()
      }
    })

    it('takes a length as a whole number of 0 or more, and a bound as any number', async () => {
      const text = harness(textField())

      for (const typed of ['-1', '1.5', 'x']) {
        await text.wrapper.find('[data-testid="min-length-f"]').setValue(typed)
        expect(text.count(), typed).toBe(0)
        expect(text.wrapper.find('[data-testid="min-length-f-refused"]').exists()).toBe(true)
      }

      await text.wrapper.find('[data-testid="max-length-f"]').setValue('0')
      expect(text.latest()).toMatchObject({ validation: { maxLength: 0 } })
      text.wrapper.unmount()

      const number = harness(textField({ type: 'number', validation: { type: 'number' } }))

      await number.wrapper.find('[data-testid="minimum-f"]').setValue('abc')
      expect(number.count()).toBe(0)

      await number.wrapper.find('[data-testid="minimum-f"]').setValue('-2.5')
      await number.wrapper.find('[data-testid="maximum-f"]').setValue('1e3')

      expect(number.latest()).toMatchObject({ validation: { minimum: -2.5, maximum: 1000 } })
      expect(violations(number.latest())).toEqual([])
    })

    it('flags a lower limit above its upper one beside the fields, and still saves it', async () => {
      const text = harness(textField({ validation: { type: 'string', maxLength: 2 } }))

      await text.wrapper.find('[data-testid="min-length-f"]').setValue('5')

      expect(text.latest()).toMatchObject({ validation: { minLength: 5, maxLength: 2 } })
      expect(text.wrapper.find('[data-testid="limits-conflict-length-f"]').exists()).toBe(true)
      expect(text.wrapper.find('[data-testid="limits-conflict-range-f"]').exists()).toBe(false)
      text.wrapper.unmount()

      const number = harness(
        textField({ type: 'number', validation: { type: 'number', minimum: 10 } }),
      )

      expect(number.wrapper.find('[data-testid="limits-conflict-range-f"]').exists()).toBe(false)

      await number.wrapper.find('[data-testid="maximum-f"]').setValue('1')

      expect(number.latest()).toMatchObject({ validation: { minimum: 10, maximum: 1 } })
      expect(number.wrapper.find('[data-testid="limits-conflict-range-f"]').exists()).toBe(true)
      expect(number.wrapper.find('[data-testid="limits-conflict-length-f"]').exists()).toBe(false)
    })

    it('does not evaluate a pattern: one no engine compiles is saved as typed', async () => {
      const { wrapper, latest } = harness(textField())

      await wrapper.find('[data-testid="pattern-f"]').setValue('([A-Z')

      expect(latest()).toMatchObject({ validation: { pattern: '([A-Z' } })
      expect(wrapper.find('[data-testid="pattern-f-error"]').exists()).toBe(false)
    })

    it('clears a limit, a placeholder and a description by removing the property', async () => {
      const { wrapper, latest } = harness(
        textField({
          placeholder: 'ABC-123',
          description: 'The supplier’s code',
          validation: { type: 'string', minLength: 1, maxLength: 9, pattern: '^[A-Z]' },
        }),
      )

      for (const id of [
        'placeholder-f',
        'description-f',
        'min-length-f',
        'max-length-f',
        'pattern-f',
      ]) {
        await wrapper.find(`[data-testid="${id}"]`).setValue('')
      }

      expect(latest()).toEqual(textField())
    })

    it('removes only the limits the new value type cannot carry', async () => {
      const kept = { required: true, format: 'email', messages: { required: 'Needed' } }
      const { wrapper, latest } = harness(
        textField({
          validation: { type: 'string', minLength: 1, maxLength: 9, pattern: '^[A-Z]', ...kept },
        }),
      )

      await wrapper.find('[data-testid="vtype-f"]').setValue('number')

      expect((latest() as { validation: unknown }).validation).toEqual({ type: 'number', ...kept })
      wrapper.unmount()

      const number = harness(textField({ type: 'number', validation: { type: 'number', ...kept } }))

      await number.wrapper.find('[data-testid="minimum-f"]').setValue('1')
      await number.wrapper.find('[data-testid="maximum-f"]').setValue('5')
      await number.wrapper.find('[data-testid="vtype-f"]').setValue('integer')

      expect(number.latest()).toMatchObject({
        validation: { type: 'integer', minimum: 1, maximum: 5 },
      })

      await number.wrapper.find('[data-testid="vtype-f"]').setValue('boolean')

      expect((number.latest() as { validation: unknown }).validation).toEqual({
        type: 'boolean',
        ...kept,
      })
    })

    it('offers an alert info and warning alone, the two looks the renderer draws', () => {
      const { wrapper } = harness({
        id: 'a',
        role: 'display',
        type: 'alert',
        content: 'A',
      } as JfssComponent)

      expect(
        wrapper
          .find('[data-testid="variant-a"]')
          .findAll('option')
          .map((option) => option.attributes('value')),
      ).toEqual(['', 'info', 'warning'])
    })

    it.each(['success', 'error'])(
      'keeps a loaded %s variant, shown and not choosable',
      async (variant) => {
        const { wrapper, latest } = harness({
          id: 'a',
          role: 'display',
          type: 'alert',
          content: 'A',
          variant,
        } as JfssComponent)
        const select = wrapper.find('[data-testid="variant-a"]')

        expect((select.element as HTMLSelectElement).value).toBe(variant)
        expect(select.find(`option[value="${variant}"]`).attributes('disabled')).toBeDefined()

        await wrapper.find('[data-testid="content-a"]').setValue('B')
        expect(latest()).toMatchObject({ variant })
      },
    )

    it('offers an alert its variant, keeps one it does not offer, and clears it', async () => {
      const { wrapper, latest } = harness({
        id: 'a',
        role: 'display',
        type: 'alert',
        content: 'Mind the cut-off',
        variant: 'fancy',
      } as JfssComponent)

      expect((wrapper.find('[data-testid="variant-a"]').element as HTMLSelectElement).value).toBe(
        'fancy',
      )

      await wrapper.find('[data-testid="content-a"]').setValue('Mind it')
      expect(latest()).toMatchObject({ variant: 'fancy' })

      await wrapper.find('[data-testid="variant-a"]').setValue('warning')
      expect(latest()).toMatchObject({ variant: 'warning' })

      await wrapper.find('[data-testid="variant-a"]').setValue('')
      expect(latest()).not.toHaveProperty('variant')
    })
  })

  // --- AC5–AC7: the expressions ------------------------------------------------------

  describe('expressions (AC5–AC7, D-110)', () => {
    function builderIn(wrapper: VueWrapper, testId: string) {
      return wrapper.find(`[data-testid="${testId}"]`).findComponent(JsonLogicBuilder)
    }

    function kinds(wrapper: VueWrapper, label: string): string[] {
      return byLabel(wrapper, `${label}: kind`)
        .findAll('option')
        .map((option) => option.attributes('value') ?? '')
    }

    function paths(variables: readonly LogicVariable[]): string[] {
      return variables.map((variable) => variable.path)
    }

    it('mounts calculate in the calculate tier and conditional.logic in the conditional tier (AC5)', () => {
      const { wrapper } = harness(textField())

      expect(builderIn(wrapper, 'calculate-f').props('tier')).toBe('calculate')
      expect(builderIn(wrapper, 'conditional-f').props('tier')).toBe('conditional')

      // By what each offers, too: §2.3 forbids comparisons and logic in `calculate`.
      expect(kinds(wrapper, 'Calculate')).toEqual(['', 'var', 'arithmetic'])
      expect(kinds(wrapper, 'Conditional logic')).toEqual([
        '',
        'var',
        'comparison',
        'arithmetic',
        'logical',
        'not',
      ])
    })

    it('holds an operator outside the tier opaque, and saves it unchanged (AC5)', async () => {
      const comparison = { '>': [{ var: 'quantity' }, 1] }
      const { wrapper, latest } = harness(textField({ calculate: { '+': [comparison, 1] } }))

      await byLabel(wrapper, 'Calculate, operand 2: number').setValue('7')

      expect(latest()).toMatchObject({ calculate: { '+': [comparison, 7] } })
    })

    it('offers calculate the scope without its own key, and conditional the scope with it (AC6)', () => {
      const { wrapper } = harness(textField({ key: 'node_1' }))

      expect(paths(builderIn(wrapper, 'calculate-f').props('variables'))).toEqual([
        'quantity',
        'unit_price',
      ])
      expect(paths(builderIn(wrapper, 'conditional-f').props('variables'))).toEqual([
        'node_1',
        'quantity',
        'unit_price',
      ])
    })

    it('keeps and badges a loaded var outside the scope, and a key edit rewrites no expression (AC6)', async () => {
      const calculate = { '*': [{ var: 'retired_field' }, { var: 'quantity' }] }
      const { wrapper, latest } = harness(textField({ calculate }))

      expect(wrapper.find('[data-testid="calculate-f"]').text()).toContain(
        'Not in the offered variables',
      )

      await wrapper.find('[data-testid="key-f"]').setValue('renamed')

      expect(latest()).toMatchObject({ key: 'renamed', calculate })
    })

    it('removes calculate when it is cleared, never writing null (AC7)', async () => {
      const { wrapper, latest } = harness(textField({ calculate: { var: 'quantity' } }))

      await byLabel(wrapper, 'Clear Calculate').trigger('click')

      expect(latest()).toEqual(textField())
    })

    it('removes the whole conditional when its logic is cleared (AC7)', async () => {
      const { wrapper, latest } = harness(
        textField({ conditional: { action: 'hide', logic: { var: 'quantity' } } }),
      )

      await byLabel(wrapper, 'Clear Conditional logic').trigger('click')

      expect(latest()).toEqual(textField())
    })

    it('keeps the last complete value while an operand is unfinished, and says so (AC7)', async () => {
      const calculate = { '+': [{ var: 'quantity' }, 1] }
      const { wrapper, latest, count } = harness(textField({ calculate }))

      expect(wrapper.find('[data-testid="calculate-unfinished-f"]').exists()).toBe(false)

      await byLabel(wrapper, 'Add operand to Calculate').trigger('click')

      expect(count()).toBe(0)
      expect(wrapper.find('[data-testid="calculate-unfinished-f"]').exists()).toBe(true)

      await wrapper.find('[data-testid="label-f"]').setValue('Total')

      // The label is written, the expression is the last complete one, and the
      // operand being written is still there.
      expect(latest()).toMatchObject({ label: 'Total', calculate })
      expect(wrapper.find('[aria-label="Calculate, operand 3: kind"]').exists()).toBe(true)
      expect(wrapper.find('[data-testid="calculate-unfinished-f"]').exists()).toBe(true)

      await byLabel(wrapper, 'Calculate, operand 3: kind').setValue('number')
      await byLabel(wrapper, 'Calculate, operand 3: number').setValue('2')

      expect(latest()).toMatchObject({ calculate: { '+': [{ var: 'quantity' }, 1, 2] } })
      expect(wrapper.find('[data-testid="calculate-unfinished-f"]').exists()).toBe(false)
    })

    it('opens a root it cannot show in its raw box, and never rewrites it (AC7)', async () => {
      const calculate = { min: [{ var: 'quantity' }, 10] }
      const logic = { if: [{ var: 'quantity' }, true, false] }
      const { wrapper, latest, count } = harness(
        textField({ calculate, conditional: { action: 'show', logic } }),
      )

      // The raw box, labelled as the builder's JSON, in place of the visual tree.
      for (const [testId, label] of [
        ['calculate-f', 'Calculate (JSON)'],
        ['conditional-f', 'Conditional logic (JSON)'],
      ]) {
        const section = wrapper.find(`[data-testid="${testId}"]`)

        expect(section.find('textarea').exists(), testId).toBe(true)
        expect(section.text(), testId).toContain(label)
        expect(section.find('[aria-label$=": kind"]').exists(), testId).toBe(false)
      }
      expect(count()).toBe(0)

      await wrapper.find('[data-testid="label-f"]').setValue('Edited')

      expect(latest()).toMatchObject({ calculate, conditional: { action: 'show', logic } })
    })

    it('disables every control when read-only, both builders included (AC7)', () => {
      for (const node of [
        textField({
          calculate: { '+': [{ var: 'quantity' }, 1] },
          conditional: { action: 'show', logic: { '>': [{ var: 'quantity' }, 1] } },
          rules: [{ rule: 'regex', scope: 'both', params: { pattern: '^A' }, message: 'm' }],
        }),
        grid({ sequenceKey: 'line_no' }),
        { id: 'a', role: 'display', type: 'alert', content: 'A' } as JfssComponent,
      ]) {
        const { wrapper } = harness(node, { disabled: true })
        const live = wrapper
          .findAll('input, select, textarea, button')
          // Expanding a branch of an expression changes the view, not the node.
          .filter((element) => element.attributes('aria-expanded') === undefined)
          .filter((element) => element.attributes('disabled') === undefined)
          .map((element) => element.attributes('aria-label') ?? element.attributes('data-testid'))

        expect(live, node.type).toEqual([])
        expect(
          wrapper.findAllComponents(JsonLogicBuilder).every((builder) => builder.props('disabled')),
        ).toBe(true)
        wrapper.unmount()
      }
    })
  })

  // --- AC9: the server's details at their field --------------------------------------

  it('shows a detail at …calculate, …conditional.logic, …rules.N.params or …validation.<limit> at that field (AC9)', () => {
    const at = (rest: string, message: string): ValidationDetail => ({
      path: `definition.components.0.${rest}`,
      rule: 'schema',
      code: 'VALIDATION_ERROR',
      message,
    })
    const details = [
      at('calculate', 'calculate reads an unknown field'),
      at('conditional.logic', 'logic uses a forbidden operator'),
      at('rules.0.params.pattern', 'the pattern does not compile'),
      at('validation.pattern', 'pattern is too slow'),
      at('validation.minLength', 'minLength must be an integer'),
      at('options', 'a stray message about options'),
    ]
    const { wrapper } = harness(
      textField({
        rules: [{ rule: 'regex', scope: 'both', params: { pattern: '(' }, message: 'm' }],
      }),
      { details },
    )

    expect(wrapper.find('[data-testid="calculate-error-f"]').text()).toBe(
      'calculate reads an unknown field',
    )
    expect(wrapper.find('[data-testid="conditional-error-f"]').text()).toBe(
      'logic uses a forbidden operator',
    )
    expect(wrapper.find('[data-testid="rule-params-error-f-0"]').text()).toBe(
      'the pattern does not compile',
    )
    expect(wrapper.find('[data-testid="pattern-f-error"]').text()).toBe('pattern is too slow')
    expect(
      wrapper.find('[data-testid="min-length-f"]').element.parentElement?.textContent,
    ).toContain('minLength must be an integer')

    // Every detail is still in the panel's list, the unclaimed one included.
    const list = wrapper.find('[data-testid="editor-errors-f"]').text()

    for (const detail of details) {
      expect(list).toContain(detail.message)
    }
  })

  // --- AC10: the rules section, self-contained ----------------------------------------

  it('keeps the rules section’s test ids, and works on its own (AC10)', async () => {
    const rules: JfssAdvancedRule[] = [
      { rule: 'regex', scope: 'both', params: { pattern: '^A' }, message: 'm' },
    ]
    const wrapper = mount(ValidationRulesSection, {
      props: {
        componentId: 'f',
        nodePath: 'components.0',
        rules,
        disabled: false,
        details: [],
        'onUpdate:rules': (next: JfssAdvancedRule[]) => wrapper.setProps({ rules: next }),
      },
    })

    for (const id of [
      'add-rule-f',
      'rule-f-0',
      'rule-scope-f-0',
      'rule-params-f-0',
      'remove-rule-f-0',
    ]) {
      expect(wrapper.find(`[data-testid="${id}"]`).exists(), id).toBe(true)
    }

    await wrapper.find('[data-testid="add-rule-f"]').trigger('click')
    expect(wrapper.find('[data-testid="rule-f-1"]').exists()).toBe(true)

    await wrapper.find('[data-testid="remove-rule-f-0"]').trigger('click')
    expect(wrapper.props('rules')).toHaveLength(1)
    expect(wrapper.props('rules')?.[0]).toMatchObject({ rule: 'matchesField' })

    wrapper.unmount()
  })

  it('shows a loaded rule it does not offer as itself, and keeps it (AC4)', async () => {
    const unknown = { rule: 'futureRule', scope: 'server', params: { x: 1 }, message: 'n' }
    const { wrapper, latest } = harness(textField({ rules: [unknown] }))
    const select = wrapper.find('[data-testid="rule-f-0"]')

    expect((select.element as HTMLSelectElement).value).toBe('futureRule')
    expect(select.find('option[value="futureRule"]').attributes('disabled')).toBeDefined()

    await wrapper.find('[data-testid="label-f"]').setValue('Edited')

    expect((latest() as { rules: unknown[] }).rules).toEqual([unknown])
  })
})
