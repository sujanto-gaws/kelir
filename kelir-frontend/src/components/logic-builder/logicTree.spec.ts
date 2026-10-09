// Node's types are pulled in for this file alone, as `jsonlogic.parity.spec.ts`
// does, because the corpus is read off disk from `parity/cases.json` rather than
// copied: a case added there must be classified here or this spec fails.
/// <reference types="node" />
import fs from 'node:fs'
import path from 'node:path'

import { beforeAll, describe, expect, it } from 'vitest'

import { loadEvaluator, type RuleEvaluator } from '@/lib/jsonlogic'

import {
  addOperandAt,
  applyEdit,
  CALCULATE_TIER_OPERATORS,
  changeOperatorAt,
  CONDITIONAL_TIER_OPERATORS,
  containsOpaque,
  createNode,
  familiesIn,
  IncompleteExpressionError,
  isComplete,
  isOfferedPath,
  isRootRepresentable,
  literalNode,
  MAX_VISUAL_DEPTH,
  nodeAt,
  OPERATOR_LABELS,
  parseExpression,
  rebuild,
  removeOperandAt,
  replaceAt,
  serialise,
  varNode,
  type LogicTier,
} from './logicTree'

/**
 * The seam under the visual JSON Logic builder (#686, decision **D-110**).
 *
 * **The round trip is the first thing this file says, because it is the whole
 * promise**: an expression the builder represents comes back as the JSON it was
 * parsed from, and one it cannot represent comes back as the same value. Two
 * paths are held to it. `serialise` is what the builder emits, and returns an
 * untouched node's original value, so for an unedited tree it is identity by
 * construction. `rebuild` is what `serialise` does for a node the user *did*
 * touch, applied everywhere — so it is the proof that the representation itself
 * loses nothing, and the evaluation test below runs it rather than `serialise`.
 *
 * "Identical" means `JSON.stringify` equality, key order included (criterion
 * C4). What `JSON.parse` already lost — a duplicate key, `1.0` against `1`, an
 * integer past 2^53 — is lost before the seam, as it is in today's box.
 */

interface CorpusEntry {
  id: string
  expr: unknown
  data: unknown
  tier: LogicTier
}

/** Calculation Rule Registry §3.1 and §6, the examples built only of offered operators. */
const REGISTRY_REPRESENTABLE: CorpusEntry[] = [
  { id: '§3.1 var key', expr: { var: 'key' }, data: { key: 4 }, tier: 'calculate' },
  { id: '§3.1 var array.0', expr: { var: 'array.0' }, data: { array: [7] }, tier: 'calculate' },
  {
    id: '§3.1 var nested',
    expr: { var: 'array.0.nestedProperty' },
    data: { array: [{ nestedProperty: 3 }] },
    tier: 'calculate',
  },
  {
    id: '§3.1 var row path',
    expr: { var: 'items.0.unit_price' },
    data: { items: [{ unit_price: 12.5 }] },
    tier: 'calculate',
  },
  {
    id: '§3.1 line item total',
    expr: { '*': [{ var: 'unit_price' }, { var: 'quantity' }] },
    data: { unit_price: 12.5, quantity: 3 },
    tier: 'calculate',
  },
  {
    id: '§3.1 weighted sum',
    expr: {
      '+': [
        { '*': [{ var: 'a.0' }, 1] },
        { '*': [{ var: 'a.1' }, 2] },
        { '*': [{ var: 'a.2' }, 3] },
      ],
    },
    data: { a: [1, 2, 3] },
    tier: 'calculate',
  },
  {
    id: '§3.1 average',
    expr: { '/': [{ var: 'total_score' }, { var: 'score_count' }] },
    data: { total_score: 90, score_count: 4 },
    tier: 'calculate',
  },
]

/** The registry examples that use an operator the builder does not offer: passed through. */
const REGISTRY_OPAQUE: unknown[] = [
  { min: [{ '*': [{ var: 'subtotal' }, 0.2] }, 100] },
  { map: [{ var: 'items' }, { '*': [{ var: 'unit_price' }, { var: 'quantity' }] }] },
  { sum: [{ map: [{ var: 'items' }, { '*': [{ var: 'unit_price' }, { var: 'quantity' }] }] }] },
  { min: [{ '*': [{ var: 'subtotal' }, { var: 'discount_percent' }, 0.01] }, 100] },
]

/** JWSS §10's one condition. */
const JWSS_CONDITIONS: CorpusEntry[] = [
  {
    id: 'JWSS §10 finance limit',
    expr: { '<=': [{ var: 'document.amount' }, 10000000] },
    data: { document: { amount: 45000000 } },
    tier: 'conditional',
  },
]

interface ParityCase {
  id: string
  expr: unknown
  data: unknown
}

const parityCases: ParityCase[] = JSON.parse(
  fs.readFileSync(path.resolve(process.cwd(), '..', 'parity', 'cases.json'), 'utf8'),
)

/**
 * Every parity case, classified by hand. **A case added to `cases.json` and not
 * listed here fails the classification test**, so the corpus cannot grow past
 * this spec unnoticed.
 */
const PARITY_REPRESENTABLE = new Set([
  'var-simple',
  'var-dot-index',
  'var-missing',
  'mul',
  'add-weighted',
  'sub',
  'div',
  'mod',
  'div-by-zero',
  'mul-null',
  'mul-missing-var',
  'add-null',
  'add-string',
  'float-repr',
  'eq-loose',
  'eq-strict',
  'neq',
  'gt',
  'and-or',
  'not-empty-string',
  'not-empty-array',
  'div-by-zero-float',
  'div-zero-by-zero',
  'mod-by-zero',
  'sub-null',
  'add-array-operand',
  'add-object-operand',
  'add-non-numeric-string-operand',
  'mul-array-operand',
])

/** Opaque at the root: the whole expression opens in the raw-JSON box. */
const PARITY_OPAQUE_ROOT = new Set([
  'var-default',
  'min-cap',
  'max',
  'map-line-totals',
  'map-empty',
  'map-missing-array',
  'filter',
  'reduce',
  'reduce-empty',
  'all',
  'all-empty',
  'some',
  'some-empty',
  'none-empty',
  'sum',
  'sum-empty',
  'invoice-pattern',
  'unknown-operator',
  'between',
  'if',
  'in-array',
  'in-string',
  'cat',
  'missing',
  'missing-some',
  'min-null',
  'datagrid-typing-race',
  'sum-non-numeric-members',
  'sum-non-array-argument',
  'sum-shorthand-argument',
  'sum-multi-argument',
])

const PARITY_REPRESENTABLE_ENTRIES: CorpusEntry[] = parityCases
  .filter((subject) => PARITY_REPRESENTABLE.has(subject.id))
  .map((subject) => ({ ...subject, tier: 'conditional' }))

// --- The generated sweep ----------------------------------------------------

const OPERAND_SAMPLES: unknown[] = [
  { var: 'a' },
  { var: 'items.0.unit_price' },
  5,
  2.5,
  -1,
  0,
  '5',
  '',
  'text',
  true,
  false,
  null,
]

const BINARY = ['-', '/', '%', '==', '===', '!=', '!==', '>', '>=', '<', '<='] as const
const VARIADIC = ['+', '*', 'and', 'or'] as const
const CONDITIONAL_ONLY = ['==', '===', '!=', '!==', '>', '>=', '<', '<=', 'and', 'or', '!']

/** Each offered operator at each arity it accepts, over every literal type and `var`. */
function representableSweep(): unknown[] {
  const out: unknown[] = []

  for (const op of BINARY) {
    for (const left of OPERAND_SAMPLES) {
      out.push({ [op]: [left, { var: 'b' }] })
      out.push({ [op]: [{ var: 'b' }, left] })
    }
  }

  for (const op of VARIADIC) {
    out.push({ [op]: [{ var: 'a' }, 1] })
    out.push({ [op]: [{ var: 'a' }, 1, '2', null] })
    out.push({ [op]: OPERAND_SAMPLES })
  }

  for (const operand of OPERAND_SAMPLES) {
    out.push({ '!': operand })
    out.push({ '!': [operand] })
  }

  // Nesting, three deep, every family inside every other.
  out.push({
    and: [
      { or: [{ '===': [{ var: 'type' }, 'invoice'] }, { '!': { var: 'draft' } }] },
      { '>=': [{ '+': [{ '*': [{ var: 'a' }, 2] }, { '%': [{ var: 'b' }, 3] }] }, 10] },
      { '!': [{ '!==': [{ '-': [{ var: 'c' }, { '/': [{ var: 'd' }, 4] }] }, 0] }] },
    ],
  })

  return out
}

/** Shapes section B makes opaque, as operands of a representable `and`. */
const OPAQUE_OPERANDS: Record<string, unknown> = {
  'B1 binary with one operand': { '-': [1] },
  'B1 binary with three operands': { '<': [1, { var: 'n' }, 10] },
  'B1 variadic with one operand': { '+': [1] },
  'B1 variadic with none': { and: [] },
  'B1 two keys': { '+': [1, 2], note: 'x' },
  'B1 not with two operands': { '!': [true, false] },
  'B1 not with none': { '!': [] },
  'B3 registry operator D-110 does not offer': { min: [1, 2] },
  'B3 unknown operator': { cat: ['INV-', { var: 'n' }] },
  'B3 made-up operator': { frobnicate: [1] },
  'B4 var array-wrapped': { var: ['a'] },
  'B4 var with a default': { var: ['a', 0] },
  'B4 var numeric': { var: 5 },
  'B4 var empty': { var: '' },
  'B6 bare array operand': [1, 2],
  'B6 operator value not an array': { '+': 5 },
  'B6 operator value an object': { '*': { var: 'a' } },
  'B7 empty object': {},
  'B7 two-key object': { b: 1, a: 2 },
}

// --- C1 ---------------------------------------------------------------------

describe('the round trip (C1)', () => {
  const representable: [string, unknown, LogicTier][] = [
    ...REGISTRY_REPRESENTABLE.map((e): [string, unknown, LogicTier] => [e.id, e.expr, e.tier]),
    ...JWSS_CONDITIONS.map((e): [string, unknown, LogicTier] => [e.id, e.expr, e.tier]),
    ...PARITY_REPRESENTABLE_ENTRIES.map((e): [string, unknown, LogicTier] => [
      `parity ${e.id}`,
      e.expr,
      e.tier,
    ]),
    ...representableSweep().map((e): [string, unknown, LogicTier] => [
      `sweep ${JSON.stringify(e)}`,
      e,
      'conditional',
    ]),
  ]

  it.each(representable)('%s comes back as the JSON it was parsed from', (_id, expr, tier) => {
    const tree = parseExpression(expr, tier)

    expect(isRootRepresentable(tree)).toBe(true)
    expect(containsOpaque(tree)).toBe(false)
    expect(JSON.stringify(serialise(tree))).toBe(JSON.stringify(expr))
    // The rebuilt value, built from the tree and not from the original.
    expect(JSON.stringify(rebuild(tree))).toBe(JSON.stringify(expr))
    expect(rebuild(tree)).not.toBe(expr)
  })

  it('emits an untouched tree as the value it was parsed from, by reference', () => {
    for (const entry of REGISTRY_REPRESENTABLE) {
      expect(serialise(parseExpression(entry.expr, entry.tier))).toBe(entry.expr)
    }
  })

  it('classifies every parity case, so a new one cannot pass unread', () => {
    const unclassified = parityCases
      .map((subject) => subject.id)
      .filter((id) => !PARITY_REPRESENTABLE.has(id) && !PARITY_OPAQUE_ROOT.has(id))

    expect(unclassified).toEqual([])
  })

  it('agrees with the classification for every parity case', () => {
    for (const subject of parityCases) {
      const tree = parseExpression(subject.expr, 'conditional')
      const whole = isRootRepresentable(tree) && !containsOpaque(tree)

      expect({ id: subject.id, whole }).toEqual({
        id: subject.id,
        whole: PARITY_REPRESENTABLE.has(subject.id),
      })
    }
  })
})

// --- C2 ---------------------------------------------------------------------

describe('pass-through (C2)', () => {
  const opaqueRoots: [string, unknown][] = [
    ...REGISTRY_OPAQUE.map((e): [string, unknown] => [`registry ${JSON.stringify(e)}`, e]),
    ...parityCases
      .filter((subject) => PARITY_OPAQUE_ROOT.has(subject.id))
      .map((subject): [string, unknown] => [`parity ${subject.id}`, subject.expr]),
  ]

  it.each(opaqueRoots)('%s is opaque at the root and comes back unchanged', (_id, expr) => {
    const tree = parseExpression(expr, 'conditional')

    expect(isRootRepresentable(tree)).toBe(false)
    expect(serialise(tree)).toBe(expr)
    expect(rebuild(tree)).toBe(expr)
  })

  it.each(Object.entries(OPAQUE_OPERANDS))(
    '%s is an opaque leaf, held and emitted by reference',
    (_shape, operand) => {
      const expr = { and: [{ var: 'x' }, operand] }
      const tree = parseExpression(expr, 'conditional')

      expect(isRootRepresentable(tree)).toBe(true)
      expect(nodeAt(tree, [1])).toEqual(expect.objectContaining({ kind: 'opaque' }))
      expect((rebuild(tree) as { and: unknown[] }).and[1]).toBe(operand)
      expect(JSON.stringify(rebuild(tree))).toBe(JSON.stringify(expr))
    },
  )

  it('keeps an opaque object’s key order, which a re-sort would change', () => {
    const operand = { z: 1, a: 2, m: [3, 1, 2] }
    const tree = parseExpression({ or: [operand, true] }, 'conditional')
    const edited = replaceAt(tree, [1], createNode('null'))

    const emitted = serialise(edited) as { or: unknown[] }

    expect(emitted.or[0]).toBe(operand)
    expect(JSON.stringify(emitted)).toBe('{"or":[{"z":1,"a":2,"m":[3,1,2]},null]}')
  })
})

// --- Section B ----------------------------------------------------------------

describe('representability (B1–B8)', () => {
  it.each([
    ['a bare number', 5],
    ['a bare string', 'x'],
    ['true', true],
    ['null', null],
    ['an array', [{ var: 'a' }]],
    ['an empty object', {}],
    ['a two-key object', { '+': [1, 2], '-': [3, 4] }],
    ['an unknown operator', { cat: ['a', 'b'] }],
  ])('B2: %s at the root is not representable', (_shape, expr) => {
    const tree = parseExpression(expr, 'conditional')

    expect(isRootRepresentable(tree)).toBe(false)
    expect(serialise(tree)).toBe(expr)
  })

  it('B2: a single-key operator object and a bare var are representable roots', () => {
    expect(isRootRepresentable(parseExpression({ '+': [1, 2] }, 'calculate'))).toBe(true)
    expect(isRootRepresentable(parseExpression({ var: 'a' }, 'calculate'))).toBe(true)
  })

  it('B1: each arity is checked', () => {
    const operandsFor = (count: number) => Array.from({ length: count }, (_, at) => at)

    for (const op of BINARY) {
      expect(parseExpression({ [op]: operandsFor(2) }, 'conditional').kind).toBe('operator')
      expect(parseExpression({ [op]: operandsFor(1) }, 'conditional').kind).toBe('opaque')
      expect(parseExpression({ [op]: operandsFor(3) }, 'conditional').kind).toBe('opaque')
    }

    for (const op of VARIADIC) {
      expect(parseExpression({ [op]: operandsFor(2) }, 'conditional').kind).toBe('operator')
      expect(parseExpression({ [op]: operandsFor(5) }, 'conditional').kind).toBe('operator')
      expect(parseExpression({ [op]: operandsFor(1) }, 'conditional').kind).toBe('opaque')
      expect(parseExpression({ [op]: operandsFor(0) }, 'conditional').kind).toBe('opaque')
    }

    expect(parseExpression({ '!': [1] }, 'conditional').kind).toBe('operator')
    expect(parseExpression({ '!': 1 }, 'conditional').kind).toBe('operator')
    expect(parseExpression({ '!': [1, 2] }, 'conditional').kind).toBe('opaque')
    expect(parseExpression({ '!': [] }, 'conditional').kind).toBe('opaque')
  })

  it('B1: a second key makes any operator opaque', () => {
    for (const op of CONDITIONAL_TIER_OPERATORS) {
      const value = op === 'var' ? 'a' : op === '!' ? [true] : [1, 2]

      expect(parseExpression({ [op]: value, extra: 1 }, 'conditional').kind).toBe('opaque')
      expect(parseExpression({ [op]: value }, 'conditional').kind).not.toBe('opaque')
    }
  })

  it('B4: a var path is a non-empty string; every other var shape is opaque', () => {
    expect(parseExpression({ var: 'items.0.unit_price' }, 'calculate')).toEqual(
      expect.objectContaining({ kind: 'var', path: 'items.0.unit_price' }),
    )

    for (const shape of [['a'], ['a', 0], 5, '', null, { a: 1 }]) {
      expect(parseExpression({ '+': [{ var: shape }, 1] }, 'calculate').kind).toBe('operator')
      expect(nodeAt(parseExpression({ '+': [{ var: shape }, 1] }, 'calculate'), [0]).kind).toBe(
        'opaque',
      )
    }
  })

  it('B4: a path outside the offered variables is representable, and is flagged', () => {
    const variables = [{ path: 'amount', label: 'Amount' }]

    expect(isOfferedPath('amount', variables)).toBe(true)
    expect(isOfferedPath('amount.0', variables)).toBe(false)
    expect(isOfferedPath('unknown_field', variables)).toBe(false)
    expect(parseExpression({ var: 'unknown_field' }, 'calculate').kind).toBe('var')
  })

  it('B5: a literal keeps its type', () => {
    const tree = parseExpression({ '==': ['5', 5] }, 'conditional')

    expect(nodeAt(tree, [0])).toEqual(expect.objectContaining({ kind: 'literal', value: '5' }))
    expect(nodeAt(tree, [1])).toEqual(expect.objectContaining({ kind: 'literal', value: 5 }))

    const edited = replaceAt(tree, [1], literalNode(6))

    expect(JSON.stringify(serialise(edited))).toBe('{"==":["5",6]}')

    for (const literal of [true, false, null]) {
      expect(nodeAt(parseExpression({ '===': [literal, 1] }, 'conditional'), [0])).toEqual(
        expect.objectContaining({ kind: 'literal', value: literal }),
      )
    }
  })

  it('B6: a nested array as an argument is opaque; its operator is not', () => {
    const tree = parseExpression({ '+': [[1, 2], 3] }, 'calculate')

    expect(tree.kind).toBe('operator')
    expect(nodeAt(tree, [0]).kind).toBe('opaque')
    expect(nodeAt(tree, [1]).kind).toBe('literal')
  })

  it('B8: not keeps the form it was parsed in, through an edit of its operand', () => {
    const bare = parseExpression({ '!': { var: 'a' } }, 'conditional')
    const wrapped = parseExpression({ '!': [{ var: 'a' }] }, 'conditional')
    const renamed = varNode('b')

    expect(JSON.stringify(serialise(replaceAt(bare, [0], renamed)))).toBe('{"!":{"var":"b"}}')
    expect(JSON.stringify(serialise(replaceAt(wrapped, [0], renamed)))).toBe('{"!":[{"var":"b"}]}')
  })
})

// --- C6 ---------------------------------------------------------------------

describe('the tier (C6)', () => {
  it('offers comparisons, and, or and not only in the conditional tier', () => {
    for (const op of CONDITIONAL_ONLY) {
      const value = op === '!' ? [true] : [1, 2]

      expect(parseExpression({ [op]: value }, 'conditional').kind).toBe('operator')
      expect(parseExpression({ [op]: value }, 'calculate').kind).toBe('opaque')
    }
  })

  it('holds a conditional-only operator inside a calculation as an opaque leaf', () => {
    const comparison = { '>': [{ var: 'a' }, 1] }
    const tree = parseExpression({ '*': [comparison, 2] }, 'calculate')

    expect(tree.kind).toBe('operator')
    expect(nodeAt(tree, [0])).toEqual(
      expect.objectContaining({ kind: 'opaque', value: comparison }),
    )
  })

  it('offers every section A operator in the conditional tier', () => {
    for (const op of CONDITIONAL_TIER_OPERATORS) {
      const value = op === 'var' ? 'a' : op === '!' ? [true] : [1, 2]

      expect(parseExpression({ [op]: value }, 'conditional').kind).not.toBe('opaque')
    }
  })
})

// --- C5, at the seam ------------------------------------------------------------

describe('a mixed expression (C5)', () => {
  it('changes only the edited child; the cat subtree and the sibling are the same values', () => {
    const cat = { cat: ['INV-', { var: 'n' }] }
    const sibling = { '>': [{ var: 'total' }, 0] }
    const expr = { and: [{ '===': [{ var: 'type' }, 'invoice'] }, cat, sibling] }

    const tree = parseExpression(expr, 'conditional')
    const edited = replaceAt(tree, [0, 1], literalNode('credit'))
    const emitted = serialise(edited) as { and: unknown[] }

    expect(emitted.and[1]).toBe(cat)
    expect(emitted.and[2]).toBe(sibling)
    expect(JSON.stringify(emitted)).toBe(
      JSON.stringify({ and: [{ '===': [{ var: 'type' }, 'credit'] }, cat, sibling] }),
    )
    // The input is not mutated by the edit.
    expect(JSON.stringify(expr.and[0])).toBe('{"===":[{"var":"type"},"invoice"]}')
  })
})

// --- Edits ------------------------------------------------------------------

describe('editing a tree', () => {
  const base = () => parseExpression({ '+': [{ var: 'a' }, 1, 2] }, 'calculate')

  it('adds an operand as an unfilled hole, which cannot be emitted', () => {
    const added = addOperandAt(base(), [])

    expect(isComplete(added)).toBe(false)
    expect(() => serialise(added)).toThrow(IncompleteExpressionError)
  })

  it('removes an operand while the operator keeps at least two', () => {
    const removed = removeOperandAt(base(), [1])

    expect(JSON.stringify(serialise(removed))).toBe('{"+":[{"var":"a"},2]}')
    expect(() => removeOperandAt(removed, [0])).toThrow()
  })

  it('changes an operator and keeps its operands', () => {
    const tree = parseExpression({ '===': [{ var: 'a' }, 1] }, 'conditional')

    expect(JSON.stringify(serialise(changeOperatorAt(tree, [], '!==')))).toBe(
      '{"!==":[{"var":"a"},1]}',
    )
  })

  it('refuses an operator change that would drop an operand', () => {
    expect(() => changeOperatorAt(base(), [], '-')).toThrow()
  })

  it('pads with a hole when the new operator needs more operands', () => {
    const tree = parseExpression({ '!': [{ var: 'a' }] }, 'conditional')
    const changed = changeOperatorAt(tree, [], 'and')

    expect(isComplete(changed)).toBe(false)
    expect(nodeAt(changed, [0])).toEqual(expect.objectContaining({ kind: 'var', path: 'a' }))
  })

  it('applies each edit the view reports', () => {
    const tree = parseExpression({ and: [{ var: 'a' }, { var: 'b' }] }, 'conditional')

    const added = applyEdit(tree, { type: 'add', path: [] })
    const filled = applyEdit(added, { type: 'replace', path: [2], node: literalNode(true) })
    const changed = applyEdit(filled, { type: 'operator', path: [], op: 'or' })
    const removed = applyEdit(changed, { type: 'remove', path: [0] })

    expect(JSON.stringify(serialise(removed))).toBe('{"or":[{"var":"b"},true]}')
    // Each step left the tree before it alone.
    expect(serialise(tree)).toEqual({ and: [{ var: 'a' }, { var: 'b' }] })
  })

  it('keeps a slot’s key through a replacement, so the view keeps its place', () => {
    const tree = parseExpression({ '+': [{ var: 'a' }, 1] }, 'calculate')
    const replaced = replaceAt(tree, [1], literalNode(2))

    expect(nodeAt(replaced, [1]).key).toBe(nodeAt(tree, [1]).key)
    expect(nodeAt(replaced, [0])).toBe(nodeAt(tree, [0]))
  })

  it('offers a calculation arithmetic alone, and a conditional every family', () => {
    expect(familiesIn('calculate')).toEqual(['arithmetic'])
    expect(familiesIn('conditional')).toEqual(['comparison', 'arithmetic', 'logical', 'not'])
  })

  it('creates a new comparison as "is", ===', () => {
    expect(createNode('comparison')).toEqual(expect.objectContaining({ op: '===' }))
  })

  it('creates every operand kind unfilled except null, which has nothing to fill', () => {
    for (const kind of ['var', 'number', 'string', 'boolean', 'json'] as const) {
      expect(isComplete(createNode(kind))).toBe(false)
    }

    expect(serialise(createNode('null'))).toBe(null)
  })
})

// --- C17 ----------------------------------------------------------------------

describe('the pinned operator list (C17)', () => {
  it('is section A of #686, tier by tier, and nothing else', () => {
    expect([...CALCULATE_TIER_OPERATORS]).toEqual(['var', '+', '*', '-', '/', '%'])
    expect([...CONDITIONAL_TIER_OPERATORS]).toEqual([
      'var',
      '+',
      '*',
      '-',
      '/',
      '%',
      '==',
      '===',
      '!=',
      '!==',
      '>',
      '>=',
      '<',
      '<=',
      'and',
      'or',
      '!',
    ])
  })

  it('labels equality as section A says', () => {
    expect(OPERATOR_LABELS['===']).toBe('is')
    expect(OPERATOR_LABELS['!==']).toBe('is not')
    expect(OPERATOR_LABELS['==']).toBe('equals (loose)')
    expect(OPERATOR_LABELS['!=']).toBe('not equal (loose)')
  })
})

// --- C14 ----------------------------------------------------------------------

describe('evaluation agrees after a round trip (C14)', () => {
  let evaluator: RuleEvaluator

  beforeAll(async () => {
    evaluator = await loadEvaluator()
  })

  function outcome(expr: unknown, data: unknown): { ok: boolean; value?: unknown } {
    try {
      return { ok: true, value: evaluator.evaluate(expr, data) }
    } catch {
      return { ok: false }
    }
  }

  const SWEEP_DATA = {
    a: 3,
    b: 4,
    c: 10,
    d: 8,
    type: 'invoice',
    draft: false,
    items: [{ unit_price: 2.5 }],
  }

  const entries: [string, unknown, unknown][] = [
    ...REGISTRY_REPRESENTABLE.map((e): [string, unknown, unknown] => [e.id, e.expr, e.data]),
    ...JWSS_CONDITIONS.map((e): [string, unknown, unknown] => [e.id, e.expr, e.data]),
    ...PARITY_REPRESENTABLE_ENTRIES.map((e): [string, unknown, unknown] => [
      `parity ${e.id}`,
      e.expr,
      e.data,
    ]),
    ...representableSweep().map((e): [string, unknown, unknown] => [
      `sweep ${JSON.stringify(e)}`,
      e,
      SWEEP_DATA,
    ]),
  ]

  it.each(entries)('%s evaluates the same before and after', (_id, expr, data) => {
    const roundTripped = rebuild(parseExpression(expr, 'conditional'))

    expect(outcome(roundTripped, data)).toEqual(outcome(expr, data))
  })

  it('is a test that can tell two expressions apart', () => {
    // Without this, an evaluator that answered everything identically would
    // pass every case above.
    expect(outcome({ '-': [10, 3] }, {})).not.toEqual(outcome({ '-': [3, 10] }, {}))
  })
})

// --- The test-engineer campaign, 2026-10-09 ------------------------------------

/**
 * Shapes the builder's own suite did not reach, found by the independent
 * campaign on #686. Each names the section B or C rule it holds the seam to.
 */
describe('the seam at its edges (campaign, 2026-10-09)', () => {
  /** What the seam emits for `expr` when every node is rebuilt, as JSON text. */
  const rebuilt = (expr: unknown, tier: LogicTier = 'conditional') =>
    JSON.stringify(rebuild(parseExpression(expr, tier)))

  it.each([
    ['negative zero', -0],
    ['the largest double', Number.MAX_VALUE],
    ['the smallest subnormal', Number.MIN_VALUE],
    ['the largest safe integer', Number.MAX_SAFE_INTEGER],
    ['a number JavaScript writes with an exponent', 1e21],
    ['a small negative fraction', -1e-7],
  ])('B5: %s stays the same number through a rebuild', (_name, literal) => {
    const expr = { '+': [literal, { var: 'a' }] }
    const operand = (rebuild(parseExpression(expr, 'calculate')) as { '+': unknown[] })['+'][0]

    expect(Object.is(operand, literal)).toBe(true)
    expect(rebuilt(expr, 'calculate')).toBe(JSON.stringify(expr))
  })

  it('B5: a string that reads as a number, boolean or null stays a string', () => {
    for (const text of ['5', '-0', '1e3', '0x10', ' 5', 'true', 'false', 'null', 'NaN']) {
      const expr = { '===': [{ var: 'a' }, text] }

      expect(nodeAt(parseExpression(expr, 'conditional'), [1])).toEqual(
        expect.objectContaining({ kind: 'literal', value: text }),
      )
      expect(rebuilt(expr)).toBe(JSON.stringify(expr))
    }
  })

  it('B5: a number a host built that JSON cannot write is not rewritten by the seam', () => {
    for (const literal of [Number.NaN, Number.POSITIVE_INFINITY, Number.NEGATIVE_INFINITY]) {
      const tree = parseExpression({ '+': [literal, 1] }, 'calculate')
      const edited = replaceAt(tree, [1], literalNode(2))

      expect(Object.is((serialise(edited) as { '+': unknown[] })['+'][0], literal)).toBe(true)
    }
  })

  it('B4: {"var": "a"} is a variable and {"var": ["a"]} is held as it was written', () => {
    const wrapped = { var: ['a'] }
    const tree = parseExpression({ '+': [{ var: 'a' }, wrapped] }, 'calculate')

    expect(nodeAt(tree, [0])).toEqual(expect.objectContaining({ kind: 'var', path: 'a' }))
    expect(nodeAt(tree, [1])).toEqual(expect.objectContaining({ kind: 'opaque' }))
    // Not unwrapped into the bare form, which reads the same and is not what was written.
    expect((rebuild(tree) as { '+': unknown[] })['+'][1]).toBe(wrapped)
    expect(rebuilt(wrapped, 'calculate')).toBe('{"var":["a"]}')
  })

  it.each([
    ['__proto__', JSON.parse('{"__proto__": [1, 2]}') as Record<string, unknown>],
    ['constructor', { constructor: [1, 2] }],
    ['toString', { toString: [1, 2] }],
    ['hasOwnProperty', { hasOwnProperty: [1, 2] }],
    ['valueOf', { valueOf: [1, 2] }],
  ])('B3: an object keyed %s is an opaque leaf, held by reference', (key, operand) => {
    expect(Object.keys(operand)).toEqual([key])

    const expr = { and: [{ var: 'x' }, operand] }
    const tree = parseExpression(expr, 'conditional')

    expect(nodeAt(tree, [1]).kind).toBe('opaque')
    expect((rebuild(tree) as { and: unknown[] }).and[1]).toBe(operand)
    expect(rebuilt(expr)).toBe(JSON.stringify(expr))
    expect(isRootRepresentable(parseExpression(operand, 'conditional'))).toBe(false)
  })

  it('B4: a var path named like an Object member is kept verbatim and is not offered', () => {
    for (const name of ['__proto__', 'constructor', 'toString', 'hasOwnProperty']) {
      expect(parseExpression({ var: name }, 'calculate')).toEqual(
        expect.objectContaining({ kind: 'var', path: name }),
      )
      expect(isOfferedPath(name, [])).toBe(false)
      expect(isOfferedPath(name, [{ path: 'amount', label: 'Amount' }])).toBe(false)
    }
  })

  it.each([
    ['upper case', 'AND'],
    ['title case', 'Or'],
    ['an upper-case var', 'VAR'],
    ['a fullwidth plus', '＋'],
    ['a fullwidth exclamation mark', '！'],
    ['a trailing space', '+ '],
    ['a leading space', ' +'],
    ['a zero-width space', '​+'],
    ['a fullwidth equals sign', '=＝='],
    ['the greater-than-or-equal sign', '≥'],
    ['a trailing NUL', 'and\u0000'],
  ])('B3: an operator name with %s is opaque in both tiers', (_name, op) => {
    for (const tier of ['calculate', 'conditional'] as const) {
      const operand = { [op]: [1, 2] }
      const tree = parseExpression({ '+': [operand, 3] }, tier)

      expect(nodeAt(tree, [0])).toEqual(expect.objectContaining({ kind: 'opaque', value: operand }))
      expect(isRootRepresentable(parseExpression(operand, tier))).toBe(false)
    }
  })

  it('B6, B8: not holds an array or object operand opaque, in the form it was written', () => {
    for (const expr of [{ '!': [[1]] }, { '!': {} }, { '!': { a: 1, b: 2 } }, { '!': [{}] }]) {
      const tree = parseExpression(expr, 'conditional')

      expect(tree.kind).toBe('operator')
      expect(nodeAt(tree, [0]).kind).toBe('opaque')
      expect(rebuilt(expr)).toBe(JSON.stringify(expr))
    }
  })

  it('A: the seam refuses an operand past an operator’s maximum, whatever the view offers', () => {
    for (const expr of [{ '-': [{ var: 'a' }, 1] }, { '<': [1, 2] }, { '!': [true] }, { '!': 1 }]) {
      expect(() => addOperandAt(parseExpression(expr, 'conditional'), [])).toThrow(RangeError)
    }

    expect(
      isComplete(addOperandAt(parseExpression({ or: [true, false] }, 'conditional'), [])),
    ).toBe(false)
  })

  it('B8: changing not to not keeps the form it was written in', () => {
    for (const expr of [{ '!': { var: 'a' } }, { '!': [{ var: 'a' }] }]) {
      const changed = changeOperatorAt(parseExpression(expr, 'conditional'), [], '!')

      expect(JSON.stringify(serialise(changed))).toBe(JSON.stringify(expr))
    }
  })

  it('C1: a representable expression nested 500 deep round-trips', () => {
    let expr: unknown = { var: 'a' }

    for (let depth = 0; depth < 500; depth += 1) {
      expr = [{ '!': expr }, { and: [expr, true] }, { '+': [expr, 1] }][depth % 3]
    }

    // Visual down to MAX_VISUAL_DEPTH and one opaque leaf below it, which is
    // still exact: the leaf is emitted by reference. (Until the cap, this
    // asserted the whole tree visual; Vue cannot render it past ~300 deep.)
    expect(containsOpaque(parseExpression(expr, 'conditional'))).toBe(true)
    expect(rebuilt(expr)).toBe(JSON.stringify(expr))
  })

  describe('the depth cap', () => {
    /** `{"!": …}` wrapped `levels` times round `{"var": "a"}`; the var sits at depth `levels`. */
    function notChain(levels: number): { expr: unknown; leaf: unknown } {
      const leaf = { var: 'a' }
      let expr: unknown = leaf

      for (let depth = 0; depth < levels; depth += 1) {
        expr = { '!': expr }
      }

      return { expr, leaf }
    }

    it('shows a node at MAX_VISUAL_DEPTH', () => {
      const { expr } = notChain(MAX_VISUAL_DEPTH)
      const tree = parseExpression(expr, 'conditional')

      expect(containsOpaque(tree)).toBe(false)
      expect(nodeAt(tree, Array(MAX_VISUAL_DEPTH).fill(0)).kind).toBe('var')
      expect(rebuilt(expr)).toBe(JSON.stringify(expr))
    })

    it('holds a node one deeper as an opaque leaf, by reference', () => {
      const { expr, leaf } = notChain(MAX_VISUAL_DEPTH + 1)
      const tree = parseExpression(expr, 'conditional')
      const below = nodeAt(tree, Array(MAX_VISUAL_DEPTH + 1).fill(0))

      expect(nodeAt(tree, Array(MAX_VISUAL_DEPTH).fill(0)).kind).toBe('operator')
      expect(below).toEqual(expect.objectContaining({ kind: 'opaque' }))
      expect((below as { value: unknown }).value).toBe(leaf)
      expect(serialise(tree)).toBe(expr)
      expect(rebuilt(expr)).toBe(JSON.stringify(expr))
    })

    it('keeps a literal past the cap a literal: it has nothing below it', () => {
      let expr: unknown = 5

      for (let depth = 0; depth <= MAX_VISUAL_DEPTH; depth += 1) {
        expr = { '!': expr }
      }

      expect(containsOpaque(parseExpression(expr, 'conditional'))).toBe(false)
    })
  })

  it(// Was `it.fails`, the campaign's defect: the doc comment promised "never
  // throws", and a value deep enough overflowed the recursive parse, so the
  // builder threw while mounting. Fixed by MAX_VISUAL_DEPTH.
  'never throws while parsing, even an expression nested 100,000 deep', () => {
    let expr: unknown = { var: 'a' }

    for (let depth = 0; depth < 100_000; depth += 1) {
      expr = { '!': expr }
    }

    expect(() => parseExpression(expr, 'conditional')).not.toThrow()
    expect(serialise(parseExpression(expr, 'conditional'))).toBe(expr)
  })

  describe('C2, C5: an opaque subtree survives an edit by reference, at depth', () => {
    const cat = { cat: ['INV-', { var: 'n' }], z: 0, a: 1 }
    const deepNot = { '!': [cat] }
    const expr = {
      and: [{ or: [deepNot, { var: 'draft' }] }, { '>': [{ var: 'total' }, 0] }, { var: 'ok' }],
    }

    type Emitted = { and: [{ or: [{ '!': unknown[] }] }] }

    it.each([
      ['a sibling of its ancestor', [1, 1]],
      ['its parent’s sibling', [0, 1]],
      ['the root’s last operand', [2]],
    ])('when the edit is at %s', (_where, path) => {
      const edited = replaceAt(parseExpression(expr, 'conditional'), path, literalNode(7))
      const emitted = serialise(edited) as Emitted

      expect(emitted.and[0].or[0]['!'][0]).toBe(cat)
      expect(JSON.stringify(emitted.and[0].or[0])).toBe(
        '{"!":[{"cat":["INV-",{"var":"n"}],"z":0,"a":1}]}',
      )
    })

    it('when the edit rebuilds every ancestor of the opaque leaf', () => {
      const tree = parseExpression(expr, 'conditional')
      const edited = changeOperatorAt(replaceAt(tree, [0, 1], literalNode(true)), [0], 'and')
      type Changed = { and: [{ and: [{ '!': unknown[] }] }] }

      // The untouched not is its original; rebuilt, it still holds the leaf itself.
      expect((serialise(edited) as Changed).and[0].and[0]).toBe(deepNot)
      expect((rebuild(edited) as Changed).and[0].and[0]['!'][0]).toBe(cat)
    })
  })
})
