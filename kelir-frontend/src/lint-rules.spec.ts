import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

import { describe, expect, it } from 'vitest'

/**
 * ADR-0046 §5's import rule, pinned (#688 G4, #698).
 *
 * `eslint.config.js` is read as text rather than loaded, so this needs no
 * ESLint at test time. The rule's behaviour — red on a refused import — was
 * seen when it was added; this holds its list, which is the part a later edit
 * could narrow without anything failing.
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
