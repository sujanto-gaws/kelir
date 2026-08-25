// Node's types are pulled in for this file alone rather than added to the
// project's `types`, which would let application code reach for `process` and
// `fs` without anything noticing. This is the only file here that reads a file
// off disk, and it is a test.
/// <reference types="node" />
import fs from 'node:fs'
import path from 'node:path'

import { beforeAll, describe, expect, it } from 'vitest'

import { loadEvaluator, type RuleEvaluator } from './jsonlogic'

/**
 * The frontend half of the JFSS parity gate (issue #154, decision **D-10**).
 *
 * `parity/cases.json` is the corpus the [operator-parity spike](../../../projects/spikes/01.%20JFSS%20Operator%20Parity.md)
 * built; `parity/expectations.json` is what the adopted engine answers, run
 * through the configuration and the custom `sum` this repository supplies. This
 * spec asserts the frontend reproduces it, and `kelir-backend/tests/jsonlogic_parity.rs`
 * asserts the backend does. **If the two engines ever stop agreeing, one of the
 * two fails and names the case** — which is the gate D-10 was bought with.
 *
 * The expectations file is generated from this side, so run
 * `npm run parity:update` after a deliberate engine change — and expect the
 * backend test to fail until the backend is bumped to match, because that is
 * exactly what a parity-affecting change looks like.
 *
 * **Error messages are not compared, and could not be**: one side words them in
 * Rust's `Debug` and the other in a JS `Error`. What is compared is whether the
 * expression produced a value at all, and which value — the same comparison the
 * spike made when it reported 51/51 "error cases included".
 */

interface Case {
  id: string
  tier: string
  expr: unknown
  data: unknown
  note?: string
}

interface Expectation {
  id: string
  ok: boolean
  value?: unknown
}

// Resolved from the Vitest root — `kelir-frontend` — rather than from
// `import.meta.url`, which the transform rewrites to something that is not a
// file URL and which `readFileSync` then refuses.
const parityDir = path.resolve(process.cwd(), '..', 'parity')
const corpusPath = path.join(parityDir, 'cases.json')
const expectationsPath = path.join(parityDir, 'expectations.json')

const cases: Case[] = JSON.parse(fs.readFileSync(corpusPath, 'utf8'))

function outcome(evaluator: RuleEvaluator, subject: Case): Expectation {
  try {
    return { id: subject.id, ok: true, value: evaluator.evaluate(subject.expr, subject.data) }
  } catch {
    return { id: subject.id, ok: false }
  }
}

describe('the JFSS parity corpus', () => {
  let evaluator: RuleEvaluator
  let produced: Expectation[]

  beforeAll(async () => {
    evaluator = await loadEvaluator()
    produced = cases.map((subject) => outcome(evaluator, subject))

    // Regenerating is opt-in and never happens in CI: an expectation file that
    // rewrote itself on every run would agree with whatever the engine did
    // today, which is not a gate.
    //
    // Signalled by Vite's `--mode` rather than by an environment variable,
    // because `PARITY_UPDATE=1 vitest` is a Unix-shell idiom and npm scripts
    // run through cmd.exe on Windows, where it is a syntax error. `npm run
    // parity:update` is the supported way in.
    if (import.meta.env.MODE === 'parity-update') {
      fs.writeFileSync(expectationsPath, `${JSON.stringify(produced, null, 1)}\n`)
    }
  })

  it('is not empty, because a gate over nothing passes', () => {
    expect(cases.length).toBeGreaterThan(50)
  })

  it('has an expectation for every case and no orphans', () => {
    const expectations: Expectation[] = JSON.parse(fs.readFileSync(expectationsPath, 'utf8'))

    expect(expectations.map((entry) => entry.id)).toEqual(cases.map((subject) => subject.id))
  })

  it('reproduces every committed expectation', () => {
    const expectations: Expectation[] = JSON.parse(fs.readFileSync(expectationsPath, 'utf8'))

    // One assertion over the whole list rather than a loop of 51: a diff of the
    // two arrays names every case that moved, where a loop stops at the first.
    expect(produced).toEqual(expectations)
  })
})
