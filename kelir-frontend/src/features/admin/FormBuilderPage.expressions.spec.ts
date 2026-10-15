import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { nextTick, toRaw } from 'vue'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'

import JsonLogicBuilder from '@/components/logic-builder/JsonLogicBuilder.vue'
import type { LogicVariable } from '@/components/logic-builder/logicTree'
import {
  errorBody,
  installFakeBackend,
  type FakeBackendHandle,
  type RecordedRequest,
} from '@/lib/testing/fake-backend'
import { jfssViolations } from '@/lib/testing/jfss-meta-schema'
import purchaseRequisition from '@/features/rad/__fixtures__/purchase-requisition.json'
import { type } from '@/lib/testing/keyboard'
import { useAuthStore } from '@/stores/auth'
import type { CurrentUser } from '@/types/auth'
import type { JfssDefinition } from '@/types/jfss'

import FormBuilderPage from './FormBuilderPage.vue'

/**
 * The properties panel's expressions, **through the page's real write path**
 * (#688 AC8, the inherited I1; and the page halves of AC4, AC6, AC7, AC9).
 *
 * `FormBuilderPage.replaceSelected` writes through `formTree.replaceNode`,
 * which copies the whole definition, so after every keystroke the panel is
 * handed a new node — and its expressions as new objects. **I1 is that the
 * builder survives this**: a number typed one key at a time keeps its input
 * and its focus, an operand not yet filled in survives a write elsewhere in the
 * panel, and the builder is never remounted.
 *
 * # Seen to fail (coding standard §2.9)
 *
 * Run 2026-10-15 against this file alone, each mutation reverted after:
 *
 * | Mutation | Reddened |
 * |---|---|
 * | `ExpressionProperties` hands the builder the node's own `calculate` and `conditional.logic`, not the steady value (a host that replaces the expression object on every write) | both I1 cases, at (b): the unfinished operand is dropped by the label write |
 * | `JsonLogicBuilder.isOwnEcho` compares by identity alone | both I1 cases, at (a): the input is replaced after the first key |
 * | The page keys the panel by the node's JSON rather than its `id` (a host that replaces the panel on every write) | both I1 cases, at (a): the input is replaced after the first key |
 * | `scopeVariables` ignores row templates (the whole form is one scope) | *offers a row template's expressions that row's keys …* |
 * | `FormPropertiesPanel` passes `disabled: false` to `ExpressionProperties` | *disables both builders on a published revision* |
 *
 * The `test-engineer` campaign, 2026-10-15, each mutation reverted after. The
 * first three **survived every case above them**: a host that does not copy
 * hands the builder its own emission back, so a builder checking echoes by
 * identity passes I1 as well. The copy is now asserted.
 *
 * | Mutation | Reddened |
 * |---|---|
 * | `replaceNode` puts the emitted node in by reference (`items[at.index] = node`) | *hands the builder an equal copy …*, both cases |
 * | `replaceNode` writes into the definition in place, copying nothing | the same |
 * | the first, with `JsonLogicBuilder.isOwnEcho` by identity alone | the same |
 * | the page keys the panel by `selected.path`, not the node's `id` | *keeps the builder and an unfinished operand when the selected node is moved …* |
 * | a 0 ms or a 30 ms trailing debounce on `replaceSelected`; either builder keyed by its value's JSON or by the node's label; `ExpressionProperties` keyed by a count of writes; a watcher resetting the builders' values from the node on every write; a label edit spread from a copy of the node taken at setup; `steady` by identity | *keeps focus, …*, both cases |
 * | either builder keyed by its `variables`; `JsonLogicBuilder` reloading on any change of `variables`, `label` or `disabled` | *keeps focus, …*: `conditional.logic` alone, since a `calculate` is not offered its own key, so its variables do not change when its label does |
 *
 * The campaign's cases on the purchase requisition and on the canvas, each
 * seen red against the mutation beside it, reverted after:
 *
 * | Mutation | Reddened |
 * |---|---|
 * | `DisplayProperties` gives a paragraph with no `content` one when it is selected | *saves the purchase requisition unchanged after every node in it is selected* |
 * | clearing a limit also deletes its `validation.messages` entry | *changes the purchase requisition at …validation.maxLength alone …* |
 * | `dataVariables` skips `columns` and `tabs` slots | *offers the purchase requisition's expressions their scope …*, *offers a row in a panel in tabs …* |
 * | `scopeListOf` does not walk into a `columns` or `tabs` slot (it **survived every other case**) | the same two |
 * | `removeNode` drops a grid's `sequenceKey` that names the removed row field | *keeps and flags sequenceKey and uniqueBy when the row fields they name are removed …* |
 * | `DataGridProperties` drops a `sequenceKey` naming no row field on any other edit (it **survived every other case**) | the same |
 * | `JsonLogicNode` counts every var as offered | *keeps and badges a var whose field was removed …*, *offers a row in a panel in tabs …* |
 * | `limitsFor('integer')` returns none | *keeps the bounds a number-to-integer switch can carry …*, *changes the purchase requisition at …maximum alone …* |
 * | `switchValueType` keeps every limit | *keeps the bounds a number-to-integer switch can carry …* |
 *
 * **Not a mutant:** the panel deriving `variables` as a new array on every
 * write. No case reddens, and none should: `JsonLogicBuilder` reads
 * `variables` for its choices and badges only, and has no watcher on it.
 */

const FORM_ID = '0199a1a0-0000-7000-8000-0000000000e1'
const blank = { template: '<div />' }

type Node = Record<string, unknown>

/** The dot paths at which `after` differs from `before`: added, removed or changed. */
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

/** Every component id in a definition, through slots and row templates. */
function idsOf(nodes: unknown): string[] {
  if (!Array.isArray(nodes)) {
    return []
  }

  return (nodes as Node[]).flatMap((node) => [
    String(node.id),
    ...idsOf(node.components),
    ...(['columns', 'tabs'] as const).flatMap((slots) =>
      Array.isArray(node[slots])
        ? (node[slots] as Node[]).flatMap((slot) => idsOf(slot.components))
        : [],
    ),
  ])
}

function numberField(key: string, extra: Node = {}): Node {
  return {
    id: key,
    role: 'data',
    type: 'number',
    key,
    label: key,
    validation: { type: 'number' },
    ...extra,
  }
}

const TOTAL = { '+': [{ var: 'quantity' }, 1] }
const NOTES_LOGIC = { and: [{ '>': [{ var: 'total' }, 1] }, { var: 'notes' }] }

function definition(components: Node[]): Node {
  return { formId: 'order', version: '2.0.1', title: 'Order', components }
}

function orderForm(): Node {
  return definition([
    numberField('quantity'),
    numberField('unit_price'),
    numberField('total', { calculate: TOTAL }),
    {
      id: 'notes',
      role: 'data',
      type: 'textfield',
      key: 'notes',
      label: 'Notes',
      validation: { type: 'string' },
      conditional: { action: 'show', logic: NOTES_LOGIC },
    },
  ])
}

function form(def: Node, overrides: Node = {}): Node {
  return {
    id: FORM_ID,
    formKey: 'order',
    title: 'Order',
    revision: 1,
    jfssVersion: '2.0.1',
    status: 'DRAFT',
    entityId: null,
    definition: def,
    publishedAt: null,
    publishedBy: null,
    createdAt: '2026-10-15T00:00:00Z',
    updatedAt: '2026-10-15T00:00:00Z',
    ...overrides,
  }
}

function principal(): CurrentUser {
  return {
    id: '0199a1a0-0000-7000-8000-0000000000u1',
    username: 'admin',
    displayName: 'Administrator',
    email: 'admin@example.test',
    roles: [],
    permissions: ['rad:form:read', 'rad:form:update', 'rad:form:publish'],
  }
}

describe('FormBuilderPage, the properties panel’s expressions', () => {
  let backend: FakeBackendHandle
  let current: Node
  let saveResponse: { status: number; body: unknown } | null
  let router: Router
  let mounted: VueWrapper[]

  beforeEach(() => {
    setActivePinia(createPinia())
    window.localStorage.clear()
    mounted = []
    current = form(orderForm())
    saveResponse = null

    backend = installFakeBackend((request: RecordedRequest) => {
      if (request.method === 'put' && saveResponse) {
        return saveResponse
      }

      if (request.method === 'put') {
        const sent = request.body as { definition: Node }

        return { status: 200, body: { success: true, data: form(sent.definition) } }
      }

      return { status: 200, body: { success: true, data: current } }
    })

    router = createRouter({
      history: createMemoryHistory(),
      routes: [{ path: '/admin/forms/:id', name: 'admin-form-builder', component: blank }],
    })
  })

  afterEach(() => {
    for (const wrapper of mounted) {
      wrapper.unmount()
    }

    backend.restore()
    document.body.innerHTML = ''
  })

  async function settle(): Promise<void> {
    await flushPromises()
    await nextTick()
    await flushPromises()
  }

  async function render(): Promise<VueWrapper> {
    useAuthStore().$patch({ user: principal() })

    await router.push(`/admin/forms/${FORM_ID}`)
    await router.isReady()

    const host = document.createElement('div')

    document.body.appendChild(host)

    const wrapper = mount(FormBuilderPage, { global: { plugins: [router] }, attachTo: host })

    mounted.push(wrapper)

    for (let round = 0; round < 8; round += 1) {
      await flushPromises()
    }

    return wrapper
  }

  async function select(wrapper: VueWrapper, id: string): Promise<void> {
    await wrapper.find(`[data-testid="select-${id}"]`).trigger('click')
    await settle()
  }

  function control(label: string): HTMLElement {
    const found = document.querySelector<HTMLElement>(`[aria-label="${label}"]`)

    if (!found) {
      throw new Error(`no control labelled "${label}"`)
    }

    return found
  }

  function builderIn(wrapper: VueWrapper, testId: string) {
    return wrapper.find(`[data-testid="${testId}"]`).findComponent(JsonLogicBuilder)
  }

  async function saved(wrapper: VueWrapper): Promise<JfssDefinition> {
    const save = wrapper.find('[data-testid="save-definition"]')

    // An unfinished expression does not hold the save (product owner, 2026-10-15).
    expect(save.attributes('disabled')).toBeUndefined()

    await save.trigger('click')
    await settle()

    const puts = backend.requests.filter((request) => request.method === 'put')
    const body = puts[puts.length - 1].body as { title: string; definition: JfssDefinition }

    // The save body is unchanged: `{title, definition: {...definition, title}}` (AC11).
    expect(body.definition.title).toBe(body.title)

    return body.definition
  }

  function node(def: JfssDefinition, id: string): Node {
    return def.components.find((candidate) => candidate.id === id) as unknown as Node
  }

  // --- AC8: I1, through the real host ----------------------------------------------

  it.each([
    {
      expression: 'calculate',
      id: 'total',
      testId: 'calculate-total',
      label: 'Calculate',
      number: 'Calculate, operand 2: number',
      saved: (def: JfssDefinition) => node(def, 'total').calculate,
      expected: { '+': [{ var: 'quantity' }, 1250] },
    },
    {
      expression: 'conditional.logic',
      id: 'notes',
      testId: 'conditional-notes',
      label: 'Conditional logic',
      number: 'Conditional logic, operand 1, operand 2: number',
      saved: (def: JfssDefinition) => (node(def, 'notes').conditional as Node).logic,
      expected: { and: [{ '>': [{ var: 'total' }, 1250] }, { var: 'notes' }] },
    },
  ])(
    'keeps focus, an unfinished operand and the builder itself across the page’s cloning writes: $expression (AC8)',
    async ({ id, testId, label, number, saved: read, expected }) => {
      const wrapper = await render()

      await select(wrapper, id)

      const builder = builderIn(wrapper, testId)
      // The component instance, not test-utils' wrapper proxy around it.
      const instance = builder.vm.$
      const root = builder.element
      const input = control(number) as HTMLInputElement

      expect(input.value).toBe('1')
      input.focus()

      // (a) A number of several characters, typed one key at a time: each key
      // is a complete value, written through `replaceNode` and echoed back as
      // a copy. The input is the same element, and has focus, after every one.
      for (const key of '250') {
        type(key)
        await settle()

        expect(control(number), `after "${key}"`).toBe(input)
        expect(document.activeElement, `after "${key}"`).toBe(input)
      }

      expect(input.value).toBe('1250')

      // (b) An operand not filled in, then a write elsewhere in the panel.
      control(`Add operand to ${label}`).click()
      await settle()

      const hole = control(`${label}, operand 3: kind`)

      expect(wrapper.find(`[data-testid="${testId.replace('-', '-unfinished-')}"]`).exists()).toBe(
        true,
      )

      await wrapper.find(`[data-testid="label-${id}"]`).setValue('Renamed')
      await settle()

      expect(control(`${label}, operand 3: kind`)).toBe(hole)
      expect(control(number)).toBe(input)

      // (c) The builder was never remounted.
      expect(builderIn(wrapper, testId).vm.$).toBe(instance)
      expect(builderIn(wrapper, testId).element).toBe(root)

      // The save carries the label and the last complete expression.
      const sent = await saved(wrapper)

      expect(node(sent, id).label).toBe('Renamed')
      expect(read(sent)).toEqual(expected)
      expect(jfssViolations(sent)).toEqual([])
    },
  )

  // I1's premise, pinned (test-engineer campaign, 2026-10-15). The case above
  // proves a JSON echo check only while the page hands the builder a *copy* of
  // what it emitted: a host that put the emitted node in by reference hands back
  // the very object, and a builder checking echoes by identity alone then passes
  // it too. So the copy is asserted, key by key.
  it.each([
    {
      expression: 'calculate',
      id: 'total',
      testId: 'calculate-total',
      number: 'Calculate, operand 2: number',
    },
    {
      expression: 'conditional.logic',
      id: 'notes',
      testId: 'conditional-notes',
      number: 'Conditional logic, operand 1, operand 2: number',
    },
  ])(
    'hands the builder an equal copy of each value it emitted, never the value itself: $expression (AC8)',
    async ({ id, testId, number }) => {
      const wrapper = await render()

      await select(wrapper, id)

      const builder = builderIn(wrapper, testId)
      const input = control(number) as HTMLInputElement

      input.focus()

      for (const key of '25') {
        type(key)
        await settle()

        const emitted = builder.emitted('update:modelValue') ?? []
        const last = emitted[emitted.length - 1]?.[0]
        const handed = toRaw(builderIn(wrapper, testId).props('modelValue'))

        expect(last, `after "${key}"`).toBeDefined()
        expect(handed, `after "${key}"`).toEqual(last)
        expect(handed, `after "${key}"`).not.toBe(last)
      }
    },
  )

  // A move on the canvas is a write too: the node's path changes, its id does
  // not, and the panel is the node's, so nothing in it is remounted
  // (test-engineer campaign, 2026-10-15).
  it('keeps the builder and an unfinished operand when the selected node is moved on the canvas (AC8)', async () => {
    const wrapper = await render()

    await select(wrapper, 'total')

    const instance = builderIn(wrapper, 'calculate-total').vm.$

    control('Add operand to Calculate').click()
    await settle()

    const hole = control('Calculate, operand 3: kind')

    await wrapper.find('[data-testid="up-total"]').trigger('click')
    await settle()

    // It moved: `total` is now the second node.
    const order = wrapper
      .findAll('[data-testid^="select-"]')
      .map((button) => button.attributes('data-testid'))

    expect(order.indexOf('select-total')).toBeLessThan(order.indexOf('select-unit_price'))

    expect(builderIn(wrapper, 'calculate-total').vm.$).toBe(instance)
    expect(control('Calculate, operand 3: kind')).toBe(hole)
    expect(wrapper.find('[data-testid="calculate-unfinished-total"]').exists()).toBe(true)
  })

  // --- AC4: what the panel does not show saves unchanged -------------------------------

  it('saves a definition unchanged after every node is selected and nothing edited, a navigate button included (AC4)', async () => {
    const loaded = definition([
      {
        id: 'code',
        role: 'data',
        type: 'textfield',
        key: 'code',
        label: 'Code',
        defaultValue: 'ABC',
        readOnly: false,
        calculateMode: 'generated',
        validation: {
          type: 'string',
          format: 'email',
          enum: ['ABC'],
          messages: { required: 'Needed' },
        },
        rules: [{ rule: 'futureRule', scope: 'server', params: { depth: 2 }, message: 'Not yet.' }],
        calculate: { min: [{ var: 'qty' }, 10] },
        conditional: { action: 'hide', logic: { if: [{ var: 'qty' }, true, false] } },
      },
      {
        id: 'panel_1',
        role: 'layout',
        type: 'panel',
        title: 'Lines',
        grid: { columns: 2 },
        conditional: { action: 'show', logic: { var: 'code' } },
        components: [
          {
            id: 'lines',
            role: 'data',
            type: 'datagrid',
            key: 'lines',
            label: 'Lines',
            sequenceKey: 'ghost',
            defaultItems: 1,
            validation: { type: 'array', uniqueBy: 'sku', uniqueItems: true },
            components: [
              {
                id: 'sku',
                role: 'data',
                type: 'textfield',
                key: 'sku',
                label: 'SKU',
                validation: { type: 'string' },
              },
            ],
          },
        ],
      },
      {
        id: 'heading_1',
        role: 'display',
        type: 'heading',
        content: 'Total',
        variant: 'h2',
        calculate: { cat: ['Order ', { var: 'code' }] },
      },
      {
        id: 'button_1',
        role: 'action',
        type: 'button',
        label: 'Next',
        action: 'navigate',
        theme: 'secondary',
      },
    ])

    current = form(loaded)

    const wrapper = await render()

    for (const id of ['code', 'panel_1', 'lines', 'sku', 'heading_1', 'button_1']) {
      await select(wrapper, id)
      expect(wrapper.find(`[data-testid="editor-${id}"]`).exists(), id).toBe(true)
    }

    expect(await saved(wrapper)).toEqual(loaded)
  })

  // --- AC6: scope through the page --------------------------------------------------

  it('offers a row template’s expressions that row’s keys, and the top level every key outside rows (AC6)', async () => {
    current = form(
      definition([
        numberField('quantity'),
        {
          id: 'panel_1',
          role: 'layout',
          type: 'panel',
          title: 'Lines',
          components: [
            {
              id: 'lines',
              role: 'data',
              type: 'datagrid',
              key: 'lines',
              label: 'Lines',
              validation: { type: 'array' },
              components: [numberField('qty'), numberField('price'), numberField('line_total')],
            },
          ],
        },
        numberField('grand_total'),
      ]),
    )

    const wrapper = await render()
    const paths = (testId: string) =>
      (builderIn(wrapper, testId).props('variables') as LogicVariable[]).map((v) => v.path)

    await select(wrapper, 'line_total')
    expect(paths('calculate-line_total')).toEqual(['qty', 'price'])
    expect(paths('conditional-line_total')).toEqual(['qty', 'price', 'line_total'])

    await select(wrapper, 'grand_total')
    expect(paths('calculate-grand_total')).toEqual(['quantity', 'lines'])

    // The grid itself is in the top-level scope, not its own row's.
    await select(wrapper, 'lines')
    expect(paths('conditional-lines')).toEqual(['quantity', 'lines', 'grand_total'])
  })

  // --- AC7: read-only ----------------------------------------------------------------

  it('disables both builders on a published revision (AC7)', async () => {
    current = form(orderForm(), { status: 'PUBLISHED' })

    const wrapper = await render()

    await select(wrapper, 'total')

    // One panel is mounted, so the page's two builders are the selected node's.
    const disabled = wrapper
      .findAllComponents(JsonLogicBuilder)
      .map((builder) => builder.props('disabled'))

    expect(disabled).toEqual([true, true])
    expect((control('Calculate, operand 2: number') as HTMLInputElement).disabled).toBe(true)
    expect((control('Add operand to Calculate') as HTMLButtonElement).disabled).toBe(true)
  })

  // --- AC9: the server's verdict at the field it names ----------------------------------

  it('shows the server’s refusal of an expression at that expression, and in the list (AC9)', async () => {
    saveResponse = {
      status: 422,
      body: errorBody('VALIDATION_ERROR', 'Validation failed', [
        {
          path: 'definition.components.2.calculate',
          rule: 'operator',
          code: 'OPERATOR_NOT_REGISTERED',
          message: '`>` is not in the JFSS rule registry for this property',
        },
        {
          path: 'definition.components.3.conditional.logic',
          rule: 'operator',
          code: 'OPERATOR_NOT_REGISTERED',
          message: '`map` is not in the JFSS rule registry for this property',
        },
      ]),
    }

    const wrapper = await render()

    await wrapper.find('[data-testid="save-definition"]').trigger('click')
    await settle()

    await select(wrapper, 'total')
    expect(wrapper.find('[data-testid="calculate-error-total"]').text()).toContain('`>`')
    expect(wrapper.find('[data-testid="editor-errors-total"]').text()).toContain('`>`')

    await select(wrapper, 'notes')
    expect(wrapper.find('[data-testid="conditional-error-notes"]').text()).toContain('`map`')
  })

  // --- The purchase requisition, through the page (test-engineer campaign, 2026-10-15) ---
  //
  // The repository's real form (`features/rad/__fixtures__`, which the backend
  // asserts it would store): tabs, columns, a fieldset, a grid in a tab with a
  // numbered row, a lookup bound in `settings`, a `calculate` the builder can
  // draw, one it cannot (`sum` over `map`), a `conditional`, an alert and a
  // button. The cases above use small definitions; this is the shape an author
  // actually opens.

  function purchaseRequisitionDefinition(): Node {
    return JSON.parse(JSON.stringify(purchaseRequisition)) as Node
  }

  /** The top level's data keys, in declaration order: every key outside the grid's row. */
  const TOP_LEVEL = [
    'title',
    'supplier_id',
    'cost_centre',
    'needed_by',
    'budget',
    'baseline_budget',
    'priority',
    'line_items',
    'grand_total',
    'justification',
    'notes',
    'category',
    'urgent',
  ]

  it('saves the purchase requisition unchanged after every node in it is selected (AC4)', async () => {
    const loaded = purchaseRequisitionDefinition()

    current = form(loaded, { title: loaded.title })

    const wrapper = await render()
    const ids = idsOf(loaded.components)

    expect(ids.length).toBe(26)

    for (const id of ids) {
      await select(wrapper, id)
      expect(wrapper.find(`[data-testid="editor-${id}"]`).exists(), id).toBe(true)
    }

    expect(changedPaths(loaded, await saved(wrapper))).toEqual([])
  })

  it.each([
    {
      id: 'title-field',
      testId: 'label-title-field',
      value: 'Subject',
      path: 'components.0.components.0.label',
    },
    // Cleared, the limit goes; its loaded `messages.maxLength` is not the panel's to touch.
    {
      id: 'title-field',
      testId: 'max-length-title-field',
      value: '',
      path: 'components.0.components.0.validation.maxLength',
    },
    {
      id: 'supplier-field',
      testId: 'description-supplier-field',
      value: 'From the register',
      path: 'components.0.components.1.description',
    },
    {
      id: 'baseline-field',
      testId: 'label-baseline-field',
      value: 'Opening budget',
      path: 'components.1.columns.0.components.1.label',
    },
    {
      id: 'line-quantity',
      testId: 'maximum-line-quantity',
      value: '99',
      path: 'components.2.tabs.0.components.0.components.2.validation.maximum',
    },
    {
      id: 'line-items',
      testId: 'default-items-line-items',
      value: '2',
      path: 'components.2.tabs.0.components.0.defaultItems',
    },
    {
      id: 'line-items',
      testId: 'unique-by-line-items-description',
      value: true,
      path: 'components.2.tabs.0.components.0.validation.uniqueBy',
    },
    {
      id: 'grand-total-field',
      testId: 'label-grand-total-field',
      value: 'Total',
      path: 'components.2.tabs.0.components.1.label',
    },
    {
      id: 'justification-field',
      testId: 'conditional-action-justification-field',
      value: 'hide',
      path: 'components.2.tabs.1.components.0.components.0.conditional.action',
    },
    {
      id: 'category-field',
      testId: 'label-category-field',
      value: 'Kind',
      path: 'components.2.tabs.1.components.0.components.2.label',
    },
    {
      id: 'notes-fieldset',
      testId: 'title-notes-fieldset',
      value: 'More',
      path: 'components.2.tabs.1.components.0.title',
    },
    {
      id: 'approval-note',
      testId: 'variant-approval-note',
      value: 'warning',
      path: 'components.4.variant',
    },
    {
      id: 'submit-button',
      testId: 'button-label-submit-button',
      value: 'Send',
      path: 'components.5.label',
    },
  ])(
    'changes the purchase requisition at $path alone when $testId is set (AC4)',
    async ({ id, testId, value, path }) => {
      const loaded = purchaseRequisitionDefinition()

      current = form(loaded, { title: loaded.title })

      const wrapper = await render()

      await select(wrapper, id)

      const control = wrapper.find(`[data-testid="${testId}"]`)

      expect(control.exists(), testId).toBe(true)
      await control.setValue(value)
      await settle()

      const sent = await saved(wrapper)

      expect(changedPaths(loaded, sent)).toEqual([path])
      expect(jfssViolations(sent)).toEqual([])
    },
  )

  it('offers the purchase requisition’s expressions their scope, in a tab’s grid row and at the top level (AC6)', async () => {
    current = form(purchaseRequisitionDefinition())

    const wrapper = await render()
    const paths = (testId: string) =>
      (builderIn(wrapper, testId).props('variables') as LogicVariable[]).map((v) => v.path)
    const row = ['line_no', 'description', 'quantity', 'unit_price']

    await select(wrapper, 'line-total')
    expect(paths('calculate-line-total')).toEqual(row)
    expect(paths('conditional-line-total')).toEqual([...row, 'line_total'])

    // `sum` over `map` is not in the `calculate` tier's set: it opens raw.
    await select(wrapper, 'grand-total-field')
    expect(paths('calculate-grand-total-field')).toEqual(
      TOP_LEVEL.filter((key) => key !== 'grand_total'),
    )
    expect(wrapper.find('[data-testid="calculate-grand-total-field"] textarea').exists()).toBe(true)

    await select(wrapper, 'justification-field')
    expect(paths('conditional-justification-field')).toEqual(TOP_LEVEL)

    // A display node is offered a `conditional` and no `calculate`.
    await select(wrapper, 'total-echo')
    expect(wrapper.find('[data-testid="calculate-total-echo"]').exists()).toBe(false)
    expect(paths('conditional-total-echo')).toEqual(TOP_LEVEL)
  })

  it('offers a row in a panel in tabs its own keys, and keeps and badges a top-level key it reads (AC6)', async () => {
    // The engine and the renderer evaluate a row against the row
    // (`engine.rs`'s scopes, `DataGridRow.vue`), so `threshold` reads null
    // there: offering it would be offering a value that is never set.
    const loaded = definition([
      numberField('threshold'),
      {
        id: 'tabs_1',
        role: 'layout',
        type: 'tabs',
        tabs: [
          {
            title: 'Lines',
            components: [
              {
                id: 'panel_1',
                role: 'layout',
                type: 'panel',
                title: 'Lines',
                components: [
                  {
                    id: 'lines',
                    role: 'data',
                    type: 'datagrid',
                    key: 'lines',
                    label: 'Lines',
                    validation: { type: 'array' },
                    components: [
                      numberField('qty'),
                      numberField('price'),
                      numberField('line_total', {
                        calculate: { '*': [{ var: 'qty' }, { var: 'price' }] },
                        conditional: { action: 'show', logic: { '>': [{ var: 'threshold' }, 1] } },
                      }),
                    ],
                  },
                ],
              },
            ],
          },
        ],
      },
    ])

    current = form(loaded)

    const wrapper = await render()
    const paths = (testId: string) =>
      (builderIn(wrapper, testId).props('variables') as LogicVariable[]).map((v) => v.path)

    await select(wrapper, 'line_total')
    expect(paths('calculate-line_total')).toEqual(['qty', 'price'])
    expect(paths('conditional-line_total')).toEqual(['qty', 'price', 'line_total'])
    expect(wrapper.find('[data-testid="conditional-line_total"]').text()).toContain(
      'Not in the offered variables',
    )

    await select(wrapper, 'lines')
    expect(paths('conditional-lines')).toEqual(['threshold', 'lines'])

    expect(changedPaths(loaded, await saved(wrapper))).toEqual([])
  })

  it('keeps and flags sequenceKey and uniqueBy when the row fields they name are removed on the canvas (AC3)', async () => {
    current = form(
      definition([
        {
          id: 'lines',
          role: 'data',
          type: 'datagrid',
          key: 'lines',
          label: 'Lines',
          sequenceKey: 'line_no',
          validation: { type: 'array', uniqueBy: 'sku' },
          components: [
            numberField('line_no'),
            { ...numberField('sku'), type: 'textfield', validation: { type: 'string' } },
          ],
        },
      ]),
    )

    const wrapper = await render()

    await select(wrapper, 'lines')
    await wrapper.find('[data-testid="remove-line_no"]').trigger('click')
    await settle()
    await wrapper.find('[data-testid="remove-sku"]').trigger('click')
    await settle()
    await select(wrapper, 'lines')

    expect(wrapper.find('[data-testid="sequence-key-unknown-lines"]').exists()).toBe(true)
    expect(wrapper.find('[data-testid="unique-by-unknown-lines-sku"]').exists()).toBe(true)

    // Kept through the grid's other edits, not only through a save.
    await wrapper.find('[data-testid="label-lines"]').setValue('Line items')
    await wrapper.find('[data-testid="default-items-lines"]').setValue('2')
    await settle()

    const grid = (await saved(wrapper)).components[0] as unknown as Node

    expect(grid.label).toBe('Line items')
    expect(grid.defaultItems).toBe(2)
    expect(grid.components).toEqual([])
    expect(grid.sequenceKey).toBe('line_no')
    expect(grid.validation).toEqual({ type: 'array', uniqueBy: 'sku' })
  })

  it('keeps and badges a var whose field was removed on the canvas (AC6)', async () => {
    const wrapper = await render()

    await select(wrapper, 'quantity')
    await wrapper.find('[data-testid="remove-quantity"]').trigger('click')
    await settle()
    await select(wrapper, 'total')

    expect(wrapper.find('[data-testid="calculate-total"]').text()).toContain(
      'Not in the offered variables',
    )
    expect(node(await saved(wrapper), 'total').calculate).toEqual(TOTAL)
  })

  it('keeps the bounds a number-to-integer switch can carry, and revives none after a switch away (AC3b)', async () => {
    const wrapper = await render()
    const minimum = () => wrapper.find('[data-testid="minimum-quantity"]')

    await select(wrapper, 'quantity')
    await minimum().setValue('1')
    await wrapper.find('[data-testid="maximum-quantity"]').setValue('9')
    await settle()

    const shown: Record<string, string | null> = {}

    for (const valueType of ['integer', 'string', 'boolean', 'number']) {
      await wrapper.find('[data-testid="vtype-quantity"]').setValue(valueType)
      await settle()
      shown[valueType] = minimum().exists() ? (minimum().element as HTMLInputElement).value : null
    }

    expect(shown).toEqual({ integer: '1', string: null, boolean: null, number: '' })

    await wrapper.find('[data-testid="maximum-quantity"]').setValue('5')
    await settle()

    const sent = await saved(wrapper)

    expect(node(sent, 'quantity').validation).toEqual({ type: 'number', maximum: 5 })
    expect(jfssViolations(sent)).toEqual([])
  })
})
