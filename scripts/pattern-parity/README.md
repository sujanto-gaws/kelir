# Pattern parity probe

Checks whether the browser and the server give the same verdict for a `both`-scoped pattern. The browser side is ECMA-262 `new RegExp(pattern, flags).test(value)`. The server side is the `regex` crate, set up as `rad::domain::validation::compile_pattern` sets it up. The figures in the [Validation Rule Registry](../../docs/schema/JFSS%20Validation%20Rule%20Registry.md)'s `regex` warning, 1.5.2 and 1.5.3, come from here ([#490](https://github.com/sujanto-gaws/kelir/pull/490), [#493](https://github.com/sujanto-gaws/kelir/issues/493), [#494](https://github.com/sujanto-gaws/kelir/issues/494)).

**This is a measurement, not a test.** Nothing in CI runs it. A zero here means no split was found among the patterns and values it drew, in the engines it ran. It does not mean the two sides agree.

## Build

```sh
cargo build --release          # the crate side; not a member of any workspace
```

`src/main.rs` copies `compile_pattern`'s builder instead of importing it, so the probe builds without the backend's database. **If `compile_pattern` changes, change the copy.** The `regex` version is pinned to the one `kelir-backend/Cargo.lock` uses.

## `diff.mjs`: grammar, in this node

| Figure (registry 1.5.2) | Command | Result on 2026-09-17, node v24.15.0 |
|---|---|---|
| Fixed probes, the tabulated constructs | `node diff.mjs targeted` | One `SPLIT` or `agree` line per construct |
| Subset, no flags | `node diff.mjs fuzz 20000 7 ""`, then `… 20000 11 ""` | 0 and 0 splits |
| Subset, wider alphabet | `WIDE=1 node diff.mjs fuzz 30000 29 ""`, then `… 30000 31 ""` | 0 and 0 |
| Positive control, negated classes allowed | `NEGATE=1 node diff.mjs fuzz 5000 3 ""` | 6 |
| Flag `i` | `node diff.mjs fuzz 5000 17 i` | 201 |
| Flag `u` | `node diff.mjs fuzz 5000 19 u` | 2,532 |
| Flags `g` and `d` | `node diff.mjs fuzz 10000 13 g`, then `… 10000 23 d` | 0 and 0 |

**The generator's values are at most six characters, and every pattern is decided by V8 alone.** That is why its zeros say nothing about backtracking. [Record 17](../../projects/verifications/17.%20Sprint%2019%20and%20Sprint%2017%20Independent%20Pass.md) §7 widened the measurement with a generator of its own, kept in that session's scratchpad and not here.

## `backtrack.mjs`: time, in three browsers

Needs the e2e harness's Playwright browsers (`npm ci` and `npx playwright install` in `e2e/`).

```sh
node backtrack.mjs                      # chromium, firefox and webkit
TIMEOUT=60000 node backtrack.mjs firefox
```

Each case gets a fresh page. A match running past `TIMEOUT` is reported as `TIMEOUT`, and the browser is relaunched, because a page stuck in a match cannot be interrupted. **An exception counts as a violation**, as it does in the renderer. It produces the registry 1.5.3 table, run 2026-09-17 in Chromium 151, Firefox 153 and WebKit 26.5. Timings are one run on one machine: read them as orders of magnitude.
