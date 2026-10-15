import { describe, expect, it } from 'vitest'

import type { ValidationDetail } from '@/types/api'
import type { JfssComponent, JfssValidation } from '@/types/jfss'

import {
  limitConflicts,
  limitsFor,
  messageAt,
  paramsFlag,
  rowKeys,
  seededParams,
  switchValueType,
  toggleUniqueBy,
  withProperty,
} from './propertyEdits'

/**
 * The panel's edits as pure functions (#688 AC3, AC3b, AC9; #722 AC15). The
 * panel's own spec drives them through the screen; these pin the edge cases.
 * The mutations seen to redden the #722 cases are listed in
 * `ValidationRulesSection.spec.ts`.
 */

describe('withProperty', () => {
  it('sets a value, and removes the property for undefined or the empty string', () => {
    const node = { a: 1, b: 'x' }

    expect(withProperty(node, 'c', 0)).toEqual({ a: 1, b: 'x', c: 0 })
    expect(withProperty(node, 'b', '')).toEqual({ a: 1 })
    expect(withProperty(node, 'a', undefined)).toEqual({ b: 'x' })
    expect(withProperty(node, 'b', false)).toEqual({ a: 1, b: false })
    // The original is not changed.
    expect(node).toEqual({ a: 1, b: 'x' })
  })
})

describe('the limits (AC3b)', () => {
  it('fits each value type its own limits, and the others none', () => {
    expect(limitsFor('string')).toEqual(['minLength', 'maxLength', 'pattern'])
    expect(limitsFor('number')).toEqual(['minimum', 'maximum'])
    expect(limitsFor('integer')).toEqual(['minimum', 'maximum'])

    for (const type of ['boolean', 'array', 'object', 'unknown']) {
      expect(limitsFor(type), type).toEqual([])
    }
  })

  it('removes on a type switch only what the new type cannot carry', () => {
    const all = {
      type: 'string',
      minLength: 1,
      maxLength: 2,
      pattern: 'x',
      minimum: 0,
      maximum: 9,
      uniqueBy: 'k',
    } as JfssValidation

    expect(switchValueType(all, 'string')).toEqual({
      type: 'string',
      minLength: 1,
      maxLength: 2,
      pattern: 'x',
      uniqueBy: 'k',
    })
    expect(switchValueType(all, 'integer')).toEqual({
      type: 'integer',
      minimum: 0,
      maximum: 9,
      uniqueBy: 'k',
    })
    expect(switchValueType(all, 'array')).toEqual({ type: 'array', uniqueBy: 'k' })
  })

  it('flags a lower limit above its upper one, and nothing when either is absent or equal', () => {
    expect(limitConflicts({ type: 'string', minLength: 3, maxLength: 2 })).toEqual({
      length: true,
      range: false,
    })
    expect(limitConflicts({ type: 'number', minimum: 2, maximum: 2 })).toEqual({
      length: false,
      range: false,
    })
    expect(limitConflicts({ type: 'number', minimum: 5 })).toEqual({ length: false, range: false })
    expect(limitConflicts({ type: 'number', minimum: 5, maximum: -1 })).toEqual({
      length: false,
      range: true,
    })
  })
})

describe('the data grid (AC3)', () => {
  // F7: through layout containers, as the server's `container_children` reads a
  // row. Was the direct children alone, which missed `deep`.
  it('reads the row template’s data keys through its layout containers, once each', () => {
    const grid = {
      id: 'g',
      role: 'data',
      type: 'datagrid',
      key: 'g',
      label: 'G',
      validation: { type: 'array' },
      components: [
        {
          id: 'a',
          role: 'data',
          type: 'textfield',
          key: 'a',
          label: 'A',
          validation: { type: 'string' },
        },
        { id: 'h', role: 'display', type: 'heading', content: 'H' },
        {
          id: 'a2',
          role: 'data',
          type: 'textfield',
          key: 'a',
          label: 'A',
          validation: { type: 'string' },
        },
        {
          id: 'p',
          role: 'layout',
          type: 'panel',
          components: [
            {
              id: 'deep',
              role: 'data',
              type: 'textfield',
              key: 'deep',
              label: 'D',
              validation: { type: 'string' },
            },
          ],
        },
        {
          id: 'sub',
          role: 'data',
          type: 'datagrid',
          key: 'sub',
          label: 'Sub',
          validation: { type: 'array' },
          components: [
            {
              id: 'inner',
              role: 'data',
              type: 'textfield',
              key: 'inner',
              label: 'I',
              validation: { type: 'string' },
            },
          ],
        },
      ],
    } as unknown as JfssComponent

    // A nested grid is a field of the row; its own row's keys are not.
    expect(rowKeys(grid)).toEqual(['a', 'deep', 'sub'])
    expect(rowKeys({ ...grid, components: undefined } as unknown as JfssComponent)).toEqual([])
  })

  it.each([
    [undefined, 'a', true, 'a'],
    ['a', 'b', true, ['a', 'b']],
    ['a', 'a', false, undefined],
    [['a'], 'b', true, ['a', 'b']],
    [['a', 'b'], 'b', false, ['a']],
    [['a'], 'a', false, undefined],
    [['a'], 'a', true, ['a']],
    ['a', 'a', true, 'a'],
  ] as const)('toggles uniqueBy %j with %s %s into %j', (current, key, on, expected) => {
    expect(toggleUniqueBy(current as string | string[] | undefined, key, on)).toEqual(expected)
  })
})

describe('messageAt (AC9)', () => {
  const detail = (path: string, message: string): ValidationDetail => ({
    path,
    rule: 'r',
    code: 'C',
    message,
  })
  const details = [
    detail('definition.components.1.calculate', 'about calculate'),
    detail('definition.components.1.rules.2.params.pattern', 'about params'),
    detail('definition.components.1.rules.3.params.target', 'about a target'),
    detail('definition.components.1.components.0.calculate', 'about a row field'),
    detail('definition.components.10.validation.pattern', 'about another node'),
  ]

  it('finds a message at a property of this node, or below it', () => {
    expect(messageAt(details, 'components.1', 'calculate')).toBe('about calculate')
    expect(messageAt(details, 'components.1', 'rules.2.params')).toBe('about params')
    // #722's RULE_PARAM_* and RULE_TARGET_NOT_FOUND land on the parameters box too.
    expect(messageAt(details, 'components.1', 'rules.3.params')).toBe('about a target')
  })

  it('does not take a message about another node, or a property that only shares a prefix', () => {
    expect(messageAt(details, 'components.1', 'validation.pattern')).toBeUndefined()
    expect(messageAt(details, 'components.1', 'rules.2.param')).toBeUndefined()
    expect(messageAt(details, 'components.1', 'components')).toBe('about a row field')
  })
})

describe('a rule’s required parameter (#722 AC15)', () => {
  const ENFORCEABLE = ['matchesField', 'notMatchesField', 'regex', 'oneOf', 'notOneOf']

  it('flags every seeded shape: the seeds and the flag read one table', () => {
    for (const rule of ENFORCEABLE) {
      expect(Object.keys(seededParams(rule)), rule).toHaveLength(1)
      expect(paramsFlag(rule, seededParams(rule))?.text, rule).toMatch(/^is empty: /)
    }
  })

  it.each([
    ['matchesField', { target: 'other' }],
    ['notMatchesField', { target: 'other' }],
    ['regex', { pattern: '^A' }],
    ['oneOf', { values: ['a'] }],
    ['notOneOf', { values: [''] }],
  ])('does not flag %s filled in as %j', (rule, params) => {
    expect(paramsFlag(rule, params)).toBeUndefined()
  })

  it('flags a missing parameter, and parameters that are not an object', () => {
    for (const params of [undefined, {}, null, [], 'x']) {
      expect(paramsFlag('regex', params)?.text, JSON.stringify(params)).toMatch(/^is missing: /)
    }
  })

  it('flags the wrong JSON type, null included', () => {
    expect(paramsFlag('matchesField', { target: null })).toEqual({
      param: 'target',
      text: 'must be a text. Saving is refused until it is one.',
    })
    expect(paramsFlag('oneOf', { values: '' })).toEqual({
      param: 'values',
      text: 'must be a list. Saving is refused until it is one.',
    })
  })

  // The server's empty is `''` and `[]` only: a blank pattern is a pattern, and
  // a blank target is its `RULE_TARGET_NOT_FOUND`, which the flag leaves to it.
  // A flag saying "Saving is refused" there would be wrong about the save.
  // Seen red 2026-10-15 (test-engineer campaign) with a trimmed text counted
  // as empty.
  it('does not flag a blank text, which the server does not call empty', () => {
    expect(paramsFlag('regex', { pattern: ' ' })).toBeUndefined()
    expect(paramsFlag('matchesField', { target: ' ' })).toBeUndefined()
    expect(paramsFlag('oneOf', { values: [''] })).toBeUndefined()
  })

  it('seeds and flags nothing for a rule outside the five', () => {
    for (const rule of ['passwordStrength', 'async', 'unique', 'exists', 'authorized', 'x']) {
      expect(seededParams(rule), rule).toEqual({})
      expect(paramsFlag(rule, {}), rule).toBeUndefined()
    }

    // Not an inherited property either.
    expect(seededParams('toString')).toEqual({})
    expect(paramsFlag('toString', {})).toBeUndefined()
  })
})
