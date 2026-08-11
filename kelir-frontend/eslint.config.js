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
    files: ['**/*.spec.ts'],
    languageOptions: {
      globals: globals.node,
    },
  },

  // Must stay last: turns off the stylistic rules Prettier owns.
  prettier,
)
