import { appendFileSync } from 'node:fs'

import { chromium, firefox } from '@playwright/test'
import type { FullConfig, FullResult, Reporter, Suite } from '@playwright/test/reporter'

/**
 * The three-browser check of the compatibility run (#962, **D-153**,
 * NFR-CMP-001).
 *
 * CI's `Browser compatibility (Chrome, Edge, Firefox)` job runs it with
 * `--list`, both switches set and no `--project`, before a test runs. It reads
 * the configuration Playwright loaded, not a copy of it, so a project dropped
 * or pointed at the wrong browser is red here:
 *
 * - `chrome`, `edge` and `firefox` exist, on the engine and channel each
 *   names;
 * - `chrome` and `edge` list exactly the flows `chromium` lists;
 * - `firefox` lists every file any project lists, its own flow included.
 *
 * Then it launches each of the three on the engine and channel it must name,
 * through Playwright as a test would, and prints the version the running
 * browser reports. A browser that cannot be launched is red, not skipped. The
 * lines are also appended to the file `KELIR_E2E_VERSIONS_FILE` names, when it
 * names one, so the job keeps them.
 */

type Target = { project: string; browserName: 'chromium' | 'firefox'; channel?: string }

const TARGETS: Target[] = [
  { project: 'chrome', browserName: 'chromium', channel: 'chrome' },
  { project: 'edge', browserName: 'chromium', channel: 'msedge' },
  { project: 'firefox', browserName: 'firefox' },
]

/** Each listed test as `file › title`, per project. */
function listed(suite: Suite): Map<string, string[]> {
  const byProject = new Map<string, string[]>()

  for (const projectSuite of suite.suites) {
    const name = projectSuite.project()?.name ?? ''
    const tests = projectSuite.allTests().map((test) => test.titlePath().slice(2).join(' › '))
    byProject.set(name, tests.sort())
  }

  return byProject
}

function fileOf(test: string): string {
  return test.split(' › ')[0] ?? ''
}

export default class CompatibilityReporter implements Reporter {
  private readonly problems: string[] = []

  printsToStdio(): boolean {
    return true
  }

  onBegin(config: FullConfig, suite: Suite): void {
    for (const target of TARGETS) {
      const project = config.projects.find((candidate) => candidate.name === target.project)

      if (project === undefined) {
        this.problems.push(`no project named ${target.project}`)
        continue
      }

      // A device descriptor names its engine as `defaultBrowserType`, which
      // Playwright reads when `browserName` is unset.
      const browserName = project.use.browserName ?? project.use.defaultBrowserType ?? 'chromium'
      const channel = project.use.channel

      if (browserName !== target.browserName || channel !== target.channel) {
        this.problems.push(
          `${target.project} runs ${browserName}${channel ? ` (channel ${channel})` : ''}, ` +
            `not ${target.browserName}${target.channel ? ` (channel ${target.channel})` : ''}`,
        )
      }
    }

    const byProject = listed(suite)
    const chromiumTests = byProject.get('chromium')

    if (chromiumTests === undefined || chromiumTests.length === 0) {
      this.problems.push('chromium lists no flow, so there is nothing to hold chrome and edge to')
    } else {
      for (const name of ['chrome', 'edge']) {
        const tests = byProject.get(name) ?? []

        if (tests.join('\n') !== chromiumTests.join('\n')) {
          this.problems.push(
            `${name} lists ${tests.length} flows, not the ${chromiumTests.length} chromium lists`,
          )
        }
      }
    }

    const everyFile = new Set([...byProject.values()].flat().map(fileOf))
    const firefoxFiles = new Set((byProject.get('firefox') ?? []).map(fileOf))
    const missing = [...everyFile].filter((file) => !firefoxFiles.has(file))

    if (missing.length > 0) {
      this.problems.push(
        `firefox lists ${firefoxFiles.size} of ${everyFile.size} files; missing ${missing.join(', ')}`,
      )
    }

    for (const [name, tests] of byProject) {
      console.log(`${name}: ${tests.length} flows in ${new Set(tests.map(fileOf)).size} files`)
    }
  }

  async onEnd(_result: FullResult): Promise<{ status: FullResult['status'] }> {
    const lines: string[] = []

    for (const target of TARGETS) {
      const engine = target.browserName === 'firefox' ? firefox : chromium

      try {
        const browser = await engine.launch({ channel: target.channel })
        lines.push(
          `${target.project}: ${target.browserName}${target.channel ? ` channel ${target.channel}` : ''} ${browser.version()}`,
        )
        await browser.close()
      } catch (error) {
        const message = error instanceof Error ? error.message.split('\n')[0] : String(error)
        this.problems.push(`${target.project} cannot be launched: ${message}`)
      }
    }

    for (const line of lines) {
      console.log(line)
    }

    const file = process.env.KELIR_E2E_VERSIONS_FILE

    if (file !== undefined && file !== '') {
      appendFileSync(file, lines.map((line) => `${line}\n`).join(''))
    }

    for (const problem of this.problems) {
      console.error(`three-browser check: ${problem}`)
    }

    return { status: this.problems.length === 0 ? 'passed' : 'failed' }
  }
}
