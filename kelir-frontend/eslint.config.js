import js from '@eslint/js'
import globals from 'globals'
import tseslint from 'typescript-eslint'
import pluginVue from 'eslint-plugin-vue'
import prettier from 'eslint-config-prettier'

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
    // ADR-0046 §5 and coding standard §3.4: packages Kelir code does not
    // import. Each is matched as the package and its subpaths, by `regex`
    // rather than a gitignore-style group, because the group `dagre` would
    // also match `@dagrejs/dagre`, the layout Kelir does use.
    files: ['**/*.{ts,vue,js}'],
    rules: {
      'no-restricted-imports': [
        'error',
        {
          patterns: [
            ['zod', 'a second validator beside the server’s (D-86, D-95, #541)'],
            [
              '@vueuse/core',
              'it arrives only as Vue Flow’s dependency and stays in the graph’s lazy chunk',
            ],
            ['reka-ui', 'shadcn-vue’s primitives in the tree carry none'],
            ['vue-sonner', 'a refusal is shown where it happened, not in a toast'],
            ['@lucide/vue', 'icons come from lucide-vue-next alone; two would ship one set twice'],
            ['json-logic-js', 'JSON Logic is evaluated by datalogic-wasm (D-10, ADR-0008)'],
            ['vuedraggable', 'the drag-and-drop library is vue-draggable-plus'],
            ['elkjs', 'the graph is laid out by @dagrejs/dagre; elkjs is copyleft'],
            ['dagre', 'the unmaintained package; the layout is @dagrejs/dagre'],
          ].map(([name, why]) => ({
            regex: `^${name.replace(/[/.]/g, '\\$&')}(?:/.*)?$`,
            message: `Kelir code does not import ${name}: ${why} (ADR-0046 §3.4, §5).`,
          })),
        },
      ],
    },
  },

  {
    files: ['**/*.spec.ts'],
    languageOptions: {
      globals: globals.node,
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
