import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

import { describe, expect, it } from 'vitest'

import { jfssViolations } from './jfss-meta-schema'

/**
 * The E3 helper, pinned (#688 E3).
 *
 * **Each refusal below is one the helper must see, or every test that uses it
 * passes by not looking.** The first is the one Ajv 6 cannot see on its own:
 * `unevaluatedProperties` is a keyword it ignores, so a stray property on a
 * component is caught by the helper's closure walk or by nothing.
 *
 * # Seen to fail (coding standard §2.9)
 *
 * Run 2026-10-10: with `undeclared(...)` removed from `jfssViolations`, *refuses
 * a property its role does not declare* and *refuses a key on a layout node*
 * go red, and nothing else does.
 */

function definition(components: unknown[]): Record<string, unknown> {
  return { formId: 'f', version: '2.0.1', components }
}

const field = {
  id: 'field_1',
  role: 'data',
  type: 'textfield',
  key: 'field_1',
  label: 'Field',
  validation: { type: 'string' },
}

describe('jfssViolations', () => {
  it('accepts a conformant definition, nested containers included', () => {
    expect(
      jfssViolations(
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
                        columns: [{ components: [] }, { components: [field] }],
                      },
                    ],
                  },
                ],
              },
            ],
          },
        ]),
      ),
    ).toEqual([])
  })

  it('refuses a property its role does not declare, at any depth', () => {
    const violations = jfssViolations(
      definition([
        {
          id: 'columns_1',
          role: 'layout',
          type: 'columns',
          columns: [{ components: [{ ...field, bogus: true }] }],
        },
      ]),
    )

    expect(violations.join('\n')).toContain('components.0.columns.0.components.0')
    expect(violations.join('\n')).toContain('"bogus"')
  })

  it('refuses a key on a layout node', () => {
    expect(
      jfssViolations(
        definition([{ id: 'p', role: 'layout', type: 'panel', key: 'p', components: [] }]),
      ),
    ).not.toEqual([])
  })

  it('refuses a stray property on a slot', () => {
    expect(
      jfssViolations(
        definition([
          { id: 'c', role: 'layout', type: 'columns', columns: [{ width: 6, components: [] }] },
        ]),
      ),
    ).not.toEqual([])
  })

  it('refuses a layout node with two shapes', () => {
    expect(
      jfssViolations(
        definition([
          {
            id: 'p',
            role: 'layout',
            type: 'panel',
            components: [],
            columns: [{ components: [] }],
          },
        ]),
      ),
    ).not.toEqual([])
  })

  it('refuses a layout node with no shape', () => {
    expect(
      jfssViolations(definition([{ id: 'p', role: 'layout', type: 'panel', title: 'P' }])),
    ).not.toEqual([])
  })

  it('refuses a tab without a title', () => {
    expect(
      jfssViolations(
        definition([{ id: 't', role: 'layout', type: 'tabs', tabs: [{ components: [] }] }]),
      ),
    ).not.toEqual([])
  })

  it('refuses a data node without validation', () => {
    const bare: Record<string, unknown> = { ...field }

    delete bare.validation

    expect(jfssViolations(definition([bare]))).not.toEqual([])
  })

  it('refuses a rule without params or a message', () => {
    expect(
      jfssViolations(definition([{ ...field, rules: [{ rule: 'regex', scope: 'both' }] }])),
    ).not.toEqual([])
  })

  // --- test-engineer campaign, 2026-10-10 (#688 row 11) -------------------------------
  //
  // The walk's two sets are transcribed from the meta-schema; these read the
  // file again and hold the transcription to it, in both directions, so a
  // property added to or dropped from one role's branch cannot leave the helper
  // passing what the server refuses, or refusing what it takes.

  const meta = JSON.parse(
    readFileSync(resolve(process.cwd(), '../docs/schema/jfss-meta-v2.0.1.json'), 'utf8'),
  ) as {
    $defs: {
      component: {
        properties: Record<string, unknown>
        allOf: {
          if: { properties: { role: { const: string } } }
          then: { properties: Record<string, unknown> }
        }[]
      }
    }
  }
  const base = Object.keys(meta.$defs.component.properties)
  const byRole = Object.fromEntries(
    meta.$defs.component.allOf.map((branch) => [
      branch.if.properties.role.const,
      Object.keys(branch.then.properties),
    ]),
  ) as Record<string, string[]>

  /** A node of each role that conforms on its own. */
  const minimal: Record<string, Record<string, unknown>> = {
    data: { ...field },
    layout: { id: 'p', role: 'layout', type: 'panel', components: [] },
    display: { id: 'h', role: 'display', type: 'heading' },
    action: { id: 'b', role: 'action', type: 'button', label: 'Go', action: 'submit' },
  }

  /** The closure walk's complaints about one property, whatever Ajv says about its value. */
  function walkFlags(node: Record<string, unknown>, key: string): boolean {
    return jfssViolations(definition([node])).some((line) =>
      line.endsWith(`undeclared property "${key}"`),
    )
  }

  it('reads four role branches from the meta-schema, so the checks below are not vacuous', () => {
    expect(Object.keys(byRole).sort()).toEqual(['action', 'data', 'display', 'layout'])
    expect(base).toEqual(expect.arrayContaining(['id', 'role', 'type']))
  })

  it.each(['data', 'layout', 'display', 'action'])(
    'takes every property the meta-schema declares for a %s node',
    (role) => {
      for (const key of [...base, ...byRole[role]]) {
        expect(walkFlags({ ...minimal[role], [key]: minimal[role][key] ?? 'x' }, key), key).toBe(
          false,
        )
      }
    },
  )

  it.each(['data', 'layout', 'display', 'action'])(
    'refuses every property the meta-schema declares only for another role, on a %s node',
    (role) => {
      const own = new Set([...base, ...byRole[role]])
      const foreign = [...new Set(Object.values(byRole).flat())].filter((key) => !own.has(key))

      expect(foreign.length).toBeGreaterThan(0)

      for (const key of foreign) {
        expect(walkFlags({ ...minimal[role], [key]: 'x' }, key), key).toBe(true)
      }
    },
  )

  it('refuses a stray property inside a tab and inside a row template', () => {
    const inTab = jfssViolations(
      definition([
        {
          id: 't',
          role: 'layout',
          type: 'tabs',
          tabs: [{ title: 'T', components: [{ ...field, bogus: true }] }],
        },
      ]),
    )
    const inRow = jfssViolations(
      definition([
        {
          id: 'g',
          role: 'data',
          type: 'datagrid',
          key: 'g',
          label: 'G',
          validation: { type: 'array' },
          components: [{ id: 'h', role: 'display', type: 'heading', key: 'h' }],
        },
      ]),
    )

    expect(inTab.join(' | ')).toContain(
      'components.0.tabs.0.components.0 has undeclared property "bogus"',
    )
    expect(inRow.join(' | ')).toContain('components.0.components.0 has undeclared property "key"')
  })
})
