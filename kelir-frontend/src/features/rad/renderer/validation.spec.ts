import { describe, expect, it } from 'vitest'

import {
  UnknownValidationRuleError,
  VALIDATION_RULES,
  validateField,
  type FieldViolation,
} from './validation'
import type { JfssDataComponent, JfssValidation } from '@/types/jfss'

/**
 * The rule catalogue, case for case against the two documents that define it
 * (#163 AC1, AC3).
 *
 * **Pure, so every case is a case rather than a rendered form.** The composable
 * that runs these over a live payload has its own spec; what is asserted here
 * is what JFSS §5 and the [Validation Rule
 * Registry](../../../../../docs/schema/JFSS%20Validation%20Rule%20Registry.md)
 * §3 say a value is worth, which is the part a browser is a poor place to read.
 *
 * **The registry's own §3.3 rules are covered here and not in the fixture**,
 * deliberately. A `unique` rule in the shared purchase-requisition definition
 * would oblige #164 to implement a real uniqueness check against a real table
 * before that fixture could be submitted at all, which is scope this issue has
 * no business creating. The behaviour that matters — the frontend does not
 * decide a `server`-scoped rule, and does not report it as passed — is a
 * property of this function and is asserted directly.
 */

function field(validation: JfssValidation, extra: Partial<JfssDataComponent> = {}) {
  return {
    id: 'f',
    role: 'data',
    type: 'textfield',
    key: 'f',
    label: 'F',
    validation,
    ...extra,
  } as JfssDataComponent
}

/** The verdict alone, which is what nearly every case below is about. */
function verdict(
  component: JfssDataComponent,
  value: unknown,
  scope: Record<string, unknown> = {},
): FieldViolation | undefined {
  return validateField(component, value, scope).violation
}

describe('JFSS §5, the basic validation keywords', () => {
  it('refuses an empty required field and accepts an empty optional one', () => {
    expect(verdict(field({ type: 'string', required: true }), '')?.rule).toBe('required')
    expect(verdict(field({ type: 'string' }), '')).toBeUndefined()
  })

  it('treats null, an empty string and an empty array as empty and false as a value', () => {
    const required = field({ type: 'boolean', required: true })

    expect(verdict(field({ type: 'string', required: true }), null)?.rule).toBe('required')
    expect(verdict(field({ type: 'array', required: true }), [])?.rule).toBe('required')

    // The parity-preserving reading, and the one JFSS §5 supports: `false` is
    // present and non-empty, and the backend's `serde_json` agrees. A client
    // that called it empty would refuse submissions the server accepts.
    expect(verdict(required, false)).toBeUndefined()
  })

  it('gives a tick-box that must be ticked the mechanism that already exists', () => {
    // `enum: [true]` says "must be ticked" without giving `required` a second
    // meaning that only one of the two runtimes would know about.
    const consent = field({ type: 'boolean', enum: [true] })

    expect(verdict(consent, false)?.rule).toBe('enum')
    expect(verdict(consent, true)).toBeUndefined()
  })

  it('checks the declared type of a present value', () => {
    expect(verdict(field({ type: 'number' }), '12')?.rule).toBe('type')
    expect(verdict(field({ type: 'integer' }), 1.5)?.rule).toBe('type')
    expect(verdict(field({ type: 'integer' }), 2)).toBeUndefined()
    expect(verdict(field({ type: 'array' }), { a: 1 })?.rule).toBe('type')
  })

  it('applies the string keywords', () => {
    expect(verdict(field({ type: 'string', minLength: 3 }), 'ab')?.rule).toBe('minLength')
    expect(verdict(field({ type: 'string', maxLength: 3 }), 'abcd')?.rule).toBe('maxLength')
    expect(verdict(field({ type: 'string', pattern: '^[A-Z]+$' }), 'abc')?.rule).toBe('pattern')
    expect(verdict(field({ type: 'string', pattern: '^[A-Z]+$' }), 'ABC')).toBeUndefined()
  })

  it('applies the numeric bounds', () => {
    expect(verdict(field({ type: 'number', minimum: 1 }), 0)?.rule).toBe('minimum')
    expect(verdict(field({ type: 'number', maximum: 10 }), 11)?.rule).toBe('maximum')
    expect(verdict(field({ type: 'number', minimum: 0 }), 0)).toBeUndefined()
  })

  it('decides every format §5 lists', () => {
    const cases: [JfssValidation['format'], string, string][] = [
      ['email', 'someone@example.com', 'someone@'],
      ['uri', 'https://example.com/a', 'not a uri'],
      ['date', '2026-08-26', '2026-08-32'],
      ['time', '09:30', '9:30'],
      ['date-time', '2026-08-26T09:30:00Z', 'yesterday'],
      ['uuid', '3f2504e0-4f89-41d3-9a0c-0305e82c3301', '3f2504e0'],
    ]

    for (const [format, good, bad] of cases) {
      expect(
        verdict(field({ type: 'string', format }), good),
        `${format} accepts ${good}`,
      ).toBeUndefined()
      expect(
        verdict(field({ type: 'string', format }), bad)?.rule,
        `${format} refuses ${bad}`,
      ).toBe('format')
    }
  })

  it('refuses a date that matches the shape and is not a day', () => {
    // `2026-02-31` passes every pattern anyone writes for a date.
    expect(verdict(field({ type: 'string', format: 'date' }), '2026-02-31')?.rule).toBe('format')
  })

  it('pins digit classes to [0-9] rather than \\d', () => {
    // The Validation Rule Registry's interim guidance under the `regex`
    // warning. ECMA-262 reads `\d` as ASCII and the Rust crate reads it as
    // Unicode `Nd`, so a date format written with `\d` would accept
    // Arabic-Indic digits here and the two sides would disagree with nothing
    // raised on either. This asserts the pin rather than the intention.
    expect(verdict(field({ type: 'string', format: 'date' }), '٢٠٢٦-٠٨-٢٦')?.rule).toBe('format')
  })

  it('applies enum, uniqueItems and uniqueBy', () => {
    expect(verdict(field({ type: 'string', enum: ['A', 'B'] }), 'C')?.rule).toBe('enum')
    expect(verdict(field({ type: 'array', uniqueItems: true }), [1, 2, 1])?.rule).toBe(
      'uniqueItems',
    )
    expect(
      verdict(field({ type: 'array', uniqueBy: 'sku' }), [{ sku: 'A' }, { sku: 'A' }])?.rule,
    ).toBe('uniqueBy')
  })

  it('reads uniqueBy with several keys as one combination', () => {
    // §5 allows a string or an array, and the meta-schema's `oneOf` is what
    // makes the array form storable. Two keys mean the *pair* is unique — each
    // column repeating on its own is fine.
    const component = field({ type: 'array', uniqueBy: ['sku', 'warehouse'] })

    expect(
      verdict(component, [
        { sku: 'A', warehouse: 'N' },
        { sku: 'A', warehouse: 'S' },
      ]),
    ).toBeUndefined()

    expect(
      verdict(component, [
        { sku: 'A', warehouse: 'N' },
        { sku: 'A', warehouse: 'N' },
      ])?.rule,
    ).toBe('uniqueBy')
  })

  it('skips every other keyword for an empty optional value', () => {
    // JSON Schema's own semantics, and what keeps an optional field optional: a
    // blank date is not a malformed date.
    expect(verdict(field({ type: 'string', format: 'date', minLength: 4 }), '')).toBeUndefined()
  })

  it('reports one violation per field, in the order a person would want them', () => {
    // "too short" about an empty box says the same thing as "required" and
    // answers the wrong question.
    expect(verdict(field({ type: 'string', required: true, minLength: 3 }), '')?.rule).toBe(
      'required',
    )
  })

  it('takes the message the definition supplies over its own', () => {
    const supplied = field({
      type: 'string',
      required: true,
      messages: { required: 'Every request needs a title.' },
    })

    expect(verdict(supplied, '')?.message).toBe('Every request needs a title.')
    // And a keyword the definition says nothing about still says something.
    expect(verdict(field({ type: 'string', required: true }), '')?.message).not.toBe('')
  })
})

describe('the Validation Rule Registry §3, the advanced rules', () => {
  const withRule = (rule: string, scope: 'client' | 'server' | 'both', params: object = {}) =>
    field(
      { type: 'string' },
      { rules: [{ rule, scope, params: params as Record<string, unknown>, message: 'No.' }] },
    )

  it('decides matchesField and notMatchesField against the current scope', () => {
    const matches = withRule('matchesField', 'both', { target: 'password' })

    expect(verdict(matches, 'secret', { password: 'secret' })).toBeUndefined()
    expect(verdict(matches, 'secret', { password: 'other' })?.rule).toBe('matchesField')

    const differs = withRule('notMatchesField', 'both', { target: 'old_password' })

    expect(verdict(differs, 'new', { old_password: 'old' })).toBeUndefined()
    expect(verdict(differs, 'old', { old_password: 'old' })?.rule).toBe('notMatchesField')
  })

  it('decides regex, and treats a pattern it cannot compile as unmet', () => {
    const cost = withRule('regex', 'both', { pattern: '^[A-Z]{2}-[0-9]{4}$' })

    expect(verdict(cost, 'FN-0142')).toBeUndefined()
    expect(verdict(cost, 'fn-142')?.rule).toBe('regex')

    // A rule that could not be applied has not been satisfied.
    expect(verdict(withRule('regex', 'both', { pattern: '([' }), 'anything')?.rule).toBe('regex')
  })

  it('decides oneOf and notOneOf', () => {
    expect(verdict(withRule('oneOf', 'both', { values: ['a', 'b'] }), 'c')?.rule).toBe('oneOf')
    expect(verdict(withRule('oneOf', 'both', { values: ['a', 'b'] }), 'a')).toBeUndefined()
    expect(verdict(withRule('notOneOf', 'both', { values: ['admin'] }), 'admin')?.rule).toBe(
      'notOneOf',
    )
  })

  it('shows the message the rule carries, which §6.2 makes required', () => {
    expect(verdict(withRule('oneOf', 'both', { values: ['a'] }), 'z')?.message).toBe('No.')
  })

  it('leaves a server-scoped rule to the server, and does not report it as passed', () => {
    // §3.3: the frontend shows the error only after the backend refuses the
    // submission. Not a violation here, and not silence either — the rule is
    // returned as undecided, which is what lets the form say so.
    const outcome = validateField(withRule('unique', 'server', { table: 'users' }), 'taken', {})

    expect(outcome.violation).toBeUndefined()
    expect(outcome.undecided).toEqual([])
  })

  it('names a client-scoped rule it cannot decide rather than skipping it', () => {
    // Registry §3.2's `passwordStrength` wants a scoring library Kelir does not
    // carry. Adding one is a decision; reporting the check as passed is a
    // defect, and this is the difference.
    const outcome = validateField(withRule('passwordStrength', 'client', { minScore: 3 }), 'x', {})

    expect(outcome.violation).toBeUndefined()
    expect(outcome.undecided.map((entry) => entry.rule)).toEqual(['passwordStrength'])
    expect(outcome.undecided[0].reason).not.toBe('')
  })

  it('skips the verdict of an advanced rule on an empty optional field', () => {
    // The same JSON Schema semantics §5's keywords get. An unfilled cost centre
    // is not a malformed one.
    expect(verdict(withRule('oneOf', 'both', { values: ['a'] }), '')).toBeUndefined()
  })
})

describe('a rule name nobody defines', () => {
  /**
   * #163 AC3, and the one assertion in this file that is about a defect in the
   * *definition* rather than in what somebody typed.
   *
   * The operator-parity spike found the previously-named crate returning
   * unknown operators instead of rejecting them; the reference implementation
   * has the same shape one layer up, where an unknown server rule returns
   * `true` and is commented as failing open. A rule that fails open is a rule
   * that is not there, reported as a rule that passed.
   */
  const mistyped = field(
    { type: 'string' },
    {
      rules: [
        { rule: 'matchesFeild', scope: 'both', params: { target: 'password' }, message: 'No.' },
      ],
    },
  )

  it('raises rather than skipping the branch', () => {
    expect(() => validateField(mistyped, 'x', { password: 'x' })).toThrow(
      UnknownValidationRuleError,
    )
  })

  it('names the rule, so the person fixing the definition knows which', () => {
    expect(() => validateField(mistyped, 'x', {})).toThrow(/matchesFeild/)
  })

  it('raises even when the field is empty and nothing else would have run', () => {
    // Otherwise a defect in the definition hides behind whatever the user has
    // not typed yet, and appears the day somebody fills that box in.
    expect(() => validateField(mistyped, '', {})).toThrow(UnknownValidationRuleError)
  })
})

describe('the catalogue itself', () => {
  it('carries every rule the registry defines and nothing else', () => {
    // Listed rather than discovered, because the registry is prose: there is no
    // machine-readable form of it to walk. What this catches is the catalogue
    // drifting from the document — a rule added to one and not the other.
    expect(Object.keys(VALIDATION_RULES).sort()).toEqual(
      [
        'async',
        'authorized',
        'exists',
        'matchesField',
        'notMatchesField',
        'notOneOf',
        'oneOf',
        'passwordStrength',
        'regex',
        'unique',
      ].sort(),
    )
  })

  it('gives every entry either a decision or a stated reason, never neither and never both', () => {
    for (const [name, entry] of Object.entries(VALIDATION_RULES)) {
      const decides = entry.decide !== undefined

      expect(decides || Boolean(entry.undecidable), `"${name}" does neither`).toBe(true)
      expect(decides && Boolean(entry.undecidable), `"${name}" does both`).toBe(false)
    }
  })

  it('decides exactly the §3.1 rules, which are the ones scoped both', () => {
    const decided = Object.entries(VALIDATION_RULES)
      .filter(([, entry]) => entry.decide !== undefined)
      .map(([name]) => name)

    expect(decided.sort()).toEqual(
      ['matchesField', 'notMatchesField', 'notOneOf', 'oneOf', 'regex'].sort(),
    )
  })
})
