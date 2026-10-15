import { flushPromises, mount } from '@vue/test-utils'
import { describe, expect, it, vi } from 'vitest'
import { defineComponent, h, ref } from 'vue'

import purchaseRequisition from '../__fixtures__/purchase-requisition.json'
import steppedForm from '../__fixtures__/stepped-form.json'
import JfssForm from '../JfssForm.vue'
import FormPropertiesPanel from '@/features/admin/form-builder/FormPropertiesPanel.vue'
import { jfssViolations } from '@/lib/testing/jfss-meta-schema'
import type { JfssComponent, JfssDefinition } from '@/types/jfss'

/**
 * The renderer, against real definitions (#162).
 *
 * Mounted through [`JfssForm`](../JfssForm.vue) rather than the renderer alone,
 * because the thing under test is what a definition turns into — and half of
 * that is the payload binding and the lookup injection the form provides. A
 * test of the dispatch in isolation would pass on a renderer that never
 * received a value.
 *
 * **These do not replace the browser harness** (AC4). They cover the mapping
 * from a definition to a component tree; #101 is why the harness exists, and
 * `e2e/tests/render-a-form.spec.ts` is where a real browser renders this same
 * fixture.
 */

// The lookup field asks the server for its options on mount. The endpoint has
// its own integration tests (`rad_lookups.rs`); what matters here is that the
// field renders and asks, not what the database happens to hold.
vi.mock('@/api/rad', () => ({
  getForm: vi.fn(),
  listLookupOptions: vi.fn().mockResolvedValue({
    items: [{ value: 'supplier-1', label: 'Acme Supplies', description: 'SUP-0001' }],
    meta: { page: 1, pageSize: 20, total: 1, totalPages: 1 },
  }),
}))

const definition = purchaseRequisition as unknown as JfssDefinition

function render(schema: JfssDefinition = definition) {
  return mount(JfssForm, { props: { definition: schema } })
}

describe('rendering a published definition', () => {
  it('takes every label from the definition', () => {
    const text = render().text()

    // Not a spot check: every data component's label, discovered from the
    // fixture rather than listed here, so a field that stops rendering is
    // caught even if nobody updates this test.
    for (const label of ['Title', 'Supplier', 'Needed by', 'Budget', 'Priority', 'Notes']) {
      expect(text).toContain(label)
    }
  })

  it('marks required fields and only required fields', () => {
    const wrapper = render()

    // `title` is required in the fixture, `needed_by` is not.
    const titleLabel = wrapper.find('label[for="jfss-title-field"]')
    const neededByLabel = wrapper.find('label[for="jfss-needed-by-field"]')

    expect(titleLabel.text()).toContain('*')
    expect(neededByLabel.text()).not.toContain('*')
  })

  it('renders help text and points the control at it', () => {
    const wrapper = render()

    const help = wrapper.find('#jfss-title-field-description')

    expect(help.text()).toBe('A short description other people will search for.')
    expect(wrapper.find('#jfss-title-field').attributes('aria-describedby')).toBe(
      'jfss-title-field-description',
    )
  })

  it('descends into a columns container', () => {
    // §4.3.1: a walk that only follows `components` loses these two entirely.
    const wrapper = render()

    expect(wrapper.find('#jfss-budget-field').exists()).toBe(true)
    expect(wrapper.find('#jfss-priority-field').exists()).toBe(true)
  })

  it('descends into a tabs container, and keeps inactive tabs mounted', () => {
    const wrapper = render()

    // `notes` lives on the second tab, which is not the active one.
    expect(wrapper.find('#jfss-notes-field').exists()).toBe(true)

    const panels = wrapper.findAll('[role="tabpanel"]')

    expect(panels).toHaveLength(2)
    expect(panels[0].attributes('hidden')).toBeUndefined()
    expect(panels[1].attributes('hidden')).toBeDefined()
  })

  it('repeats a datagrid row template rather than rendering it once', async () => {
    const wrapper = render()

    // `defaultItems` is applied in `onMounted`, so the row arrives a tick after
    // the first render rather than in it.
    await flushPromises()

    // `defaultItems: 1`, so exactly one row of the four-field template.
    expect(wrapper.text()).toContain('Row 1')
    expect(wrapper.findAll('input[type="number"]').length).toBeGreaterThanOrEqual(3)
  })

  it('builds select options from the definition', () => {
    const options = render().find('#jfss-priority-field').findAll('option')

    // Four options plus the placeholder the Select renders.
    expect(options.map((option) => option.text())).toEqual(
      expect.arrayContaining(['Low', 'Normal', 'High', 'Urgent']),
    )
  })

  it('builds radio options from validation.enum when options are absent', () => {
    const wrapper = render()

    const radios = wrapper.findAll('input[type="radio"][name="jfss-category-field"]')

    expect(radios).toHaveLength(2)
  })

  it('shows a named placeholder for a type it declares it cannot render', () => {
    const wrapper = render(steppedForm as unknown as JfssDefinition)

    // Visible, naming the type, and carrying the registry's reason — rather
    // than an absent section that reads as a form which never had one.
    expect(wrapper.text()).toContain('steps')
    expect(wrapper.text()).toContain('ask for this form to use tabs instead')
  })

  it('shows a placeholder for a type no one declared at all', () => {
    const unknown: JfssDefinition = {
      formId: 'unknown',
      version: '2.0.1',
      components: [{ id: 'x', role: 'display', type: 'hologram' }],
    }

    const wrapper = render(unknown)

    expect(wrapper.text()).toContain('hologram')
  })

  it('refuses to render a type whose role the definition disagrees about', () => {
    // The backend stores this: the meta-schema constrains properties by role,
    // not which types belong to which role. Binding a value to a panel is not a
    // thing to attempt quietly.
    const mismatched: JfssDefinition = {
      formId: 'mismatch',
      version: '2.0.1',
      components: [
        {
          id: 'wrong',
          role: 'data',
          type: 'panel',
          key: 'wrong',
          label: 'Wrong',
          validation: { type: 'string' },
        },
      ],
    }

    expect(render(mismatched).text()).toContain('panel')
  })
})

describe('the payload a rendered form starts with', () => {
  it('carries every top-level data key, including those inside columns and tabs', async () => {
    const wrapper = render()

    await flushPromises()
    await wrapper.find('#jfss-title-field').setValue('A new laptop')

    const changes = wrapper.emitted('change')

    expect(changes).toBeTruthy()

    const payload = changes![changes!.length - 1][0] as Record<string, unknown>

    // Every key, not the ones that were typed into: JFSS S10.1 requires a
    // submission to carry the `key` of every data component, and a payload
    // that grows as fields are touched is one whose shape depends on what the
    // user happened to click. `budget` and `priority` live inside a `columns`
    // container and `notes` inside a `tabs` one — a walk that followed only
    // `components` would lose all three.
    //
    // `justification` is here while its `conditional` keeps it off the screen,
    // which is S10.1.1 rather than an oversight: a hidden key is submitted and
    // the server discards it, because the alternative makes the two engines
    // evaluate the same condition against different data.
    expect(Object.keys(payload).sort()).toEqual(
      [
        'baseline_budget',
        'budget',
        'category',
        'cost_centre',
        'grand_total',
        'justification',
        'line_items',
        'needed_by',
        'notes',
        'priority',
        'supplier_id',
        'title',
        'urgent',
      ].sort(),
    )

    expect(payload.title).toBe('A new laptop')
  })

  it('starts an array field as an array and a scalar as null', () => {
    const wrapper = render()

    // Present and empty, rather than absent. A `null` where an array belongs is
    // what turns `sum` over line items into an evaluation error.
    const payload = (wrapper.vm as unknown as { values: Record<string, unknown> }).values

    expect(Array.isArray(payload.line_items)).toBe(true)
    expect(payload.title).toBeNull()
  })

  it('takes a number from a number field, which Vue hands over already cast', async () => {
    // The regression test for a defect #162 shipped and none of its tests could
    // reach: Vue's `vModelText` casts for `type="number"` without being asked,
    // so `NumberField`'s setter was handed a `number` where it expected a
    // `string` and threw `next.trim is not a function` on the first keystroke
    // into any number field on any form. Nothing here typed into one until
    // #163's first calculation test did.
    const wrapper = render()

    await flushPromises()
    await wrapper.find('#jfss-budget-field').setValue('250')

    const changes = wrapper.emitted('change')!
    const payload = changes[changes.length - 1][0] as Record<string, unknown>

    // And it is a number rather than the string the DOM carries, which is the
    // property `NumberField` exists for.
    expect(payload.budget).toBe(250)
  })

  it('fills a datagrid sequenceKey with the 1-based row index', async () => {
    const wrapper = render()

    await flushPromises()

    const lineNo = wrapper.find('#jfss-row-0-line-no')

    expect((lineNo.element as HTMLInputElement).value).toBe('1')
  })
})

describe('identifiers inside a repeater', () => {
  /**
   * A repeater renders one template instance once per row, so §4.1's
   * per-instance uniqueness does not by itself keep the DOM ids apart.
   *
   * Duplicate ids are invalid HTML and the consequence is not cosmetic:
   * `<label for>` binds to the first match, so every row's label would point at
   * row one's input, and a radio group's shared `name` would make choosing an
   * option in row two clear row one.
   */
  it('gives every element in the document a unique id', async () => {
    const wrapper = render()

    await flushPromises()
    await wrapper.find('button.bg-secondary').trigger('click')
    await flushPromises()

    const ids = wrapper.findAll('[id]').map((element) => element.attributes('id'))
    const duplicates = ids.filter((id, index) => ids.indexOf(id) !== index)

    expect(duplicates, `duplicated ids: ${duplicates.join(', ')}`).toHaveLength(0)
  })

  it('scopes a row field id by its row', async () => {
    const wrapper = render()

    await flushPromises()
    await wrapper.find('button.bg-secondary').trigger('click')
    await flushPromises()

    // Deterministic rather than generated, because the browser harness locates
    // by these and a test that has to discover an id cannot assert one.
    expect(wrapper.find('#jfss-row-0-line-no').exists()).toBe(true)
    expect(wrapper.find('#jfss-row-1-line-no').exists()).toBe(true)
  })

  it('does not scope a field outside a repeater', () => {
    const wrapper = render()

    expect(wrapper.find('#jfss-title-field').exists()).toBe(true)
  })
})

/**
 * **The renderer draws what the form builder's properties panel writes**
 * (#688 AC3, AC3b; **D-116**).
 *
 * Each definition below is not written by hand: a node is put through the
 * panel, edited through its controls as an author would, and what the panel
 * emitted is rendered. So a property the panel writes under a name, a shape or
 * a type the renderer does not read fails here, rather than in a browser.
 *
 * Seen to fail, 2026-10-15: the panel writing `defaultItems` as the string
 * `"2"` reddens *opens a data grid with the rows …*; the limits section
 * dropping a `minLength` it was given reddens its `minLength` case.
 */
describe('a definition the form builder’s properties panel wrote (#688)', () => {
  /** The node after the panel's controls, by test id, were given these values. */
  async function authored(
    start: Record<string, unknown>,
    edits: [testId: string, value: string | boolean][],
  ): Promise<JfssComponent> {
    let latest = start as unknown as JfssComponent

    const Host = defineComponent({
      setup() {
        const node = ref(latest)

        return () =>
          h(FormPropertiesPanel, {
            component: node.value,
            path: 'components.0',
            disabled: false,
            details: [],
            lookupSource: '',
            scope: [],
            onUpdate: (next: JfssComponent) => {
              latest = JSON.parse(JSON.stringify(next)) as JfssComponent
              node.value = latest
            },
          })
      },
    })
    const host = mount(Host)

    for (const [testId, value] of edits) {
      await host.get(`[data-testid="${testId}"]`).setValue(value)
    }

    host.unmount()

    return latest
  }

  function textField(): Record<string, unknown> {
    return {
      id: 'code-field',
      role: 'data',
      type: 'textfield',
      key: 'code',
      label: 'Code',
      validation: { type: 'string' },
    }
  }

  function numberField(): Record<string, unknown> {
    return {
      id: 'qty-field',
      role: 'data',
      type: 'number',
      key: 'qty',
      label: 'Quantity',
      validation: { type: 'number' },
    }
  }

  const SUBMIT = { id: 'submit', role: 'action', type: 'button', label: 'Submit', action: 'submit' }

  function formOf(node: JfssComponent): JfssDefinition {
    const definition = {
      formId: 'authored',
      version: '2.0.1',
      title: 'Authored',
      components: [node, SUBMIT],
    } as unknown as JfssDefinition

    expect(jfssViolations(definition)).toEqual([])

    return definition
  }

  async function submit(wrapper: ReturnType<typeof render>): Promise<void> {
    await wrapper
      .findAll('button')
      .find((button) => button.text() === 'Submit')!
      .trigger('click')
    await flushPromises()
  }

  /** Types `value` into the field, presses Submit, and returns the field's message. */
  async function refusal(definition: JfssDefinition, field: string, value: string) {
    const wrapper = render(definition)

    await flushPromises()
    await wrapper.find(field).setValue(value)
    await submit(wrapper)

    return wrapper.find('[data-testid="field-error"]').text()
  }

  it('draws the placeholder the panel wrote', async () => {
    const node = await authored(textField(), [['placeholder-code-field', 'ABC-123']])

    expect(render(formOf(node)).find('#jfss-code-field').attributes('placeholder')).toBe('ABC-123')
  })

  it('draws the description the panel wrote, and points the control at it', async () => {
    const node = await authored(textField(), [['description-code-field', 'The supplier’s code']])
    const wrapper = render(formOf(node))

    expect(wrapper.find('#jfss-code-field-description').text()).toBe('The supplier’s code')
    expect(wrapper.find('#jfss-code-field').attributes('aria-describedby')).toBe(
      'jfss-code-field-description',
    )
  })

  it.each([
    {
      limit: 'minLength',
      start: textField,
      control: 'min-length-code-field',
      written: '3',
      field: '#jfss-code-field',
      typed: 'AB',
      message: 'Use at least 3 characters.',
    },
    {
      limit: 'maxLength',
      start: textField,
      control: 'max-length-code-field',
      written: '5',
      field: '#jfss-code-field',
      typed: 'ABCDEFG',
      message: 'Use at most 5 characters.',
    },
    {
      limit: 'pattern',
      start: textField,
      control: 'pattern-code-field',
      written: '^[A-Z]+$',
      field: '#jfss-code-field',
      typed: 'abc',
      message: 'This value is not in the expected format.',
    },
    {
      limit: 'minimum',
      start: numberField,
      control: 'minimum-qty-field',
      written: '1',
      field: '#jfss-qty-field',
      typed: '0',
      message: 'Enter 1 or more.',
    },
    {
      limit: 'maximum',
      start: numberField,
      control: 'maximum-qty-field',
      written: '10',
      field: '#jfss-qty-field',
      typed: '11',
      message: 'Enter 10 or less.',
    },
  ])(
    'applies the $limit the panel wrote',
    async ({ start, control, written, field, typed, message }) => {
      const node = await authored(start(), [[control, written]])

      expect(await refusal(formOf(node), field, typed)).toBe(message)
    },
  )

  it('opens a data grid with the rows, numbering and uniqueness the panel wrote', async () => {
    const node = await authored(
      {
        id: 'lines',
        role: 'data',
        type: 'datagrid',
        key: 'lines',
        label: 'Lines',
        validation: { type: 'array' },
        components: [
          {
            id: 'line-no',
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
      },
      [
        ['sequence-key-lines', 'line_no'],
        ['default-items-lines', '2'],
        ['unique-by-lines-sku', true],
      ],
    )

    expect(node).toMatchObject({
      sequenceKey: 'line_no',
      defaultItems: 2,
      validation: { uniqueBy: 'sku' },
    })

    const wrapper = render(formOf(node))

    await flushPromises()

    // `defaultItems: 2`, numbered by `sequenceKey` from 1.
    expect((wrapper.find('#jfss-row-0-line-no').element as HTMLInputElement).value).toBe('1')
    expect((wrapper.find('#jfss-row-1-line-no').element as HTMLInputElement).value).toBe('2')
    expect(wrapper.find('#jfss-row-2-line-no').exists()).toBe(false)

    // `uniqueBy: "sku"`: two rows with one SKU are refused.
    await wrapper.find('#jfss-row-0-sku').setValue('A-1')
    await wrapper.find('#jfss-row-1-sku').setValue('A-1')
    await submit(wrapper)

    expect(wrapper.text()).toContain('Two rows repeat a value that must be unique.')
  })
})
