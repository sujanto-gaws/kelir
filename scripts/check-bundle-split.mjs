#!/usr/bin/env node
/**
 * Four libraries' bundle conditions, as a build assertion (issue #163 AC4; #447
 * AC4; #687; #688 G3, ADR-0046; #749).
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
 * **#688 took the form builder's drag-and-drop library on the same condition**
 * (ADR-0046 §5): `vue-draggable-plus`, with the Sortable it carries, is
 * imported by the canvas components alone, on the builder's lazy route.
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
 * anyway — and the third check for a payload riding along as an asset or a
 * stylesheet, which it finds by the map and by the stylesheet's text (#749).
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
 * # And by what a stylesheet contains, whatever route brought it (#749)
 *
 * **The map sees a stylesheet a module imported, and a CSS `@import` is not
 * one.** `@import '@vue-flow/core/dist/style.css'` in `src/styles/main.css` is
 * inlined by Tailwind before Rollup sees a module, so the entry's
 * `index-*.css` carried Vue Flow's rules, the map recorded nothing for it, and
 * this check passed (record 23's finding 3; reproduced 2026-11-05). So a
 * subject that ships CSS names a **`stylesheetMarker`**, a string each of its
 * stylesheets contains (`.vue-flow__` for all three of Vue Flow's), and the
 * third check also searches **the text** of every first-load stylesheet for
 * it. That holds by any route: a module import, a CSS `@import`, the rules
 * pasted in.
 *
 * *First-load stylesheets* are every first-load chunk's `css`, every
 * `<link rel="stylesheet">` in `dist/index.html` (a file copied from `public/`
 * is linked only there), and every `@import` left in one of those that points
 * at a file of the build, followed. **An `@import` or `<link>` the build does
 * not hold**, such as a CDN URL, which survives the build as written, fails
 * when it names one of a subject's packages, since its text is not there to
 * search. One that names none is not read. A first-load stylesheet the build
 * names and lacks fails as a stale build.
 *
 * **A marker must be found**, as the map must be: a subject whose marker is in
 * none of the stylesheets the map credits to its packages fails, so a marker
 * that drifted from the library's class names fails rather than passing on
 * nothing. Kelir's own rules against `.vue-flow__` classes are scoped to
 * `WorkflowGraph.vue` and ship in that same stylesheet, so this proves the
 * marker matches the graph's CSS, not that Vue Flow's rules rather than
 * Kelir's supplied it; an upgrade that renamed the library's classes would
 * strand both. **And a subject must have a marker if it ships CSS**: one whose
 * package the map records in a stylesheet, with no marker, fails until it is
 * given one. Only Vue Flow ships CSS today; the other subjects' packages were
 * searched for `.css` files on 2026-11-05 and have none (Unovis styles through
 * emotion, inside its JavaScript chunk).
 *
 * **What the marker cannot tell** is whose rules it found: a rule Kelir writes
 * itself against `.vue-flow__node` in a first-load stylesheet fails too, which
 * is a right place to fail, since it styles a component first load never
 * draws. Rules carried in JavaScript, as `?inline` and `?raw` imports carry
 * them, are the first two checks' (ADR-0046 §5). **Not read**: CSS text pasted
 * into a JavaScript string by hand, and a stylesheet a script adds at run
 * time.
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

import { existsSync, readFileSync } from 'node:fs'
import { dirname, join, posix } from 'node:path'
import { fileURLToPath } from 'node:url'

/**
 * What each decision keeps off first load. `packages` is matched against
 * `chunk-packages.json`, so a library is identified by code that shipped.
 * `stylesheetMarker`, for a subject that ships CSS, is searched for in the text
 * of every first-load stylesheet (#749); a subject with none ships no CSS.
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
    // Each of `@vue-flow/core`'s two stylesheets and `@vue-flow/controls`' one
    // styles `.vue-flow__` classes, 50, 46 and 6 lines of them (1.48.2 and
    // 1.1.3, 2026-11-05). Kelir's own rules naming them are scoped to
    // WorkflowGraph.vue, so they ship in the graph's lazy CSS too.
    stylesheetMarker: '.vue-flow__',
    howToFix:
      '  ADR-0046 took Vue Flow, dagre and the @vueuse/core Vue Flow brings on the basis\n' +
      '  that only the workflow graph pays for them. Reach them through\n' +
      '  src/features/workflow-builder/WorkflowGraph.vue, which WorkflowEditorPage.vue loads\n' +
      "  with `defineAsyncComponent`, and import Vue Flow's stylesheets there — never\n" +
      '  `@vue-flow/*`, `@dagrejs/*`, `@vueuse/core` or their CSS on a page, layout or main.ts.',
  },
  {
    // #688 G3, ADR-0046 §5. vue-draggable-plus 0.6.1 inlines Sortable 1.15.2 in
    // its own dist, so `sortablejs` is listed for a build that ever resolves it
    // as a package of its own; today the first name is the one that matches.
    name: "the form builder's drag-and-drop library",
    packages: ['vue-draggable-plus', 'sortablejs'],
    howToFix:
      '  ADR-0046 took vue-draggable-plus on the basis that only the form builder pays for\n' +
      '  it. Import it from src/features/admin/form-builder/FormCanvasList.vue and\n' +
      '  FormBuilderPalette.vue alone, which only FormBuilderPage.vue reaches, on a\n' +
      '  router-lazy route — never from a store, the layout, or anything they share.',
  },
]

/** The chunks `/` renders besides the entry. Router-lazy, and fetched by every sign-in. */
const HOME_ROUTE = ['src/layouts/AppLayout.vue', 'src/features/dashboard/DashboardPage.vue']

const root = dirname(dirname(fileURLToPath(import.meta.url)))
const distDir = join(root, 'kelir-frontend', 'dist')
const viteDir = join(distDir, '.vite')

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

/**
 * The manifest key of the chunk a home-route module loads as.
 *
 * Its own path when the chunk is that module's facade, which is the usual case.
 * **When the chunk also exports to other chunks, Vite keys it `_<name>-<hash>.js`
 * instead**: since #688, `AppLayout.vue`'s chunk holds the icons the layout
 * imports by name, and the lazily loaded menu-icon map imports them from it.
 * Such a chunk is still what the router loads, so it is found by its `name`,
 * as the one dynamic entry of that name; two would be ambiguous, and are
 * reported as missing rather than guessed at.
 */
function homeChunk(source) {
  if (manifest[source]) {
    return source
  }

  const name = source.slice(source.lastIndexOf('/') + 1).replace(/\.\w+$/, '')
  const named = Object.keys(manifest).filter(
    (key) => key.startsWith('_') && manifest[key].name === name && manifest[key].isDynamicEntry,
  )

  return named.length === 1 ? named[0] : null
}

const homeChunks = HOME_ROUTE.map(homeChunk)
const missingRoots = HOME_ROUTE.filter((_, at) => homeChunks[at] === null)

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
const queue = [...entries, ...homeChunks]

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

// --- First-load stylesheets by text (#749) -----------------------------------
//
// The map above records what a *module* imported. The text is read too, from
// every stylesheet first load fetches: the chunks' `css`, the document's
// `<link rel="stylesheet">`s, and the `@import`s left in either.

/** A built file's text, by its path under `dist/`. A file the build names and lacks is a stale build. */
function readDist(file) {
  try {
    return readFileSync(join(distDir, file), 'utf8')
  } catch (error) {
    die(
      `dist/${file} is not readable (${error.code ?? error.message}).\n` +
        '  The build names it, so the build is stale or partial. Run `npm run build` again.',
    )
  }
}

/**
 * A stylesheet reference resolved to a path under `dist/`, or `null` when the
 * build does not hold it: a remote URL, or a path to no file of the build.
 */
function inBuild(reference, from) {
  if (/^(?:[a-z][a-z0-9+.-]*:|\/\/)/i.test(reference)) {
    return null
  }

  const bare = reference.split(/[?#]/)[0]
  const path = bare.startsWith('/')
    ? posix.normalize(bare.slice(1))
    : posix.join(posix.dirname(from), bare)

  return existsSync(join(distDir, path)) ? path : null
}

const STYLESHEET_LINK = /<link\b[^>]*>/gi
const CSS_IMPORT = /@import\s*(?:url\(\s*)?(['"]?)([^'")\s;]+)\1/gi

const firstLoadCss = new Map()
const unreadReferences = []
const cssQueue = [...firstLoadStylesheets]

for (const link of readDist('index.html').match(STYLESHEET_LINK) ?? []) {
  const href = /\bhref\s*=\s*["']?([^"'\s>]+)/i.exec(link)?.[1]

  if (!/\brel\s*=\s*["']?stylesheet\b/i.test(link) || href === undefined) {
    continue
  }

  const path = inBuild(href, 'index.html')

  if (path === null) {
    unreadReferences.push({ reference: href, from: 'index.html' })
  } else {
    cssQueue.push(path)
  }
}

while (cssQueue.length > 0) {
  const file = cssQueue.shift()

  if (firstLoadCss.has(file)) {
    continue
  }

  const text = readDist(file)
  firstLoadCss.set(file, text)

  for (const [, , reference] of text.matchAll(CSS_IMPORT)) {
    const path = inBuild(reference, file)

    if (path === null) {
      unreadReferences.push({ reference, from: file })
    } else {
      cssQueue.push(path)
    }
  }
}

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
  // JavaScript chunk or asset shows it. And by a route no module shows (#749):
  // a CSS `@import`, inlined before Rollup sees it, is found by its marker.
  const marker = subject.stylesheetMarker

  if (marker === undefined && stylesheets.length > 0) {
    die(
      `${subject.name} ships a stylesheet now, and its subject names no stylesheetMarker,\n` +
        '  so a first-load stylesheet carrying its rules by a CSS `@import` would pass:\n\n' +
        stylesheets.map((file) => `    ${file}`).join('\n') +
        "\n\n  Give the subject a string every one of the library's stylesheets contains.",
    )
  }

  if (marker !== undefined && stylesheets.length === 0) {
    die(
      `no stylesheet in the build carries ${subject.name}'s CSS, so the stylesheet half of\n` +
        '  this check would pass by not finding it. Either .vite/stylesheet-packages.json\n' +
        "  stopped recording stylesheets (vite.config.ts's `kelir-chunk-packages`), or the\n" +
        "  library's stylesheets are no longer imported, and it cannot draw without them.",
    )
  }

  if (marker !== undefined && !stylesheets.some((file) => readDist(file).includes(marker))) {
    die(
      `${subject.name}'s stylesheet marker \`${marker}\` is in none of the stylesheets that\n` +
        '  carry its CSS, so searching first load for it would pass by not finding it:\n\n' +
        stylesheets.map((file) => `    ${file}`).join('\n') +
        '\n\n  The library renamed its classes, or the marker drifted. Pick a string its\n' +
        '  stylesheets contain.',
    )
  }

  // By the map, by the marker in a first-load stylesheet's text, and by a
  // first-load `@import` or `<link>` the build does not hold that names a
  // package of the subject, whose text cannot be searched.
  const marked = [...firstLoadCss.keys()].filter(
    (file) => marker !== undefined && firstLoadCss.get(file).includes(marker),
  )
  const unread = unreadReferences
    .filter(({ reference }) => subject.packages.some((name) => reference.includes(name)))
    .map(({ reference, from }) => `${reference} (named by ${from}, not in the build)`)
  const eager = [
    ...new Set([
      ...payload.filter((file) => firstLoadAssets.has(file)),
      ...stylesheets.filter((file) => firstLoadStylesheets.has(file)),
      ...marked,
      ...unread,
    ]),
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
      `${stylesheets.length} stylesheet(s)` +
      (marker === undefined
        ? ''
        : `; \`${marker}\` in none of ${firstLoadCss.size} first-load stylesheet(s)`),
  )
}

// --- The icon set: a handful of its modules on first load, never the set ----
//
// **A different shape of condition, so a different check** (#688, after #698).
// `@lucide/vue` belongs on first load: the layout draws its own icons, and a
// named import tree-shakes to those. What does not belong there is the *set*:
// `import * as` the package, which the layout did to look up a configured
// menu's icon by name, ships all ~1,870 icons to every sign-in — 34.7 KB
// gzipped more on the swap to `@lucide/vue`. So this counts the package's
// modules on first-load chunks rather than asking whether it is there at all,
// from `chunk-package-modules.json`, which `vite.config.ts` writes beside
// `chunk-packages.json`. Menu icons are loaded one per chunk by
// `src/layouts/menuIcon.ts`.
const ICON_PACKAGE = '@lucide/vue'
// 28 on 2026-10-10: the layout's and the shared components' own icons, and the
// package's few runtime modules. The set is 1,870, so the limit leaves room for
// many more named imports and none for the barrel.
const ICON_MODULE_LIMIT = 100
const moduleCounts = readBuild(
  'chunk-package-modules.json',
  "vite.config.ts's `kelir-chunk-packages` plugin",
)
const iconModulesIn = (file) => moduleCounts[file]?.[ICON_PACKAGE] ?? 0
const iconModulesTotal = Object.keys(moduleCounts).reduce((sum, file) => sum + iconModulesIn(file), 0)
const iconModulesFirstLoad = [...firstLoadFiles].reduce((sum, file) => sum + iconModulesIn(file), 0)

if (iconModulesTotal <= ICON_MODULE_LIMIT) {
  die(
    `the build carries ${iconModulesTotal} ${ICON_PACKAGE} module(s) in all, no more than the\n` +
      `  first-load limit of ${ICON_MODULE_LIMIT}, so the check below would pass by not finding\n` +
      '  the set. Either menu icons stopped being loaded one per chunk — in which case\n' +
      '  this check should change with that decision — or the build is stale.',
  )
}

if (iconModulesFirstLoad > ICON_MODULE_LIMIT) {
  const heaviest = [...firstLoadFiles]
    .filter((file) => iconModulesIn(file) > 0)
    .sort((left, right) => iconModulesIn(right) - iconModulesIn(left))
    .slice(0, 3)

  die(
    `first load carries ${iconModulesFirstLoad} ${ICON_PACKAGE} modules, over the limit of ` +
      `${ICON_MODULE_LIMIT}: the icon set is\n  on the first-load path. Heaviest:\n\n` +
      heaviest.map((file) => `    ${file} (${iconModulesIn(file)})`).join('\n') +
      '\n\n  Import icons by name, never `import * as` the package. A configured menu icon\n' +
      '  is data: draw it with `menuIcon()` from src/layouts/menuIcon.ts, which loads\n' +
      '  each icon in a chunk of its own.',
  )
}

console.log(
  `✓ the ${ICON_PACKAGE} icon set is off the first-load path — ` +
    `${iconModulesFirstLoad} of its ${iconModulesTotal} module(s) on first load (limit ${ICON_MODULE_LIMIT})`,
)

console.log(
  `  ${reached.size} chunk(s) on first load: the entry and ${HOME_ROUTE.length} home-route chunk(s), ` +
    'through static imports',
)
