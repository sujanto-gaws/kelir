// @vitest-environment node
// Node's types are pulled in for this file alone: it loads `vite.config.ts`,
// which reads `package.json` and asks git for the commit.
/// <reference types="node" />
import { describe, expect, it } from 'vitest'

import config from '../../vite.config'

/**
 * `vite.config.ts`'s `kelir-chunk-packages` plugin (#687, ADR-0046 §5): the
 * test-engineer campaign over PR #719.
 *
 * **`check:bundle` sees only what this plugin writes.** It reads
 * `chunk-packages.json` for the packages each chunk's JavaScript carries, and
 * `stylesheet-packages.json` for the packages each CSS file carries; a module
 * filed under the wrong one is invisible to the check that would have caught
 * it. So the plugin's `generateBundle` is run here over bundles shaped as
 * Rollup hands them over, and what it emits is read back.
 */

interface Emitted {
  fileName: string
  source: string
}

function plugin() {
  const found = ((config.plugins ?? []) as unknown[])
    .flat()
    .find(
      (candidate): candidate is { name: string; generateBundle: unknown } =>
        typeof candidate === 'object' &&
        candidate !== null &&
        'name' in candidate &&
        candidate.name === 'kelir-chunk-packages',
    )

  if (!found) {
    throw new Error('vite.config.ts has no kelir-chunk-packages plugin')
  }

  return found
}

/** A chunk as Rollup hands it to `generateBundle`: its modules by id, and the CSS Vite extracted. */
function chunk(fileName: string, modules: Record<string, number>, importedCss: string[] = []) {
  return {
    type: 'chunk' as const,
    fileName,
    modules: Object.fromEntries(
      Object.entries(modules).map(([id, renderedLength]) => [id, { renderedLength }]),
    ),
    viteMetadata: { importedCss: new Set(importedCss), importedAssets: new Set<string>() },
  }
}

/** What the plugin writes for a bundle: both maps, by file. */
function written(bundle: Record<string, unknown>): {
  carried: Record<string, string[]>
  styled: Record<string, string[]>
} {
  const emitted: Emitted[] = []
  const hook = plugin().generateBundle as (
    this: { emitFile: (file: Emitted) => void },
    options: unknown,
    bundle: unknown,
  ) => void

  hook.call({ emitFile: (file) => emitted.push(file) }, {}, bundle)

  const read = (name: string) =>
    JSON.parse(emitted.find((file) => file.fileName === `.vite/${name}`)!.source) as Record<
      string,
      string[]
    >

  return { carried: read('chunk-packages.json'), styled: read('stylesheet-packages.json') }
}

const NM = 'D:/Projects/kelir/kelir-frontend/node_modules'

describe('the kelir-chunk-packages plugin', () => {
  it('files a JavaScript module under its chunk, and a stylesheet under the CSS that chunk extracted', () => {
    const { carried, styled } = written({
      'assets/WorkflowGraph-H.js': chunk(
        'assets/WorkflowGraph-H.js',
        {
          [`${NM}/@vue-flow/core/dist/vue-flow-core.mjs`]: 90000,
          [`${NM}/@vue-flow/core/dist/style.css`]: 0,
          [`${NM}/@vue-flow/controls/dist/style.css`]: 0,
          [`${NM}/@dagrejs/dagre/dist/dagre.esm.js`]: 40000,
          '/src/features/workflow-builder/WorkflowGraph.vue': 5000,
        },
        ['assets/WorkflowGraph-H.css'],
      ),
      'assets/WorkflowGraph-H.css': { type: 'asset', fileName: 'assets/WorkflowGraph-H.css' },
    })

    expect(carried).toEqual({ 'assets/WorkflowGraph-H.js': ['@dagrejs/dagre', '@vue-flow/core'] })
    expect(styled).toEqual({
      'assets/WorkflowGraph-H.css': ['@vue-flow/controls', '@vue-flow/core'],
    })
  })

  it('names a scoped package from a Windows path, and counts no module shaken to nothing', () => {
    const { carried } = written({
      'assets/index.js': chunk('assets/index.js', {
        'D:\\Projects\\kelir\\kelir-frontend\\node_modules\\@vueuse\\core\\index.mjs': 1200,
        [`${NM}/@dagrejs/dagre/dist/dagre.esm.js`]: 0,
      }),
    })

    expect(carried).toEqual({ 'assets/index.js': ['@vueuse/core'] })
  })

  it('files the stylesheet of a chunk with no CSS of its own nowhere, as Vite emits none', () => {
    const { styled } = written({
      'assets/AppLayout.js': chunk('assets/AppLayout.js', {
        [`${NM}/@vue-flow/core/dist/style.css`]: 0,
      }),
    })

    expect(styled).toEqual({})
  })

  // DEFECT (#719 campaign, 2026-10-10): `isStylesheet` reads any id with
  // `.css` before its query as a stylesheet, so `style.css?inline` — which
  // Vite turns into a JavaScript string of the whole stylesheet, shipped in
  // the importing chunk — is filed as CSS, not JavaScript, and the chunk has
  // no CSS file to file it under. Seen on a real build: with
  // `import css from '@vue-flow/core/dist/style.css?inline'` in AppLayout.vue,
  // AppLayout's chunk carried Vue Flow's rules, chunk-packages.json said
  // `['lucide-vue-next']`, and `npm run check:bundle` passed all three subjects.
  it.fails('files a stylesheet imported `?inline` as the JavaScript it ships in', () => {
    const { carried } = written({
      'assets/AppLayout.js': chunk('assets/AppLayout.js', {
        [`${NM}/@vue-flow/core/dist/style.css?inline`]: 4100,
        [`${NM}/lucide-vue-next/dist/esm/icons/plus.js`]: 300,
      }),
    })

    expect(carried['assets/AppLayout.js']).toContain('@vue-flow/core')
  })
})
