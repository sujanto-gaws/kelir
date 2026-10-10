import { execFileSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { fileURLToPath, URL } from 'node:url'

import { defineConfig, type Plugin } from 'vitest/config'
import vue from '@vitejs/plugin-vue'
import tailwindcss from '@tailwindcss/vite'

/**
 * The commit this bundle was built from, resolved the way `kelir-backend`'s
 * `build.rs` resolves it (#362).
 *
 *   1. `KELIR_BUILD_SHA` from the environment — how CI and the Docker build
 *      supply it, since neither has a usable `.git` directory.
 *   2. `git rev-parse --short HEAD` — the local development case.
 *   3. `unknown` — **never fails the build.** A missing SHA is a deployment
 *      finding, not a reason a developer cannot build.
 *
 * Two resolution orders for one fact would drift, so this is deliberately the
 * same three steps in the same order as the backend's.
 */
function buildSha(): string {
  const supplied = process.env.KELIR_BUILD_SHA?.trim()
  if (supplied) {
    return supplied
  }

  try {
    return execFileSync('git', ['rev-parse', '--short', 'HEAD'], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore'],
    }).trim()
  } catch {
    return 'unknown'
  }
}

/**
 * Emits `version.json` beside the bundle, so a deployed frontend can say which
 * release it is (#362).
 *
 * # Why an asset and not a banner comment
 *
 * `kelir-frontend:0.6.0`'s served bundle was **byte-identical** to
 * `0.6.0-rc`'s — 45 files, same hash — because the version never reached the
 * built assets, so Docker gave the new image the rc's creation timestamp. The
 * backend answers `/version` with its version and SHA and the frontend answered
 * nothing, which means [release process](../docs/standards/04.%20Release%20Process.md)
 * §4 step 7 could be run against one artefact of the two.
 *
 * **A file rather than a comment inside a chunk**, because the point is that it
 * can be read from a deployed site with `curl` and no shell access to the
 * container. Caddy's `try_files {path} /index.html` serves a real file before
 * it falls back to the SPA, and `/version` itself is proxied to the backend, so
 * `/version.json` collides with nothing.
 *
 * It also gives the images the property whose absence hid this: two builds from
 * different commits now differ in their served assets, because this file does.
 */
function versionAsset(version: string, commit: string): Plugin {
  return {
    name: 'kelir-version-asset',
    apply: 'build',
    generateBundle() {
      this.emitFile({
        type: 'asset',
        fileName: 'version.json',
        source: `${JSON.stringify({ name: 'Kelir', version, commit }, null, 2)}\n`,
      })
    },
  }
}

/**
 * Emits `.vite/chunk-packages.json` — for each chunk, the npm packages whose
 * code it actually carries (#447).
 *
 * # Why the manifest is not enough
 *
 * The manifest names a chunk by the module it was split at. That names the
 * JSON Logic evaluator, which `lib/jsonlogic.ts` reaches with an `import()` of
 * the package itself. **It does not name Unovis**, which is reached through
 * `DocumentStatusChart.vue`: the library and its d3 land inside that
 * component's chunk, under that component's key, and no key mentions them. A
 * check matching keys would find nothing — and nothing is also what it would
 * find if a static import had folded the library into the layout.
 *
 * **`renderedLength`, not `moduleIds`.** A module Rollup shook down to nothing
 * is still listed on the chunk, and counting a package for code that did not
 * ship would redden the check over a type-only import.
 */
function chunkPackages(): Plugin {
  return {
    name: 'kelir-chunk-packages',
    apply: 'build',
    generateBundle(_options, bundle) {
      const packages: Record<string, string[]> = {}
      const modules: Record<string, Record<string, number>> = {}

      for (const output of Object.values(bundle)) {
        if (output.type !== 'chunk') {
          continue
        }

        const names = new Set<string>()
        const counts: Record<string, number> = {}

        for (const [id, module] of Object.entries(output.modules)) {
          const name = packageOf(id)

          if (name !== null && module.renderedLength > 0) {
            names.add(name)
            counts[name] = (counts[name] ?? 0) + 1
          }
        }

        packages[output.fileName] = [...names].sort()
        modules[output.fileName] = counts
      }

      this.emitFile({
        type: 'asset',
        fileName: '.vite/chunk-packages.json',
        source: `${JSON.stringify(packages, null, 2)}\n`,
      })
      // How many of each package's modules a chunk carries. A package that
      // ships one module per item, as an icon set does, is only "on first
      // load" in the sense that matters when it is there by the hundred (#688).
      this.emitFile({
        type: 'asset',
        fileName: '.vite/chunk-package-modules.json',
        source: `${JSON.stringify(modules, null, 2)}\n`,
      })
    },
  }
}

/** The id a configured menu icon's alias map is imported by. */
const LUCIDE_ALIASES = 'virtual:lucide-icon-aliases'

/**
 * `virtual:lucide-icon-aliases`: the Lucide export names that are not the
 * PascalCase of their own icon file, each mapped to that file (#688).
 *
 * **Why it exists.** A configured menu entry names its icon as data
 * (`layouts/menuIcon.ts`), and each icon is loaded on its own from
 * `@lucide/vue`'s per-icon modules, keyed by file name. A renamed icon keeps
 * its old name only as an extra export of the package's barrel —
 * `AlertTriangle` for `triangle-alert.mjs` — and the barrel is the whole set.
 * So the alias names are read from it here, at build time, and the map is
 * imported lazily, only for a name no file carries.
 *
 * The `…Icon` and `Lucide…` spellings of every name are left out: a menu
 * names an icon in kebab-case, and both would double the map for spellings
 * nobody types.
 */
function lucideIconAliases(): Plugin {
  const resolved = `\0${LUCIDE_ALIASES}`

  return {
    name: 'kelir-lucide-icon-aliases',
    resolveId: (id) => (id === LUCIDE_ALIASES ? resolved : null),
    load(id) {
      if (id !== resolved) {
        return null
      }

      const barrel = readFileSync(
        fileURLToPath(
          new URL('./node_modules/@lucide/vue/dist/esm/lucide-vue.mjs', import.meta.url),
        ),
        'utf8',
      )
      const pascal = (kebab: string) =>
        kebab
          .split('-')
          .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
          .join('')
      const aliases: Record<string, string> = {}

      for (const [, names, file] of barrel.matchAll(
        /export \{([^}]*)\} from '\.\/icons\/([^']+)\.mjs'/g,
      )) {
        for (const name of names.split(',').map((part) => part.replace('default as', '').trim())) {
          if (name !== pascal(file) && !name.endsWith('Icon') && !name.startsWith('Lucide')) {
            aliases[name] = file
          }
        }
      }

      return `export default ${JSON.stringify(aliases)}\n`
    },
  }
}

/** The package a module belongs to, from its last `node_modules` segment; `null` for our own source. */
function packageOf(id: string): string | null {
  const match = /.*node_modules[\\/]((?:@[^\\/]+[\\/])?[^\\/]+)/.exec(id)

  return match ? match[1].replace('\\', '/') : null
}

const { version } = JSON.parse(
  readFileSync(fileURLToPath(new URL('./package.json', import.meta.url)), 'utf8'),
) as { version: string }

export default defineConfig({
  plugins: [
    vue(),
    tailwindcss(),
    versionAsset(version, buildSha()),
    chunkPackages(),
    lucideIconAliases(),
  ],
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
  /**
   * The manifest exists so that a CI step can read the chunk graph.
   *
   * **D-10 accepted 588 KB gzipped on one condition — the evaluator stays off
   * the first-load path — and #447 took a chart library on the same one. No
   * unit test can observe either.** A build assertion can:
   * `scripts/check-bundle-split.mjs` walks this file through *static* imports
   * alone, reads which packages each reached chunk carries from
   * `chunk-packages.json` above, and fails when either library is on the path.
   * That is the form the Sprint 7 retrospective judged holds, having watched the
   * actions encoded in a retrospective fail and the ones encoded in a standard
   * or a test survive.
   */
  build: {
    manifest: true,
  },
  test: {
    environment: 'jsdom',
    include: ['src/**/*.spec.ts'],
  },
})
