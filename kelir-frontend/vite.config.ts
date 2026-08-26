import { fileURLToPath, URL } from 'node:url'

import { defineConfig } from 'vitest/config'
import vue from '@vitejs/plugin-vue'
import tailwindcss from '@tailwindcss/vite'

export default defineConfig({
  plugins: [vue(), tailwindcss()],
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
