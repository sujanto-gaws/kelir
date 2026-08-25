import { defineConfig, devices } from '@playwright/test'

import { baseUrl } from './support/env'

/**
 * The browser-driving harness (decision **D-14**, issue #153).
 *
 * **It drives a deployed stack, never a dev server.** There is no `webServer`
 * block on purpose: the flow has to run against what a release contains, which
 * is the Caddy image serving the built bundle and proxying `/api/*` to the
 * backend image — the stack `deploy/staging/deploy-local.sh` brings up. A
 * harness pointed at `vite dev` would exercise a bundle nobody ships and a
 * proxy nobody deploys, and would have proved nothing about either.
 *
 * The address comes from the environment because the stack's address does: the
 * deploy script publishes on a host IP and a port that both vary.
 */
export default defineConfig({
  testDir: './tests',

  /**
   * Serial, one worker.
   *
   * The suite seeds through the API into one shared database — the deployment's
   * database, not a per-worker fixture — so two workers would be two writers
   * against one list and the filter assertions would race each other. Parallel
   * workers are worth revisiting when a test needs its own tenant; until then
   * the honest configuration is the slow one.
   */
  fullyParallel: false,
  workers: 1,

  /**
   * No retries, in CI included.
   *
   * A retry turns a flake into a pass and a real intermittent defect into a
   * pass as well, which is the failure mode this project has already paid for
   * once in its verification passes. If a spec is flaky the spec is wrong.
   */
  retries: 0,

  /** Refuses `test.only` left behind in a branch. */
  forbidOnly: Boolean(process.env.CI),

  timeout: 60_000,
  expect: { timeout: 15_000 },

  /**
   * `list` for whoever is watching, `html` for whoever reads the failure later.
   * `open: 'never'` in both environments: a report that launches a browser is
   * useful on a laptop and hangs a CI step.
   */
  reporter: [['list'], ['html', { open: 'never' }]],

  use: {
    baseURL: baseUrl(),
    /**
     * `retain-on-failure` rather than `on-first-retry`, which is the Playwright
     * default and is dead configuration here: with `retries: 0` there is never
     * a first retry, so the default would capture nothing at all.
     */
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
    video: 'retain-on-failure',
    // The local-testing deployment serves plain HTTP over an IP, because no
    // certificate can be issued for one (deploy-local.sh). A hostname
    // deployment serves TLS and needs nothing here.
    ignoreHTTPSErrors: true,
  },

  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
    },
  ],
})
