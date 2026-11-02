import { readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'

import { expect, test, type BrowserContext, type Page } from '@playwright/test'

/**
 * One Chromium session under the load (#777 AC6): 30 list opens and 30 detail
 * opens, interleaved, five seconds apart, inside the measured window.
 *
 * The shape is #772's: the page times itself with `performance.now()`, so what
 * is read is the page's own clock and not the harness's round trip to it.
 *
 * # A sample is a navigation inside the app
 *
 * The session signs in once, as the 51st user, and every sample after that is a
 * route change in the loaded shell. It cannot click its way there: a detail
 * sample picks at random from the 500 typical documents (AC3), and the list's
 * first page shows 20 rows. So it does what the back button does: it writes the
 * address, then fires the `popstate` vue-router listens for. The router, the
 * guards, `AppLayout`'s unread-count watch and the lazy route chunks all run as
 * they do on a click. `performance.timeOrigin` is compared at the end, which
 * would show a document load.
 *
 * # What is the time
 *
 * From just before the route change to the frame that shows the content:
 * `documents-table` with a row in it for a list, and a control inside
 * `document-form` for a detail. Polled once a frame, so the figure carries up
 * to one frame of the display's cadence.
 *
 * # Cold
 *
 * The context is fresh, so the first sample of each view fetches that view's
 * lazy chunk and everything under it. Those two samples are flagged `cold` and
 * stay in the 30, as AC6 has them. `report.mjs` also gives the figures without
 * them, because that difference is what #771's trigger is judged on.
 *
 * Bytes come from the DevTools protocol: `encodedDataLength` is what crossed
 * the wire, and it is why this session sets no route handler, which would
 * disable the HTTP cache being observed.
 */

const SAMPLES_PER_VIEW = 30
// AC6 says at least 5 seconds. A timer lands a few milliseconds either side of
// its moment, and an exact 5,000 measured 4,988 at its closest on 2026-11-02.
const SPACING_MS = 5_020
/** Just inside the window, so the last sample of the 60 is inside it as well. */
const FIRST_SAMPLE_DELAY_MS = 1_000
const RAMP_MS = 30_000
/** Partway up the ramp, so the 51st sign-in is one of its 51. */
const SIGN_IN_DELAY_MS = 15_000
const WINDOW_DELAY_MS = RAMP_MS + 5_000
/** No view is expected near this. It only keeps a dead stack from hanging a run. */
const SAMPLE_TIMEOUT_MS = 60_000

const startMs = Number(process.env.KELIR_LOAD_START_MS)
const windowMs = Number(process.env.KELIR_LOAD_WINDOW_MS ?? 300_000)
const runDirectory = process.env.KELIR_LOAD_RUN_DIRECTORY as string
const password = process.env.KELIR_LOAD_PASSWORD as string
const detailIds = JSON.parse(
  readFileSync(process.env.KELIR_LOAD_DETAIL_IDS as string, 'utf8'),
) as string[]

interface Tally {
  count: number
  bytes: number
  notModified: number
  fromCache: number
}

interface Sample {
  index: number
  view: 'list' | 'detail'
  cold: boolean
  at: number
  ms: number
  settledMs: number
  ok: boolean
  api: Tally
  assets: Tally
  urls: string[]
}

interface Seen {
  url: string
  type: string
  sample: number
  sentAt: number
  sentWallMs: number
  status?: number
  fromCache?: boolean
  bytes?: number
  finishedAt?: number
}

const emptyTally = (): Tally => ({ count: 0, bytes: 0, notModified: 0, fromCache: 0 })

/** Every network request of the session, each filed under the sample it fell in. */
async function watchNetwork(context: BrowserContext, page: Page) {
  const client = await context.newCDPSession(page)
  const seen = new Map<string, Seen>()
  let current = -1

  await client.send('Network.enable')

  client.on('Network.requestWillBeSent', (event) => {
    seen.set(event.requestId, {
      url: event.request.url,
      type: event.type ?? 'Other',
      sample: current,
      sentAt: event.timestamp,
      sentWallMs: Math.round(event.wallTime * 1000),
    })
  })
  client.on('Network.requestServedFromCache', (event) => {
    const entry = seen.get(event.requestId)

    if (entry) {
      entry.fromCache = true
    }
  })
  client.on('Network.responseReceived', (event) => {
    const entry = seen.get(event.requestId)

    if (entry) {
      entry.status = event.response.status
      entry.fromCache ||= event.response.fromDiskCache
    }
  })
  client.on('Network.loadingFinished', (event) => {
    const entry = seen.get(event.requestId)

    if (entry) {
      entry.bytes = event.encodedDataLength
      entry.finishedAt = event.timestamp
    }
  })

  return {
    enter(sample: number) {
      current = sample
    },
    entries() {
      return [...seen.values()]
    },
  }
}

/** Runs in the page: change the route, and wait for the frame with the content. */
async function openInsideTheApp({
  url,
  content,
  timeoutMs,
}: {
  url: string
  content: string
  timeoutMs: number
}): Promise<{ ms: number; ok: boolean }> {
  const started = performance.now()
  const state = history.state ?? {}

  history.pushState(
    {
      ...state,
      back: state.current ?? null,
      current: url,
      forward: null,
      position: (state.position ?? 0) + 1,
      replaced: false,
    },
    '',
    url,
  )
  window.dispatchEvent(new PopStateEvent('popstate', { state: history.state }))

  return new Promise((resolve) => {
    const look = (): void => {
      const elapsed = performance.now() - started
      const found = document.querySelector(content)

      if (found && found.getClientRects().length > 0) {
        resolve({ ms: elapsed, ok: true })
      } else if (
        document.querySelector('[data-testid="documents-error"], [data-testid="document-error"]')
      ) {
        resolve({ ms: elapsed, ok: false })
      } else if (elapsed > timeoutMs) {
        resolve({ ms: elapsed, ok: false })
      } else {
        requestAnimationFrame(look)
      }
    }

    look()
  })
}

function waitUntil(moment: number): Promise<void> {
  return new Promise((done) => setTimeout(done, Math.max(0, moment - Date.now())))
}

function tallies(entries: Seen[]): Pick<Sample, 'api' | 'assets' | 'settledMs' | 'urls'> {
  const api = emptyTally()
  const assets = emptyTally()

  for (const entry of entries) {
    const path = new URL(entry.url).pathname
    const tally = path.startsWith('/api/') ? api : path.startsWith('/assets/') ? assets : null

    if (tally) {
      tally.count += 1
      tally.bytes += entry.fromCache ? 0 : (entry.bytes ?? 0)
      tally.notModified += entry.status === 304 ? 1 : 0
      tally.fromCache += entry.fromCache ? 1 : 0
    }
  }

  const sent = entries.map((entry) => entry.sentAt)
  const done = entries.map((entry) => entry.finishedAt ?? entry.sentAt)

  return {
    api,
    assets,
    settledMs: sent.length ? (Math.max(...done) - Math.min(...sent)) * 1000 : 0,
    urls: entries.map((entry) => {
      const url = new URL(entry.url)

      return `${entry.status ?? '-'} ${url.pathname}${url.search}`
    }),
  }
}

test('each view is timed in Chromium while the scenarios run', async ({ browser }) => {
  const context = await browser.newContext({ baseURL: process.env.KELIR_LOAD_BASE_URL })
  const page = await context.newPage()
  const network = await watchNetwork(context, page)
  const windowStart = startMs + WINDOW_DELAY_MS
  const windowEnd = windowStart + windowMs

  await waitUntil(startMs + SIGN_IN_DELAY_MS)
  await page.goto('/login')
  await page.getByLabel('Username or email').fill('load-user-051')
  await page.getByLabel('Password').fill(password)

  const signedInAt = Date.now()

  await page.getByRole('button', { name: 'Sign in' }).click()
  await expect(page).toHaveURL(/\/$/, { timeout: 30_000 })
  await page.waitForLoadState('networkidle')

  const timeOrigin = await page.evaluate(() => performance.timeOrigin)
  const samples: Sample[] = []

  for (let index = 0; index < SAMPLES_PER_VIEW * 2; index += 1) {
    await waitUntil(windowStart + FIRST_SAMPLE_DELAY_MS + index * SPACING_MS)

    if (Date.now() >= windowEnd) {
      break
    }

    const view = index % 2 === 0 ? 'list' : 'detail'
    const id = detailIds[Math.floor(Math.random() * detailIds.length)]
    const at = Date.now()

    network.enter(index)

    const opened = await page.evaluate(openInsideTheApp, {
      url: view === 'list' ? '/documents' : `/documents/${id}`,
      content:
        view === 'list'
          ? '[data-testid="documents-table"] tbody tr'
          : '[data-testid="document-form"] input, [data-testid="document-form"] textarea',
      timeoutMs: SAMPLE_TIMEOUT_MS,
    })

    samples.push({
      index,
      view,
      cold: index < 2,
      at,
      ...opened,
      settledMs: 0,
      api: emptyTally(),
      assets: emptyTally(),
      urls: [],
    })
  }

  // The last sample's stragglers: nothing is timed after this.
  await page.waitForTimeout(1_000)

  const entries = network.entries()

  for (const sample of samples) {
    Object.assign(
      sample,
      tallies(entries.filter((entry) => entry.sample === sample.index)),
    )
  }

  const documentLoads = entries.filter((entry) => entry.sample >= 0 && entry.type === 'Document')
  const sameDocument = (await page.evaluate(() => performance.timeOrigin)) === timeOrigin

  writeFileSync(
    join(runDirectory, 'chromium.json'),
    JSON.stringify(
      {
        browserVersion: browser.version(),
        signedInAt,
        firstAt: samples[0]?.at,
        lastAt: samples.at(-1)?.at,
        // What the session paid before its first sample: login, shell, dashboard.
        shell: tallies(entries.filter((entry) => entry.sample < 0)),
        samples,
        refused: entries
          .filter((entry) => (entry.status ?? 200) >= 400)
          .map((entry) => ({
            status: entry.status,
            url: new URL(entry.url).pathname,
            at: entry.sentWallMs,
            sample: entry.sample,
          })),
      },
      null,
      1,
    ),
  )

  await context.close()

  // A failure here is a harness failure, stated, and not a number.
  expect(documentLoads, 'a sample loaded a document instead of navigating').toEqual([])
  expect(sameDocument, 'the page was reloaded during the session').toBe(true)
})
