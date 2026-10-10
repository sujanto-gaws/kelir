// @vitest-environment node
// Node's types are pulled in for this file alone: it runs ESLint over source
// text, with the project's own configuration.
/// <reference types="node" />
import { resolve } from 'node:path'

import { ESLint } from 'eslint'
import { beforeAll, describe, expect, it } from 'vitest'

/**
 * `eslint.config.js`'s `no-restricted-imports` rule, and the
 * `no-restricted-syntax` selectors generated from the same list for `import()`
 * (ADR-0046 §3.4, §5; #687): the test-engineer campaign over PR #719.
 *
 * **For `@vueuse/core` the lint rule is the guard**, ADR-0046 §5 says, since
 * Vue Flow inlines it and no chunk names it for `check:bundle` to see. The
 * rule was seen red by hand on one import of each package. Here it runs
 * over source text with the project's own configuration, so that a package
 * dropped from the list, a pattern that stops matching a subpath, or one that
 * starts matching `@dagrejs/dagre` turns `npm test` red.
 */

const RESTRICTED = [
  'zod',
  '@vueuse/core',
  'reka-ui',
  'vue-sonner',
  '@lucide/vue',
  'json-logic-js',
  'vuedraggable',
  'elkjs',
  'dagre',
]

let eslint: ESLint

beforeAll(() => {
  eslint = new ESLint({ cwd: resolve(__dirname, '../..') })
})

/** The two rules that refuse a restricted package: a declaration, and an `import()`. */
const RULES = ['no-restricted-imports', 'no-restricted-syntax']

/** The restricted-import messages ESLint gives a file of this text. */
async function refusals(text: string, filePath = 'src/features/probe/probe.ts'): Promise<string[]> {
  const [result] = await eslint.lintText(text, { filePath: resolve(__dirname, '../..', filePath) })

  return result.messages
    .filter((message) => RULES.includes(message.ruleId ?? ''))
    .map((message) => message.message)
}

describe('no-restricted-imports, the packages Kelir code does not import', () => {
  it.each(RESTRICTED)('refuses %s, citing ADR-0046', async (name) => {
    const messages = await refusals(`import probe from '${name}'\nvoid probe\n`)

    expect(messages).toHaveLength(1)
    expect(messages[0]).toContain(`Kelir code does not import ${name}:`)
    expect(messages[0]).toContain('ADR-0046')
  })

  it.each([
    'zod/v4',
    '@vueuse/core/index.mjs',
    '@vueuse/core/',
    'reka-ui/dist/index.js',
    '@lucide/vue/icons/plus',
    'elkjs/lib/elk.bundled.js',
    'dagre/lib/layout',
  ])('refuses the subpath %s', async (path) => {
    expect(await refusals(`import probe from '${path}'\nvoid probe\n`)).toHaveLength(1)
  })

  it('refuses a type-only import, a side-effect import and a re-export', async () => {
    expect(
      await refusals(`import type { ZodType } from 'zod'\nexport type T = ZodType\n`),
    ).toHaveLength(1)
    expect(await refusals(`import '@vueuse/core'\n`)).toHaveLength(1)
    expect(await refusals(`export * from 'json-logic-js'\n`)).toHaveLength(1)
    expect(await refusals(`export { useStorage } from '@vueuse/core'\n`)).toHaveLength(1)
  })

  it('refuses one in a single-file component’s script', async () => {
    const component = [
      '<script setup lang="ts">',
      "import { useDraggable } from '@vueuse/core'",
      'void useDraggable',
      '</script>',
      '',
      '<template><div /></template>',
      '',
    ].join('\n')

    expect(await refusals(component, 'src/features/probe/ProbeGraph.vue')).toHaveLength(1)
  })

  it.each([
    '@dagrejs/dagre',
    '@dagrejs/dagre/dist/dagre.esm.js',
    '@dagrejs/graphlib',
    '@vue-flow/core',
    '@vue-flow/controls',
    'lucide-vue-next',
    'vue-draggable-plus',
    'dagre-d3',
    'zodiac',
    'reka-ui-extra',
  ])('allows %s, which only looks like a restricted name', async (name) => {
    expect(await refusals(`import probe from '${name}'\nvoid probe\n`)).toEqual([])
  })

  // ESLint's core `no-restricted-imports` checks import and export
  // declarations only, not `import()` expressions, so a dynamic import of a
  // restricted package passed lint (the #719 campaign). For `@vueuse/core`
  // that was a hole in the guard ADR-0046 §5 names as *the* guard:
  // `check:bundle` cannot see it either, because a dynamic import splits the
  // package into a lazy chunk, off first load.
  it('refuses a dynamic import of a restricted package', async () => {
    expect(await refusals(`export const load = () => import('@vueuse/core')\n`)).toHaveLength(1)
  })

  it.each(RESTRICTED)(
    'refuses a dynamic import of %s with the declaration’s message',
    async (name) => {
      // ESLint puts its own preamble before the declaration's; the message
      // after it is the one list's, word for word.
      const [declared] = await refusals(`import probe from '${name}'\nvoid probe\n`)

      for (const text of [
        `export const load = () => import('${name}')\n`,
        `export const load = () => import(\`${name}/sub\`)\n`,
      ]) {
        const dynamic = await refusals(text)

        expect(dynamic).toHaveLength(1)
        expect(dynamic[0]).toMatch(/^Kelir code does not import /)
        expect(declared.endsWith(dynamic[0])).toBe(true)
      }
    },
  )

  it('refuses a dynamic import of a subpath, and by a double-quoted string', async () => {
    expect(await refusals(`void import('zod/v4')\n`)).toHaveLength(1)
    expect(await refusals(`void import("elkjs/lib/elk.bundled.js")\n`)).toHaveLength(1)
  })

  it('refuses a dynamic import in a single-file component’s script', async () => {
    const component = [
      '<script setup lang="ts">',
      "const load = () => import('@vueuse/core')",
      'void load',
      '</script>',
      '',
      '<template><div /></template>',
      '',
    ].join('\n')

    expect(await refusals(component, 'src/features/probe/ProbeGraph.vue')).toHaveLength(1)
  })

  it.each(['@dagrejs/dagre', '@vue-flow/core', 'dagre-d3', 'zodiac', './dagre', '@/lib/zod'])(
    'allows a dynamic import of %s, which only looks like a restricted name',
    async (name) => {
      expect(await refusals(`void import('${name}')\n`)).toEqual([])
    },
  )

  it('leaves a template with an expression to review: what it names is not known', async () => {
    expect(await refusals('export const load = (name: string) => import(`zod/${name}`)\n')).toEqual(
      [],
    )
  })

  it.each([
    'src/features/probe/probe.mts',
    'src/features/probe/probe.cts',
    'src/features/probe/Probe.tsx',
    'src/features/probe/probe.mjs',
    'src/features/probe/probe.js',
  ])('refuses a declaration and an `import()` in %s', async (filePath) => {
    expect(await refusals(`import probe from 'zod'\nvoid probe\n`, filePath)).toHaveLength(1)
    expect(await refusals(`void import('json-logic-js')\n`, filePath)).toHaveLength(1)
  })
})
