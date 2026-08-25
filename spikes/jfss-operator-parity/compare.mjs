import fs from 'node:fs';

// Paths resolve against this file, not the shell's working directory.
const here = (name) => new URL(name, import.meta.url);

// The corpus moved to parity/ when Sprint 7 promoted it into a CI gate (#154).
// This harness reads it from there rather than keeping a second copy: two
// corpora that drift is the failure the promotion was meant to end.
const cases = JSON.parse(fs.readFileSync(here('../../parity/cases.json'), 'utf8'));
const js = JSON.parse(fs.readFileSync(here('results-js.json'), 'utf8'));
const rust = JSON.parse(fs.readFileSync(here('results-rust.json'), 'utf8'));

const byId = (list) => Object.fromEntries(list.map((r) => [r.id, r]));
const J = byId(js);
const R = byId(rust);

// The Calculation Rule Registry S7.3 wrapper: Number(result) || 0.
// json-logic-js encodes non-finite results as '#NaN' / '#Infinity'; decode
// before applying the wrapper so the wrapper sees what the engine saw.
const decode = (v) => (v === '#NaN' ? NaN : v === '#Infinity' ? Infinity : v === '#-Infinity' ? -Infinity : v);
const wrap = (o) => (o.ok ? Number(decode(o.raw)) || 0 : 'THREW');

const show = (o) => (o.ok ? JSON.stringify(o.raw) : `ERR(${String(o.error).split('\n')[0].slice(0, 46)})`);
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);

const COLS = ['jsonlogic_rs', 'datalogic_stock', 'datalogic_tuned'];
const tally = Object.fromEntries(COLS.map((c) => [c, { raw: 0, wrapped: 0 }]));
const rows = [];

for (const c of cases) {
  const j = J[c.id];
  const r = R[c.id];
  const numeric = c.tier !== 'conditional';
  const row = { id: c.id, tier: c.tier, js: show(j) };
  for (const col of COLS) {
    const o = r[col];
    row[col] = show(o);
    const rawMatch = j.ok && o.ok ? same(j.raw, o.raw) : j.ok === o.ok;
    const wrapMatch = numeric ? same(wrap(j), wrap(o)) : rawMatch;
    if (rawMatch) tally[col].raw++;
    if (wrapMatch) tally[col].wrapped++;
    row[`${col}_v`] = rawMatch ? '=' : wrapMatch ? '~' : 'X';
  }
  rows.push(row);
}

const pad = (s, n) => String(s).padEnd(n);
const w = { id: 20, tier: 14, js: 26, col: 34 };
console.log(
  pad('case', w.id) + pad('tier', w.tier) + pad('json-logic-js', w.js) +
  pad('jsonlogic-rs', w.col) + pad('datalogic (stock)', w.col) + 'datalogic (tuned)',
);
console.log('-'.repeat(160));
for (const row of rows) {
  console.log(
    pad(row.id, w.id) + pad(row.tier, w.tier) + pad(row.js, w.js) +
    pad(`${row.jsonlogic_rs_v} ${row.jsonlogic_rs}`, w.col) +
    pad(`${row.datalogic_stock_v} ${row.datalogic_stock}`, w.col) +
    `${row.datalogic_tuned_v} ${row.datalogic_tuned}`,
  );
}
console.log('\nAgainst json-logic-js 2.0.5, over ' + cases.length + ' cases:');
for (const col of COLS) {
  console.log(
    `  ${pad(col, 18)} raw agreement ${tally[col].raw}/${cases.length}` +
    `   after the S7.3 wrapper ${tally[col].wrapped}/${cases.length}`,
  );
}
console.log('\n= raw agreement   ~ agrees only after the mandated wrapper   X divergence');
// The decisive number: the same engine compiled for two runtimes. Only prints
// once node/wasm.mjs has been run.
if (fs.existsSync(here('results-wasm.json'))) {
  const W = byId(JSON.parse(fs.readFileSync(here('results-wasm.json'), 'utf8')));
  const key = (o) => (o.ok ? ['ok', JSON.stringify(o.raw)] : ['err']);
  for (const [rustCol, wasmCol] of [['datalogic_stock', 'wasm_stock'], ['datalogic_tuned', 'wasm_tuned']]) {
    const identical = cases.filter((c) => same(key(R[c.id][rustCol]), key(W[c.id][wasmCol]))).length;
    console.log(
      `datalogic-rs (${rustCol.replace('datalogic_', '')}) vs @goplasmatic/datalogic-wasm: ` +
        `${identical}/${cases.length} identical, error cases included`,
    );
  }
}

fs.writeFileSync(here('comparison.json'), JSON.stringify({ tally, rows }, null, 1));
