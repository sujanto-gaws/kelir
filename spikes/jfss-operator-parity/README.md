# JFSS operator-parity harness

Evidence for the spike recorded in
[projects/spikes/01. JFSS Operator Parity.md](../../projects/spikes/01.%20JFSS%20Operator%20Parity.md)
(issue #31). Read the finding for what the numbers mean; this directory is only
how they were produced.

**This is spike scaffolding, not production code.** Nothing here is wired into
CI, imported by the backend, or shipped in the frontend bundle. It exists so the
finding can be re-run rather than believed. Promoting the corpus into a real
parity gate is Sprint 7 work, and depends on which evaluator is adopted.

## What it does

`cases.json` is one corpus of 51 JSON Logic expressions, each derived from a
claim the [Calculation Rule Registry](../../docs/schema/JFSS%20Calculation%20Rule%20Registry.md)
or [JFSS](../../docs/schema/JSON%20Form%20Schema.md) makes — the base operators,
the array operators §2.1 says must be verified in CI, the normalizations §3.1
and §7.3 mandate, the §6.1 invoice pattern, and the boolean operators
`conditional.logic` needs but the registry never registered.

Every case is run through four evaluators and compared:

| Column | What it is |
|---|---|
| `json-logic-js` 2.0.5 | the frontend library JFSS §9.1 names — the reference |
| `jsonlogic-rs` 0.5.0 | the crate published from `bestowinc/json-logic-rs`, which the registry calls "json-logic-rs" |
| `datalogic-rs` 5.2.0 (stock) | the other maintained Rust implementation, default configuration |
| `datalogic-rs` 5.2.0 (tuned) | the same, configured to the registry's mandated normalizations |

`node/wasm.mjs` adds a fifth run through `@goplasmatic/datalogic-wasm` — the same
engine as `datalogic-rs`, compiled to WebAssembly — so the two can be compared to
each other rather than only to `json-logic-js`.

`sum` is registered as a custom operator on every engine that allows it, because
Calculation Rule Registry §3.2 requires exactly that in all environments.

## Running it

```bash
cd node && npm install && cd ..
./run.sh
```

Needs Node 20+ and a Rust toolchain. `run.sh` also prints four side
demonstrations the finding cites: the Tamper-Proof Pattern under `jsonlogic-rs`,
what that crate does with an unregistered operator, the Validation Rule
Registry's `regex` rule under the Rust `regex` crate against ECMA-262, and
whether a client engine will evaluate a `calculateMode: "generated"` operator.

Results land in `results-*.json` and `comparison.json`, which are gitignored —
a committed copy would go stale without anyone noticing, which is the failure
mode this spike exists to find.

## Reading the comparison

| Mark | Meaning |
|---|---|
| `=` | the engine returned exactly what `json-logic-js` returned |
| `~` | it differs raw, but agrees once the §7.3 normalization wrapper is applied |
| `X` | it still differs after the wrapper, or one side threw and the other did not |

`json-logic-js` results are encoded before serialization: `JSON.stringify` turns
`Infinity` and `NaN` into `null`, which would hide precisely the values the
normalization rule exists to catch, so they are written as `"#Infinity"` and
`"#NaN"` instead.
