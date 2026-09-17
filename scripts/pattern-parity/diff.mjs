// Two-engine differential probe: ECMA-262 as this node runs it, against the
// `regex` crate as `rad::domain::validation::compile_pattern` builds it.
//
// The figures in Validation Rule Registry 1.5.2 and 1.5.3 come from this file
// (#490, #494). Build the crate side first with `cargo build --release` in
// this directory; README.md lists each figure's command.
//
// Usage:
//   node diff.mjs targeted
//   [WIDE=1] [NEGATE=1] node diff.mjs fuzz <patterns> <seed> <flags>
//
// **Every character outside printable ASCII is a `\u` escape.** The copy
// posted in #490's description lost two of them (U+0085 and U+0000), and a
// run of it gave 5 and 198 where the registry says 6 and 201 (#494).
import { execFileSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const BIN = fileURLToPath(
  new URL(`./target/release/pattern-parity${process.platform === 'win32' ? '.exe' : ''}`, import.meta.url),
)

function ecma(p, f, s) {
  try { return new RegExp(p, f).test(s) ? '1' : '0' } catch { return 'E' }
}
function crate(cases) {
  const input = cases.map((c) => JSON.stringify(c)).join('\n') + '\n'
  return execFileSync(BIN, { input, maxBuffer: 1 << 28 }).toString().trim().split('\n')
}

const VALUES = ['', 'a', 'abc', 'xabc', 'A', 'k', 'K', 's', 'S', '\u212a', '\u017f', '\u{1F600}', '\u{1F600}\u{1F600}',
  '\u0663', '1234', '\u0661\u0662\u0663\u0664', '\n', '\r', '\r\n', 'a\r\nb', '\u2028', '\u0085', '\u00e9', ',', '-', ']', '[',
  '{', '}', 'a{,3}', 'aaa', 'a{1, 3}', '\t', ' ', '\u0000', '\\', '/', '&', '~', 'b', 'z', '_', '.']

// ---- targeted ---------------------------------------------------------------
const TARGETED = [
  // record 17's rows
  ['^[^,]{1,3}$', ''], ['^[^a]$', ''], ['abc', 'y'], ['abc', 'q'], ['^[a-z]+$', 'i'], ['^[0-9]{4}$', ''],
  // suspected, not in the registry's table
  ['[]a]', ''], ['[^]', ''], ['^a{,3}$', ''], ['a{,3}', ''], ['[a~~b]', ''], ['[a&&b]', ''], ['[a--b]', ''],
  ['a{1, 3}', ''], ['a{2}{3}', ''], ['a**', ''], ['()', ''], ['(?:)', ''], ['a{3,2}', ''], ['\\0', ''],
  ['\\/', ''], ['\\-', ''], ['\\ ', ''], ['\\#', ''], ['\\&', ''], ['\\~', ''], ['\\e', ''], ['\\cA', ''], ['[\\b]', ''],
  ['\\x41', ''], ['\\u0041', ''], ['\\t', ''], ['\\v', ''], ['\\f', ''], ['^*', ''], ['$+', ''], ['', ''],
  ['a|', ''], ['|', ''], ['[a-]', ''], ['[-a]', ''], ['{', ''], ['a{', ''], ['}', ''], [']', ''], ['a{2,3}?', ''],
  ['[a-z]', ''], ['[A-z]', ''], ['[\\]]', ''], ['[\\-]', ''], ['[\\\\]', ''], ['[\\^]', ''], ['[a^]', ''],
  ['[&]', ''], ['[&&]', ''], ['[~]', ''], ['[-]', ''], ['[a-c-e]', ''], ['[--]', ''], ['[a\\-z]', ''],
  ['^abc$', 'g'], ['^abc$', 'd'], ['^abc$', 'u'], ['^abc$', 'v'], ['^abc$', 'ii'], ['^abc$', 'gi'], ['abc', 's'],
  ['^[^,]$', 'u'], ['^.$', 'u'], ['^[a-j]$', 'i'], ['^[l-r]$', 'i'], ['^[t-z]$', 'i'], ['^abc$', 'i'], ['^k$', 'i'],
  ['^s$', 'i'], ['^[0-9]$', 'i'],
]

function targeted() {
  const cases = []
  for (const [p, f] of TARGETED) for (const v of VALUES) cases.push([p, f, v])
  const out = crate(cases)
  const byPattern = new Map()
  cases.forEach(([p, f, v], i) => {
    const e = ecma(p, f, v)
    if (e !== out[i]) {
      const key = `${JSON.stringify(p)} flags=${JSON.stringify(f)}`
      if (!byPattern.has(key)) byPattern.set(key, [])
      byPattern.get(key).push(`${JSON.stringify(v)} ecma=${e} crate=${out[i]}`)
    }
  })
  for (const [p, f] of TARGETED) {
    const key = `${JSON.stringify(p)} flags=${JSON.stringify(f)}`
    const d = byPattern.get(key)
    console.log(d ? `SPLIT  ${key}\n         ${d.slice(0, 3).join('\n         ')}${d.length > 3 ? ` (+${d.length - 3})` : ''}` : `agree  ${key}`)
  }
}

// ---- fuzz over the candidate subset -----------------------------------------
let seed = 1
function rnd(n) { seed = (seed + 0x6d2b79f5) | 0; let t = Math.imul(seed ^ (seed >>> 15), 1 | seed); t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t; return (((t ^ (t >>> 14)) >>> 0) % n) }
function pick(a) { return a[rnd(a.length)] }

const LIT = (process.env.WIDE ? 'abcksxyzABKSZ019 _,:;@/\'"#%=!<>&~-]}`' : 'abcksxyzABKSZ019 _,:;@/\'"#%=!<>').split('')
const ESC = ['\\.', '\\+', '\\*', '\\?', '\\(', '\\)', '\\[', '\\]', '\\{', '\\}', '\\|', '\\^', '\\$', '\\\\', '\\/', '\\-']
const CLASS_MEMBER = ['a', 'b', 'k', 's', 'z', 'A', 'K', 'S', 'Z', '0', '9', '_', ',', '.', ' ', 'a-z', 'A-Z', '0-9', 'a-f', 'j-t', '\\-', '\\]', '\\\\', '\\^',
  ...(process.env.WIDE ? ['!-/', ' -~', ':-@', '&', '~', '}', '{', ')', '(', '|', '*', '+', '?', '$', '/', '\\[', 'A-z'] : [])]

function klass() {
  let m = ''
  const n = 1 + rnd(3)
  for (let i = 0; i < n; i++) m += pick(CLASS_MEMBER)
  if (rnd(4) === 0) m = m + '-' // trailing hyphen
  return `[${process.env.NEGATE && rnd(3) === 0 ? '^' : ''}${m}]`
}
function atom(depth) {
  const r = rnd(10)
  if (r < 4) return pick(LIT)
  if (r < 5) return pick(ESC)
  if (r < 8) return klass()
  if (depth > 2) return pick(LIT)
  return (rnd(2) ? '(' : '(?:') + alt(depth + 1) + ')'
}
function quant() {
  const r = rnd(12)
  const q = ['', '', '', '', '*', '+', '?', `{${rnd(3)}}`, `{${rnd(3)},}`, `{${rnd(2)},${2 + rnd(2)}}`, '', ''][r]
  return q && rnd(4) === 0 ? q + '?' : q
}
function seq(depth) {
  let s = ''
  const n = 1 + rnd(4)
  for (let i = 0; i < n; i++) s += atom(depth) + quant()
  return s
}
function alt(depth) { return rnd(4) === 0 ? seq(depth) + '|' + seq(depth) : seq(depth) }
function pattern() { return (rnd(2) ? '^' : '') + alt(0) + (rnd(2) ? '$' : '') }

const ALPHA = ['a', 'b', 'c', 'k', 's', 'x', 'z', 'A', 'K', 'S', 'Z', '0', '1', '9', ' ', '_', ',', '.', '-', ']', '[', '\\', '^', '$',
  '/', '\n', '\r', '\u2028', '\u0085', '\u00e9', '\u212a', '\u017f', '\u{1F600}', '\u0663', '\t', '\u0000', '{', '}', '(', ')', '|', '*', '+', '?']
function value() { let v = ''; const n = rnd(7); for (let i = 0; i < n; i++) v += pick(ALPHA); return v }

function fuzz(n, s, flags) {
  seed = s
  const cases = []
  for (let i = 0; i < n; i++) {
    const p = pattern()
    for (let j = 0; j < 12; j++) cases.push([p, flags, value()])
  }
  for (let i = 0; i < 4; i++) console.log('sample', JSON.stringify(cases[i * 12]))
  const out = crate(cases)
  let splits = 0, errs = 0
  const seen = new Set()
  cases.forEach(([p, f, v], i) => {
    const e = ecma(p, f, v)
    if (e === 'E' || out[i] === 'E') errs++
    if (e !== out[i]) {
      splits++
      if (!seen.has(p) && seen.size < 15) { seen.add(p); console.log(`SPLIT ${JSON.stringify(p)} flags=${JSON.stringify(f)} value=${JSON.stringify(v)} ecma=${e} crate=${out[i]}`) }
    }
  })
  console.log(`patterns=${n} cases=${cases.length} flags=${JSON.stringify(flags)} splits=${splits} compile-errors=${errs}`)
}

const [mode, a, b, c] = process.argv.slice(2)
if (mode === 'targeted') targeted()
else fuzz(Number(a), Number(b), c ?? '')
