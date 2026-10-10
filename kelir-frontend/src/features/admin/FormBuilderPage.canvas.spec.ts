import { flushPromises, mount, type DOMWrapper, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { nextTick } from 'vue'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'

import FormBuilderPage from './FormBuilderPage.vue'
import FormComponentEditor from './FormComponentEditor.vue'
import {
  errorBody,
  installFakeBackend,
  type FakeBackendHandle,
  type RecordedRequest,
} from '@/lib/testing/fake-backend'
import { jfssViolations } from '@/lib/testing/jfss-meta-schema'
import { useAuthStore } from '@/stores/auth'
import type { CurrentUser } from '@/types/auth'
import type { JfssDefinition } from '@/types/jfss'

/**
 * The form builder's nesting canvas, through the page (#688 A–E).
 *
 * **The tree's rules are pinned in `formTree.spec.ts`; this file pins that the
 * screen reaches them**: that the palette, every list's *Add component*, the
 * card controls and a drop each change the definition the save sends, that
 * focus and the live region follow, and that a read-only canvas changes
 * nothing by any path.
 *
 * **A drag is driven through its drop handler** (D1): jsdom has no layout, so
 * Sortable cannot be dragged here, and the handler — `vue-draggable-plus`'s
 * `end` event, as the canvas list receives it — is what a real drop reaches.
 * The keyboard-only browser flow is `e2e/tests/build-a-nested-form-by-keyboard.spec.ts`.
 *
 * # Seen to fail (coding standard §2.9)
 *
 * Run 2026-10-10 against this file alone:
 *
 * | Mutation | Reddened |
 * |---|---|
 * | `move`, which a drop calls, skips its read-only check | *ignores a drop on a read-only canvas* |
 * | `FormCanvasNode` draws its controls on a read-only canvas | *draws no palette, handle, Add, Move or Remove for …*, both cases |
 * | `focusAfterRemoval` skips the next sibling | *focuses the next sibling after a removal, else the previous, else the container* |
 * | A move strips an unsupported node to its `id`, `role` and `type` | *draws an unsupported node as a card, and saves it unchanged* |
 * | `save` sends the definition without `title` | *saves a container in a column in a tab in a panel* |
 */

const FORM_ID = '0199a1a0-0000-7000-8000-0000000000f1'
const blank = { template: '<div />' }

type Node = Record<string, unknown>

function field(id: string, extra: Node = {}): Node {
  return {
    id,
    role: 'data',
    type: 'textfield',
    key: id,
    label: `Label ${id}`,
    validation: { type: 'string' },
    ...extra,
  }
}

function definition(components: Node[], settings?: Node): Node {
  return {
    formId: 'purchase_requisition',
    version: '2.0.1',
    title: 'Purchase requisition',
    components,
    ...(settings ? { settings } : {}),
  }
}

function form(def: Node, overrides: Node = {}): Node {
  return {
    id: FORM_ID,
    formKey: 'purchase_requisition',
    title: 'Purchase requisition',
    revision: 1,
    jfssVersion: '2.0.1',
    status: 'DRAFT',
    entityId: null,
    definition: def,
    publishedAt: null,
    publishedBy: null,
    createdAt: '2026-10-10T00:00:00Z',
    updatedAt: '2026-10-10T00:00:00Z',
    ...overrides,
  }
}

function principal(permissions: string[]): CurrentUser {
  return {
    id: '0199a1a0-0000-7000-8000-0000000000u1',
    username: 'admin',
    displayName: 'Administrator',
    email: 'admin@example.test',
    roles: [],
    permissions,
  }
}

const EVERY_PERMISSION = ['rad:form:read', 'rad:form:update', 'rad:form:publish']

describe('FormBuilderPage, the nesting canvas', () => {
  let backend: FakeBackendHandle
  let current: Node
  let saveResponse: { status: number; body: unknown } | null
  let router: Router
  let mounted: VueWrapper[]

  beforeEach(() => {
    setActivePinia(createPinia())
    window.localStorage.clear()
    mounted = []

    current = form(definition([field('field_1'), field('field_2')]))
    saveResponse = null

    backend = installFakeBackend((request: RecordedRequest) => {
      if (request.method === 'put' && saveResponse) {
        return saveResponse
      }

      if (request.method === 'put') {
        // Echo what was sent, as a server that stores it does.
        const sent = request.body as { definition: Node }

        return { status: 200, body: { success: true, data: form(sent.definition) } }
      }

      if (request.url.includes('/lookups/')) {
        return {
          status: 200,
          body: { success: true, data: [], meta: { page: 1, pageSize: 20, total: 0 } },
        }
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

  async function render(permissions: string[] = EVERY_PERMISSION): Promise<VueWrapper> {
    useAuthStore().$patch({ user: principal(permissions) })

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

  async function settle(): Promise<void> {
    await flushPromises()
    await nextTick()
    await flushPromises()
  }

  function byLabel(wrapper: VueWrapper, label: string): DOMWrapper<HTMLButtonElement> {
    return wrapper.find<HTMLButtonElement>(`button[aria-label="${label}"]`)
  }

  async function saved(
    wrapper: VueWrapper,
  ): Promise<{ title: string; definition: JfssDefinition }> {
    await wrapper.find('[data-testid="save-definition"]').trigger('click')
    await settle()

    const puts = backend.requests.filter((request) => request.method === 'put')

    return puts[puts.length - 1].body as { title: string; definition: JfssDefinition }
  }

  function draggables(wrapper: VueWrapper) {
    return wrapper.findAllComponents({ name: 'VueDraggable' })
  }

  /** The canvas list component whose `<ul>` carries this list path. */
  function listNamed(wrapper: VueWrapper, listPath: string) {
    const found = draggables(wrapper).find(
      (component) => (component.element as HTMLElement).dataset.listPath === listPath,
    )

    if (!found) {
      throw new Error(`no canvas list at ${listPath}`)
    }

    return found
  }

  /** A drop, as `vue-draggable-plus` reports it to the list the drag left. */
  async function drop(
    wrapper: VueWrapper,
    from: string,
    oldIndex: number,
    to: string,
    newIndex: number,
  ): Promise<void> {
    const source = listNamed(wrapper, from)
    const target = listNamed(wrapper, to)

    source.vm.$emit('end', {
      from: source.element,
      to: target.element,
      oldIndex,
      newIndex,
      oldDraggableIndex: oldIndex,
      newDraggableIndex: newIndex,
    })
    await settle()
  }

  // --- A. Coexistence with the shipped editor ---------------------------------

  it('selects the first root component on load, and edits one node at a time (A2)', async () => {
    const wrapper = await render()

    expect(wrapper.findAllComponents(FormComponentEditor)).toHaveLength(1)
    expect(wrapper.find('[data-testid="editor-field_1"]').exists()).toBe(true)

    await wrapper.find('[data-testid="select-field_2"]').trigger('click')

    expect(wrapper.find('[data-testid="editor-field_2"]').exists()).toBe(true)
    expect(wrapper.find('[data-testid="editor-field_1"]').exists()).toBe(false)
  })

  it('has no move or remove control on the editor (A4)', async () => {
    const wrapper = await render()
    const editor = wrapper.find('[data-testid="editor-field_1"]')

    expect(editor.find('[data-testid="up-field_1"]').exists()).toBe(false)
    expect(editor.find('[data-testid="down-field_1"]').exists()).toBe(false)
    expect(editor.find('[data-testid="remove-field_1"]').exists()).toBe(false)
    // They live on the card instead.
    expect(
      wrapper.find('[data-testid="component-field_1"] [data-testid="remove-field_1"]').exists(),
    ).toBe(true)
  })

  it('selects a node when it is added, focuses it, and says so (A2, D6)', async () => {
    const wrapper = await render()

    await wrapper.find('[data-testid="palette-panel"]').trigger('click')
    await settle()

    expect(wrapper.find('[data-testid="editor-panel_1"]').exists()).toBe(true)
    expect(document.activeElement?.getAttribute('data-testid')).toBe('select-panel_1')
    expect(wrapper.find('[data-testid="canvas-announcement"]').text()).toBe(
      'Added Panel “Panel” to the form.',
    )
  })

  // --- D2. Adding without dragging -------------------------------------------------

  it('adds from the palette into the selected container (D2)', async () => {
    current = form(
      definition([
        field('field_1'),
        { id: 'panel_1', role: 'layout', type: 'panel', title: 'Details', components: [] },
      ]),
    )

    const wrapper = await render()

    expect(wrapper.find('[data-testid="palette-target"]').text()).toBe('Adds to the form')

    await wrapper.find('[data-testid="select-panel_1"]').trigger('click')

    expect(wrapper.find('[data-testid="palette-target"]').text()).toBe('Adds to Panel “Details”')

    await wrapper.find('[data-testid="palette-number"]').trigger('click')
    await settle()

    const { definition: sent } = await saved(wrapper)

    expect((sent.components[1] as unknown as { components: Node[] }).components).toEqual([
      {
        id: 'field_2',
        role: 'data',
        type: 'number',
        key: 'field_2',
        label: 'Number',
        validation: { type: 'number' },
      },
    ])
  })

  it('gives every list, empty ones included, an Add component button that adds there (D2)', async () => {
    current = form(
      definition([
        { id: 'panel_1', role: 'layout', type: 'panel', title: 'Details', components: [] },
        {
          id: 'grid_1',
          role: 'data',
          type: 'datagrid',
          key: 'lines',
          label: 'Lines',
          validation: { type: 'array' },
          components: [],
        },
      ]),
    )

    const wrapper = await render()

    for (const listPath of ['components', 'components.0.components', 'components.1.components']) {
      expect(wrapper.find(`[data-list-add="${listPath}"]`).exists(), listPath).toBe(true)
    }

    // A row template's chooser offers data and display, never layout or a button.
    await wrapper.find('[data-list-add="components.1.components"]').trigger('click')

    const offered = wrapper
      .findAll('[data-add-type]')
      .map((button) => button.attributes('data-add-type'))

    expect(offered).toContain('textfield')
    expect(offered).toContain('heading')
    expect(offered).not.toContain('panel')
    expect(offered).not.toContain('button')

    await wrapper.find('[data-add-type="number"]').trigger('click')
    await settle()

    expect(document.activeElement?.getAttribute('data-testid')).toBe('select-field_1')

    const { definition: sent } = await saved(wrapper)

    expect(
      (sent.components[1] as unknown as { components: Node[] }).components.map((node) => node.type),
    ).toEqual(['number'])
  })

  // --- D3. Moving without dragging ---------------------------------------------------

  it('moves up and down, disabled at the ends, and keeps focus on the moved node (D3, D6)', async () => {
    current = form(definition([field('a'), field('b'), field('c')]))

    const wrapper = await render()

    expect(wrapper.find('[data-testid="up-a"]').attributes('disabled')).toBeDefined()
    expect(wrapper.find('[data-testid="down-c"]').attributes('disabled')).toBeDefined()

    await wrapper.find('[data-testid="down-a"]').trigger('click')
    await settle()

    expect(document.activeElement?.getAttribute('data-testid')).toBe('down-a')
    expect(wrapper.find('[data-testid="canvas-announcement"]').text()).toContain(
      'Moved Text field “Label a” down',
    )

    const { definition: sent } = await saved(wrapper)

    expect(sent.components.map((node) => node.id)).toEqual(['b', 'a', 'c'])
  })

  it('moves to only the destinations containment allows, named by path (D3)', async () => {
    current = form(
      definition([
        { id: 'panel_1', role: 'layout', type: 'panel', title: 'Details', components: [] },
        {
          id: 'grid_1',
          role: 'data',
          type: 'datagrid',
          key: 'lines',
          label: 'Lines',
          validation: { type: 'array' },
          components: [],
        },
        {
          id: 'tabs_1',
          role: 'layout',
          type: 'tabs',
          tabs: [{ title: 'General', components: [] }],
        },
      ]),
    )

    const wrapper = await render()

    await wrapper.find('[data-testid="move-to-tabs_1"]').trigger('click')

    const offered = wrapper
      .findAll('[data-destination]')
      .map((button) => button.attributes('data-destination'))

    // Into the panel, yes; into itself or the row template, no.
    expect(offered).toEqual(['components.0.components'])
    expect(document.activeElement?.getAttribute('data-destination')).toBe('components.0.components')

    await wrapper.find('[data-destination="components.0.components"]').trigger('click')
    await settle()

    expect(document.activeElement?.getAttribute('data-node-control')).toBe('move-to')
    expect(wrapper.find('[data-testid="canvas-announcement"]').text()).toBe(
      'Moved Tabs to Panel “Details”.',
    )
  })

  // --- D1, D5. Dragging, and its keyboard equivalent ----------------------------------

  it('gives a drag and its keyboard equivalent the same definition (D1, D5)', async () => {
    current = form(
      definition([
        { id: 'panel_1', role: 'layout', type: 'panel', title: 'Details', components: [] },
        field('field_1'),
        field('field_2'),
      ]),
    )

    const dragged = await render()

    await drop(dragged, 'components', 1, 'components.0.components', 0)

    const byDrag = await saved(dragged)

    dragged.unmount()
    mounted = []
    backend.requests.length = 0

    const keyed = await render()

    await keyed.find('[data-testid="move-to-field_1"]').trigger('click')
    await keyed.find('[data-destination="components.0.components"]').trigger('click')
    await settle()

    const byKeyboard = await saved(keyed)

    expect(byKeyboard).toEqual(byDrag)
    expect(
      (byDrag.definition.components[0] as unknown as { components: Node[] }).components[0].id,
    ).toBe('field_1')
    expect(jfssViolations(byDrag.definition)).toEqual([])
  })

  it('reorders within one list by a drop, as the server would count it (D1)', async () => {
    current = form(definition([field('a'), field('b'), field('c')]))

    const wrapper = await render()

    await drop(wrapper, 'components', 0, 'components', 2)

    expect((await saved(wrapper)).definition.components.map((node) => node.id)).toEqual([
      'b',
      'c',
      'a',
    ])
  })

  it('refuses a drop the tree refuses, before and after it lands (C1)', async () => {
    current = form(
      definition([
        {
          id: 'grid_1',
          role: 'data',
          type: 'datagrid',
          key: 'lines',
          label: 'Lines',
          validation: { type: 'array' },
          components: [],
        },
        { id: 'panel_1', role: 'layout', type: 'panel', title: 'Details', components: [] },
      ]),
    )

    const wrapper = await render()
    const before = wrapper.find('[data-testid="form-canvas"]').html()
    const template = listNamed(wrapper, 'components.0.components')
    const source = listNamed(wrapper, 'components')

    // Before it lands: Sortable's `put` asks the tree.
    source.vm.$emit('start', {
      from: source.element,
      to: source.element,
      oldIndex: 1,
      oldDraggableIndex: 1,
    })
    const put = (template.props('group') as { put: () => boolean }).put

    expect(put()).toBe(false)

    // After it lands: the drop handler refuses, and nothing changes.
    await drop(wrapper, 'components', 1, 'components.0.components', 0)

    expect(wrapper.find('[data-testid="form-canvas"]').html()).toBe(before)
  })

  // --- C3, C4, D4, D6. Removing ---------------------------------------------------------

  it('asks before removing a container that holds components, and takes its bindings (C3, C4)', async () => {
    current = form(
      definition(
        [
          {
            id: 'panel_1',
            role: 'layout',
            type: 'panel',
            title: 'Details',
            components: [field('supplier', { type: 'lookup' })],
          },
          field('field_1'),
        ],
        { lookups: { supplier: 'supplier' } },
      ),
    )

    const wrapper = await render()

    await wrapper.find('[data-testid="remove-panel_1"]').trigger('click')
    await settle()

    const confirm = document.querySelector<HTMLButtonElement>('[data-testid="confirm-action"]')

    expect(confirm).not.toBeNull()
    expect(wrapper.find('[data-testid="component-panel_1"]').exists()).toBe(true)

    confirm?.click()
    await settle()

    expect(wrapper.find('[data-testid="component-panel_1"]').exists()).toBe(false)

    const { definition: sent } = await saved(wrapper)

    expect(sent.settings?.lookups).toEqual({})
  })

  it('focuses the next sibling after a removal, else the previous, else the container (D6)', async () => {
    current = form(
      definition([
        {
          id: 'panel_1',
          role: 'layout',
          type: 'panel',
          title: 'Details',
          components: [field('a'), field('b'), field('c')],
        },
      ]),
    )

    const wrapper = await render()

    await wrapper.find('[data-testid="remove-b"]').trigger('click')
    await settle()
    expect(document.activeElement?.getAttribute('data-testid')).toBe('select-c')
    expect(wrapper.find('[data-testid="canvas-announcement"]').text()).toBe(
      'Removed Text field “Label b”.',
    )

    await wrapper.find('[data-testid="remove-c"]').trigger('click')
    await settle()
    expect(document.activeElement?.getAttribute('data-testid')).toBe('select-a')

    await wrapper.find('[data-testid="remove-a"]').trigger('click')
    await settle()
    expect(document.activeElement?.getAttribute('data-testid')).toBe('select-panel_1')
  })

  // --- C3. Slots ------------------------------------------------------------------------

  it('adds, titles and removes slots, refusing a blank tab title (C3)', async () => {
    current = form(
      definition([
        {
          id: 'columns_1',
          role: 'layout',
          type: 'columns',
          columns: [{ components: [] }, { components: [field('x')] }],
        },
        {
          id: 'tabs_1',
          role: 'layout',
          type: 'tabs',
          tabs: [{ title: 'General', components: [] }],
        },
      ]),
    )

    const wrapper = await render()

    await wrapper.find('[data-testid="add-slot-columns_1"]').trigger('click')
    await wrapper.find('[data-testid="add-slot-tabs_1"]').trigger('click')
    await settle()

    await wrapper.find('[data-testid="tab-title-tabs_1-1"]').setValue('Approvals')
    await wrapper.find('[data-testid="tab-title-tabs_1-0"]').setValue('')

    expect(wrapper.text()).toContain('A tab needs a title.')

    // The empty column goes at once; the one holding a field asks first.
    await wrapper.find('[data-testid="remove-slot-columns_1-0"]').trigger('click')
    await settle()
    expect(document.querySelector('[data-testid="confirm-action"]')).toBeNull()

    await wrapper.find('[data-testid="remove-slot-columns_1-0"]').trigger('click')
    await settle()
    expect(document.querySelector('[data-testid="confirm-action"]')).not.toBeNull()

    const { definition: sent } = await saved(wrapper)

    expect(sent.components[0]).toMatchObject({
      columns: [{ components: [{ id: 'x' }] }, { components: [] }],
    })
    expect(sent.components[1]).toMatchObject({
      tabs: [{ title: 'General' }, { title: 'Approvals' }],
    })
    expect(jfssViolations(sent)).toEqual([])
  })

  // --- B5. What the canvas cannot draw ------------------------------------------------

  it('draws an unsupported node as a card, and saves it unchanged (B5)', async () => {
    const steps = {
      id: 'steps_1',
      role: 'layout',
      type: 'steps',
      tabs: [{ title: 'One', components: [field('inside', { placeholder: 'kept' })] }],
    }
    const mismatch = {
      id: 'odd',
      role: 'data',
      type: 'panel',
      key: 'odd',
      label: 'Odd',
      validation: { type: 'string' },
    }
    const unknown = { id: 'marquee_1', role: 'display', type: 'marquee', content: 'Hello' }
    const gridWithPanel = {
      id: 'grid_1',
      role: 'data',
      type: 'datagrid',
      key: 'lines',
      label: 'Lines',
      validation: { type: 'array' },
      components: [
        { id: 'row_panel', role: 'layout', type: 'panel', title: 'Row', components: [] },
      ],
    }

    current = form(definition([steps, mismatch, unknown, gridWithPanel, field('field_1')]))

    const wrapper = await render()

    for (const id of ['steps_1', 'odd', 'marquee_1', 'row_panel']) {
      expect(wrapper.find(`[data-testid="unsupported-${id}"]`).exists(), id).toBe(true)
      expect(wrapper.find(`[data-testid="select-${id}"]`).exists(), id).toBe(false)
    }

    // Nothing inside an unsupported card is drawn.
    expect(wrapper.find('[data-testid="component-inside"]').exists()).toBe(false)

    // It can be moved and removed.
    await wrapper.find('[data-testid="down-steps_1"]').trigger('click')
    await settle()

    const { definition: sent } = await saved(wrapper)

    expect(sent.components[1]).toEqual(steps)
    expect(sent.components[0]).toEqual(mismatch)
    expect(sent.components[2]).toEqual(unknown)
    expect(sent.components[3]).toEqual(gridWithPanel)
  })

  // --- E. Save, schema, S10.3 -------------------------------------------------------------

  it('shows a nested refusal on its node’s card and in the editor, and drops none (E6)', async () => {
    current = form(
      definition([
        {
          id: 'panel_1',
          role: 'layout',
          type: 'panel',
          title: 'P',
          components: [
            {
              id: 'tabs_1',
              role: 'layout',
              type: 'tabs',
              tabs: [
                {
                  title: 'T',
                  components: [
                    {
                      id: 'columns_1',
                      role: 'layout',
                      type: 'columns',
                      columns: [{ components: [] }, { components: [field('deep')] }],
                    },
                  ],
                },
              ],
            },
          ],
        },
      ]),
    )
    saveResponse = {
      status: 422,
      body: errorBody('VALIDATION_ERROR', 'Validation failed', [
        {
          path: 'definition.components.0.components.0.tabs.0.components.0.columns.1.components.0.key',
          rule: 'jfss',
          code: 'INVALID_DEFINITION',
          message: 'The key is not a valid identifier',
        },
        {
          path: 'definition.settings.lookups.ghost',
          rule: 'reference',
          code: 'LOOKUP_COMPONENT_NOT_FOUND',
          message: '`ghost` is not the id of a data component',
        },
      ]),
    }

    const wrapper = await render()

    await wrapper.find('[data-testid="save-definition"]').trigger('click')
    await settle()

    expect(wrapper.find('[data-testid="component-errors-deep"]').text()).toContain(
      'not a valid identifier',
    )
    expect(wrapper.find('[data-testid="component-errors-panel_1"]').exists()).toBe(false)
    expect(wrapper.find('[data-testid="form-errors"]').text()).toContain('`ghost`')

    await wrapper.find('[data-testid="select-deep"]').trigger('click')

    expect(wrapper.find('[data-testid="editor-errors-deep"]').text()).toContain(
      'not a valid identifier',
    )
    // The editor's own field claims it too, by the rest of the path.
    expect(wrapper.find('[data-testid="editor-deep"]').text()).toContain('not a valid identifier')
  })

  it('saves a container in a column in a tab in a panel, and the body validates (C5, E1, E3)', async () => {
    current = form(definition([]))

    const wrapper = await render()

    await wrapper.find('[data-testid="palette-panel"]').trigger('click')
    await settle()
    // The panel is selected, so the palette adds into it.
    await wrapper.find('[data-testid="palette-tabs"]').trigger('click')
    await settle()
    await wrapper.find('[data-testid="palette-columns"]').trigger('click')
    await settle()

    // Columns is selected; its first slot is the palette's target. Column 2 by its own button.
    await wrapper
      .find('[data-list-add="components.0.components.0.tabs.0.components.0.columns.1.components"]')
      .trigger('click')
    await wrapper.find('[data-add-type="fieldset"]').trigger('click')
    await settle()
    await wrapper.find('[data-testid="palette-textfield"]').trigger('click')
    await settle()
    // E1: the save body is `{title, definition: {...definition, title}}`, so a
    // retitled form carries the new title in both places.
    await wrapper.find('[data-testid="definition-title"]').setValue('Nested requisition')

    const body = await saved(wrapper)

    expect(body.title).toBe('Nested requisition')
    expect(body.definition.title).toBe('Nested requisition')
    expect(jfssViolations(body.definition)).toEqual([])
    expect(body.definition.components[0]).toMatchObject({
      type: 'panel',
      components: [
        {
          type: 'tabs',
          tabs: [
            {
              components: [
                {
                  type: 'columns',
                  columns: [
                    { components: [] },
                    {
                      components: [
                        { type: 'fieldset', components: [{ type: 'textfield', id: 'field_1' }] },
                      ],
                    },
                  ],
                },
              ],
            },
          ],
        },
      ],
    })
  })

  it('writes a rule the server takes (E7)', async () => {
    const wrapper = await render()

    await wrapper.find('[data-testid="add-rule-field_1"]').trigger('click')

    const { definition: sent } = await saved(wrapper)

    expect((sent.components[0] as { rules: unknown[] }).rules).toEqual([
      { rule: 'matchesField', scope: 'both', params: {}, message: 'This value is not valid.' },
    ])
    expect(jfssViolations(sent)).toEqual([])
  })

  // --- D6. Names --------------------------------------------------------------------------

  it('names every control on a card with the node’s label or type (D6)', async () => {
    current = form(
      definition([
        {
          id: 'columns_1',
          role: 'layout',
          type: 'columns',
          columns: [{ components: [field('a')] }, { components: [] }],
        },
        { id: 'divider_1', role: 'display', type: 'divider' },
      ]),
    )

    const wrapper = await render()

    for (const [id, name] of [
      ['columns_1', 'Columns'],
      ['a', 'Text field “Label a”'],
      ['divider_1', 'Divider'],
    ]) {
      const card = wrapper.find(`[data-testid="component-${id}"]`)
      const own = card.findAll('button').filter((button) => {
        // Controls of this card, not of a card nested in it.
        return button.element.closest('[data-node-id]') === card.element
      })

      expect(own.length, id).toBeGreaterThan(0)

      for (const button of own) {
        const label = button.attributes('aria-label') ?? button.text()

        if (label === 'Cancel') {
          continue
        }

        expect(label, `${id}: ${label}`).toContain(name)
      }
    }

    expect(byLabel(wrapper, 'Add component to column 1 of Columns').exists()).toBe(true)
  })

  // --- D7. Read-only ------------------------------------------------------------------------

  it.each([
    ['a published revision', EVERY_PERMISSION, 'PUBLISHED'],
    ['a caller without rad:form:update', ['rad:form:read'], 'DRAFT'],
  ])(
    'draws no palette, handle, Add, Move or Remove for %s, and disables the drag (D7)',
    async (_case, permissions, status) => {
      current = form(
        definition([
          { id: 'panel_1', role: 'layout', type: 'panel', title: 'P', components: [field('a')] },
          field('b'),
        ]),
        { status },
      )

      const wrapper = await render(permissions)

      expect(wrapper.find('[data-testid="form-palette"]').exists()).toBe(false)
      expect(wrapper.find('[data-testid^="drag-"]').exists()).toBe(false)
      expect(wrapper.find('[data-list-add]').exists()).toBe(false)

      for (const control of ['up', 'down', 'move-to', 'remove']) {
        expect(wrapper.find(`[data-node-control="${control}"]`).exists(), control).toBe(false)
      }

      for (const list of draggables(wrapper)) {
        expect(list.props('disabled')).toBe(true)
      }

      // Selecting still shows the properties, disabled.
      await wrapper.find('[data-testid="select-b"]').trigger('click')
      expect(wrapper.find('[data-testid="key-b"]').attributes('disabled')).toBeDefined()
    },
  )

  it('ignores a drop on a read-only canvas (D7)', async () => {
    current = form(definition([field('a'), field('b')]), { status: 'PUBLISHED' })

    const wrapper = await render()
    const before = wrapper.find('[data-testid="form-canvas"]').html()

    await drop(wrapper, 'components', 0, 'components', 1)

    expect(wrapper.find('[data-testid="form-canvas"]').html()).toBe(before)
  })
})
