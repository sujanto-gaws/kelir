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
})
