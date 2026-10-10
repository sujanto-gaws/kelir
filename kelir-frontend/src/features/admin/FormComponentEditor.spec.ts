import { mount, type VueWrapper } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'
import { defineComponent, h, ref, type PropType } from 'vue'

import { offeredEntries } from './form-builder/builderRegistry'
import FormComponentEditor from './FormComponentEditor.vue'
import { jfssViolations } from '@/lib/testing/jfss-meta-schema'
import type { JfssComponent } from '@/types/jfss'

/**
 * The editor, mounted for one selected node (#688 A3, A4, E3, E7, F2).
 *
 * **Every edit of every one of the 18 types is checked against the
 * meta-schema** (E3): the editor is driven through a host that holds the node
 * the way the page does — replacing it with what the editor emits — and the
 * node is validated after each change.
 *
 * # Seen to fail (coding standard §2.9)
 *
 * Run 2026-10-10 against this file alone:
 *
 * | Mutation | Reddened |
 * |---|---|
 * | `addRule` writes `{rule, scope}` alone, as before E7 | *every edit of a … validates* (each data type), *adds a rule the meta-schema accepts* |
 * | The title input shown for every layout type | *offers columns and tabs no property body* |
 * | `datagrid` left in the data type list | *switches a data type among the data types, datagrid excluded* |
 */

const ENFORCEABLE = ['matchesField', 'notMatchesField', 'regex', 'oneOf', 'notOneOf']

function harness(initial: JfssComponent) {
  const history: JfssComponent[] = []

  const Host = defineComponent({
    props: { start: { type: Object as PropType<JfssComponent>, required: true } },
    setup(props) {
      const node = ref<JfssComponent>(props.start)
      const source = ref('')

      return () =>
        h(FormComponentEditor, {
          component: node.value,
          disabled: false,
          details: [],
          lookupSource: source.value,
          onUpdate: (next: JfssComponent) => {
            node.value = next
            history.push(next)
          },
          'onUpdate:lookupSource': (next: string) => {
            source.value = next
          },
        })
    },
  })

  const wrapper = mount(Host, { props: { start: initial } })

  return {
    wrapper,
    latest: () => history[history.length - 1] ?? initial,
    count: () => history.length,
  }
}

function violations(node: JfssComponent): string[] {
  return jfssViolations({ formId: 'f', version: '2.0.1', components: [node] })
}

function controls(wrapper: VueWrapper, selector: string): string[] {
  return wrapper
    .findAll(selector)
    .filter((element) => element.attributes('disabled') === undefined)
    .map((element) => element.attributes('data-testid') ?? '')
    .filter((id) => id !== '')
}

const TEXT_FOR: Record<string, string> = {
  options: 'First=first\nSecond',
  calculate: '{"+": [1, 2]}',
  conditional: '{"==": [1, 1]}',
  'rule-params': '{"pattern": "^[A-Z]+$"}',
}

/** Drives every enabled control the editor draws, validating after each change. */
async function editEverything(node: JfssComponent): Promise<number> {
  const { wrapper, latest, count } = harness(node)
  const check = (what: string) => expect(violations(latest()), what).toEqual([])

  const addRule = wrapper.find(`[data-testid="add-rule-${node.id}"]`)

  if (addRule.exists()) {
    await addRule.trigger('click')
    check('add rule')
  }

  for (const id of controls(wrapper, 'input:not([type="checkbox"])')) {
    await wrapper.find(`[data-testid="${id}"]`).setValue('Edited_value')
    check(id)
  }

  for (const id of controls(wrapper, 'input[type="checkbox"]')) {
    await wrapper.find(`[data-testid="${id}"]`).setValue(true)
    check(`${id} on`)
    await wrapper.find(`[data-testid="${id}"]`).setValue(false)
    check(`${id} off`)
  }

  for (const id of controls(wrapper, 'textarea')) {
    const kind = Object.keys(TEXT_FOR).find((prefix) => id.startsWith(`${prefix}-`))

    await wrapper.find(`[data-testid="${id}"]`).setValue(kind ? TEXT_FOR[kind] : '{}')
    check(id)
    await wrapper.find(`[data-testid="${id}"]`).setValue('')
    check(`${id} cleared`)
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

    for (const option of select.findAll('option').map((o) => o.attributes('value') ?? '')) {
      if (option === '') {
        continue
      }

      await wrapper.find(`[data-testid="${id}"]`).setValue(option)
      check(`${id} = ${option}`)
    }
  }

  return count()
}

describe('FormComponentEditor', () => {
  it.each(offeredEntries().map((entry) => [entry.type, entry] as const))(
    'every edit of a %s validates against the meta-schema (A3, E3)',
    async (_type, entry) => {
      const node = entry.create({ id: 'node_1', key: 'node_1' })

      expect(violations(node)).toEqual([])
      // Columns and tabs have nothing to edit; every other type has something.
      const edits = await editEverything(node)

      if (entry.type === 'columns' || entry.type === 'tabs') {
        expect(edits).toBe(0)
      } else {
        expect(edits).toBeGreaterThan(0)
      }
    },
  )

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
    const data = harness({
      id: 'f',
      role: 'data',
      type: 'textfield',
      key: 'f',
      label: 'F',
      validation: { type: 'string' },
    } as JfssComponent)
    const display = harness({
      id: 'd',
      role: 'display',
      type: 'heading',
      content: 'H',
    } as JfssComponent)
    const options = (wrapper: VueWrapper, id: string) =>
      wrapper
        .find(`[data-testid="type-${id}"]`)
        .findAll('option')
        .map((option) => option.attributes('value'))

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
    expect(options(display.wrapper, 'd')).toEqual(['heading', 'paragraph', 'divider', 'alert'])
  })

  it('shows a data grid’s type read-only and does not offer its value type (A3)', () => {
    const { wrapper } = harness({
      id: 'g',
      role: 'data',
      type: 'datagrid',
      key: 'g',
      label: 'G',
      validation: { type: 'array' },
      components: [],
    } as JfssComponent)

    expect(wrapper.find('[data-testid="type-g"]').attributes('disabled')).toBeDefined()
    expect(wrapper.find('[data-testid="vtype-g"]').exists()).toBe(false)
  })

  it('has no Up, Down or Remove of its own (A4)', () => {
    const { wrapper } = harness({
      id: 'f',
      role: 'data',
      type: 'textfield',
      key: 'f',
      label: 'F',
      validation: { type: 'string' },
    } as JfssComponent)

    for (const id of ['up-f', 'down-f', 'remove-f']) {
      expect(wrapper.find(`[data-testid="${id}"]`).exists()).toBe(false)
    }
  })

  it('adds a rule the meta-schema accepts, and an emptied params box is {} (E7)', async () => {
    const { wrapper, latest } = harness({
      id: 'f',
      role: 'data',
      type: 'textfield',
      key: 'f',
      label: 'F',
      validation: { type: 'string' },
    } as JfssComponent)

    await wrapper.find('[data-testid="add-rule-f"]').trigger('click')

    expect((latest() as { rules: unknown[] }).rules[0]).toEqual({
      rule: 'matchesField',
      scope: 'both',
      params: {},
      message: 'This value is not valid.',
    })
    expect(violations(latest())).toEqual([])

    await wrapper.find('[data-testid="rule-params-f-0"]').setValue('{"target": "other"}')
    await wrapper.find('[data-testid="rule-params-f-0"]').setValue('')

    expect((latest() as { rules: { params: unknown }[] }).rules[0].params).toEqual({})
  })

  it('offers the five enforceable rules and no others (F2)', async () => {
    const { wrapper } = harness({
      id: 'f',
      role: 'data',
      type: 'textfield',
      key: 'f',
      label: 'F',
      validation: { type: 'string' },
    } as JfssComponent)

    await wrapper.find('[data-testid="add-rule-f"]').trigger('click')

    expect(
      wrapper
        .find('[data-testid="rule-f-0"]')
        .findAll('option')
        .map((option) => option.attributes('value')),
    ).toEqual(ENFORCEABLE)
  })
})
