import { defineConfig, devices } from '@playwright/test'

import { baseUrl } from './support/env'

/** The flows that run in Firefox and not in Chromium. */
const FIREFOX_ONLY = /a-pattern-the-browser-gives-up-on\.spec\.ts$/

/**
 * The two one-off switches of the compatibility check (#962, **D-153**,
 * NFR-CMP-001). Each is read as exactly `1`, so an unset, empty or other value
 * leaves the configuration as it was before them, and `npm test` and the
 * `e2e` job run what they ran.
 *
 * - `KELIR_E2E_BRANDED=1` adds `chrome` and `edge`: the installed Google Chrome
 *   and Microsoft Edge, each running exactly what `chromium` runs.
 * - `KELIR_E2E_FIREFOX_ALL=1` widens `firefox` from its one flow to the whole
 *   suite.
 *
 * CI's `Browser compatibility (Chrome, Edge, Firefox)` job, started by hand,
 * sets both. Serial, one worker and no retries hold for every project.
 */
const BRANDED = process.env.KELIR_E2E_BRANDED === '1'
const FIREFOX_ALL = process.env.KELIR_E2E_FIREFOX_ALL === '1'

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

  /**
   * Chromium runs every flow but one, and Firefox runs that one (#496).
   *
   * `a-pattern-the-browser-gives-up-on.spec.ts` is about a match Firefox
   * throws on, so it needs Firefox. Chromium does not throw on the same match,
   * it keeps matching, so the spec is kept out of it: there it would hold the
   * tab until the test timed out. Every other flow stays Chromium-only, as it
   * was.
   *
   * The compatibility check's switches, above, add two Chromium-engine
   * projects with Chromium's scope, and drop Firefox's `testMatch`. Firefox
   * then runs every flow, its own included; Chrome and Edge never get that one,
   * for Chromium's reason.
   */
  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
      testIgnore: FIREFOX_ONLY,
    },
    {
      name: 'firefox',
      use: { ...devices['Desktop Firefox'] },
      ...(FIREFOX_ALL ? {} : { testMatch: FIREFOX_ONLY }),
    },
    ...(BRANDED
      ? [
          {
            name: 'chrome',
            use: { ...devices['Desktop Chrome'], channel: 'chrome' },
            testIgnore: FIREFOX_ONLY,
          },
          {
            name: 'edge',
            use: { ...devices['Desktop Edge'], channel: 'msedge' },
            testIgnore: FIREFOX_ONLY,
          },
        ]
      : []),
  ],
})
