#!/usr/bin/env bash
# Runs the whole JFSS operator-parity harness and prints the comparison.
# Needs Node 20+ and a Rust toolchain; `npm install` in node/ once beforehand.
set -euo pipefail
cd "$(dirname "$0")"

node node/run.mjs
(cd rust && cargo run --quiet)
node node/wasm.mjs
echo
node compare.mjs
echo
echo "=== Tamper-Proof Pattern under jsonlogic-rs (registry S6.1 invoice) ==="
(cd rust && cargo run --quiet --bin tamper)
echo
echo "=== Unregistered operators under jsonlogic-rs ==="
(cd rust && cargo run --quiet --bin typo)
echo
echo "=== Validation Rule Registry 'regex' rule: Rust regex crate ==="
(cd rust && cargo run --quiet --bin regexrule)
echo "=== the same patterns under ECMA-262 ==="
node node/regexrule.mjs
echo
echo "=== JFSS S3.3: a generated operator on the client engine ==="
node node/generated.mjs
