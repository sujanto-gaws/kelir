import { beforeAll, describe, expect, it } from 'vitest'

import {
  EvaluationError,
  ENGINE_VERSION,
  loadEvaluator,
  normalizeNumeric,
  sumOperator,
  type RuleEvaluator,
} from './jsonlogic'

/**
 * The evaluator, and the parts of it that are ours rather than the engine's.
 *
 * `normalizeNumeric` and `sumOperator` are the two pieces of behaviour this
 * repository writes on both sides, so they are tested here case for case
 * against the backend's tests of the same two. Everything else is the engine,
 * and `jsonlogic.parity.spec.ts` is where the engine is held to the corpus.
 */

describe('normalizeNumeric', () => {
  it('keeps a finite number', () => {
    expect(normalizeNumeric(42.5)).toBe(42.5)
    expect(normalizeNumeric(0)).toBe(0)
    expect(normalizeNumeric(-3)).toBe(-3)
  })

  it('coerces a numeric string, which is what a form field carries', () => {
    expect(normalizeNumeric('42.5')).toBe(42.5)
    expect(normalizeNumeric('  7 ')).toBe(7)
  })

  it('is 0 for everything that is not a finite number', () => {
    // The §3.1 rule, and the reason it is a finiteness test rather than the
    // `Number(result) || 0` the registry wrote: `Infinity` is truthy.
    expect(normalizeNumeric(Number.POSITIVE_INFINITY)).toBe(0)
    expect(normalizeNumeric(Number.NEGATIVE_INFINITY)).toBe(0)
    expect(normalizeNumeric(Number.NaN)).toBe(0)
    expect(normalizeNumeric(null)).toBe(0)
    expect(normalizeNumeric(undefined)).toBe(0)
    expect(normalizeNumeric('')).toBe(0)
    expect(normalizeNumeric('not a number')).toBe(0)
    expect(normalizeNumeric([1, 2])).toBe(0)
    expect(normalizeNumeric({ a: 1 })).toBe(0)
  })

  it('is 1 for true and 0 for false', () => {
    expect(normalizeNumeric(true)).toBe(1)
    expect(normalizeNumeric(false)).toBe(0)
  })
})

describe('sumOperator', () => {
  it('sums the numbers in its single array argument', () => {
    expect(sumOperator('[[1, 2, 3.5]]')).toBe('6.5')
  })

  it('sums an empty array to 0', () => {
    expect(sumOperator('[[]]')).toBe('0')
  })

  it('sums only the numeric members', () => {
    // A total of what is there, not NaN because one row was blank.
    expect(sumOperator('[[1, null, "x", 2]]')).toBe('3')
  })

  it('sums a non-array argument to 0', () => {
    expect(sumOperator('[42]')).toBe('0')
  })
})

describe('sum comes from one place', () => {
  /**
   * Calculation Rule Registry §4.3 forbids reimplementing a standard operator,
   * and #163 AC2 requires the `sum` #154 registers rather than a local one.
   *
   * **The property is about the whole source tree and not about this module**,
   * so the test discovers its subjects rather than listing them — the [Sprint 6
   * retrospective](../../../projects/retrospectives/04.%20Sprint%206%20Retrospective.md)'s
   * eighth action. A field component added next sprint that quietly totals its
   * own rows fails here, which is the cost the construction plan §5.4 priced at
   * one grep.
   */
  // Paths come back relative to this spec, which sits beside the module the
  // property is about — so `src/lib/jsonlogic.ts` is `./jsonlogic.ts` here.
  const EVALUATOR = './jsonlogic.ts'

  const sources = Object.entries(
    import.meta.glob<string>('../**/*.{ts,vue}', {
      query: '?raw',
      import: 'default',
      eager: true,
    }),
    // Specs are excluded, and this one is why: a test that names the thing it
    // forbids would otherwise be the first thing it caught.
  ).filter(([path]) => !path.endsWith('.spec.ts'))

  /** How the engine is told about an operator, in either of its two spellings. */
  const REGISTRATION = /customOperators|addOperator/

  it('found a source tree to look at', () => {
    // Without this, the assertion below passes over an empty set — a green test
    // proving that nothing was looked at.
    expect(sources.length).toBeGreaterThan(20)
  })

  it('registers an operator in this module and nowhere else', () => {
    const elsewhere = sources
      .filter(([path]) => path !== EVALUATOR)
      .filter(([, source]) => REGISTRATION.test(source))
      .map(([path]) => path)

    expect(
      elsewhere,
      `operator registration outside lib/jsonlogic.ts: ${elsewhere.join(', ')}`,
    ).toEqual([])
  })

  it('is the module that registers one, so the assertion above is not vacuous', () => {
    const evaluator = sources.find(([path]) => path === EVALUATOR)

    expect(evaluator, 'the evaluator module was not in the glob').toBeDefined()
    expect(REGISTRATION.test(evaluator![1])).toBe(true)
  })
})

describe('the evaluator', () => {
  let evaluator: RuleEvaluator

  beforeAll(async () => {
    evaluator = await loadEvaluator()
  })

  it('pins the version the backend pins', () => {
    expect(ENGINE_VERSION).toBe('5.2.0')
  })

  it('evaluates the registry line total', () => {
    expect(
      evaluator.evaluateNumeric(
        { '*': [{ var: 'unit_price' }, { var: 'quantity' }] },
        {
          unit_price: 12.5,
          quantity: 3,
        },
      ),
    ).toBe(37.5)
  })

  it('evaluates the registry §6.1 invoice total', () => {
    const expression = {
      sum: [{ map: [{ var: 'items' }, { '*': [{ var: 'unit_price' }, { var: 'quantity' }] }] }],
    }
    const data = {
      items: [
        { unit_price: 10, quantity: 2 },
        { unit_price: 11, quantity: 2 },
      ],
    }

    expect(evaluator.evaluateNumeric(expression, data)).toBe(42)
  })

  it('refuses an unknown operator instead of quietly returning zero', () => {
    // The defect that disqualified `jsonlogic-rs`, asserted against the engine
    // that replaced it. Mistype `sum` and an engine that returns unknown
    // operators unevaluated hands back the expression object, which the
    // wrapper then turns into 0 — a 42-rupiah line persisted as free with
    // nothing logged. The assertion is not that some error occurs; it is that
    // 42 does not silently become 0.
    const mistyped = {
      summ: [{ map: [{ var: 'items' }, { '*': [{ var: 'unit_price' }, { var: 'quantity' }] }] }],
    }
    const data = {
      items: [
        { unit_price: 10, quantity: 2 },
        { unit_price: 11, quantity: 2 },
      ],
    }

    expect(() => evaluator.evaluate(mistyped, data)).toThrow(EvaluationError)
    expect(normalizeNumeric({ summ: [1, 2] })).toBe(0)
  })

  it('treats a misspelled variable as absent rather than as an error', () => {
    // Distinct from an unknown operator on purpose: a half-filled form is
    // normal, and refusing to evaluate it would make every draft an error.
    expect(
      evaluator.evaluateNumeric(
        { '*': [{ var: 'unit_price' }, { var: 'quantitee' }] },
        {
          unit_price: 12.5,
          quantity: 3,
        },
      ),
    ).toBe(0)
  })

  it('refuses a fractional division by zero like every other', () => {
    // The case that used to be the odd one out. Under `return_null` this
    // returned `null` and the wrapper turned it into the 0 §3.1 then asked for,
    // while `10 / 0` beside it threw — the same expression answering two ways
    // on the numerator's type. **D-24** removed the split.
    expect(() => evaluator.evaluate({ '/': [10.5, 0] }, {})).toThrow(EvaluationError)
  })

  it('never yields a non-finite number', () => {
    // An overflow returns null rather than Infinity, so nothing non-finite
    // reaches a numeric field even before the wrapper.
    expect(evaluator.evaluateNumeric({ '*': [1e308, 10] }, {})).toBe(0)
  })

  it('refuses every integer division by zero', () => {
    // Registry §3.1 at v1.6.0. The name no longer says "rather than
    // normalizing" because there is nothing left to normalize against: **D-24**
    // closed the split by requiring all four to refuse rather than the `0` that
    // was never reachable by configuration. The backend refuses the same four.
    for (const expression of [{ '/': [10, 0] }, { '/': [0, 0] }, { '%': [10, 0] }]) {
      expect(() => evaluator.evaluate(expression, {}), JSON.stringify(expression)).toThrow(
        EvaluationError,
      )
    }
  })

  it('refuses a cap with an absent operand rather than zeroing it', () => {
    // §6.2's discount cap. The reference implementation returns 0, which
    // satisfies "the result is 0" and silently turns the cap into no discount
    // at all; refusing is the better of the two failures.
    expect(() => evaluator.evaluate({ min: [{ var: 'computed' }, 100] }, { other: 1 })).toThrow(
      EvaluationError,
    )
  })

  it('evaluates a conditional expression as a boolean', () => {
    expect(evaluator.evaluate({ '>': [{ var: 'total' }, 1000] }, { total: 1500 })).toBe(true)
  })
})
