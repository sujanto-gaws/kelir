// What the three browser engines do when a match backtracks for a long time,
// against the `regex` crate's verdict (#493, Validation Rule Registry 1.5.3).
//
// Needs the crate side built (`cargo build --release` here) and the e2e
// harness's Playwright browsers (`npm ci` and `npx playwright install` in
// e2e/). Each case runs in a fresh page. A case that runs past TIMEOUT
// milliseconds (default 20000) is reported as TIMEOUT and its browser is
// relaunched, because a page stuck in a match cannot be interrupted.
//
// Usage: [TIMEOUT=ms] node backtrack.mjs [chromium] [firefox] [webkit]
import { createRequire } from 'node:module'
import { execFileSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const require = createRequire(new URL('../../e2e/package.json', import.meta.url))
const pw = require('playwright')
const BIN = fileURLToPath(
  new URL(`./target/release/pattern-parity${process.platform === 'win32' ? '.exe' : ''}`, import.meta.url),
)

const A = (n, tail = '!') => 'a'.repeat(n) + tail
const CASES = [
  // [label, pattern, value-builder, lengths]
  ['overlapping alternatives', '(?:[a-z]|[a-z0-9])*$', A, [10, 20, 24, 28, 32]],
  ['anchored overlapping alternatives', '^(?:[a-z]|[a-z0-9])*$', A, [10, 20, 24, 28, 32]],
  ['nested quantifiers', '(a+)+$', A, [10, 20, 24, 28, 32]],
  ['nested quantifiers, anchored', '^(a+)+$', A, [10, 20, 24, 28, 32]],
  ['quantified group, one bounded quantifier inside', '^(?:a?){2,3}$', A, [10, 1000]],
  ['two adjacent stars, unanchored', '[a-z]*[a-z]*$', A, [1000, 10000, 50000]],
  ['three adjacent stars, anchored', '^[a-z]*[a-z]*[a-z]*$', A, [1000, 3000, 10000]],
  ['star then plus on one class, anchored', '^[a-z]*[a-z]+$', A, [1000, 10000, 50000]],
  ['control: one star', '[a-z]*$', A, [1000, 100000]],
  ['control: quantified group, no | and no inner quantifier', '^(?:[a-z][0-9])*$', (n) => 'a1'.repeat(n) + '!', [1000, 50000]],
  ['control: quantified group of literals', '^(?:ab)+$', (n) => 'ab'.repeat(n) + '!', [1000, 50000]],
]

const TIMEOUT = Number(process.env.TIMEOUT ?? 20000)

function crate(cases) {
  const input = cases.map((c) => JSON.stringify([c.pattern, '', c.value])).join('\n') + '\n'
  return execFileSync(BIN, { input, maxBuffer: 1 << 28 }).toString().trim().split('\n')
}

const flat = []
for (const [label, pattern, build, lengths] of CASES)
  for (const n of lengths) flat.push({ label, pattern, n, value: build(n) })
const crateOut = crate(flat)
flat.forEach((c, i) => (c.crate = crateOut[i]))

const engines = process.argv.slice(2).length ? process.argv.slice(2) : ['chromium', 'firefox', 'webkit']
for (const name of engines) {
  let browser = await pw[name].launch()
  console.log(`\n== ${name} ${browser.version()}`)
  for (const c of flat) {
    const page = await browser.newPage()
    const run = page.evaluate(([p, v]) => {
      const t = performance.now()
      try {
        const r = new RegExp(p).test(v)
        return { r: r ? '1' : '0', ms: performance.now() - t }
      } catch (e) {
        return { r: 'E', ms: performance.now() - t, err: String(e).slice(0, 60) }
      }
    }, [c.pattern, c.value])
    const timed = await Promise.race([run, new Promise((res) => setTimeout(() => res(null), TIMEOUT))])
    if (timed === null) {
      console.log(`  TIMEOUT>${TIMEOUT / 1000}s  n=${c.n}  ${c.label}  ${c.pattern}  crate=${c.crate}`)
      await browser.close().catch(() => {})
      browser = await pw[name].launch()
      continue
    }
    // Since #496 the renderer leaves a match that throws undecided, and the
    // server decides it, so a throw is reported as that rather than as a split.
    const verdict = timed.r === 'E' ? 'E (renderer: undecided, the server decides)' : timed.r
    const split = timed.r === 'E' ? 'UNDECIDED' : timed.r !== c.crate ? 'SPLIT' : 'agree'
    console.log(`  ${split}  ${timed.ms.toFixed(0).padStart(6)} ms  n=${c.n}  ${c.label}  ${c.pattern}  browser=${verdict}${timed.err ? ' ' + timed.err : ''}  crate=${c.crate}`)
    await page.close().catch(() => {})
  }
  await browser.close().catch(() => {})
}
