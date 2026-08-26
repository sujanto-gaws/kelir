#!/usr/bin/env node
/**
 * D-10's bundle condition, as a build assertion (issue #163 AC4).
 *
 * **The decision bought a ~596 KB gzipped evaluator on one condition: it stays
 * off the first-load path.** `lib/jsonlogic.ts` reaches for it through a
 * dynamic `import()` so that Vite emits it as its own chunk, and a single
 * static import anywhere on that path — a store, the router, a shared helper —
 * folds it back into the entry with nothing failing and nothing said.
 *
 * **No unit test can see that.** A chunk graph is a property of the build, and
 * the module a test imports is reachable either way. So the condition is
 * checked here, against the manifest Vite writes, which is the form the [Sprint
 * 7 retrospective](../projects/retrospectives/05.%20Sprint%207%20Retrospective.md)
 * judged holds: the actions that survived were encoded in a standard or a test,
 * and the ones that failed were encoded in a retrospective.
 *
 *     cd kelir-frontend && npm run build
 *     node scripts/check-bundle-split.mjs
 *
 * **Two checks, and the second is the one that catches the real defect.**
 * Reachability alone came back green against a reintroduced static import, for
 * a reason worth recording: when the engine is folded into the entry it stops
 * being a chunk at all, so a check that walks the graph looking for it finds
 * nothing and reports success. Requiring the split chunk to *exist* is what
 * sees that. Reachability is kept for the other shape — a chunk that survives
 * and is imported statically anyway.
 *
 * The walk follows **static** imports only. `dynamicImports` is exactly the
 * edge the decision permits, so following it would make the check pass on the
 * build it exists to refuse.
 */

import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

/** The package whose payload D-10 priced. Matched on the manifest key. */
const ENGINE = '@goplasmatic/datalogic-wasm'

const root = dirname(dirname(fileURLToPath(import.meta.url)))
const manifestPath = join(root, 'kelir-frontend', 'dist', '.vite', 'manifest.json')

function die(message) {
  console.error(`\n✗ ${message}\n`)
  process.exit(1)
}

let manifest

try {
  manifest = JSON.parse(readFileSync(manifestPath, 'utf8'))
} catch (error) {
  die(
    `${manifestPath} is not readable: ${error.message}\n` +
      '  Run `npm run build` in kelir-frontend first — this reads the build, not the source.',
  )
}

const entries = Object.keys(manifest).filter((key) => manifest[key].isEntry)

if (entries.length === 0) {
  // A manifest with no entry would make everything below vacuous, which is the
  // failure mode a check like this has: green because it looked at nothing.
  die('the manifest declares no entry chunk, so nothing below was actually checked')
}

const fromEngine = Object.keys(manifest).filter((key) => key.includes(ENGINE))
const engineCode = fromEngine.filter((key) => key.endsWith('.js'))
const enginePayload = fromEngine.filter((key) => key.endsWith('.wasm'))

if (fromEngine.length === 0) {
  die(
    `no chunk or asset in the manifest comes from ${ENGINE}, so this check passed by not\n` +
      '  finding its subject. Either the dependency was removed — in which case decision\n' +
      '  D-10 changed and this script should change with it — or the manifest is stale.',
  )
}

const HOW_TO_FIX =
  '  D-10 accepted ~596 KB gzipped of WebAssembly against json-logic-js\'s 4.1 KB on\n' +
  '  the basis that only a page rendering a form pays it. Reach the evaluator through\n' +
  '  `loadEvaluator()` in src/lib/jsonlogic.ts, which is the one dynamic `import()`\n' +
  '  that keeps the chunk separate — never by importing the package directly.'

// --- 1. The engine has a chunk of its own ----------------------------------
//
// A static import does not merely add an edge; Rollup merges the module into
// whatever imported it, and the chunk disappears. That is what a reachability
// check alone cannot see, and it is the shape the mutation actually took.
if (engineCode.length === 0) {
  die(
    'the JSON Logic evaluator has no chunk of its own, which means a static import\n' +
      '  folded it into the chunk that imported it. That breaks the terms of D-10.\n\n' +
      `  The manifest still carries ${enginePayload.length} asset(s) from the package:\n` +
      enginePayload.map((key) => `    ${key}`).join('\n') +
      '\n\n' +
      HOW_TO_FIX,
  )
}

// --- 2. And nothing on the first-load path imports it statically ------------
const reached = new Set()
const queue = [...entries]

while (queue.length > 0) {
  const key = queue.shift()

  if (reached.has(key) || !manifest[key]) {
    continue
  }

  reached.add(key)
  // `imports` only. `dynamicImports` is the split D-10 paid for.
  queue.push(...(manifest[key].imports ?? []))
}

const onFirstLoad = engineCode.filter((key) => reached.has(key))

if (onFirstLoad.length > 0) {
  die(
    'the JSON Logic evaluator is on the first-load path, which breaks the terms of D-10.\n\n' +
      onFirstLoad.map((key) => `    ${key}  →  ${manifest[key].file}`).join('\n') +
      '\n\n' +
      HOW_TO_FIX,
  )
}

// --- 3. Nor does the entry carry its payload as an asset -------------------
//
// The `.wasm` binary is the 596 KB; the JavaScript wrapper beside it is under
// four. A build that split the wrapper and attached the binary to the entry
// would satisfy both checks above and none of the decision.
const entryAssets = new Set(entries.flatMap((key) => manifest[key].assets ?? []))
const payloadFiles = enginePayload.map((key) => manifest[key].file)
const eager = payloadFiles.filter((file) => entryAssets.has(file))

if (eager.length > 0) {
  die(
    'the evaluator\'s WebAssembly payload is attached to the entry chunk, so it is\n' +
      '  fetched before any route runs:\n\n' +
      eager.map((file) => `    ${file}`).join('\n') +
      '\n\n' +
      HOW_TO_FIX,
  )
}

console.log(
  `✓ the JSON Logic evaluator is off the first-load path — ` +
    `${engineCode.length} split chunk(s), ${enginePayload.length} payload asset(s), ` +
    `${reached.size} chunk(s) on first load`,
)
