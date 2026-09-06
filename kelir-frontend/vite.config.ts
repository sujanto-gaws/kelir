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

const { version } = JSON.parse(
  readFileSync(fileURLToPath(new URL('./package.json', import.meta.url)), 'utf8'),
) as { version: string }

export default defineConfig({
  plugins: [vue(), tailwindcss(), versionAsset(version, buildSha())],
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
  /**
   * The manifest exists so that a CI step can read the chunk graph.
   *
   * **D-10 accepted 588 KB gzipped on one condition — the evaluator stays off
   * the first-load path — and no unit test can observe that.** A build
   * assertion can: `scripts/check-bundle-split.mjs` walks this file from the
   * entry chunk through *static* imports alone and fails when the engine is
   * reachable. That is the form the Sprint 7 retrospective judged holds, having
   * watched the actions encoded in a retrospective fail and the ones encoded in
   * a standard or a test survive.
   */
  build: {
    manifest: true,
  },
  test: {
    environment: 'jsdom',
    include: ['src/**/*.spec.ts'],
  },
})
