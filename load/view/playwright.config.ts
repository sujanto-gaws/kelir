import { defineConfig, devices } from '@playwright/test'

/**
 * The Chromium measurement (#777 AC6): one session, one test, no retries.
 *
 * Its own configuration and its own install, kept apart from `e2e/` on purpose:
 * the two share a Playwright version, which `load/package.json` pins to the one
 * `e2e/package-lock.json` resolves, and nothing else. The runner starts it, and
 * it reads where and when from the runner's environment.
 */
export default defineConfig({
  testDir: '.',
  fullyParallel: false,
  workers: 1,
  retries: 0,
  // The whole session: the ramp, the five-minute window and the lead-in.
  timeout: 11 * 60_000,
  reporter: 'line',
  outputDir: '../.run/playwright',
  use: {
    ...devices['Desktop Chrome'],
    baseURL: process.env.KELIR_LOAD_BASE_URL,
    trace: 'off',
    video: 'off',
    screenshot: 'off',
  },
})
