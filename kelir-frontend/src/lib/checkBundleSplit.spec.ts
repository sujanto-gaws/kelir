// @vitest-environment node
// Node's types are pulled in for this file alone, as `jsonlogic.parity.spec.ts`
// does: it runs a script and writes a build's metadata to a scratch directory.
/// <reference types="node" />
import { spawnSync } from 'node:child_process'
import { copyFileSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'

import { afterEach, beforeEach, describe, expect, it } from 'vitest'

/**
 * `scripts/check-bundle-split.mjs` against builds made to break it (#687,
 * ADR-0046 §5): the test-engineer campaign over PR #719.
 *
 * **The script is the only thing holding Vue Flow and dagre off first load,
 * and until now it was seen red only by hand**, on the builds its author
 * thought to try. Here it runs against `.vite` metadata written to a scratch
 * directory — a manifest, `chunk-packages.json` and `stylesheet-packages.json`
 * shaped as the real build writes them (2026-10-10) — once clean, and once
 * for each way the graph's subject can reach first load. Each case names the
 * check it expects to fail by that check's own words, so a check that stops
 * failing is seen even when another one catches the build.
 *
 * The script finds the build at `../kelir-frontend/dist/.vite` from its own
 * folder, so it is copied into the same layout under the scratch directory.
 * It is copied on every run, so an edit to the script is what is tested.
 * This file lives in `src/` because that is what Vitest runs.
 */

const SCRIPT = resolve(__dirname, '../../../scripts/check-bundle-split.mjs')

type Manifest = Record<
  string,
  {
    file: string
    isEntry?: boolean
    imports?: string[]
    dynamicImports?: string[]
    css?: string[]
    assets?: string[]
  }
>

interface Build {
  manifest: Manifest
  carried: Record<string, string[]>
  styled: Record<string, string[]> | null
}

const ENTRY = 'assets/index-CQaDf5LZ.js'
const ENTRY_CSS = 'assets/index-Q-iWR29A.css'
const LAYOUT = 'assets/AppLayout-CXiqFNvo.js'
const DASHBOARD = 'assets/DashboardPage-Cuw3tNRG.js'
const GRAPH = 'assets/WorkflowGraph-Jmrvbe9i.js'
const GRAPH_CSS = 'assets/WorkflowGraph-C2GtfgTO.css'
const VUE_FLOW_CSS = ['@vue-flow/controls', '@vue-flow/core']

/** A clean build, as `npm run build` wrote it on this branch: every subject off first load. */
function clean(): Build {
  return {
    manifest: {
      'index.html': {
        file: ENTRY,
        isEntry: true,
        css: [ENTRY_CSS],
        dynamicImports: [
          'src/layouts/AppLayout.vue',
          'src/features/dashboard/DashboardPage.vue',
          '_WorkflowEditorPage-Bc_YsB1j.js',
        ],
      },
      'src/layouts/AppLayout.vue': { file: LAYOUT, imports: ['index.html'] },
      'src/features/dashboard/DashboardPage.vue': {
        file: DASHBOARD,
        imports: ['index.html'],
        dynamicImports: ['src/features/dashboard/DocumentStatusChart.vue'],
      },
      'src/features/dashboard/DocumentStatusChart.vue': {
        file: 'assets/DocumentStatusChart-BtcyQ087.js',
        imports: ['index.html'],
      },
      'node_modules/@goplasmatic/datalogic-wasm/web/datalogic_wasm.js': {
        file: 'assets/datalogic_wasm-DNVD5O3Z.js',
        assets: ['assets/datalogic_wasm_bg-BaXwXh77.wasm'],
      },
      'node_modules/@goplasmatic/datalogic-wasm/web/datalogic_wasm_bg.wasm': {
        file: 'assets/datalogic_wasm_bg-BaXwXh77.wasm',
      },
      '_WorkflowEditorPage-Bc_YsB1j.js': {
        file: 'assets/WorkflowEditorPage-Bc_YsB1j.js',
        imports: ['index.html'],
        dynamicImports: ['src/features/workflow-builder/WorkflowGraph.vue'],
      },
      'src/features/workflow-builder/WorkflowGraph.vue': {
        file: GRAPH,
        imports: ['index.html'],
        css: [GRAPH_CSS],
      },
    },
    carried: {
      [ENTRY]: ['@vue/runtime-core', 'axios', 'pinia', 'vue-router'],
      [LAYOUT]: ['lucide-vue-next'],
      [DASHBOARD]: [],
      'assets/DocumentStatusChart-BtcyQ087.js': ['@unovis/ts', '@unovis/vue', 'd3-selection'],
      'assets/datalogic_wasm-DNVD5O3Z.js': ['@goplasmatic/datalogic-wasm'],
      'assets/WorkflowEditorPage-Bc_YsB1j.js': [],
      [GRAPH]: ['@dagrejs/dagre', '@vue-flow/controls', '@vue-flow/core'],
    },
    styled: { [ENTRY_CSS]: [], [GRAPH_CSS]: VUE_FLOW_CSS },
  }
}

const GRAPH_SUBJECT = 'the workflow graph (Vue Flow and dagre)'
const CHECK_1 = `${GRAPH_SUBJECT} has no chunk of its own`
const CHECK_2 = `${GRAPH_SUBJECT} is on the first-load path`
const CHECK_3 = `${GRAPH_SUBJECT}'s payload is attached to a first-load chunk`
const NO_STYLESHEET = `no stylesheet in the build carries ${GRAPH_SUBJECT}'s CSS`

describe('check-bundle-split.mjs, the workflow graph’s subject', () => {
  let scratch: string

  beforeEach(() => {
    scratch = mkdtempSync(join(tmpdir(), 'kelir-bundle-split-'))
    mkdirSync(join(scratch, 'scripts'))
    mkdirSync(join(scratch, 'kelir-frontend', 'dist', '.vite'), { recursive: true })
    copyFileSync(SCRIPT, join(scratch, 'scripts', 'check-bundle-split.mjs'))
  })

  afterEach(() => {
    rmSync(scratch, { recursive: true, force: true })
  })

  function check(build: Build): { status: number | null; output: string } {
    const vite = join(scratch, 'kelir-frontend', 'dist', '.vite')

    writeFileSync(join(vite, 'manifest.json'), JSON.stringify(build.manifest))
    writeFileSync(join(vite, 'chunk-packages.json'), JSON.stringify(build.carried))

    if (build.styled) {
      writeFileSync(join(vite, 'stylesheet-packages.json'), JSON.stringify(build.styled))
    }

    const run = spawnSync(process.execPath, [join(scratch, 'scripts', 'check-bundle-split.mjs')], {
      encoding: 'utf8',
    })

    return { status: run.status, output: `${run.stdout}${run.stderr}` }
  }

  it('passes the build as it is, naming the graph’s split chunk and its stylesheet', () => {
    const run = check(clean())

    expect(run.output).toContain(
      `✓ ${GRAPH_SUBJECT} is off the first-load path — 1 split chunk(s), 0 payload asset(s), 1 stylesheet(s)`,
    )
    expect(run.status).toBe(0)
  })

  describe('check 1: it has a chunk of its own', () => {
    it('fails a build that folded Vue Flow and dagre into the entry', () => {
      const build = clean()

      delete build.manifest['src/features/workflow-builder/WorkflowGraph.vue']
      delete build.carried[GRAPH]
      build.carried[ENTRY].push('@dagrejs/dagre', '@vue-flow/controls', '@vue-flow/core')
      build.styled = { [ENTRY_CSS]: VUE_FLOW_CSS }

      const run = check(build)

      expect(run.output).toContain(CHECK_1)
      expect(run.output).toContain(`    ${ENTRY}`)
      expect(run.status).toBe(1)
    })
  })

  describe('check 2: nothing on first load carries it', () => {
    it.each([
      ['dagre, in the layout', LAYOUT, '@dagrejs/dagre'],
      ['graphlib, in the dashboard', DASHBOARD, '@dagrejs/graphlib'],
      ['@vueuse/core, in the entry', ENTRY, '@vueuse/core'],
      ['the controls, in the entry', ENTRY, '@vue-flow/controls'],
    ])('fails a build with %s, while the split chunk survives', (_, file, name) => {
      const build = clean()

      build.carried[file] = [...build.carried[file], name]

      const run = check(build)

      expect(run.output).toContain(CHECK_2)
      expect(run.output).toContain(`    ${file}`)
      expect(run.output).not.toContain(CHECK_1)
      expect(run.status).toBe(1)
    })

    it('fails a chunk the home route reaches only through a chain of static imports', () => {
      const build = clean()

      build.manifest['_shared-Xy.js'] = { file: 'assets/shared-Xy.js', imports: ['_deeper-Zz.js'] }
      build.manifest['_deeper-Zz.js'] = { file: 'assets/deeper-Zz.js' }
      build.manifest['src/layouts/AppLayout.vue'].imports = ['index.html', '_shared-Xy.js']
      build.carried['assets/shared-Xy.js'] = []
      build.carried['assets/deeper-Zz.js'] = ['@vue-flow/core']

      const run = check(build)

      expect(run.output).toContain(CHECK_2)
      expect(run.output).toContain('    assets/deeper-Zz.js')
      expect(run.status).toBe(1)
    })

    it('passes a chunk reached only by a dynamic import, the edge the decision paid for', () => {
      const build = clean()

      build.manifest['src/layouts/AppLayout.vue'].dynamicImports = [
        'src/features/workflow-builder/WorkflowGraph.vue',
      ]

      expect(check(build).status).toBe(0)
    })
  })

  describe('check 3: no first-load chunk carries its stylesheets', () => {
    it('fails Vue Flow’s stylesheets folded into the entry’s CSS, the gap ADR-0046 §5 named', () => {
      const build = clean()

      build.styled = { [ENTRY_CSS]: ['@vue-flow/core'], [GRAPH_CSS]: VUE_FLOW_CSS }

      const run = check(build)

      expect(run.output).toContain(CHECK_3)
      expect(run.output).toContain(`    ${ENTRY_CSS}`)
      expect(run.output).not.toContain(CHECK_1)
      expect(run.output).not.toContain(CHECK_2)
      expect(run.status).toBe(1)
    })

    it('fails them in a CSS file of any name a home-route chunk lists', () => {
      const build = clean()
      const renamed = 'assets/style-0a1b2c3d.css'

      build.manifest['src/layouts/AppLayout.vue'].css = [renamed]
      build.styled = { ...build.styled, [renamed]: ['@vue-flow/controls'] }

      const run = check(build)

      expect(run.output).toContain(CHECK_3)
      expect(run.output).toContain(`    ${renamed}`)
      expect(run.status).toBe(1)
    })

    it('passes them in a CSS file of any name that only the graph’s chunk lists', () => {
      const build = clean()
      const renamed = 'assets/vue-flow-9f8e7d6c.css'

      build.manifest['src/features/workflow-builder/WorkflowGraph.vue'].css = [renamed]
      build.styled = { [ENTRY_CSS]: [], [renamed]: VUE_FLOW_CSS }

      const run = check(build)

      expect(run.output).toContain('1 split chunk(s), 0 payload asset(s), 1 stylesheet(s)')
      expect(run.status).toBe(0)
    })

    it('reads a stylesheet off first load only from `css`, not from `assets`', () => {
      const build = clean()

      // Listed as an asset of the entry and as nothing's CSS: not first-load CSS.
      build.manifest['index.html'].assets = [GRAPH_CSS]

      expect(check(build).status).toBe(0)
    })
  })

  describe('a subject that ships a stylesheet must be seen in one', () => {
    it('fails a build whose stylesheet map records none of Vue Flow’s', () => {
      const build = clean()

      build.styled = { [ENTRY_CSS]: [], [GRAPH_CSS]: [] }

      const run = check(build)

      expect(run.output).toContain(NO_STYLESHEET)
      expect(run.status).toBe(1)
    })

    it('fails a build whose stylesheet map is empty', () => {
      const build = clean()

      build.styled = {}

      const run = check(build)

      expect(run.output).toContain(NO_STYLESHEET)
      expect(run.status).toBe(1)
    })

    it('fails a build with no stylesheet map at all, and says what writes it', () => {
      const build = clean()

      build.styled = null

      const run = check(build)

      expect(run.output).toContain('stylesheet-packages.json is not readable')
      expect(run.output).toContain('`kelir-chunk-packages` plugin')
      expect(run.status).toBe(1)
    })

    it('asks no stylesheet of a subject that ships none', () => {
      const build = clean()

      // Unovis and the evaluator have no stylesheet in the map; the build passes.
      expect(Object.values(build.styled!).flat()).not.toContain('@unovis/ts')
      expect(check(build).output).toContain(
        '✓ the Unovis chart library is off the first-load path — 1 split chunk(s), 0 payload asset(s), 0 stylesheet(s)',
      )
    })
  })

  it('fails a build where no chunk carries the graph at all, rather than passing on nothing', () => {
    const build = clean()

    delete build.carried[GRAPH]

    const run = check(build)

    expect(run.output).toContain(
      'no chunk in the build carries @vue-flow/core or @vue-flow/controls or @dagrejs/dagre or @dagrejs/graphlib or @vueuse/core',
    )
    expect(run.status).toBe(1)
  })
})
