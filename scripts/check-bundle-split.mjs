#!/usr/bin/env node
/**
 * Three libraries' bundle conditions, as a build assertion (issue #163 AC4; #447
 * AC4; #687).
 *
 * **D-10 bought a 588 KB gzipped evaluator on one condition: it stays off the
 * first-load path.** `lib/jsonlogic.ts` reaches for it through a dynamic
 * `import()` so that Vite emits it as its own chunk, and a single static import
 * anywhere on that path — a store, the router, a shared helper — folds it back
 * into the entry with nothing failing and nothing said.
 *
 * **#447 took Unovis on the same condition, and the dashboard is the harder
 * place to keep it.** Unovis draws with d3, and the dashboard is the home
 * route: every session that signs in renders it. `DashboardPage.vue` reaches
 * the chart through `defineAsyncComponent`, and a static `import` of
 * `@unovis/vue` on the page, the layout or anything they share puts d3 in front
 * of every sign-in the same silent way.
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
 * **Three checks per library, and the first is the one that caught the real
 * defect.** Reachability alone came back green against a reintroduced static
 * import, for a reason worth recording: when the engine is folded into the
 * entry it stops being a chunk of its own, so a check that walks the graph
 * looking for *its chunk* finds nothing and reports success. Requiring a chunk
 * off the first-load path to *carry* it is what sees that. Reachability is kept
 * for the other shape — a chunk that survives and is imported statically
 * anyway — and the third check for a payload riding along as an asset.
 *
 * # A chunk is identified by what it carries, not by its manifest key
 *
 * The first version matched manifest keys on the package name. That works for
 * the evaluator, whose chunk is split *at* the package, and **cannot work for
 * Unovis**: it is reached through a component, so its code sits in
 * `DocumentStatusChart.vue`'s chunk and no key names it. So `vite.config.ts`
 * writes `.vite/chunk-packages.json` — the packages whose code each chunk
 * renders — and every check below reads that, for every library alike.
 *
 * # The workflow graph, and the stylesheets a chunk carries (#687)
 *
 * **ADR-0046 took Vue Flow and dagre for a read-only graph on the same terms.**
 * `WorkflowEditorPage.vue` reaches `WorkflowGraph.vue` through
 * `defineAsyncComponent`, and nothing else imports `@vue-flow/*` or
 * `@dagrejs/*`. **`@vueuse/core` is in the subject on purpose**: it arrives as
 * Vue Flow's own dependency, Kelir code does not import it (the lint rule
 * says so), and a build that put it on first load for any reason is the
 * record being reversed. Vue Flow 1.48.2's ESM build inlines the helpers it
 * takes from `@vueuse/core` and d3, so today no chunk names either package;
 * listing it is what catches a direct import that slipped past the lint.
 *
 * **Vue Flow ships two stylesheets, and the third check could not see them.**
 * It read a first-load chunk's `assets`, and an imported stylesheet is not an
 * asset: its rules are folded into the CSS file of the chunk that imported it,
 * which the manifest lists under `css`. So a `import '@vue-flow/core/dist/style.css'`
 * in `main.ts` passed. `vite.config.ts` now writes
 * `.vite/stylesheet-packages.json`, which packages' stylesheets went into each
 * CSS file, and the third check reads every first-load chunk's `css` against
 * it. **A subject that ships a stylesheet must be seen in one**, so a build
 * that stopped writing the map fails rather than checking nothing.
 *
 * # What *first load* is, and why the walk starts at three chunks
 *
 * The walk used to start at the entry alone, and for the evaluator that was
 * enough. **For the dashboard it is not.** The router lazy-loads every page,
 * `/` included, so `AppLayout.vue` and `DashboardPage.vue` are dynamic imports
 * of the entry — and a walk that begins there calls anything they import
 * statically *off* first load, while every sign-in fetches it. That was
 * measured rather than argued: with the entry as the only root, a static
 * `@unovis/vue` import in `DashboardPage.vue` came back green (2026-09-14). The
 * two chunks the home route renders are roots too, and their absence from the
 * manifest is a failure rather than a silent narrowing.
 *
 * The walk follows **static** imports only. `dynamicImports` is exactly the
 * edge both decisions permit, so following it would make the check pass on the
 * build it exists to refuse.
 */

import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

/**
 * What each decision keeps off first load. `packages` is matched against
 * `chunk-packages.json`, so a library is identified by code that shipped.
 */
const SUBJECTS = [
  {
    name: 'the JSON Logic evaluator',
    packages: ['@goplasmatic/datalogic-wasm'],
    howToFix:
      "  D-10 accepted 588 KB gzipped of WebAssembly against json-logic-js's 4.1 KB on\n" +
      '  the basis that only a page rendering a form pays it. Reach the evaluator through\n' +
      '  `loadEvaluator()` in src/lib/jsonlogic.ts, which is the one dynamic `import()`\n' +
      '  that keeps the chunk separate — never by importing the package directly.',
  },
  {
    name: 'the Unovis chart library',
    packages: ['@unovis/ts', '@unovis/vue'],
    howToFix:
      '  #447 took Unovis, and the d3 it draws with, on the basis that only the chart pays\n' +
      '  for it — and the chart is on the home route. Reach it through\n' +
      '  src/features/dashboard/DocumentStatusChart.vue, which DashboardPage.vue loads with\n' +
      '  `defineAsyncComponent` — never by importing `@unovis/*` on a page or layout.',
  },
  {
    name: 'the workflow graph (Vue Flow and dagre)',
    packages: [
      '@vue-flow/core',
      '@vue-flow/controls',
      '@dagrejs/dagre',
      '@dagrejs/graphlib',
      '@vueuse/core',
    ],
    // Vue Flow cannot draw without its stylesheets, so a build with none
    // carrying them is a map that stopped being written, not a clean build.
    stylesheet: true,
    howToFix:
      '  ADR-0046 took Vue Flow, dagre and the @vueuse/core Vue Flow brings on the basis\n' +
      '  that only the workflow graph pays for them. Reach them through\n' +
      '  src/features/workflow-builder/WorkflowGraph.vue, which WorkflowEditorPage.vue loads\n' +
      "  with `defineAsyncComponent`, and import Vue Flow's stylesheets there — never\n" +
      '  `@vue-flow/*`, `@dagrejs/*`, `@vueuse/core` or their CSS on a page, layout or main.ts.',
  },
]

/** The chunks `/` renders besides the entry. Router-lazy, and fetched by every sign-in. */
const HOME_ROUTE = ['src/layouts/AppLayout.vue', 'src/features/dashboard/DashboardPage.vue']

const root = dirname(dirname(fileURLToPath(import.meta.url)))
const viteDir = join(root, 'kelir-frontend', 'dist', '.vite')

function die(message) {
  console.error(`\n✗ ${message}\n`)
  process.exit(1)
}

function readBuild(file, whatWritesIt) {
  const path = join(viteDir, file)

  try {
    return JSON.parse(readFileSync(path, 'utf8'))
  } catch (error) {
    die(
      `${path} is not readable: ${error.message}\n` +
        `  Run \`npm run build\` in kelir-frontend first — this reads the build, not the source.\n` +
        `  The file is written by ${whatWritesIt}.`,
    )
  }
}

const manifest = readBuild('manifest.json', "Vite's `build.manifest`")
const carried = readBuild('chunk-packages.json', "vite.config.ts's `kelir-chunk-packages` plugin")
const styled = readBuild(
  'stylesheet-packages.json',
  "vite.config.ts's `kelir-chunk-packages` plugin",
)

const entries = Object.keys(manifest).filter((key) => manifest[key].isEntry)

if (entries.length === 0) {
  // A manifest with no entry would make everything below vacuous, which is the
  // failure mode a check like this has: green because it looked at nothing.
  die('the manifest declares no entry chunk, so nothing below was actually checked')
}

const missingRoots = HOME_ROUTE.filter((key) => !manifest[key])

if (missingRoots.length > 0) {
  // A renamed page would otherwise shrink "first load" back to the entry, which
  // is the narrowing that let a static Unovis import through.
  die(
    'the home route has no chunk in the manifest for:\n\n' +
      missingRoots.map((key) => `    ${key}`).join('\n') +
      '\n\n  so the first-load walk would silently start from fewer places than `/` loads.\n' +
      '  If the route moved, HOME_ROUTE in this script moves with it.',
  )
}

// --- First load: the entry and the home route, through static imports -------
const reached = new Set()
const queue = [...entries, ...HOME_ROUTE]

while (queue.length > 0) {
  const key = queue.shift()

  if (reached.has(key) || !manifest[key]) {
    continue
  }

  reached.add(key)
  // `imports` only. `dynamicImports` is the split both decisions paid for.
  queue.push(...(manifest[key].imports ?? []))
}

const firstLoadFiles = new Set([...reached].map((key) => manifest[key].file))
const firstLoadAssets = new Set([...reached].flatMap((key) => manifest[key].assets ?? []))
const firstLoadStylesheets = new Set([...reached].flatMap((key) => manifest[key].css ?? []))

for (const subject of SUBJECTS) {
  const carries = (file) => (carried[file] ?? []).some((name) => subject.packages.includes(name))
  const carriers = Object.keys(carried).filter(carries)
  const payload = Object.keys(manifest)
    .filter((key) => !key.endsWith('.js') && subject.packages.some((name) => key.includes(name)))
    .map((key) => manifest[key].file)
  const stylesheets = Object.keys(styled).filter((file) =>
    styled[file].some((name) => subject.packages.includes(name)),
  )

  if (carriers.length === 0) {
    die(
      `no chunk in the build carries ${subject.packages.join(' or ')}, so this check passed by\n` +
        '  not finding its subject. Either the dependency was removed — in which case the\n' +
        '  decision behind it changed and this script should change with it — or the build\n' +
        '  is stale.',
    )
  }

  // --- 1. It has a chunk of its own ----------------------------------------
  //
  // A static import does not merely add an edge; Rollup merges the module into
  // whatever imported it, and the split chunk disappears. That is what a
  // reachability check alone could not see, and it is the shape the mutation
  // actually took.
  const split = carriers.filter((file) => !firstLoadFiles.has(file))

  if (split.length === 0) {
    die(
      `${subject.name} has no chunk of its own, which means a static import folded it\n` +
        '  into a chunk on the first-load path:\n\n' +
        carriers.map((file) => `    ${file}`).join('\n') +
        '\n\n' +
        subject.howToFix,
    )
  }

  // --- 2. And nothing on the first-load path carries it --------------------
  const onFirstLoad = carriers.filter((file) => firstLoadFiles.has(file))

  if (onFirstLoad.length > 0) {
    die(
      `${subject.name} is on the first-load path, which breaks the terms it was taken on.\n\n` +
        onFirstLoad.map((file) => `    ${file}`).join('\n') +
        '\n\n' +
        subject.howToFix,
    )
  }

  // --- 3. Nor does a first-load chunk carry its payload: an asset, or CSS ---
  //
  // The evaluator's `.wasm` binary is the 588 KB; the JavaScript wrapper beside
  // it is under four. A build that split the wrapper and attached the binary to
  // the entry would satisfy both checks above and none of the decision. Unovis
  // ships no assets today, so for it this holds over an empty set — said below
  // as a count rather than left implicit.
  //
  // A stylesheet is the same payload by another route (#687): imported from a
  // first-load module, its rules land in a first-load chunk's `css`, and no
  // JavaScript chunk or asset shows it.
  if (subject.stylesheet && stylesheets.length === 0) {
    die(
      `no stylesheet in the build carries ${subject.name}'s CSS, so the stylesheet half of\n` +
        '  this check would pass by not finding it. Either .vite/stylesheet-packages.json\n' +
        "  stopped recording stylesheets (vite.config.ts's `kelir-chunk-packages`), or the\n" +
        "  library's stylesheets are no longer imported, and it cannot draw without them.",
    )
  }

  const eager = [
    ...payload.filter((file) => firstLoadAssets.has(file)),
    ...stylesheets.filter((file) => firstLoadStylesheets.has(file)),
  ]

  if (eager.length > 0) {
    die(
      `${subject.name}'s payload is attached to a first-load chunk, so it is fetched\n` +
        '  before any route decides it needs it:\n\n' +
        eager.map((file) => `    ${file}`).join('\n') +
        '\n\n' +
        subject.howToFix,
    )
  }

  console.log(
    `✓ ${subject.name} is off the first-load path — ` +
      `${split.length} split chunk(s), ${payload.length} payload asset(s), ` +
      `${stylesheets.length} stylesheet(s)`,
  )
}

console.log(
  `  ${reached.size} chunk(s) on first load: the entry and ${HOME_ROUTE.length} home-route chunk(s), ` +
    'through static imports',
)
