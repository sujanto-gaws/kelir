import { flushPromises, mount } from '@vue/test-utils'
import { describe, expect, it, vi } from 'vitest'
import { defineComponent, h, ref } from 'vue'

import JfssForm from '@/features/rad/JfssForm.vue'
import { jfssViolations } from '@/lib/testing/jfss-meta-schema'
import type { JfssComponent, JfssDefinition } from '@/types/jfss'

import { offeredEntries } from './builderRegistry'
import FormPropertiesPanel from './FormPropertiesPanel.vue'
import { PLACEHOLDER_TYPES } from './propertyEdits'

/**
 * **The renderer draws what the properties panel writes** (#688 AC3, AC3b;
 * **D-116**).
 *
 * Each definition here is not written by hand: a node is put through the
 * panel, edited through its controls as an author would, and what the panel
 * emitted is rendered through `JfssForm`, as a document renders it. So a
 * property the panel writes under a name, a shape or a type the renderer does
 * not read fails here, rather than in a browser.
 *
 * **The dependency runs one way**: this builder-side spec imports the
 * renderer, and the renderer's own spec (`JfssRenderer.spec.ts`) knows nothing
 * of the builder.
 *
 * # Seen to fail (coding standard §2.9)
 *
 * Run 2026-10-15 against this file alone, each mutation reverted after:
 *
 * | Mutation | Reddened |
 * |---|---|
 * | the panel writes `defaultItems` as the string `"2"` | *opens a data grid with the rows …* |
 * | the limits section drops a `minLength` it was given | *applies the minLength …* |
 * | `PLACEHOLDER_TYPES` gains `date` | *offers a placeholder on exactly the types the renderer draws one on* |
 * | `PLACEHOLDER_TYPES` loses `textarea` | the same |
 */

// The lookup field asks the server for its options on mount; what matters here
// is what it draws, not what the database holds.
vi.mock('@/api/rad', () => ({
  getForm: vi.fn(),
  listLookupOptions: vi.fn().mockResolvedValue({
    items: [],
    meta: { page: 1, pageSize: 20, total: 0, totalPages: 0 },
  }),
}))

function render(definition: JfssDefinition) {
  return mount(JfssForm, { props: { definition } })
}

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

function formOf(node: JfssComponent, settings?: Record<string, unknown>): JfssDefinition {
  const definition = {
    formId: 'authored',
    version: '2.0.1',
    title: 'Authored',
    components: [node, SUBMIT],
    ...(settings ? { settings } : {}),
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

describe('a definition the properties panel wrote, rendered (#688)', () => {
  it('draws the placeholder the panel wrote', async () => {
    const node = await authored(textField(), [['placeholder-code-field', 'ABC-123']])

    expect(render(formOf(node)).find('#jfss-code-field').attributes('placeholder')).toBe('ABC-123')
  })

  it('offers a placeholder on exactly the types the renderer draws one on (D-116)', async () => {
    const text = 'Placeholder from the definition'
    const dataTypes = offeredEntries().filter(
      (entry) => entry.role === 'data' && entry.type !== 'datagrid',
    )

    expect(dataTypes).toHaveLength(8)

    for (const entry of dataTypes) {
      const node = { ...entry.create({ id: 'field', key: 'field' }), placeholder: text }
      // A lookup draws its search box only once it is bound to a source (D-23).
      const settings = entry.type === 'lookup' ? { lookups: { field: 'supplier' } } : undefined
      const wrapper = render(formOf(node as JfssComponent, settings))

      await flushPromises()

      const drawn =
        wrapper.find(`[placeholder="${text}"]`).exists() ||
        wrapper.findAll('option').some((option) => option.text() === text)

      expect(drawn, `${entry.type} draws a placeholder`).toBe(
        PLACEHOLDER_TYPES.includes(entry.type),
      )

      const panel = mount(FormPropertiesPanel, {
        props: {
          component: node as JfssComponent,
          path: 'components.0',
          disabled: false,
          details: [],
          lookupSource: '',
          scope: [],
        },
      })

      expect(
        panel.find('[data-testid="placeholder-field"]').exists(),
        `${entry.type} offers one`,
      ).toBe(drawn)

      panel.unmount()
      wrapper.unmount()
    }
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
