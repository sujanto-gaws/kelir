// Evaluates the shared corpus with json-logic-js, the library JFSS S9.1 names
// for the Vue side. Registers `sum` exactly as Calculation Rule Registry S3.2
// requires ("must register custom operator via jsonLogic.add_operation").
import fs from 'node:fs';
import jsonLogic from 'json-logic-js';

// Paths resolve against this file, not the shell's working directory.
const ROOT = new URL('../', import.meta.url);
const here = (name) => new URL(name, ROOT);

const cases = JSON.parse(fs.readFileSync(here('cases.json'), 'utf8'));

const encode = (v) => {
  if (typeof v === 'number' && !Number.isFinite(v)) return Number.isNaN(v) ? '#NaN' : v > 0 ? '#Infinity' : '#-Infinity';
  if (Array.isArray(v)) return v.map(encode);
  if (v && typeof v === 'object') return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, encode(x)]));
  return v;
};

jsonLogic.add_operation('sum', (arr) =>
  Array.isArray(arr) ? arr.reduce((a, v) => a + (Number(v) || 0), 0) : 0,
);

const results = cases.map((c) => {
  try {
    // JSON.stringify turns Infinity and NaN into null, which would hide
    // exactly the values the S7.3 normalization exists to catch.
    const raw = encode(jsonLogic.apply(c.expr, c.data));
    return { id: c.id, ok: true, raw };
  } catch (error) {
    return { id: c.id, ok: false, error: String(error.message || error) };
  }
});

fs.writeFileSync(here('results-js.json'), JSON.stringify(results, null, 1));
console.log(`json-logic-js ${JSON.parse(fs.readFileSync(new URL('node/node_modules/json-logic-js/package.json', ROOT),'utf8')).version}: ${results.length} cases, ${results.filter(r=>!r.ok).length} threw`);
