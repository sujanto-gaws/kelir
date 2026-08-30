/// <reference types="node" />
import fs from 'node:fs'
import path from 'node:path'

import { flushPromises, mount } from '@vue/test-utils'
import { beforeAll, describe, expect, it } from 'vitest'

import JfssForm from '../JfssForm.vue'
import { loadEvaluator } from '@/lib/jsonlogic'
import type { JfssDefinition } from '@/types/jfss'

/**
 * The frontend half of the **whole-form** parity gate (#164 AC5).
 *
 * `parity/cases.json` holds expressions and `jsonlogic.parity.spec.ts` holds
 * the two engines to the same answers over them. That is not enough for the
 * Tamper-Proof Pattern. Two engines can agree about every operator and still
 * disagree about a whole form: `calculateMode`, the order the calculations
 * settle in, and what happens to a field a `conditional` hides are all
 * decisions *above* the evaluator, and each of them is written twice.
 *
 * So `parity/forms.json` holds a definition, the payload a client sent, and the
 * payload the server must store. `kelir-backend/tests/jsonlogic_parity.rs`
 * asserts the server produces `secure`; this asserts that **a real rendered
 * form settles on the same values** — which together is the property AC5
 * actually asks for: *the number the server persists is the number the person
 * filling in the form was looking at.*
 *
 * **Beside `jsonlogic.parity.spec.ts` rather than inside it**, because that
 * file deliberately contains no Vue: it is about the engine. The client's
 * answer to a whole form is a rendered form's answer, which is this folder's.
 *
 * **Only the keys `secure` carries are compared.** A key it omits is one the
 * server *discarded* under S10.2, and the browser is required to keep and
 * submit it anyway (S10.1.1) — so a difference there is the pattern working
 * rather than a divergence.
 */

interface FormCase {
  id: string
  note?: string
  definition: JfssDefinition
  payload: Record<string, unknown>
  /** What the server must store, or `null` where the submission is refused. */
  secure: Record<string, unknown> | null
  /** For a refused case, the fields the refusal names. */
  refusedPaths?: string[]
}

// Resolved from the Vitest root — `kelir-frontend` — for the reason
// `jsonlogic.parity.spec.ts` gives about `import.meta.url`.
const corpusPath = path.resolve(process.cwd(), '..', 'parity', 'forms.json')
const cases: FormCase[] = JSON.parse(fs.readFileSync(corpusPath, 'utf8'))

/**
 * Mounts the definition over the payload, lets it settle, and presses submit.
 *
 * **The payload is taken from the `submit` action rather than from the `change`
 * stream**, and that is the whole point: the action carries what a submission
 * would actually post. Reading the change stream would compare against
 * something that leaked out of the form on its way to settling, and a form that
 * computes nothing emits nothing at all — so two of the cases below would have
 * been asserting on an empty object.
 *
 * Every corpus definition therefore carries a submit button. It is not
 * decoration: `JfssForm` gates the action on the definition's own rules, so a
 * case that cannot be submitted in a browser cannot claim parity with what the
 * server stores either.
 */
/**
 * The engine, before any case mounts ([#266]).
 *
 * **This is a wait on the condition rather than a longer wait**, which is the
 * distinction [#266] AC2 draws: `loadEvaluator` resolves a module-level promise
 * that every later `useFormEvaluation` shares, so once it has resolved here the
 * composable's own `.then` lands on the first flush of every case below.
 *
 * # What it was, and how the cause was established
 *
 * `settle` counted six `flushPromises` and its comment listed *the engine's
 * dynamic import* as one of the things they covered. They cannot cover it —
 * `@goplasmatic/datalogic-wasm` instantiates WebAssembly, which no number of
 * microtask flushes bounds. So the corpus passed when the engine happened to
 * arrive in time and failed when it did not, in whole groups rather than singly.
 *
 * The cause was not guessed. Two experiments settled it:
 *
 * * **Starving the flushes refutes the obvious theory.** At one and two
 *   flushes exactly one case fails; at three the corpus passes. Six was
 *   double the margin the microtask work needs, so the flake was never a
 *   flush-count problem.
 * * **An engine that never arrives reproduces the flake exactly.** Mocking
 *   `loadEvaluator` to a promise that never settles fails **seven** of ten —
 *   the same count first observed, including the same three cases named in
 *   [#266], with the same value-mismatch assertions.
 *
 * **A failed load is silent by design**, which is why it surfaced as seven
 * wrong numbers instead of one error: `useFormEvaluation` fires the load
 * unawaited and leaves computed fields as they are until it lands, so a form
 * renders and accepts typing without the engine. That is right for a browser
 * and is exactly what a parity corpus must not do.
 *
 * [#266]: https://github.com/sujanto-gaws/kelir/issues/266
 */
beforeAll(async () => {
  const evaluator = await loadEvaluator()

  // So a load that resolves to nothing fails here, once and legibly, rather
  // than as a wall of value mismatches in the cases that needed it.
  expect(evaluator, 'the JSON Logic engine loaded before the corpus ran').toBeTruthy()
})

async function settle(subject: FormCase): Promise<Record<string, unknown>> {
  const wrapper = mount(JfssForm, {
    props: { definition: subject.definition, initialValues: subject.payload },
  })

  // Microtask work, and only that: the calculation pass the engine's arrival
  // wakes, a repeater's `onMounted` sequence write, and the passes a chain
  // declared out of order takes to reach its fixed point.
  //
  // **The engine's own arrival is not among them, and used to be counted as if
  // it were** ([#266]). `loadEvaluator` instantiates WebAssembly; that is not a
  // microtask, so no number of flushes bounds it. Six usually covered it and
  // sometimes did not — which is the whole of the intermittence, and why
  // raising the number would have hidden it rather than fixed it. `beforeAll`
  // now waits for the engine before any case mounts.
  for (let round = 0; round < 6; round += 1) {
    await flushPromises()
  }

  // By its label, not by position: a repeater renders "Remove" and "Add row"
  // buttons of its own, and the first `button` on an invoice is one of those.
  const submit = wrapper.findAll('button').find((button) => button.text() === 'Submit')

  expect(submit, `${subject.id} renders its submit button`).toBeTruthy()

  await submit?.trigger('click')
  await flushPromises()

  const actions = wrapper.emitted('action') as [string, Record<string, unknown>][] | undefined
  const last = actions?.[actions.length - 1]

  wrapper.unmount()

  expect(last, `${subject.id} is submittable in a browser`).toBeTruthy()
  expect(last?.[0]).toBe('submit')

  return last?.[1] ?? {}
}

/**
 * The value a dot-notation path names, so a row is addressable the way S10.3
 * addresses one.
 */
function at(values: Record<string, unknown>, dotted: string): unknown {
  let current: unknown = values

  for (const segment of dotted.split('.')) {
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

/**
 * Every leaf of `secure`, as `(path, value)` pairs.
 *
 * Flattened rather than compared whole, so a failure names the field that moved
 * instead of printing two payloads and leaving the reader to diff them — which
 * is the same reason the backend half collects divergences rather than
 * asserting case by case.
 */
function leaves(value: unknown, prefix = ''): [string, unknown][] {
  if (Array.isArray(value)) {
    return value.flatMap((item, index) => leaves(item, `${prefix}${index}.`))
  }

  if (typeof value === 'object' && value !== null) {
    return Object.entries(value).flatMap(([key, inner]) => leaves(inner, `${prefix}${key}.`))
  }

  return [[prefix.replace(/\.$/, ''), value]]
}

describe('the JFSS whole-form parity corpus', () => {
  it('is not empty, because a gate over nothing passes', () => {
    expect(cases.length).toBeGreaterThan(0)
  })

  it.each(cases.map((subject) => [subject.id, subject] as const))(
    'settles %s on the values the server stores',
    async (_id, subject) => {
      const settled = await settle(subject)

      if (subject.secure === null) {
        // A refused case. **D-24** across both sides: the browser renders the
        // field blank and does not block typing, and the submission is refused
        // with the field named. Blank is `null` in the payload.
        for (const dotted of subject.refusedPaths ?? []) {
          expect(at(settled, dotted), `${dotted} renders blank`).toBeNull()
        }

        return
      }

      for (const [dotted, expected] of leaves(subject.secure)) {
        expect(at(settled, dotted), `${dotted} agrees with the server`).toEqual(expected)
      }
    },
  )
})
