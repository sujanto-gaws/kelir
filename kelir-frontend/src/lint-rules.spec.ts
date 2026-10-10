import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { pathToFileURL } from 'node:url'

import { Linter } from 'eslint'
import { describe, expect, it } from 'vitest'

/**
 * ADR-0046 §5's import rule, pinned (#688 G4, #698).
 *
 * `eslint.config.js` is read as text rather than loaded, so this needs no
 * ESLint at test time. The rule's behaviour — red on a refused import — was
 * seen when it was added; this holds its list, which is the part a later edit
 * could narrow without anything failing. (The second block below does load it,
 * and runs the rule.)
 *
 * # Seen to fail (coding standard §2.9)
 *
 * Run 2026-10-10: deleting the `lucide-vue-next` entry reddens *names every
 * package ADR-0046 refuses*; adding `@lucide/vue` reddens *does not refuse
 * @lucide/vue*.
 */

const config = readFileSync(resolve(process.cwd(), 'eslint.config.js'), 'utf8')

/** The names in `REFUSED_IMPORTS`, in order. */
function refused(): string[] {
  const block = /const REFUSED_IMPORTS = \[([\s\S]*?)\n\]/.exec(config)?.[1] ?? ''

  return [...block.matchAll(/^\s*\['([^']+)',/gm)].map((match) => match[1])
}

describe('the refused-imports rule', () => {
  it('names every package ADR-0046 refuses, and the icon package #698 replaced', () => {
    expect(refused()).toEqual([
      'zod',
      '@vueuse/core',
      'reka-ui',
      'vue-sonner',
      'json-logic-js',
      'vuedraggable',
      'elkjs',
      'dagre',
      'lucide-vue-next',
    ])
  })

  it('does not refuse @lucide/vue, which is the icon package the tree uses', () => {
    expect(refused()).not.toContain('@lucide/vue')
  })

  it('applies the list as no-restricted-imports, citing the ADR', () => {
    expect(config).toContain("'no-restricted-imports'")
    expect(config).toContain('patterns: REFUSED_IMPORTS.map(')
    expect(config).toContain('(ADR-0046)')
  })
})

/**
 * **The rule itself, run** (test-engineer campaign, 2026-10-10, #688 row 11).
 *
 * The cases above read the list as text; these hand sample imports to ESLint's
 * own `no-restricted-imports` with the options `eslint.config.js` builds, so
 * what the list *does* is pinned too. Its patterns are gitignore patterns, and
 * a pattern with no slash matches at any depth.
 */
describe('the refused-imports rule, run', () => {
  async function verdicts(imports: string[]): Promise<Record<string, boolean>> {
    const url = pathToFileURL(resolve(process.cwd(), 'eslint.config.js')).href
    const config = (await import(/* @vite-ignore */ url)).default as Linter.Config[]
    const rule = config.find((entry) => entry.rules?.['no-restricted-imports'])?.rules?.[
      'no-restricted-imports'
    ]

    expect(rule).toBeDefined()

    const linter = new Linter({ configType: 'flat' })

    return Object.fromEntries(
      imports.map((source) => [
        source,
        linter
          .verify(`import x from '${source}'; export default x`, [
            { rules: { 'no-restricted-imports': rule } },
          ])
          .some((message) => message.ruleId === 'no-restricted-imports'),
      ]),
    )
  }

  it('refuses lucide-vue-next and every path into it, and allows @lucide/vue', async () => {
    expect(
      await verdicts([
        'lucide-vue-next',
        'lucide-vue-next/dist/esm/icons/circle.mjs',
        '@lucide/vue',
        '@lucide/vue/dist/esm/icons/circle.mjs',
      ]),
    ).toEqual({
      'lucide-vue-next': true,
      'lucide-vue-next/dist/esm/icons/circle.mjs': true,
      '@lucide/vue': false,
      '@lucide/vue/dist/esm/icons/circle.mjs': false,
    })
  })

  it('refuses each package the list names', async () => {
    const names = refused()

    expect(Object.values(await verdicts(names))).toEqual(names.map(() => true))
  })

  // DEFECT (test-engineer, 2026-10-10): `dagre` has no slash, so as a gitignore
  // pattern it matches at any depth and refuses `@dagrejs/dagre` — the layout
  // ADR-0046 §1 chose for the workflow graph, which row 10 (#687) imports. Seen
  // with ESLint 10.8.1: "'@dagrejs/dagre' import is restricted from being used
  // by a pattern". Flip to `it` when the pattern is anchored.
  it.fails('allows @dagrejs/dagre, the layout ADR-0046 chose for the workflow graph', async () => {
    expect(await verdicts(['@dagrejs/dagre'])).toEqual({ '@dagrejs/dagre': false })
  })
})
