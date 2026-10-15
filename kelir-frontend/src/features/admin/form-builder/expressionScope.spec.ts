import { describe, expect, it } from 'vitest'

import type { JfssDefinition } from '@/types/jfss'

import { scopeVariables } from './expressionScope'

/**
 * What an expression on a node may read (#688 AC6).
 *
 * # Seen to fail (coding standard §2.9)
 *
 * Run 2026-10-15 against this file alone: `dataVariables` descending into a
 * data node's `components` reddens *offers the top level every data key
 * outside row templates …*, *offers a row template its own row's keys* and
 * *falls back to the top level …*; `scopeListOf` never entering a row template
 * reddens *offers a row template its own row's keys* and *offers a nested
 * grid's row its own keys*.
 */

function field(key: string, type = 'number'): Record<string, unknown> {
  return {
    id: key,
    role: 'data',
    type: 'number',
    key,
    label: key.toUpperCase(),
    validation: { type },
  }
}

const DEFINITION = {
  formId: 'order',
  version: '2.0.1',
  components: [
    field('quantity'),
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
            field('qty'),
            field('price'),
            {
              id: 'parts',
              role: 'data',
              type: 'datagrid',
              key: 'parts',
              label: 'Parts',
              validation: { type: 'array' },
              components: [field('part_no', 'string')],
            },
          ],
        },
      ],
    },
    {
      id: 'columns_1',
      role: 'layout',
      type: 'columns',
      columns: [{ components: [field('discount')] }, { components: [] }],
    },
    {
      id: 'tabs_1',
      role: 'layout',
      type: 'tabs',
      tabs: [{ title: 'More', components: [field('notes', 'string'), field('quantity')] }],
    },
    { id: 'h', role: 'display', type: 'heading', content: 'Totals' },
  ],
} as unknown as JfssDefinition

function paths(nodePath: string): string[] {
  return scopeVariables(DEFINITION, nodePath).map((variable) => variable.path)
}

describe('scopeVariables (AC6)', () => {
  it('offers the top level every data key outside row templates, through every container', () => {
    const expected = ['quantity', 'lines', 'discount', 'notes']

    expect(paths('components.0')).toEqual(expected)
    // A node inside a column or a tab is still at the top level.
    expect(paths('components.2.columns.0.components.0')).toEqual(expected)
    expect(paths('components.3.tabs.0.components.0')).toEqual(expected)
    // And so is a display node, and the grid itself.
    expect(paths('components.4')).toEqual(expected)
    expect(paths('components.1.components.0')).toEqual(expected)
  })

  it('offers a row template its own row’s keys', () => {
    expect(paths('components.1.components.0.components.0')).toEqual(['qty', 'price', 'parts'])
  })

  it('offers a nested grid’s row its own keys', () => {
    expect(paths('components.1.components.0.components.2.components.0')).toEqual(['part_no'])
  })

  it('carries each key’s label and value type, and the first declaration of a repeated key', () => {
    expect(scopeVariables(DEFINITION, 'components.0')[0]).toEqual({
      path: 'quantity',
      label: 'QUANTITY',
      type: 'number',
    })
    expect(paths('components.0').filter((path) => path === 'quantity')).toHaveLength(1)
  })

  it('falls back to the top level for a path that names no node', () => {
    expect(paths('components.9')).toEqual(['quantity', 'lines', 'discount', 'notes'])
  })
})
