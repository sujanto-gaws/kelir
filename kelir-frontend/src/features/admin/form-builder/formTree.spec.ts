import { describe, expect, it } from 'vitest'

import { offeredEntries } from './builderRegistry'
import {
  addComponent,
  addSlot,
  canContain,
  cardReason,
  freshIdentity,
  insertNode,
  listAt,
  moveBy,
  moveDestinations,
  moveNode,
  placeDetails,
  removeNode,
  removeSlot,
  renameSlot,
  resolveDetail,
  unsupportedReason,
} from './formTree'
import { jfssViolations } from '@/lib/testing/jfss-meta-schema'
import type { JfssComponent, JfssDefinition } from '@/types/jfss'

/**
 * The builder's tree operations (#688 C, D3–D5, E3, E6).
 *
 * **Every structural operation's result is checked against the meta-schema**
 * (E3), because each one is a definition the next save sends.
 *
 * # Seen to fail (coding standard §2.9)
 *
 * Run 2026-10-10 against this file alone; each reddened at least the test
 * named:
 *
 * | Mutation | Reddened |
 * |---|---|
 * | `placementReason` allows a layout in a row template | *refuses a layout or action node in a row template* |
 * | `canContain` drops the into-itself check | *refuses a container into itself or a descendant* |
 * | `listAt` returns a list for a non-container | *refuses a child into a non-container* |
 * | `unsupportedReason` accepts a layout with two shapes | *draws a layout node with two shapes, or none, as unsupported* |
 * | `freshIdentity` scans the root list only | *generates an id unique across slots and row templates* |
 * | `removeNode` drops only the removed node's own binding | *drops every binding in a removed subtree* |
 * | `removeSlot` keeps the slot's bindings | *drops the bindings of a removed slot* |
 * | `moveBy` allows a move past the end | *refuses a move past either end* |
 * | `moveNode` inserts before removing | *moves a node down one place in its own list* |
 * | `resolveDetail` reads `columns` as `tabs` | *resolves a column index of one or more* |
 * | `resolveDetail` off by one on a slot index | *resolves a column index of one or more* |
 * | `resolveDetail` accepts a path without `definition.` | *leaves a path without the server's prefix unresolved* |
 */

function field(id: string, extra: Record<string, unknown> = {}): JfssComponent {
  return {
    id,
    role: 'data',
    type: 'textfield',
    key: id,
    label: id,
    validation: { type: 'string' },
    ...extra,
  } as JfssComponent
}

function panel(id: string, components: JfssComponent[] = []): JfssComponent {
  return { id, role: 'layout', type: 'panel', title: id, components } as JfssComponent
}

function columns(id: string, slots: JfssComponent[][]): JfssComponent {
  return {
    id,
    role: 'layout',
    type: 'columns',
    columns: slots.map((components) => ({ components })),
  } as JfssComponent
}

function tabs(id: string, slots: [string, JfssComponent[]][]): JfssComponent {
  return {
    id,
    role: 'layout',
    type: 'tabs',
    tabs: slots.map(([title, components]) => ({ title, components })),
  } as JfssComponent
}

function grid(id: string, components: JfssComponent[]): JfssComponent {
  return {
    id,
    role: 'data',
    type: 'datagrid',
    key: id,
    label: id,
    validation: { type: 'array' },
    components,
  } as JfssComponent
}

const heading = (id: string) =>
  ({ id, role: 'display', type: 'heading', content: id }) as JfssComponent
const button = (id: string) =>
  ({ id, role: 'action', type: 'button', label: id, action: 'submit' }) as JfssComponent

function form(components: JfssComponent[], settings?: Record<string, unknown>): JfssDefinition {
  return {
    formId: 'f',
    version: '2.0.1',
    title: 'F',
    components,
    ...(settings ? { settings } : {}),
  }
}

/**
 * A panel holding tabs holding columns, a datagrid at the root, and a field —
 * every shape at once.
 */
function nested(): JfssDefinition {
  return form(
    [
      panel('panel_1', [
        tabs('tabs_1', [
          ['General', [columns('columns_1', [[field('field_1')], [field('field_2')]])]],
          ['More', [field('field_3')]],
        ]),
      ]),
      grid('grid_1', [field('sku'), field('qty')]),
      field('field_4'),
    ],
    { lookups: {} },
  )
}

function valid(definition: JfssDefinition | null): JfssDefinition {
  expect(definition).not.toBeNull()
  expect(jfssViolations(definition)).toEqual([])

  return definition as JfssDefinition
}

describe('containment (C1, C2)', () => {
  it('accepts any role into the root, a panel, a column and a tab', () => {
    const definition = nested()

    for (const list of [
      'components',
      'components.0.components',
      'components.0.components.0.tabs.1.components',
      'components.0.components.0.tabs.0.components.0.columns.1.components',
    ]) {
      for (const node of [field('x'), panel('p'), heading('h'), button('b')]) {
        expect(canContain(definition, list, node).ok, `${node.type} into ${list}`).toBe(true)
      }
    }
  })

  it('refuses a child into a non-container', () => {
    const definition = nested()

    expect(canContain(definition, 'components.2.components', field('x')).ok).toBe(false)
    expect(insertNode(definition, 'components.2.components', 0, field('x'))).toBeNull()
  })

  it('refuses a layout or action node in a row template, and takes data and display', () => {
    const definition = nested()
    const template = 'components.1.components'

    expect(canContain(definition, template, panel('p')).ok).toBe(false)
    expect(canContain(definition, template, button('b')).ok).toBe(false)
    expect(canContain(definition, template, field('x')).ok).toBe(true)
    expect(canContain(definition, template, heading('h')).ok).toBe(true)
    // A nested datagrid is allowed.
    expect(canContain(definition, template, grid('g', [])).ok).toBe(true)
  })

  it('refuses a container into itself or a descendant, and leaves the tree as it was', () => {
    const definition = nested()
    const before = JSON.stringify(definition)

    expect(moveNode(definition, 'components.0', 'components.0.components', 0)).toBeNull()
    expect(
      moveNode(definition, 'components.0', 'components.0.components.0.tabs.0.components', 0),
    ).toBeNull()
    expect(JSON.stringify(definition)).toBe(before)
  })

  it('refuses a list in a shape the owner does not have', () => {
    const definition = nested()

    // A panel's children are `components`, not slots; tabs' are slots.
    expect(listAt(definition, 'components.0.columns.0.components')).toBeNull()
    expect(listAt(definition, 'components.0.components.0.components')).toBeNull()
    expect(listAt(definition, 'components.0.components.0.tabs.9.components')).toBeNull()
  })
})

describe('unsupported nodes (B5)', () => {
  it.each([
    ['steps', { id: 's', role: 'layout', type: 'steps', tabs: [] }],
    [
      'file',
      { id: 'f', role: 'data', type: 'file', key: 'f', label: 'F', validation: { type: 'string' } },
    ],
    [
      'signature',
      {
        id: 'g',
        role: 'data',
        type: 'signature',
        key: 'g',
        label: 'G',
        validation: { type: 'string' },
      },
    ],
    [
      'repeater',
      {
        id: 'r',
        role: 'data',
        type: 'repeater',
        key: 'r',
        label: 'R',
        validation: { type: 'array' },
      },
    ],
    ['an unknown type', { id: 'u', role: 'display', type: 'marquee' }],
    [
      'a role/type mismatch',
      {
        id: 'm',
        role: 'data',
        type: 'panel',
        key: 'm',
        label: 'M',
        validation: { type: 'string' },
      },
    ],
  ])('draws %s as unsupported', (_name, node) => {
    expect(unsupportedReason(node as JfssComponent)).toBeTruthy()
  })

  it('draws a layout node with two shapes, or none, as unsupported', () => {
    expect(
      unsupportedReason({
        id: 'p',
        role: 'layout',
        type: 'panel',
        components: [],
        columns: [{ components: [] }],
      } as JfssComponent),
    ).toBeTruthy()
    expect(
      unsupportedReason({ id: 'p', role: 'layout', type: 'panel' } as JfssComponent),
    ).toBeTruthy()
    // A panel holding tabs is a shape a panel's entry does not have.
    expect(
      unsupportedReason({ id: 'p', role: 'layout', type: 'panel', tabs: [] } as JfssComponent),
    ).toBeTruthy()
  })

  it('draws a layout node loaded into a row template as unsupported there', () => {
    expect(cardReason('rowTemplate', panel('p'))).toBeTruthy()
    expect(cardReason('components', panel('p'))).toBeNull()
  })

  it('offers no list inside an unsupported node, however well formed', () => {
    const definition = form([
      { id: 's', role: 'layout', type: 'steps', components: [panel('inner')] } as JfssComponent,
    ])

    expect(listAt(definition, 'components.0.components')).toBeNull()
    expect(listAt(definition, 'components.0.components.0.components')).toBeNull()
  })

  it('moves an unsupported node and keeps every property it had', () => {
    const odd = {
      id: 's',
      role: 'layout',
      type: 'steps',
      tabs: [{ title: 'One', components: [field('inside')] }],
      grid: { columns: 3 },
    } as JfssComponent
    const definition = form([field('field_1'), odd])
    const moved = moveBy(definition, 'components.1', -1) as JfssDefinition

    expect(moved.components[0]).toEqual(odd)
  })
})

describe('identity (B4)', () => {
  it('generates an id unique across slots and row templates', () => {
    const definition = form([
      columns('c', [[field('field_1')], [field('field_2')]]),
      grid('field_3', [field('field_4')]),
    ])

    expect(freshIdentity(definition, 'textfield')).toEqual({ id: 'field_5', key: 'field_5' })
  })

  it('avoids a key another node took, not only an id', () => {
    const definition = form([field('a', { key: 'field_1' })])

    expect(freshIdentity(definition, 'number').id).toBe('field_2')
  })

  it('names a non-data node for its type', () => {
    expect(freshIdentity(form([]), 'panel').id).toBe('panel_1')
    expect(freshIdentity(form([panel('panel_1')]), 'panel').id).toBe('panel_2')
  })
})

describe('adding (B4, E3)', () => {
  const lists = [
    'components',
    'components.0.components',
    'components.0.components.0.tabs.0.components',
    'components.0.components.0.tabs.0.components.0.columns.1.components',
    'components.1.components',
  ]

  it.each(offeredEntries().map((entry) => entry.type))(
    'adds a %s wherever containment allows it, and the result validates',
    (type) => {
      for (const list of lists) {
        const result = addComponent(nested(), type, list)

        if (
          list === 'components.1.components' &&
          ['panel', 'fieldset', 'columns', 'tabs', 'button'].includes(type)
        ) {
          expect(result, `${type} into the row template`).toBeNull()
          continue
        }

        const items = listAt(valid(result?.definition ?? null), list)?.items ?? []

        expect(items[items.length - 1]?.id).toBe(result?.node.id)
      }
    },
  )

  it('binds a new lookup to a source, so its first save is not refused', () => {
    const result = addComponent(nested(), 'lookup', 'components') as NonNullable<
      ReturnType<typeof addComponent>
    >

    expect(result.definition.settings?.lookups).toEqual({ [result.node.id]: 'supplier' })
  })

  it('creates a row template on a datagrid that had none', () => {
    const bare = form([
      {
        id: 'g',
        role: 'data',
        type: 'datagrid',
        key: 'g',
        label: 'G',
        validation: { type: 'array' },
      } as JfssComponent,
    ])
    const result = addComponent(bare, 'number', 'components.0.components')

    expect(
      (valid(result?.definition ?? null).components[0] as { components: unknown[] }).components,
    ).toHaveLength(1)
  })

  it('saves a container inside a column, inside a tab, inside a panel (C5)', () => {
    let definition = form([])

    definition = addComponent(definition, 'panel', 'components')!.definition
    definition = addComponent(definition, 'tabs', 'components.0.components')!.definition
    definition = addComponent(
      definition,
      'columns',
      'components.0.components.0.tabs.0.components',
    )!.definition
    definition = addComponent(
      definition,
      'fieldset',
      'components.0.components.0.tabs.0.components.0.columns.1.components',
    )!.definition
    definition = addComponent(
      definition,
      'textfield',
      'components.0.components.0.tabs.0.components.0.columns.1.components.0.components',
    )!.definition

    valid(definition)
    expect(
      listAt(
        definition,
        'components.0.components.0.tabs.0.components.0.columns.1.components.0.components',
      )?.items[0].type,
    ).toBe('textfield')
  })
})

describe('moving (D3, D5)', () => {
  it('moves a node down one place in its own list', () => {
    const definition = form([field('a'), field('b'), field('c')])
    const moved = valid(moveBy(definition, 'components.0', 1))

    expect(moved.components.map((node) => node.id)).toEqual(['b', 'a', 'c'])
  })

  it('counts a same-list drop index after the node has left', () => {
    const definition = form([field('a'), field('b'), field('c')])
    const moved = valid(moveNode(definition, 'components.0', 'components', 2))

    expect(moved.components.map((node) => node.id)).toEqual(['b', 'c', 'a'])
  })

  it('refuses a move past either end', () => {
    const definition = form([field('a'), field('b')])

    expect(moveBy(definition, 'components.0', -1)).toBeNull()
    expect(moveBy(definition, 'components.1', 1)).toBeNull()
  })

  it('moves into a list whose path the removal shifts', () => {
    // Removing components.0 makes the panel components.1 → components.0; the
    // destination is held by reference, so the node still lands in the panel.
    const definition = form([field('a'), field('b'), panel('p')])
    const moved = valid(moveNode(definition, 'components.0', 'components.2.components', 0))

    expect(moved.components.map((node) => node.id)).toEqual(['b', 'p'])
    expect((moved.components[1] as { components: JfssComponent[] }).components[0].id).toBe('a')
  })

  it('moves out of a deep slot to the root, and does not touch lookups', () => {
    const definition = nested()

    definition.settings = { lookups: { field_2: 'supplier' } }

    const moved = valid(
      moveNode(
        definition,
        'components.0.components.0.tabs.0.components.0.columns.1.components.0',
        'components',
        0,
      ),
    )

    expect(moved.components[0].id).toBe('field_2')
    expect(moved.settings?.lookups).toEqual({ field_2: 'supplier' })
  })

  it('offers as destinations only the lists containment allows, named by path', () => {
    const definition = nested()
    const forPanel = moveDestinations(definition, 'components.0')
    const forField = moveDestinations(definition, 'components.2')

    // Not the root it is in, not inside itself, not the row template.
    expect(forPanel.map((destination) => destination.listPath)).toEqual([])
    expect(forField.map((destination) => destination.listPath)).toEqual([
      'components.0.components',
      'components.0.components.0.tabs.0.components',
      'components.0.components.0.tabs.0.components.0.columns.0.components',
      'components.0.components.0.tabs.0.components.0.columns.1.components',
      'components.0.components.0.tabs.1.components',
      'components.1.components',
    ])
    expect(forField.map((destination) => destination.label)).toContain(
      'Form › Panel “panel_1” › Tab “General” of Tabs › Column 2 of Columns',
    )
  })
})

describe('removing (C4, D4)', () => {
  it('drops every binding in a removed subtree, row templates included', () => {
    const definition = form(
      [panel('p', [field('a'), grid('g', [field('row_lookup')])]), field('kept')],
      { lookups: { a: 'supplier', row_lookup: 'customer', kept: 'employee' }, other: 1 },
    )
    const removed = valid(removeNode(definition, 'components.0'))

    expect(removed.settings).toEqual({ lookups: { kept: 'employee' }, other: 1 })
  })

  it('drops the bindings of a removed slot', () => {
    const definition = form([columns('c', [[field('a')], [field('b')]])], {
      lookups: { a: 'supplier', b: 'customer' },
    })
    const removed = valid(removeSlot(definition, 'components.0', 0))

    expect(removed.settings?.lookups).toEqual({ b: 'customer' })
    expect((removed.components[0] as { columns: unknown[] }).columns).toHaveLength(1)
  })
})

describe('slots (C3)', () => {
  it('adds a column, and a tab titled for its place', () => {
    const definition = form([columns('c', [[], []]), tabs('t', [['One', []]])])
    const withColumn = valid(addSlot(definition, 'components.0'))
    const withTab = valid(addSlot(withColumn, 'components.1'))

    expect((withTab.components[0] as { columns: unknown[] }).columns).toHaveLength(3)
    expect((withTab.components[1] as { tabs: { title: string }[] }).tabs[1].title).toBe('Tab 2')
  })

  it('renames a tab and refuses a blank title', () => {
    const definition = form([tabs('t', [['One', []]])])

    expect(
      (
        valid(renameSlot(definition, 'components.0', 0, 'Details')).components[0] as {
          tabs: { title: string }[]
        }
      ).tabs[0].title,
    ).toBe('Details')
    expect(renameSlot(definition, 'components.0', 0, '   ')).toBeNull()
  })

  it('refuses to remove the last slot', () => {
    expect(removeSlot(form([columns('c', [[]])]), 'components.0', 0)).toBeNull()
  })

  it('refuses slot changes on a container that has no slots', () => {
    expect(addSlot(form([panel('p')]), 'components.0')).toBeNull()
  })
})

describe('S10.3 paths (E6)', () => {
  const definition = nested()

  it('resolves the server’s real depth-four path', () => {
    // The shape row 11's probe got back from a running backend on 2026-10-10.
    expect(
      resolveDetail(
        definition,
        'definition.components.0.components.0.tabs.0.components.0.columns.1.components.0',
      ),
    ).toEqual({
      nodePath: 'components.0.components.0.tabs.0.components.0.columns.1.components.0',
      nodeId: 'field_2',
      rest: '',
    })
  })

  it('resolves a column index of one or more, and keeps the rest of the path', () => {
    expect(
      resolveDetail(
        definition,
        'definition.components.0.components.0.tabs.0.components.0.columns.1.components.0.validation.pattern',
      ),
    ).toMatchObject({ nodeId: 'field_2', rest: 'validation.pattern' })
  })

  it('resolves a tab slot', () => {
    expect(
      resolveDetail(definition, 'definition.components.0.components.0.tabs.1.components.0.key'),
    ).toMatchObject({ nodeId: 'field_3', rest: 'key' })
  })

  it('resolves into a row template', () => {
    expect(resolveDetail(definition, 'definition.components.1.components.1.rules.0')).toMatchObject(
      { nodeId: 'qty', rest: 'rules.0' },
    )
  })

  it('stops at the deepest node that exists', () => {
    expect(
      resolveDetail(definition, 'definition.components.0.components.0.tabs.7.components.0'),
    ).toMatchObject({ nodeId: 'tabs_1', rest: 'tabs.7.components.0' })
  })

  it('leaves an unresolvable path unresolved', () => {
    expect(resolveDetail(definition, 'definition.settings.lookups.field_9')).toBeNull()
    expect(resolveDetail(definition, 'definition.components.9.key')).toBeNull()
    expect(resolveDetail(definition, 'definition')).toBeNull()
  })

  it('leaves a path without the server’s prefix unresolved', () => {
    expect(resolveDetail(definition, 'components.2.key')).toBeNull()
  })

  it('places a detail inside an unsupported card on the card, and drops none', () => {
    const odd = form([
      {
        id: 's',
        role: 'layout',
        type: 'steps',
        tabs: [{ title: 'One', components: [field('inside')] }],
      } as JfssComponent,
      field('plain'),
    ])
    const placed = placeDetails(odd, [
      {
        path: 'definition.components.0.tabs.0.components.0.key',
        rule: 'r',
        code: 'C',
        message: 'one',
      },
      { path: 'definition.components.1', rule: 'r', code: 'C', message: 'two' },
      { path: 'definition.title', rule: 'r', code: 'C', message: 'three' },
    ])

    expect(placed.byNode.get('s')?.map((detail) => detail.message)).toEqual(['one'])
    expect(placed.byNode.get('plain')?.map((detail) => detail.message)).toEqual(['two'])
    expect(placed.unplaced.map((detail) => detail.message)).toEqual(['three'])
  })
})
