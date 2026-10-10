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
    //
    // `no-restricted-imports` sees import and export declarations only, so
    // `no-restricted-syntax` refuses an `import()` of the same packages, by a
    // string or a template with no `${}`. Both are generated from the one
    // list, with one message each, so neither can name a package the other
    // misses. Every `/` in the pattern is escaped, because esquery reads it
    // inside a `/…/` literal.
    files: ['**/*.{ts,mts,cts,tsx,vue,js,mjs}'],
    rules: (() => {
      const restricted = [
        ['zod', 'a second validator beside the server’s (D-86, D-95, #541)'],
        [
          '@vueuse/core',
          'Vue Flow inlines what it takes from it, so no chunk names it and this rule is the guard',
        ],
        ['reka-ui', 'shadcn-vue’s primitives in the tree carry none'],
        ['vue-sonner', 'a refusal is shown where it happened, not in a toast'],
        ['@lucide/vue', 'icons come from lucide-vue-next alone; two would ship one set twice'],
        ['json-logic-js', 'JSON Logic is evaluated by datalogic-wasm (D-10, ADR-0008)'],
        ['vuedraggable', 'the drag-and-drop library is vue-draggable-plus'],
        ['elkjs', 'the graph is laid out by @dagrejs/dagre; elkjs is copyleft'],
        ['dagre', 'the unmaintained package; the layout is @dagrejs/dagre'],
      ].map(([name, why]) => ({
        pattern: `^${name.replace(/[/.]/g, '\\$&')}(?:\\/.*)?$`,
        message: `Kelir code does not import ${name}: ${why} (ADR-0046 §3.4, §5).`,
      }))

      return {
        'no-restricted-imports': [
          'error',
          { patterns: restricted.map(({ pattern, message }) => ({ regex: pattern, message })) },
        ],
        'no-restricted-syntax': [
          'error',
          ...restricted.map(({ pattern, message }) => ({
            selector:
              `ImportExpression:matches([source.value=/${pattern}/], ` +
              `[source.type="TemplateLiteral"][source.expressions.length=0]` +
              `[source.quasis.0.value.cooked=/${pattern}/])`,
            message,
          })),
        ],
      }
    })(),
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
