import { describe, expect, it } from 'vitest'

import {
  BUILDER_REGISTRY,
  PALETTE_GROUPS,
  entryFor,
  offeredEntries,
  offeredEntry,
  type OfferedEntry,
} from './builderRegistry'
import { NOT_YET_RENDERED, SUPPORTED } from '@/features/rad/renderer/registry'
import { jfssViolations } from '@/lib/testing/jfss-meta-schema'
import type { JfssComponent } from '@/types/jfss'

/**
 * The builder's registry, pinned against the renderer's (#688 B1–B4).
 *
 * **The builder offers what the renderer draws, and nothing else.** Both
 * directions are asserted: a type offered and not drawn saves, publishes and
 * renders as a placeholder; a type drawn and not offered is a gap nobody
 * chose.
 *
 * # Seen to fail (coding standard §2.9)
 *
 * Run 2026-10-10, each against this file alone:
 *
 * | Mutation | Reddened |
 * |---|---|
 * | `steps` made an offered entry | *offers exactly the renderer's SUPPORTED types*, *offers nothing the renderer declares undrawn* |
 * | `alert`'s default loses `content` | *gives alert content* |
 * | `tabs`' default loses its slot `title` | *gives columns two empty slots and tabs one slot titled Tab 1*, *creates a tabs default the meta-schema accepts on its own* |
 */

const identity = { id: 'node_1', key: 'node_1' }

function inRoot(node: JfssComponent): Record<string, unknown> {
  return { formId: 'f', version: '2.0.1', components: [node] }
}

describe('the form builder registry', () => {
  it('offers exactly the renderer’s SUPPORTED types, each with the same role', () => {
    const offered = Object.fromEntries(offeredEntries().map((entry) => [entry.type, entry.role]))
    const supported = Object.fromEntries(
      Object.entries(SUPPORTED).map(([type, entry]) => [type, entry.role]),
    )

    expect(offered).toEqual(supported)
    expect(offeredEntries()).toHaveLength(18)
  })

  it('offers nothing the renderer declares undrawn, and says why for each', () => {
    for (const type of Object.keys(NOT_YET_RENDERED)) {
      const entry = entryFor(type)

      expect(entry, `"${type}" has no entry`).toBeDefined()
      expect(entry?.offered, `"${type}" is offered`).toBe(false)
      expect(entry && !entry.offered ? entry.reason : '').not.toBe('')
    }
  })

  it.each(['repeater', 'spacer'])('withholds %s with a reason (D-86)', (type) => {
    const entry = entryFor(type)

    expect(entry?.offered).toBe(false)
    expect(entry && !entry.offered ? entry.reason : '').not.toBe('')
  })

  it('has no opinion about a type nobody declared, nor about Object’s own names', () => {
    expect(entryFor('nonexistent-widget')).toBeUndefined()
    expect(entryFor('constructor')).toBeUndefined()
    expect(offeredEntry('toString')).toBeUndefined()
  })

  it('groups every offered type under its role, and every group is a palette group', () => {
    const groups = PALETTE_GROUPS.map((group) => group.group)

    for (const entry of offeredEntries()) {
      expect(entry.group).toBe(entry.role)
      expect(groups).toContain(entry.group)
    }
  })

  it('makes panel, fieldset, columns, tabs and datagrid the containers', () => {
    const containers = offeredEntries()
      .filter((entry) => entry.children !== 'none')
      .map((entry) => [entry.type, entry.children])

    expect(Object.fromEntries(containers)).toEqual({
      panel: 'components',
      fieldset: 'components',
      columns: 'columns',
      tabs: 'tabs',
      datagrid: 'rowTemplate',
    })
  })

  it.each(offeredEntries().map((entry) => [entry.type, entry] as const))(
    'creates a %s default the meta-schema accepts on its own (B4, E3)',
    (_type, entry: OfferedEntry) => {
      const node = entry.create(identity)

      expect(node.type).toBe(entry.type)
      expect(node.role).toBe(entry.role)
      expect(node.id).toBe('node_1')
      expect(jfssViolations(inRoot(node))).toEqual([])
    },
  )

  it('gives a data node the key it was handed and a layout node none', () => {
    for (const entry of offeredEntries()) {
      const node = entry.create(identity) as unknown as Record<string, unknown>

      if (entry.role === 'data') {
        expect(node.key).toBe('node_1')
        expect(String(node.key)).toMatch(/^[A-Za-z_][A-Za-z0-9_]*$/)
      } else {
        expect(node).not.toHaveProperty('key')
      }
    }
  })

  it.each(['heading', 'paragraph', 'alert'])('gives %s content', (type) => {
    const node = (BUILDER_REGISTRY[type] as OfferedEntry).create(identity) as {
      content?: string
    }

    expect(node.content).toBeTruthy()
  })

  it.each(['select', 'radio'])('gives %s at least one option', (type) => {
    const node = (BUILDER_REGISTRY[type] as OfferedEntry).create(identity) as {
      options?: unknown[]
    }

    expect(node.options?.length).toBeGreaterThan(0)
  })

  it('gives columns two empty slots and tabs one slot titled Tab 1', () => {
    expect((BUILDER_REGISTRY.columns as OfferedEntry).create(identity)).toMatchObject({
      columns: [{ components: [] }, { components: [] }],
    })
    expect((BUILDER_REGISTRY.tabs as OfferedEntry).create(identity)).toMatchObject({
      tabs: [{ title: 'Tab 1', components: [] }],
    })
  })

  it('gives a datagrid an array value and an empty row template', () => {
    expect((BUILDER_REGISTRY.datagrid as OfferedEntry).create(identity)).toMatchObject({
      validation: { type: 'array' },
      components: [],
    })
  })

  it('gives a button the submit action and a Submit label', () => {
    expect((BUILDER_REGISTRY.button as OfferedEntry).create(identity)).toMatchObject({
      label: 'Submit',
      action: 'submit',
    })
  })

  it('never shares an options array between two defaults', () => {
    const select = BUILDER_REGISTRY.select as OfferedEntry
    const first = select.create(identity) as { options: unknown[] }
    const second = select.create(identity) as { options: unknown[] }

    expect(first.options).not.toBe(second.options)
    expect(first.options[0]).not.toBe(second.options[0])
  })
})
