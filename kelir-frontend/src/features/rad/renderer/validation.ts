import type { JfssAdvancedRule, JfssDataComponent, JfssScope, JfssValidation } from '@/types/jfss'

/**
 * What a definition's `validation` and `rules` decide about one value (#163).
 *
 * **Pure, and deliberately so.** Nothing here touches Vue, the evaluator, or
 * the DOM: a value and the scope its `key` addresses go in, a violation or
 * nothing comes out. [`useFormEvaluation`](./useFormEvaluation.ts) is what runs
 * it over a live form, and keeping the two apart is what lets every rule below
 * be tested case for case the way the two registries state them.
 *
 * **This is the rule catalogue and not the rule engine.** The dependency graph,
 * cycle detection and error-code mapping are FR-RAD-006 in Sprints 14–16 under
 * decision **D-2**. What is here is the catalogue's *membership* question — is
 * this a rule the [Validation Rule Registry](../../../../../docs/schema/JFSS%20Validation%20Rule%20Registry.md)
 * defines, and what does it decide — which #163 cannot avoid answering because
 * a form has to do something when it meets one.
 *
 * **The three tiers below are three different answers, and the difference is
 * the point** (#163 AC3, construction plan §5.3):
 *
 * 1. A rule this side **decides** — the five scoped `both` in registry §3.1.
 * 2. A rule this side **does not decide, and says which and why** — the two
 *    scoped `server`-only, plus the two scoped `client` whose implementation
 *    notes name a dependency Kelir does not carry. Declared in one place, the
 *    way [`registry.ts`](./registry.ts) declares an unrendered component type.
 * 3. A rule **nobody has heard of** — an error, never a skipped branch. The
 *    reference implementation returns `true` here, commented as failing open,
 *    and *a rule that fails open is a rule that is not there, reported as a
 *    rule that passed*.
 *
 * The second tier is decision **D-26**; the first and third are §5.3's.
 *
 * **JFSS §9.1 says "build dynamic Zod/Yup schemas" and this does not**, which
 * is a stated divergence rather than an omission. §9 is *Cross-Language
 * Implementation Guidelines* and neither library is a dependency of
 * `kelir-frontend`; adding one is a decision rather than an import, which is
 * the position the [construction plan](../../../../../projects/planning/03.%20Sprint%208%20Form%20Renderer%20Construction%20Plan.md)
 * §2 takes. What a schema builder would buy is composition over a *static*
 * shape, and JFSS validation is neither static nor composed: it is a flat list
 * of keywords per field plus a registry lookup, which is what is below.
 */

/** Why a field is not acceptable, in the shape JFSS S10.3 names its parts. */
export interface FieldViolation {
  /**
   * The keyword or rule name that decided it.
   *
   * Named `rule` because S10.3's error envelope names it `rule`, and #164 has
   * to put a client-side refusal and a server-side one beside each other.
   */
  rule: string
  /** What the person filling in the form is shown. */
  message: string
}

/**
 * Thrown when a definition names a validation rule the registry does not define.
 *
 * **A defect in the definition, and loud** (#163 AC3). The backend does not
 * catch this at save — `domain/jfss.rs` checks the meta-schema, the approved
 * operator set and the lookup allow-list, and a rule *name* is none of the
 * three — so the renderer is the first thing to meet it, and swallowing it
 * would leave a `both`-scoped check reported as passing on a form where it
 * never ran.
 *
 * Distinct from a *calculation* that fails, which is the ordinary state of a
 * form nobody has filled in yet and renders blank (construction plan §5.6).
 */
export class UnknownValidationRuleError extends Error {
  constructor(readonly ruleName: string) {
    super(
      `"${ruleName}" is not a rule in the JFSS Validation Rule Registry. ` +
        'Adding one is registry §4, not a branch in the renderer.',
    )
    this.name = 'UnknownValidationRuleError'
  }
}

/** What a registry rule is handed to decide a value. */
export interface RuleContext {
  /** The value under test. */
  value: unknown
  /**
   * The scope the component's `key` addresses.
   *
   * The form payload at the top level and a **row object** inside a datagrid,
   * which is what makes `matchesField` mean the right thing in both: a rule
   * inside a row template targets a sibling column of the same row, not a
   * top-level field that happens to share its name (JFSS §4.3.1).
   */
  scope: Record<string, unknown>
  /** The rule's `params`, which the meta-schema makes required. */
  params: Record<string, unknown>
}

/** One entry of the Validation Rule Registry, as this side sees it. */
interface RegistryRule {
  /**
   * The scope **the registry** gives the rule (§3.1–§3.3).
   *
   * Not the scope the definition declares. The two are different questions: a
   * definition's `scope` says where the author wants the rule run, and this
   * says where it *can* be. A definition pairing `unique` with `scope: "both"`
   * is stored happily by the backend and cannot be decided in a browser
   * whatever it declares, which is what makes this the field that answers.
   */
  scope: JfssScope
  /** Decides the rule here. Absent exactly when `undecidable` is present. */
  decide?: (context: RuleContext) => boolean
  /** Why this side does not decide it. Absent exactly when `decide` is. */
  undecidable?: string
}

/** The values a rule compares against, when its params carry a list. */
function paramValues(params: Record<string, unknown>): unknown[] {
  return Array.isArray(params.values) ? params.values : []
}

/**
 * Every rule name the Validation Rule Registry defines. **This list is the
 * catalogue**, and a name outside it is [`UnknownValidationRuleError`].
 *
 * Registry §4 is the procedure for adding one, and its step 4 says what the
 * default branch below does: *"An unrecognised rule name MUST be an error, not
 * a skipped arm"*. It says that of the Rust evaluator; the same sentence is
 * true of a `switch` in a browser, and the operator-parity spike is why —
 * `jsonlogic-rs` returned unknown operators rather than rejecting them, and
 * that is the same defect one layer down.
 */
export const VALIDATION_RULES: Readonly<Record<string, RegistryRule>> = {
  // --- §3.1, scope `both`: shared data integrity ---------------------------
  //
  // Decided here for the immediate feedback the scope is for, and re-decided by
  // the server at #164. Neither side is trusting the other.

  matchesField: {
    scope: 'both',
    decide: ({ value, scope, params }) => value === scope[String(params.target)],
  },

  notMatchesField: {
    scope: 'both',
    decide: ({ value, scope, params }) => value !== scope[String(params.target)],
  },

  /**
   * §3.1's `regex`, **and the registry's own warning applies to every use of
   * it.**
   *
   * `new RegExp` is exactly what the registry's Vue note prescribes, so this
   * side is conformant. The divergence is on the other one: the Rust `regex`
   * crate refuses lookahead and backreferences at compile time and reads `\d`
   * as Unicode `Nd` where ECMA-262 reads it as ASCII. For a `both`-scoped rule
   * that means the two sides can reach opposite verdicts on the same input,
   * with nothing raised on either. The registry's interim guidance is to pin
   * digit classes explicitly, and resolving it properly is **D-15**.
   *
   * An uncompilable pattern is a violation rather than a pass: a rule that
   * cannot be applied has not been satisfied.
   */
  regex: {
    scope: 'both',
    decide: ({ value, params }) => {
      if (value == null || value === '') {
        return true
      }

      try {
        return new RegExp(String(params.pattern), String(params.flags ?? '')).test(String(value))
      } catch {
        return false
      }
    },
  },

  oneOf: {
    scope: 'both',
    decide: ({ value, params }) => paramValues(params).includes(value),
  },

  notOneOf: {
    scope: 'both',
    decide: ({ value, params }) => !paramValues(params).includes(value),
  },

  // --- §3.2, scope `client`: UX enhancements this side does not have -------
  //
  // Declared rather than skipped. Both are UX-only by the registry's own
  // wording — the backend ignores them and relies on `validation.minLength`
  // and `validation.pattern` for password security — so not deciding them
  // lets no data through that the server would have refused. What it would
  // let through is the *impression* that a check ran, which is what naming
  // them prevents.

  passwordStrength: {
    scope: 'client',
    undecidable:
      'a strength score needs a scoring library (the registry names zxcvbn), which is a ' +
      'dependency decision rather than an import — and a hand-rolled scorer would give ' +
      '`minScore: 3` a different meaning on each side',
  },

  async: {
    scope: 'client',
    undecidable:
      'the registry requires the endpoint to be allow-listed in the API client and served as ' +
      'read-only and rate-limited; no such endpoint exists yet',
  },

  // --- §3.3, scope `server`: security and business logic -------------------
  //
  // Not a gap. §3.3 says it in as many words: these are evaluated exclusively
  // by the backend, and *"the frontend will only display the error message if
  // the backend returns a 400 Bad Request upon form submission"*. Deciding a
  // `unique` in a browser would mean a database query from a browser.

  unique: {
    scope: 'server',
    undecidable: 'a uniqueness check is a database query, and §3.3 scopes it to the server',
  },

  exists: {
    scope: 'server',
    undecidable: 'a foreign-key check is a database query, and §3.3 scopes it to the server',
  },

  authorized: {
    scope: 'server',
    undecidable:
      "a permission check reads the caller's authenticated claims, which a browser cannot be " +
      'trusted to report about itself',
  },
}

/** A rule the registry defines that this side does not decide, and why. */
export interface UndecidedRule {
  rule: string
  /** The scope the registry gives it, which is why the reason reads as it does. */
  scope: JfssScope
  reason: string
}

/**
 * Decides one advanced rule, or says that this side does not.
 *
 * `undefined` means the rule passed. An [`UndecidedRule`] means nobody decided
 * it here and the caller may surface that; a [`FieldViolation`] means it failed.
 *
 * @throws UnknownValidationRuleError when the name is not in the catalogue.
 */
export function applyRule(
  rule: JfssAdvancedRule,
  context: Omit<RuleContext, 'params'>,
): FieldViolation | UndecidedRule | undefined {
  const entry = VALIDATION_RULES[rule.rule]

  if (!entry) {
    throw new UnknownValidationRuleError(rule.rule)
  }

  // The definition's own `scope` first: an author who scoped a `both` rule to
  // `server` has said where they want it run, and JFSS §6.1 makes that the
  // property's meaning. Only then does the catalogue get asked whether this
  // side could have run it anyway.
  if (rule.scope === 'server') {
    return undefined
  }

  if (!entry.decide) {
    return { rule: rule.rule, scope: entry.scope, reason: entry.undecidable ?? '' }
  }

  if (entry.decide({ ...context, params: rule.params ?? {} })) {
    return undefined
  }

  // §6.2 makes `message` required, so a definition that reaches here has one.
  // The fallback is for a document stored before that was enforced.
  return { rule: rule.rule, message: rule.message ?? 'This value is not accepted.' }
}

/** Whether an outcome of [`applyRule`] is the failing kind. */
export function isViolation(
  outcome: FieldViolation | UndecidedRule | undefined,
): outcome is FieldViolation {
  return outcome !== undefined && 'message' in outcome
}

/**
 * JFSS §5's *"present and non-empty"*.
 *
 * **`false` is a value, not an absence**, and that is the parity-preserving
 * reading rather than the convenient one: the backend sees `Value::Bool(false)`
 * as present, and a client that called it empty would refuse submissions the
 * server accepts — the exact divergence class S10.1.1 was errata'd to close.
 *
 * A consent checkbox that must be ticked is expressible without inventing a
 * second meaning for `required`: `validation.enum: [true]` says it, and this
 * file already decides `enum`.
 */
function isEmpty(value: unknown): boolean {
  return value == null || value === '' || (Array.isArray(value) && value.length === 0)
}

/**
 * Whether a present value is the `type` the definition declares.
 *
 * Checked because §5 makes `type` a validation keyword like the others, and
 * because the payload a rendered form carries is not only what its own fields
 * produced — an edit re-opens a stored document, and a definition may have been
 * revised since it was written.
 *
 * `object` is every non-array object: JFSS uses it for shapes no component type
 * currently collects, and refusing one for not being an array would be this
 * file inventing a constraint.
 */
function matchesType(value: unknown, type: JfssValidation['type']): boolean {
  switch (type) {
    case 'string':
      return typeof value === 'string'
    case 'number':
      return typeof value === 'number' && Number.isFinite(value)
    case 'integer':
      return typeof value === 'number' && Number.isInteger(value)
    case 'boolean':
      return typeof value === 'boolean'
    case 'array':
      return Array.isArray(value)
    case 'object':
      return typeof value === 'object' && value !== null && !Array.isArray(value)
  }
}

/**
 * §5's `format` keyword.
 *
 * **Digit classes are pinned to `[0-9]` rather than written `\d`.** That is the
 * Validation Rule Registry's own interim guidance under the `regex` warning,
 * and the reason is not stylistic: ECMA-262 `\d` is ASCII-only while Rust's is
 * Unicode `Nd`, so `\d` here and `\d` on the server disagree about `٣٤٥` with
 * nothing raised on either side. Pinning the class costs two characters and
 * removes the divergence from a keyword scoped `both`.
 *
 * `uri` is decided by the URL parser rather than by a pattern: a regular
 * expression that accepts every valid URI and no invalid one is famously not
 * worth writing, and the platform already has the parser.
 */
const FORMATS: Readonly<Record<string, (value: string) => boolean>> = {
  // Deliberately permissive, as the HTML `email` input is: the only proof an
  // address exists is a message delivered to it, and a stricter pattern refuses
  // real addresses long before it catches a fake one.
  email: (value) => /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(value),
  uri: (value) => {
    try {
      new URL(value)
      return true
    } catch {
      return false
    }
  },
  // A shape *and* a real date: `2026-02-31` matches every pattern anybody writes
  // for a date and is not a day. Parsing the bare date form is UTC, so no
  // timezone shifts the round trip by one.
  date: (value) => {
    if (!/^[0-9]{4}-[0-9]{2}-[0-9]{2}$/.test(value)) {
      return false
    }

    const parsed = new Date(value)

    return !Number.isNaN(parsed.getTime()) && parsed.toISOString().startsWith(value)
  },
  // Bounded rather than "two digits, a colon, two digits": `25:99` is a shape
  // and not a time, and the round trip that catches it for a date has no
  // equivalent here.
  time: (value) => /^([01][0-9]|2[0-3]):[0-5][0-9](:[0-5][0-9])?$/.test(value),
  'date-time': (value) =>
    /^[0-9]{4}-[0-9]{2}-[0-9]{2}[T ]/.test(value) && !Number.isNaN(Date.parse(value)),
  uuid: (value) => /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(value),
}

/**
 * What a failing keyword says when the definition supplies nothing.
 *
 * `validation.messages` overrides any of these per keyword (§5), and a
 * definition that supplies one is what #163 AC1 means by *"messages the
 * definition supplies"*. These are the fallback, phrased for the person filling
 * in the form rather than for whoever wrote the schema.
 */
function defaultMessage(keyword: string, validation: JfssValidation): string {
  switch (keyword) {
    case 'required':
      return 'This field is required.'
    case 'type':
      return `This field takes a ${validation.type}.`
    case 'minLength':
      return `Use at least ${validation.minLength} characters.`
    case 'maxLength':
      return `Use at most ${validation.maxLength} characters.`
    case 'minimum':
      return `Enter ${validation.minimum} or more.`
    case 'maximum':
      return `Enter ${validation.maximum} or less.`
    case 'pattern':
      return 'This value is not in the expected format.'
    case 'format':
      return `Enter a valid ${validation.format}.`
    case 'enum':
      return 'Choose one of the offered values.'
    case 'uniqueItems':
      return 'Every row must be different.'
    case 'uniqueBy':
      return 'Two rows repeat a value that must be unique.'
    default:
      return 'This value is not accepted.'
  }
}

/** The keys a row must be unique by. §5 allows one or several. */
function uniqueByKeys(validation: JfssValidation): string[] {
  const declared = validation.uniqueBy

  if (declared === undefined) {
    return []
  }

  return Array.isArray(declared) ? declared : [declared]
}

/** Whether the rows of an array repeat a value under the declared keys. */
function repeatsAKey(rows: unknown[], keys: string[]): boolean {
  const seen = new Set<string>()

  for (const row of rows) {
    if (typeof row !== 'object' || row === null) {
      continue
    }

    const record = row as Record<string, unknown>
    // Every declared key together, so `uniqueBy: ["sku", "warehouse"]` means
    // the pair is unique rather than each column being unique on its own.
    const fingerprint = JSON.stringify(keys.map((key) => record[key] ?? null))

    if (seen.has(fingerprint)) {
      return true
    }

    seen.add(fingerprint)
  }

  return false
}

/**
 * The `validation` object's keywords, in the order a person would want them.
 *
 * One violation per field rather than a list: a form shows one message under an
 * input, and reporting "required" and "too short" about the same empty box at
 * once is two ways of saying the field is empty.
 *
 * **Every keyword after `required` is skipped for an empty value**, which is
 * JSON Schema's own semantics and is what keeps an optional field optional: a
 * blank `needed_by` is not a malformed date.
 */
function checkValidation(validation: JfssValidation, value: unknown): FieldViolation | undefined {
  const fail = (keyword: string): FieldViolation => ({
    rule: keyword,
    message: validation.messages?.[keyword] ?? defaultMessage(keyword, validation),
  })

  if (isEmpty(value)) {
    return validation.required ? fail('required') : undefined
  }

  if (!matchesType(value, validation.type)) {
    return fail('type')
  }

  if (typeof value === 'string') {
    if (validation.minLength !== undefined && value.length < validation.minLength) {
      return fail('minLength')
    }

    if (validation.maxLength !== undefined && value.length > validation.maxLength) {
      return fail('maxLength')
    }

    if (validation.pattern !== undefined && !matchesPattern(validation.pattern, value)) {
      return fail('pattern')
    }

    if (validation.format !== undefined && !(FORMATS[validation.format]?.(value) ?? true)) {
      return fail('format')
    }
  }

  if (typeof value === 'number') {
    if (validation.minimum !== undefined && value < validation.minimum) {
      return fail('minimum')
    }

    if (validation.maximum !== undefined && value > validation.maximum) {
      return fail('maximum')
    }
  }

  if (validation.enum !== undefined && !validation.enum.includes(value)) {
    return fail('enum')
  }

  if (Array.isArray(value)) {
    if (validation.uniqueItems === true) {
      const encoded = value.map((item) => JSON.stringify(item))

      if (new Set(encoded).size !== encoded.length) {
        return fail('uniqueItems')
      }
    }

    const keys = uniqueByKeys(validation)

    if (keys.length > 0 && repeatsAKey(value, keys)) {
      return fail('uniqueBy')
    }
  }

  return undefined
}

/**
 * `validation.pattern`, with the same caveat the `regex` rule carries.
 *
 * A pattern the browser cannot compile is a violation and not a pass, for the
 * reason the rule gives: a check that could not be applied has not been met.
 */
function matchesPattern(pattern: string, value: string): boolean {
  try {
    return new RegExp(pattern).test(value)
  } catch {
    return false
  }
}

/** What a definition decides about one field, and what it leaves undecided. */
export interface FieldOutcome {
  /** The single message shown under the field, or none. */
  violation?: FieldViolation
  /** Rules the registry defines that this side did not decide. */
  undecided: UndecidedRule[]
}

/**
 * Everything a definition says about one value: §5's keywords, then §6's rules.
 *
 * **Basic before advanced**, because §5's keywords describe the value's own
 * shape and §6's rules describe its relationship to other values — telling
 * somebody their password does not match the confirmation, when what they typed
 * is four characters long, answers the wrong question.
 *
 * **Every rule name is resolved even when the field is empty**, though an empty
 * field's advanced verdicts are discarded. The two are separate: resolving the
 * name is what raises on one nobody defines, and a defect in the definition
 * must not be able to hide behind whatever the user has not typed yet. The
 * verdicts are discarded for the reason JSON Schema discards them — an optional
 * field left blank is not a field whose value is wrong.
 *
 * @throws UnknownValidationRuleError when a rule name is not in the catalogue.
 */
export function validateField(
  component: JfssDataComponent,
  value: unknown,
  scope: Record<string, unknown>,
): FieldOutcome {
  const basic = checkValidation(component.validation, value)
  const undecided: UndecidedRule[] = []
  const empty = isEmpty(value)
  let advanced: FieldViolation | undefined

  for (const rule of component.rules ?? []) {
    const outcome = applyRule(rule, { value, scope })

    if (isViolation(outcome)) {
      if (!empty) {
        advanced ??= outcome
      }
    } else if (outcome) {
      undecided.push(outcome)
    }
  }

  return { violation: basic ?? advanced, undecided }
}
