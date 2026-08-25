# JFSS evaluator parity gate

The corpus that holds the two halves of decision **D-10** to the same answers,
and the committed expectations they are both held to. Built as Sprint 7 item 2
(issue #154) by promoting the [operator-parity spike](../projects/spikes/01.%20JFSS%20Operator%20Parity.md)'s
one-off harness into something CI runs.

## What the gate is for

JFSS S8.1 makes the backend re-evaluate every `calculate` expression and
overwrite the submitted value before persistence; S10.2 does the same for
`conditional`. That is the Tamper-Proof Pattern, and it is only safe if the two
sides compute the same thing — otherwise re-evaluation quietly *changes* correct
figures instead of catching tampered ones.

**D-10 bought that property by construction rather than by testing for it.** The
backend's `datalogic-rs` and the frontend's `@goplasmatic/datalogic-wasm` are one
Rust core compiled for two runtimes, pinned to the **same exact version** on both
sides. This directory is what makes a drift between them loud.

| Side | Package | Version | Held by |
|---|---|---|---|
| Backend | `datalogic-rs` (Apache-2.0) | `=5.2.0` | `kelir-backend/tests/jsonlogic_parity.rs` |
| Frontend | `@goplasmatic/datalogic-wasm` (Apache-2.0) | `5.2.0` | `kelir-frontend/src/lib/jsonlogic.parity.spec.ts` |

Both replaced `json-logic-js` (MIT) on the client and the never-published
`json-logic-rs` on the server; the licence is Apache-2.0 on both sides now.

## The files

| File | What it is |
|---|---|
| `cases.json` | The corpus. 55 expressions, each derived from a claim the [Calculation Rule Registry](../docs/schema/JFSS%20Calculation%20Rule%20Registry.md) or [JFSS](../docs/schema/JSON%20Form%20Schema.md) makes |
| `expectations.json` | What the adopted engine answers for each, `{id, ok, value}`. Generated from the frontend side; asserted by both |

## How it runs

Nothing here is a separate CI job. Each side asserts the corpus in the test job
it already has, so a divergence fails in the half that caused it:

```bash
cd kelir-backend && cargo test --test jsonlogic_parity
cd kelir-frontend && npm test
```

After a **deliberate** engine change, regenerate and bump both sides:

```bash
cd kelir-frontend && npm run parity:update
```

Expect the backend test to fail until its pin is moved to match. That is what a
parity-affecting change is supposed to look like.

## What is compared, and what is not

- **Compared:** whether an expression produced a value at all, and which value.
  Numbers compare numerically — `serde_json` distinguishes `0` from `0.0` and
  JavaScript has one number type, so comparing the encodings would report a
  divergence on every whole-numbered result.
- **Not compared:** error *messages*. One side words them in Rust's `Debug` and
  the other in a JS `Error`; there is no shared vocabulary to hold them to. The
  spike's "51/51 including the error cases" meant the same thing.

## The configuration both sides build

Listed here because it is the one thing that is written twice — once in
`modules/rad/evaluator.rs` and once in `lib/jsonlogic.ts` — so a change to one
is visibly a change to both. It expresses the registry's mandated
normalizations as engine configuration rather than as a wrapper each
environment hand-writes, which is how two environments end up subtly different.

| Setting | Value | Why |
|---|---|---|
| `arithmetic_nan_handling` | coerce to zero | §7.3: a non-numeric operand yields 0, not NaN |
| `division_by_zero` | return null | §3.1, as close as the engine gets; the numeric wrapper turns the null into 0 |
| `loose_equality_errors` | false | The reference implementation compares across types silently |
| `numeric_coercion` | null, empty string and bool coerce; non-numeric not rejected | A half-filled form is normal, not an error |

## Two things the corpus is honest about

**It is 55 cases, not the spike's 51.** Four were added on 2026-08-25 by #154
and are marked `"added": "#154"` in the file. They exist because a mutation
came back green: flipping `arithmetic_nan_handling` to `throw` changed nothing
the original 51 could see, since every non-numeric operand in them was `null`,
`""` or a bool — all of which `numeric_coercion` handles first. The four new
cases put an array, an object and a non-numeric string in front of `+` and `*`,
and the same mutation now fails on all four. The spike finding's numbers refer
to the original 51 and are not re-derivable from this file.

**Agreement here is not agreement with `json-logic-js`.** Against the JFSS
reference the adopted engine agrees on 44/51 raw and 46/51 after the mandated
wrapper; the residual divergences are all cases where `json-logic-js` produces
`NaN` or `Infinity`, and the spike §2.3–§2.4 found the registry's own text
defective in the same place. What this gate asserts is that Kelir's two sides
agree with **each other**. Correcting the normalization spec is renderer work
and belongs to the rule engines under **D-15**.
