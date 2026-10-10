import js from '@eslint/js'
import globals from 'globals'
import tseslint from 'typescript-eslint'
import pluginVue from 'eslint-plugin-vue'
import prettier from 'eslint-config-prettier'

/**
 * Packages Kelir code does not import, each with its reason
 * ([ADR-0046](../docs/architectures/adr/0046.%20The%20Builders%20Drag%20with%20Vue%20Draggable%20Plus%20and%20Draw%20with%20Vue%20Flow%20and%20Dagre,%20Off%20the%20First-Load%20Path.md)
 * §5; #688 G4).
 *
 * Each was in one of the builders' references, or is the icon package #698
 * replaced, and **several would resolve if imported**: `@vueuse/core` arrives
 * with Vue Flow and `elkjs` with Unovis. So a missing package is not the
 * guard; this is.
 *
 * **`@lucide/vue` is not on the list.** It is the icon package the tree uses
 * since #698. `lucide-vue-next` is the refused one, and `components.json`'s
 * `iconLibrary: "lucide"` may make `shadcn-vue add` emit it.
 */
const REFUSED_IMPORTS = [
  ['zod', 'D-86 and D-95 refuse a Zod mirror of a meta-schema; the server validates'],
  ['@vueuse/core', 'It ships inside the workflow graph chunk only; Kelir code does not import it'],
  ['reka-ui', 'No shadcn-vue primitive in the tree needs it; ADR-0040 §6 decides when one does'],
  ['vue-sonner', 'A refusal is shown where it happened, not in a toast'],
  ['json-logic-js', 'D-10: JSON Logic runs on datalogic-wasm, through src/lib/jsonlogic.ts'],
  ['vuedraggable', 'The form builder drags with vue-draggable-plus'],
  ['elkjs', 'Refused on its licence; the workflow graph lays out with @dagrejs/dagre'],
  ['dagre', 'Unmaintained; the workflow graph lays out with @dagrejs/dagre'],
  ['lucide-vue-next', 'Deprecated (#698); import icons from @lucide/vue'],
]

export default tseslint.config(
  { ignores: ['dist/**', 'node_modules/**', 'coverage/**'] },

  js.configs.recommended,
  ...tseslint.configs.recommended,
  ...pluginVue.configs['flat/recommended'],

  {
    files: ['**/*.{ts,vue}'],
    languageOptions: {
      globals: globals.browser,
      parserOptions: {
        parser: tseslint.parser,
        ecmaVersion: 'latest',
        sourceType: 'module',
      },
    },
    rules: {
      // Coding standard 3.1: `any` needs an inline justification, so it is an
      // error rather than a warning; prefer `unknown` plus narrowing.
      '@typescript-eslint/no-explicit-any': 'error',
      // Coding standard 3.2: SFC block order is script, template, style.
      'vue/block-order': ['error', { order: ['script', 'template', 'style'] }],
    },
  },

  {
    files: ['**/*.spec.ts'],
    languageOptions: {
      globals: globals.node,
    },
  },

  // ADR-0046 §5 (#688 G4, #698): packages Kelir code does not import.
  {
    files: ['**/*.{ts,vue}'],
    rules: {
      'no-restricted-imports': [
        'error',
        {
          patterns: REFUSED_IMPORTS.map(([name, why]) => ({
            group: [name, `${name}/*`],
            message: `${why} (ADR-0046)`,
          })),
        },
      ],
    },
  },

  {
    // shadcn-vue primitives are single-word by convention (Button, Input) and
    // are added by its generator. Renaming them to satisfy the rule would break
    // `shadcn-vue add` and diverge from every upstream example, so the rule is
    // scoped off here rather than fought file by file.
    files: ['src/components/ui/**/*.vue'],
    rules: {
      'vue/multi-word-component-names': 'off',
    },
  },

  // Must stay last: turns off the stylistic rules Prettier owns.
  prettier,
)
