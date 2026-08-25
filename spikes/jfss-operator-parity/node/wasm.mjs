// The same corpus through @goplasmatic/datalogic-wasm — the browser/Node build
// of the very engine `datalogic-rs` compiles on the server.
import fs from 'node:fs';
import { createRequire } from 'node:module';
const require = createRequire(import.meta.url);
const { Engine } = require('@goplasmatic/datalogic-wasm');

// Paths resolve against this file, not the shell's working directory.
const ROOT = new URL('../', import.meta.url);
// The corpus lives in parity/ since Sprint 7 promoted it into a CI gate (#154).
const cases = JSON.parse(fs.readFileSync(new URL('../../parity/cases.json', ROOT), 'utf8'));

const sum = (argsJson) => {
  const args = JSON.parse(argsJson);
  const arr = Array.isArray(args[0]) ? args[0] : [];
  return JSON.stringify(arr.reduce((a, v) => a + (Number(v) || 0), 0));
};

const config = {
  arithmetic_nan_handling: 'coerce_to_zero',
  division_by_zero: 'return_null',
  loose_equality_errors: false,
  numeric_coercion: { null_to_zero: true, empty_string_to_zero: true, bool_to_number: true, reject_non_numeric: false },
};

const engines = {
  wasm_stock: new Engine({ customOperators: { sum } }),
  wasm_tuned: new Engine({ customOperators: { sum }, config }),
};

const results = cases.map((c) => {
  const row = { id: c.id };
  for (const [name, engine] of Object.entries(engines)) {
    try {
      row[name] = { ok: true, raw: JSON.parse(engine.evalStr(JSON.stringify(c.expr), JSON.stringify(c.data))) };
    } catch (error) {
      row[name] = { ok: false, error: String(error.message || error) };
    }
  }
  return row;
});

fs.writeFileSync(new URL('results-wasm.json', ROOT), JSON.stringify(results, null, 1));
console.log(`datalogic-wasm: ${results.length} cases`);
