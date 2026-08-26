/**
 * The JSON Logic evaluator behind JFSS `calculate` and `conditional` rules
 * (FR-RAD-006, decision **D-10**).
 *
 * **This is the same engine the backend runs.** `@goplasmatic/datalogic-wasm`
 * and the backend's `datalogic-rs` are one Rust core compiled for two runtimes;
 * the [operator-parity spike](../../../projects/spikes/01.%20JFSS%20Operator%20Parity.md)
 * measured them at 51/51 agreement over the corpus, error cases included. That
 * is what makes JFSS Polyglot Parity a property that holds rather than one a
 * test suite chases — and it is why both sides pin `5.2.0` exactly and
 * [`parity/`](../../../parity/README.md) fails the build if they ever stop
 * agreeing.
 *
 * **The payload is loaded on demand and never on the first-load path.** The
 * engine is 588 KB gzipped against `json-logic-js`'s 4.1 KB, and D-10 accepted
 * that cost on one condition: only a page that renders a form pays it. Hence
 * [`loadEvaluator`] and its dynamic `import()` — Vite splits it into its own
 * chunk, so a user who signs in and reads a list never fetches it.
 *
 * What is **not** here is the rule engine around the evaluator: the catalogue,
 * the dependency graph, cycle detection and error mapping are Sprints 14–16
 * under decision **D-2**.
 */

/** The engine version both sides carry. A bump is a parity-affecting change. */
export const ENGINE_VERSION = '5.2.0'

/**
 * The registry's mandated normalizations, as engine configuration.
 *
 * The backend builds the same set from the same values (`modules/rad/evaluator.rs`),
 * and `parity/README.md` lists them in one place so a change to one side is
 * visibly a change to both. Configuration rather than a hand-written wrapper,
 * because a hand-written wrapper is exactly how two environments end up subtly
 * different — which is the defect the registry's own §7.3 wrapper has: it
 * normalizes `NaN`, which is falsy, and not `Infinity`, which is truthy.
 */
export const JFSS_ENGINE_CONFIG = {
  // §7.3: a null or missing operand yields 0, not NaN.
  arithmetic_nan_handling: 'coerce_to_zero',
  // §3.1 as decision **D-24** settles it: a division by zero does not produce a
  // value, so every one of them fails evaluation. `return_null` up to
  // 2026-08-26, which normalized `10.5 / 0` to 0 while the integer path threw
  // regardless of the setting — one expression behaving two ways depending on
  // whether the numerator happened to be fractional.
  division_by_zero: 'throw_error',
  // The reference implementation compares across types silently, and a form
  // that refuses to render because a blank field met a number is worse than one
  // that treats it as absent.
  loose_equality_errors: false,
  numeric_coercion: {
    null_to_zero: true,
    empty_string_to_zero: true,
    bool_to_number: true,
    reject_non_numeric: false,
  },
} as const

/**
 * `sum` as Calculation Rule Registry §3.2 requires it.
 *
 * Not a JSON Logic operator anywhere, so every environment registers its own
 * and the two have to agree on the same three edge cases: a non-array argument,
 * an empty array, and non-numeric members. Empty sums to `0`; anything that is
 * not a number contributes nothing rather than poisoning the total with `NaN`.
 *
 * The engine hands arguments in as a JSON array string and requires a JSON
 * string back — a thrown exception or a non-string return becomes an
 * evaluation error, which is the right outcome for a bug in here.
 */
export function sumOperator(argsJson: string): string {
  const args: unknown = JSON.parse(argsJson)
  const first = Array.isArray(args) ? args[0] : undefined
  const items = Array.isArray(first) ? first : []

  const total = items.reduce<number>((running, item) => {
    const value = typeof item === 'number' ? item : Number.NaN

    return Number.isFinite(value) ? running + value : running
  }, 0)

  return JSON.stringify(total)
}

/**
 * Calculation Rule Registry §7.3, as §3.1 means it rather than as §7.3 writes
 * it.
 *
 * §7.3 gives the wrapper as `Number(result) || 0`, and that does not implement
 * the §3.1 rule it is cited for: `Infinity` is truthy, so `Infinity || 0` is
 * `Infinity` and division by zero survived the wrapper untouched — the field
 * rendered `Infinity` and `JSON.stringify` turned it into `null` on submission.
 * A finiteness test is what the rule means.
 *
 * Kept identical to the backend's `normalize_numeric`, including the cases
 * nobody expects to hit: a numeric string coerces, `true` is 1, and every other
 * shape — an array, an object, `null` — is 0. A `null` reaching here is an
 * overflow rather than a division by zero, which since **D-24** fails
 * evaluation and never arrives.
 */
export function normalizeNumeric(value: unknown): number {
  if (typeof value === 'number') {
    return Number.isFinite(value) ? value : 0
  }

  if (typeof value === 'string') {
    const trimmed = value.trim()

    if (trimmed === '') {
      return 0
    }

    const parsed = Number(trimmed)

    return Number.isFinite(parsed) ? parsed : 0
  }

  if (typeof value === 'boolean') {
    return value ? 1 : 0
  }

  return 0
}

/** Thrown when an expression does not produce a value. */
export class EvaluationError extends Error {
  constructor(message: string) {
    super(message)
    this.name = 'EvaluationError'
  }
}

/** The engine surface this module uses, which is all of it. */
interface WasmEngine {
  evalStr(logic: string, data: string): string
}

/** Evaluates JFSS rule expressions. Obtain one from [`loadEvaluator`]. */
export class RuleEvaluator {
  constructor(private readonly engine: WasmEngine) {}

  /**
   * Evaluates `expression` against `data` and returns the raw result.
   *
   * Raw: `conditional` rules want the boolean the expression produced and
   * `calculate` rules want it put through {@link normalizeNumeric}. Deciding
   * that here would make one of the two tiers wrong.
   */
  evaluate(expression: unknown, data: unknown): unknown {
    let evaluated: string

    try {
      evaluated = this.engine.evalStr(JSON.stringify(expression), JSON.stringify(data))
    } catch (error) {
      // An unknown operator lands here, and that is the point: the engine
      // rejects one instead of returning the expression unevaluated. A
      // passthrough would be laundered into a plausible 0 by the wrapper above,
      // which is how a mistyped `sum` turned a 42-rupiah invoice line into
      // free (Calculation Rule Registry §4.1).
      throw new EvaluationError(error instanceof Error ? error.message : String(error))
    }

    return JSON.parse(evaluated)
  }

  /**
   * Evaluates a `calculate` expression and normalizes the result.
   *
   * **A division by zero never reaches this wrapper**, and since decision
   * **D-24** that is uniform rather than a gap. The registry asked for `0` from
   * v1.0.0 and the engine could not deliver it by configuration — its integer
   * division path throws under every `division_by_zero` setting — so under the
   * `return_null` this module carried until 2026-08-26, `10.5 / 0` normalized
   * to `0` while `10 / 0` threw: one expression behaving two ways depending on
   * whether the numerator happened to be fractional. Registry v1.6.0 §3.1 now
   * says a division by zero does not produce a value at all, and both sides
   * configure `throw_error`. What the renderer does with the failure is
   * [`useFormEvaluation`](../features/rad/renderer/useFormEvaluation.ts)'s: the
   * field renders blank while the form is being filled in, and #164 refuses the
   * submission with the S10.3 envelope naming it.
   */
  evaluateNumeric(expression: unknown, data: unknown): number {
    return normalizeNumeric(this.evaluate(expression, data))
  }
}

/**
 * The loaded engine, kept so a form with forty rules instantiates one.
 *
 * The promise is memoized rather than the result: two fields asking during the
 * same tick would otherwise start two downloads.
 */
let loading: Promise<RuleEvaluator> | undefined

/**
 * Loads the evaluator, fetching the WebAssembly payload the first time.
 *
 * `await import()` rather than a top-level import is the whole of D-10's
 * bundle condition — Vite emits the engine as its own chunk, so it is fetched
 * by the pages that render a form and by nothing else.
 */
export async function loadEvaluator(): Promise<RuleEvaluator> {
  loading ??= (async () => {
    const module = await import('@goplasmatic/datalogic-wasm')

    // The web build exports an async initializer that fetches the `.wasm`
    // asset; the Node build has none and initializes on import. Both are the
    // same binary, so this branch is glue rather than behaviour — but without
    // it the browser gets an engine whose memory was never set up.
    const init = (module as { default?: unknown }).default

    if (typeof init === 'function') {
      await (init as () => Promise<unknown>)()
    }

    return new RuleEvaluator(
      new module.Engine({
        customOperators: { sum: sumOperator },
        config: JFSS_ENGINE_CONFIG,
      }) as WasmEngine,
    )
  })()

  return loading
}

/**
 * Forgets the loaded engine.
 *
 * For tests that need a fresh one. Nothing in the application calls it: a
 * second load would refetch the payload D-10 spent its bundle budget on.
 */
export function resetEvaluatorForTests(): void {
  loading = undefined
}
