import {
  computed,
  inject,
  provide,
  ref,
  shallowRef,
  watch,
  type ComputedRef,
  type InjectionKey,
  type Ref,
} from 'vue'

import {
  UnknownValidationRuleError,
  validateField,
  type FieldViolation,
  type UndecidedRule,
} from './validation'
import {
  EvaluationError,
  loadEvaluator,
  normalizeNumeric,
  type RuleEvaluator,
} from '@/lib/jsonlogic'
import type { ValidationDetail } from '@/types/api'
import {
  childComponents,
  isDataComponent,
  isLayoutComponent,
  type JfssComponent,
  type JfssDataComponent,
  type JfssDefinition,
  type JfssDisplayComponent,
} from '@/types/jfss'

/**
 * A rendered form evaluating its own rules (FR-RAD-010, FR-RAD-006, #163).
 *
 * **The evaluation, not the engine.** What is here is a form taking the
 * `validate` and `calculate` expressions its definition carries, evaluating
 * them as the user types, and showing the result. The rule catalogue, the
 * dependency graph, cycle detection and error mapping are FR-RAD-006 in
 * Sprints 14–16 under decision **D-2**, and the boundary is stated because it
 * is the kind that erodes quietly — every one of those concerns can be argued
 * into *"we need a little of it to make this work"*.
 *
 * **No dependency graph, and that is the cheap version taken on purpose**
 * (construction plan §5.2). Every `derived` field is re-evaluated in definition
 * order whenever any value changes — a few dozen evaluations per keystroke on a
 * form of tens of fields, which is affordable. A form of several hundred
 * computed fields would need the graph; none has that shape yet, and Sprint 14
 * should replace this having seen one rather than having guessed at it.
 *
 * **JFSS §9.1 asks for the graph in as many words** — *"build the dependency
 * graph once … recompute only the fields downstream of a change"* — so this is
 * a stated divergence and not an oversight. §9 is *Cross-Language
 * Implementation Guidelines* rather than a conformance clause, and the reason
 * §9.1 gives for the graph is the reason **D-2** reserves it: it is already
 * required for cycle detection (S12.2), which is the rule engine's, in Sprints
 * 14–16. Building half of it here to save keystrokes on forms that do not exist
 * yet is exactly the erosion §1 of the construction plan names.
 *
 * **The engine arrives late and nothing waits for it.** `loadEvaluator` fetches
 * 588 KB gzipped, which **D-10** accepted on the single condition that only a
 * page rendering a form pays it — so the import is dynamic, this composable is
 * the only thing in the renderer that reaches for it, and a form renders,
 * accepts typing and checks its §5 keywords before it lands. What waits is
 * `calculate` and `conditional`, and both have a defined answer for "not yet":
 * a computed field is left as it is, and every component is visible.
 *
 * **And a defined answer for "not ever"** ([#273](https://github.com/sujanto-gaws/kelir/issues/273),
 * **D-54**). Those two used to be the same state: a rejected load left `engine`
 * undefined exactly as a pending one does, so a form whose engine had failed
 * looked like a form whose engine was slow, for ever. The behaviour is
 * unchanged — the unawaited load stays, nothing blocks, nothing is refused —
 * and what is added is that the failure is **caught**, and that the form says
 * so. Waiting still says nothing, which is D-10's condition and the half a fix
 * here could easily have broken.
 */

/** What a form's rules decide, for the components rendering inside it. */
export interface FormEvaluation {
  /** True once the engine has arrived, or when the definition needs none. */
  ready: ComputedRef<boolean>
  /**
   * True when the engine **will not** arrive — the load was attempted and
   * rejected ([#273](https://github.com/sujanto-gaws/kelir/issues/273)).
   *
   * **`ready === false` says two different things and this is the second one.**
   * Before this flag existed, *the engine is still coming* and *the engine is
   * never coming* were the same state on the screen: totals blank or stale,
   * conditional sections all visible, nothing said. The first resolves itself
   * on a slow connection; the second never does, and the person filling the
   * form in had no way to tell which they were in.
   *
   * **It stays true for the life of the page**, because `loadEvaluator`
   * memoizes its promise — a rejected load is a rejected load until a reload.
   * That is what makes *not ever* an honest thing to say rather than a guess.
   */
  engineUnavailable: ComputedRef<boolean>
  /**
   * A defect in the **definition**, which stops nothing else from rendering.
   *
   * Today the one way here is a `rules` entry naming something the Validation
   * Rule Registry does not define (#163 AC3). It is surfaced rather than
   * thrown, because a form that will not render at all tells the person in
   * front of it less than a form that renders and says what is wrong with it —
   * and it makes the form invalid, so nothing is reported as checked that was
   * not.
   */
  defect: ComputedRef<string | undefined>
  /** Registry rules this side does not decide, one entry per rule name. */
  undecided: ComputedRef<UndecidedRule[]>
  /** Whether `conditional` leaves this component rendered (JFSS §7). */
  isVisible(component: JfssComponent, scope: Record<string, unknown>): boolean
  /** Whether this component's control accepts input. */
  isEditable(component: JfssComponent, scope: Record<string, unknown>): boolean
  /** The text a display component shows, computed or literal (JFSS §4.4). */
  displayContent(component: JfssDisplayComponent, scope: Record<string, unknown>): string
  /** The message shown under a field right now, or none. */
  violationFor(
    component: JfssDataComponent,
    scope: Record<string, unknown>,
  ): FieldViolation | undefined
  /**
   * What the **server** said about this field when the form was last submitted.
   *
   * Addressed by JFSS S10.3 dot-notation path rather than by `key`, which is
   * why the envelope calls it `path`: `line_items.2.quantity` is not a key.
   *
   * A message is dropped as soon as the value it was about changes, so a field
   * the person has since corrected stops carrying the server's complaint about
   * what it used to hold. The rest stand until the next submission is answered
   * — they are the only thing that ever reports a `server`-scoped rule, and
   * clearing them on any keystroke would erase a `unique` verdict because
   * somebody edited a different box.
   */
  serverViolationFor(path: string): FieldViolation | undefined
  /**
   * Records the S10.3 details a refused submission came back with.
   *
   * Replaces whatever was there: they are the server's answer to *one* payload,
   * and two answers to two payloads shown together would say nothing about
   * either.
   */
  reportServerViolations(details: ValidationDetail[]): void
  /** Whether every visible field is acceptable and the definition is sound. */
  isValid: ComputedRef<boolean>
  /**
   * Reveals the messages, and answers whether there were any.
   *
   * A form does not show its verdict until it is asked to — see
   * {@link createFormEvaluation} for why that is a decision rather than a
   * default.
   */
  reveal(): boolean
}

const EVALUATION_KEY: InjectionKey<FormEvaluation> = Symbol('jfss-form-evaluation')

/**
 * The record a component's `key` addresses where it renders.
 *
 * The form payload at the top level and a **row object** inside a datagrid's
 * row template (JFSS §4.3.1). Provided rather than threaded as a prop for the
 * reason [`useFieldScope`](./useFieldScope.ts) gives about its own prefix: the
 * components that care are the field and the shell around it, and a prop would
 * put it on every layout container in between that never reads it.
 */
const VALUE_SCOPE_KEY: InjectionKey<ComputedRef<Record<string, unknown>>> =
  Symbol('jfss-value-scope')

export function provideValueScope(scope: () => Record<string, unknown>): void {
  provide(
    VALUE_SCOPE_KEY,
    computed(() => scope()),
  )
}

/**
 * The **dot-notation path prefix** a component's `key` sits under (JFSS S10.3).
 *
 * Empty at the top of a form, `line_items.0.` inside the first row of a
 * repeater. It is not the same thing as [`useFieldScope`](./useFieldScope.ts)'s
 * prefix, and the two are deliberately separate: that one addresses the *DOM*
 * (`jfss-row-0-line-total`) and this one addresses the *payload*. They look
 * alike and they diverge the moment a `key` and an `id` differ, which they
 * usually do.
 *
 * It exists because a server violation arrives keyed by path — S10.3 names the
 * field `path` rather than `key` precisely so a row's address can be one — and
 * a field has to be able to ask whether one of them is about it.
 *
 * Appending rather than replacing, for the reason `useFieldScope` gives about
 * its own: a datagrid inside a datagrid row is a shape JFSS permits, and the
 * inner row's fields live under the outer row's path.
 */
const VALUE_PATH_KEY: InjectionKey<ComputedRef<string>> = Symbol('jfss-value-path')

export function provideValuePath(segment: () => string): void {
  const parent = inject(VALUE_PATH_KEY, undefined)

  provide(
    VALUE_PATH_KEY,
    computed(() => `${parent?.value ?? ''}${segment()}`),
  )
}

/** The payload path prefix in force here — empty at the top level of a form. */
export function useValuePath(): ComputedRef<string> {
  return inject(
    VALUE_PATH_KEY,
    computed(() => ''),
  )
}

/** The scope in force here. Empty outside a form, so a field still mounts alone. */
export function useValueScope(): ComputedRef<Record<string, unknown>> {
  return inject(
    VALUE_SCOPE_KEY,
    computed(() => ({})),
  )
}

/** The evaluation in force, or `undefined` for a component mounted outside a form. */
export function useFormEvaluation(): FormEvaluation | undefined {
  return inject(EVALUATION_KEY, undefined)
}

/** Makes an evaluation available to every component rendered beneath. */
export function provideFormEvaluation(evaluation: FormEvaluation): void {
  provide(EVALUATION_KEY, evaluation)
}

/**
 * JFSS §4.2.3's mode, **read and never inferred**.
 *
 * S8.1.1 forbids inferring it from whether the operators look deterministic:
 * that would require every language to maintain an identical operator
 * classification, and the Calculation Rule Registry does not carry one. A
 * missing mode is `derived`, which the specification states rather than leaves
 * to an implementation's default.
 */
function calculateMode(component: JfssDataComponent): 'derived' | 'generated' {
  return component.calculateMode === 'generated' ? 'generated' : 'derived'
}

/** Whether a field's value is computed and therefore not the user's to set (Case B). */
export function isDerived(component: JfssComponent): boolean {
  return (
    isDataComponent(component) &&
    component.calculate !== undefined &&
    calculateMode(component) === 'derived'
  )
}

/** One data component, and the record its `key` addresses. */
interface ScopedField {
  component: JfssDataComponent
  scope: Record<string, unknown>
}

/**
 * Every data component of a definition, each with the scope it belongs to.
 *
 * Descends JFSS §4.3.1's three container shapes **and** a repeater's rows —
 * which is the one walk in the frontend that goes further than
 * [`dataComponents`](../../../types/jfss.ts). That function stops at a data
 * component on purpose, because a row template's keys are not siblings of the
 * payload's; here the row objects are in hand, so the template is walked once
 * per row against the row it belongs to.
 *
 * **`hidden` is skipped, and that is S10.1.1 from this side.** A hidden field
 * is still submitted and the server discards its value; what it must never be
 * is the reason a form cannot be submitted at all. A `required` field on a
 * branch the user's answers took them away from would otherwise block a
 * submission over a box nobody can see.
 */
function scopedFields(
  components: JfssComponent[],
  scope: Record<string, unknown>,
  visible: (component: JfssComponent, scope: Record<string, unknown>) => boolean,
): ScopedField[] {
  const found: ScopedField[] = []

  for (const component of components) {
    if (!visible(component, scope)) {
      continue
    }

    if (isLayoutComponent(component)) {
      found.push(...scopedFields(childComponents(component), scope, visible))
      continue
    }

    if (!isDataComponent(component)) {
      continue
    }

    found.push({ component, scope })

    // A repeater: its template once per row, against that row.
    if (component.components && Array.isArray(scope[component.key])) {
      for (const row of scope[component.key] as unknown[]) {
        if (typeof row === 'object' && row !== null) {
          found.push(...scopedFields(component.components, row as Record<string, unknown>, visible))
        }
      }
    }
  }

  return found
}

/**
 * Whether a definition carries anything the engine would have to evaluate.
 *
 * **This is D-10's bundle condition applied one notch tighter than it asks
 * for.** The decision requires the engine off the first-load path; this also
 * keeps it off a form that has no expression in it, which several will. It
 * costs one walk of a document that has already been walked twice.
 */
function needsEvaluator(components: JfssComponent[]): boolean {
  return components.some((component) => {
    if (component.conditional !== undefined) {
      return true
    }

    if ('calculate' in component && component.calculate !== undefined) {
      return true
    }

    const template = isDataComponent(component) ? (component.components ?? []) : []

    return needsEvaluator([...childComponents(component), ...template])
  })
}

/**
 * Runs a definition's rules over a live payload.
 *
 * `values` is mutated in place — a `derived` field *is* a value in the payload
 * that something else computes, which is what makes S10.1's "every data key"
 * true of computed fields without a second store to keep in step.
 *
 * **Messages are hidden until {@link FormEvaluation.reveal} is called**, which
 * is a decision and not an oversight. Construction plan §5.6 argues it for
 * calculations — *a form that greets its user with three red fields it filled
 * in itself is a worse failure than the one D-24 prevented* — and the argument
 * is the same for a `required` marker on a box nobody has reached yet. The
 * verdict is computed continuously all the same, so `isValid` is right from the
 * first render and a revealed message clears the moment the field is fixed.
 */
export function createFormEvaluation(
  definition: Ref<JfssDefinition>,
  values: Record<string, unknown>,
): FormEvaluation {
  const engine = shallowRef<RuleEvaluator | undefined>()
  const unavailable = ref(false)
  const revealed = ref(false)
  const reported = ref<ReportedViolation[]>([])

  const expected = computed(() => needsEvaluator(definition.value.components))
  const ready = computed(() => engine.value !== undefined || !expected.value)

  watch(
    definition,
    () => {
      revealed.value = false

      if (engine.value === undefined && expected.value && !unavailable.value) {
        // Not awaited: a form renders, accepts typing and checks its §5
        // keywords without the engine. The alternative — a spinner over the
        // whole form until 588 KB arrives — would make D-10's bundle cost
        // visible on exactly the page that was supposed to absorb it.
        //
        // **Handled, though** (#273 AC1). The rejection used to go nowhere: an
        // unhandled promise rejection, `engine` left undefined, and a form that
        // computed nothing and said nothing. What it now sets is a flag the
        // form reads, and *nothing else changes* — the same fields render, the
        // same typing is accepted, the same submission is allowed, because the
        // server recomputes every calculated value at submit (#163) and this
        // side has never been what decides them.
        loadEvaluator()
          .then((loaded) => {
            engine.value = loaded
          })
          .catch((error: unknown) => {
            unavailable.value = true

            // Logged as well as shown: the banner tells the person what they
            // can do about it, and this tells whoever is looking at the console
            // what actually failed — a chunk that 404s and a WebAssembly
            // instantiation the browser refused are the same sentence on screen
            // and different problems underneath.
            console.error('the JSON Logic engine did not load', error)
          })
      }
    },
    { immediate: true },
  )

  /**
   * Evaluates one expression, or reports that it produced nothing.
   *
   * **A failure is not an error state**, and the distinction is construction
   * plan §5.6: on a zero-filled payload `{"/": [{"var": "total"}, {"var":
   * "count"}]}` fails before the first keystroke, because **D-24** makes every
   * division by zero an evaluation failure while `count` is still the 0 S12.4
   * put there. So a failed calculation is `undefined` here, renders blank, and
   * is refused at submit by #164 rather than at render.
   *
   * An unknown *operator* lands here too and is not distinguished, because the
   * backend already refuses one at save (`domain/jfss.rs` checks the approved
   * set) — so a definition that reaches a browser cannot carry one, and a
   * second opinion about it here would be #162 AC2's second validator.
   */
  function evaluate(expression: unknown, scope: Record<string, unknown>): unknown {
    if (!engine.value || expression === undefined) {
      return undefined
    }

    try {
      return engine.value.evaluate(expression, scope)
    } catch (error) {
      if (error instanceof EvaluationError) {
        return undefined
      }

      throw error
    }
  }

  /**
   * A computed result as the field's declared type wants it.
   *
   * Numeric fields go through §7.3's normalization and every other type takes
   * the raw result: the wrapper is about arithmetic, and putting a boolean or a
   * string through it would turn every computed label into `0`.
   */
  function coerce(component: JfssDataComponent, result: unknown): unknown {
    const numeric =
      component.validation.type === 'number' || component.validation.type === 'integer'

    return numeric ? normalizeNumeric(result) : result
  }

  /** Writes only a changed value, so a pass that decides nothing wakes nothing. */
  function write(scope: Record<string, unknown>, key: string, value: unknown): void {
    const next = value === undefined ? null : value

    if (!Object.is(scope[key], next)) {
      scope[key] = next
    }
  }

  /**
   * Applies every `calculate` the definition carries, in definition order.
   *
   * S4.2.3 Case B and Case C, branched on the **declared** mode:
   *
   * - `derived` — recomputed on every pass, and the computed value always wins
   *   over runtime input, existing payload and `defaultValue` alike. There is
   *   no priority list because no other source may ever take effect.
   * - `generated` — resolved once, and a persisted value is **never**
   *   recomputed. `defaultValue` is the fallback when the expression yields
   *   nothing, which is Case C's third priority rather than a convenience.
   *
   * **Case C's "exactly once" is delivered by the null check and by nothing
   * else, and that is worth stating.** A resolved value is non-null, so the
   * branch does not run again; an expression that yields nothing where no
   * `defaultValue` exists leaves the field null and is retried on the next
   * pass. For every operator Kelir approves that retry is indistinguishable
   * from the first attempt — the backend's `CALCULATE_OPERATORS` carries no
   * non-deterministic operator at all, so `generateInvoiceId` is refused at
   * save and this tier has nothing yet that a second evaluation could answer
   * differently. **The day one is approved, this needs a real once-marker**,
   * and it has to be keyed to the row rather than to the row's index: a
   * repeater rebuilds its row objects on every cell edit and renumbers them on
   * every deletion, so both identity and position are unusable as a memo key.
   *
   * Nothing is written before the engine arrives. A derived field is blank when
   * its expression fails, and "the engine has not loaded" is not the same fact
   * — writing blank for it would wipe the stored value of every computed field
   * on a document being re-opened, for as long as the fetch takes.
   */
  function applyCalculations(components: JfssComponent[], scope: Record<string, unknown>): void {
    if (!engine.value) {
      return
    }

    for (const component of components) {
      if (isLayoutComponent(component)) {
        applyCalculations(childComponents(component), scope)
        continue
      }

      if (!isDataComponent(component)) {
        continue
      }

      const derived = calculateMode(component) === 'derived'

      // The generated branch's guard comes before the evaluation rather than
      // after it: a resolved generated field would otherwise be re-evaluated on
      // every keystroke for a result that is thrown away.
      if (component.calculate !== undefined && (derived || scope[component.key] == null)) {
        const result = evaluate(component.calculate, scope)

        if (derived) {
          // Blank when the expression produced nothing, which construction plan
          // §5.6 asks for and D-24 made ordinary rather than rare.
          write(scope, component.key, result === undefined ? null : coerce(component, result))
        } else {
          // Case C's priority 2 then 3. `null` is "the operator yielded null",
          // which the table answers with `defaultValue` — so the check is
          // before the numeric wrapper rather than after it, where §7.3 would
          // already have turned the null into a 0 nobody asked for.
          write(
            scope,
            component.key,
            result == null ? component.defaultValue : coerce(component, result),
          )
        }
      }

      // A repeater: the same walk over the template, once per row, against that
      // row — because a template's `key`s address properties of the row (§4.3.1)
      // and an expression written in one means its own row's siblings.
      if (component.components && Array.isArray(scope[component.key])) {
        for (const row of scope[component.key] as unknown[]) {
          if (typeof row === 'object' && row !== null) {
            applyCalculations(component.components, row as Record<string, unknown>)
          }
        }
      }
    }
  }

  // Every value change re-runs the calculations, including the ones the
  // calculations themselves make. It terminates because `write` assigns only a
  // changed value: a second pass over a settled payload assigns nothing and
  // wakes nothing. This is §5.2's "all derived fields, in definition order".
  watch(
    [definition, () => values, engine],
    () => applyCalculations(definition.value.components, values),
    {
      deep: true,
      immediate: true,
    },
  )

  /**
   * JFSS §7: what a `conditional` decides about one component.
   *
   * **A conditional that cannot be evaluated leaves the component alone** —
   * rendered and editable — for §5.6's reason applied to visibility. Before the
   * engine arrives, and on a payload whose arithmetic has not settled, the
   * alternatives are a form that hides fields it will show a moment later and
   * one that disables an input because a division has not been filled in yet.
   */
  function conditionalHolds(component: JfssComponent, scope: Record<string, unknown>): boolean {
    return evaluate(component.conditional?.logic, scope) === true
  }

  function isVisible(component: JfssComponent, scope: Record<string, unknown>): boolean {
    const action = component.conditional?.action

    if (action === 'show') {
      return conditionalHolds(component, scope)
    }

    if (action === 'hide') {
      return !conditionalHolds(component, scope)
    }

    return true
  }

  function isEditable(component: JfssComponent, scope: Record<string, unknown>): boolean {
    // Case B: a derived field MUST be read-only. The specification says it
    // rather than the definition, so a definition that forgets `readOnly` does
    // not get an input whose value is overwritten as it is typed into.
    if (isDerived(component)) {
      return false
    }

    if (isDataComponent(component) && component.readOnly) {
      return false
    }

    const action = component.conditional?.action

    if (action === 'enable') {
      return conditionalHolds(component, scope)
    }

    if (action === 'disable') {
      return !conditionalHolds(component, scope)
    }

    return true
  }

  function displayContent(component: JfssDisplayComponent, scope: Record<string, unknown>): string {
    if (component.calculate === undefined) {
      return component.content ?? ''
    }

    const result = evaluate(component.calculate, scope)

    // Blank rather than the literal `content` beside it: §4.4 makes the two
    // alternatives, and falling back would show a stale label where a number
    // failed to compute. Blank is what §5.6 asks a failed calculation to look
    // like, and it is the answer a computed *field* gives.
    if (result === undefined || result === null) {
      return ''
    }

    // No currency prefix and no fixed decimals. Formatting decided inside a
    // renderer is the reference implementation's defect and #162 AC3's stated
    // anti-pattern: nothing about a specific form belongs here.
    return typeof result === 'object' ? JSON.stringify(result) : String(result)
  }

  /**
   * Every visible field's verdict, recomputed as the payload changes.
   *
   * One pass over the whole tree rather than a computed per field: the walk has
   * to descend into rows anyway, and an unknown rule name has to be raised once
   * for the form rather than once per field that happens to be on screen.
   */
  const outcomes = computed(() => {
    const undecided = new Map<string, UndecidedRule>()
    let failures = 0
    let defect: string | undefined

    try {
      for (const { component, scope } of scopedFields(
        definition.value.components,
        values,
        isVisible,
      )) {
        const outcome = validateField(component, scope[component.key], scope)

        for (const rule of outcome.undecided) {
          undecided.set(rule.rule, rule)
        }

        if (outcome.violation) {
          failures += 1
        }
      }
    } catch (error) {
      if (!(error instanceof UnknownValidationRuleError)) {
        throw error
      }

      // Loud, and it does not report the form as valid: a definition naming a
      // rule nobody defines has not been checked, and calling it checked is the
      // failure #163 AC3 is about.
      defect = error.message
    }

    return { failures, defect, undecided: [...undecided.values()] }
  })

  /**
   * One field's verdict, decided where it is asked for.
   *
   * Re-derived rather than looked up in the pass above, which would have to key
   * a map by the scope object a row happens to be — an identity that survives
   * every edit until the day a row is copied. `validateField` is pure and a
   * field is validated twice per change; that is cheaper than the class of bug
   * the map invites.
   */
  function violationFor(
    component: JfssDataComponent,
    scope: Record<string, unknown>,
  ): FieldViolation | undefined {
    if (!revealed.value) {
      return undefined
    }

    try {
      return validateField(component, scope[component.key], scope).violation
    } catch (error) {
      if (error instanceof UnknownValidationRuleError) {
        // Already reported once, for the form. Repeating it under every field
        // would bury the field's own message in a defect that is not the
        // reader's to fix.
        return undefined
      }

      throw error
    }
  }

  const defect = computed(() => outcomes.value.defect)
  const isValid = computed(() => defect.value === undefined && outcomes.value.failures === 0)

  function reveal(): boolean {
    revealed.value = true

    return isValid.value
  }

  /**
   * The server's answer, kept beside the value it was about.
   *
   * Storing `at` is what lets a corrected field clear its message without
   * clearing every other field's: the alternative — drop them all on the first
   * keystroke — would erase a `unique` verdict because somebody edited an
   * unrelated box, and that verdict is the only report a `server`-scoped rule
   * ever gets (Validation Rule Registry §3.3).
   */
  function reportServerViolations(details: ValidationDetail[]): void {
    reported.value = details.map((detail) => ({
      path: detail.path,
      rule: detail.rule,
      message: detail.message,
      at: JSON.stringify(valueAtPath(values, detail.path) ?? null),
    }))
  }

  function serverViolationFor(path: string): FieldViolation | undefined {
    const found = reported.value.find((violation) => violation.path === path)

    if (!found) {
      return undefined
    }

    // Still about what it was about. A changed value makes the message stale,
    // and a stale message beside a corrected field is worse than none.
    if (JSON.stringify(valueAtPath(values, path) ?? null) !== found.at) {
      return undefined
    }

    return { rule: found.rule, message: found.message }
  }

  // A new definition is a new form; the previous form's server answers are not
  // about it.
  watch(definition, () => {
    reported.value = []
  })

  return {
    ready,
    engineUnavailable: computed(() => unavailable.value && expected.value),
    defect,
    undecided: computed(() => outcomes.value.undecided),
    isVisible,
    isEditable,
    displayContent,
    violationFor,
    serverViolationFor,
    reportServerViolations,
    isValid,
    reveal,
  }
}

/** One S10.3 detail, and the value it was raised about. */
interface ReportedViolation {
  path: string
  rule: string
  message: string
  /** The value at `path` when the server answered, encoded for comparison. */
  at: string
}

/**
 * The value a JFSS S10.3 dot-notation path names, inside a payload.
 *
 * `line_items.2.quantity` is the third row's quantity. A `key` may not contain
 * a `.` — JFSS §4.2 reserves it as the separator and the meta-schema enforces
 * the pattern — so splitting on it is unambiguous.
 */
function valueAtPath(values: Record<string, unknown>, path: string): unknown {
  let current: unknown = values

  for (const segment of path.split('.')) {
    if (Array.isArray(current)) {
      current = current[Number(segment)]
    } else if (typeof current === 'object' && current !== null) {
      current = (current as Record<string, unknown>)[segment]
    } else {
      return undefined
    }
  }

  return current
}
