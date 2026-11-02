// Reads one measurement's raw samples and states what they say (#777 AC4, AC6).
//
// **P95 is nearest-rank**: the value at position ceil(0.95 n) of the sorted
// samples. It is computed here from the raw samples, never taken from k6, whose
// Trend interpolates.
//
// **A void is a finding, not a number.** `void` lists every reason this
// measurement cannot be read against the target, and the runner exits 1 on any.

import { existsSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'

const TARGETS_MS = { list: 1_000, detail: 2_000 }
const VOIDING = (status) => status === 401 || status === 403 || status === 429 || status >= 500
const REQUIRED_SCENARIO_SAMPLES = 1_000
const REQUIRED_CHROMIUM_SAMPLES = 30
const CHROMIUM_SPACING_MS = 5_000
const RAMP_MS = 30_000
/** From first sign-in to last request (AC4): the access token lives 15 minutes. */
const RUN_LIMIT_MS = 12 * 60_000

/** The value at position ceil(percent / 100 * n) of the sorted samples. */
export function nearestRank(samples, percent) {
  const sorted = [...samples].sort((left, right) => left - right)

  return sorted[Math.ceil((percent * sorted.length) / 100) - 1]
}

export function describe(samples) {
  if (samples.length === 0) {
    return { n: 0 }
  }

  return {
    n: samples.length,
    p50: nearestRank(samples, 50),
    p95: nearestRank(samples, 95),
    max: Math.max(...samples),
  }
}

/** The scenario's console lines, which k6 wraps in its own log format. */
export function parseConsole(text) {
  const parsed = { samples: {}, signIns: [], refused: [], statuses: {} }

  for (const match of text.matchAll(/"?msg"?[=:]"([SLET]),([^"]*)"/g)) {
    const [kind, body] = [match[1], match[2].split(',')]

    if (kind === 'S') {
      const [view, at, ms, bytes] = body

      ;(parsed.samples[view] ??= []).push({ at: Number(at), ms: Number(ms), bytes: Number(bytes) })
    } else if (kind === 'L') {
      const [vu, at, ms, status] = body.map(Number)

      parsed.signIns.push({ vu, at, ms, status })
    } else if (kind === 'E') {
      const [name, status, at, vu] = body

      parsed.refused.push({ name, status: Number(status), at: Number(at), vu: Number(vu) })
    } else {
      for (const pair of body[1].split(' ')) {
        const [status, count] = pair.split(':')

        parsed.statuses[status] = (parsed.statuses[status] ?? 0) + Number(count)
      }
    }
  }

  return parsed
}

/** One figure per view from the Chromium session's samples. */
function chromiumReading(chromium, view) {
  const samples = chromium.samples.filter((sample) => sample.view === view)
  const usable = samples.filter((sample) => sample.ok)
  const warm = usable.filter((sample) => !sample.cold)
  const cold = usable.find((sample) => sample.cold)
  const sum = (list, key) => list.reduce((total, sample) => total + sample[key], 0)

  return {
    ...describe(usable.map((sample) => sample.ms)),
    warm: describe(warm.map((sample) => sample.ms)),
    settled: describe(usable.map((sample) => sample.settledMs)),
    cold: cold && {
      ms: cold.ms,
      settledMs: cold.settledMs,
      assetRequests: cold.assets.count,
      assetBytes: cold.assets.bytes,
      apiBytes: cold.api.bytes,
    },
    warmAssets: {
      requests: sum(warm.map((sample) => sample.assets), 'count'),
      bytes: sum(warm.map((sample) => sample.assets), 'bytes'),
      revalidated: sum(warm.map((sample) => sample.assets), 'notModified'),
    },
    apiBytesPerOpen: warm.length ? Math.round(sum(warm.map((sample) => sample.api), 'bytes') / warm.length) : 0,
    apiRequestsPerOpen: warm.length ? sum(warm.map((sample) => sample.api), 'count') / warm.length : 0,
    failures: samples.length - usable.length,
  }
}

function count(text, pattern) {
  return (text.match(pattern) ?? []).length
}

function logCounts(directory) {
  const read = (name) => {
    const path = join(directory, name)

    return existsSync(path) ? readFileSync(path, 'utf8') : ''
  }
  const backend = read('backend.log')
  const statuses = JSON.parse(read('caddy-status.json') || '{}')
  const entries = Object.entries(statuses)

  return {
    backendErrorLines: count(backend, /\bERROR\b/g),
    backendWarnLines: count(backend, /\bWARN\b/g),
    poolTimedOut: count(backend, /pool timed out/gi),
    slowAcquire: count(backend, /time to acquire exceeded/gi),
    slowStatements: count(backend, /slow statement/gi),
    caddy: statuses,
    caddyRequests: entries.reduce((total, [, value]) => total + value, 0),
    caddy5xx: entries
      .filter(([status]) => status.startsWith('5'))
      .reduce((total, [, value]) => total + value, 0),
  }
}

/** What the five-a-second samples of the connections say (ADR-0045 §6). */
function poolReading(directory, windowStart, windowEnd) {
  const path = join(directory, 'pool-samples.csv')

  if (!existsSync(path)) {
    return { samples: 0 }
  }

  const rows = readFileSync(path, 'utf8')
    .split(/\r?\n/)
    .filter((line) => /^\d+,/.test(line))
    .map((line) => line.split(',').map(Number))
    .filter(([at]) => at >= windowStart && at < windowEnd)
  const active = rows.map((row) => row[2])
  const inCheck = rows.reduce((total, row) => total + row[3], 0)
  const activeTotal = active.reduce((total, value) => total + value, 0)

  return {
    samples: rows.length,
    connectionsOpenMax: Math.max(0, ...rows.map((row) => row[1])),
    active: describe(active),
    activeMean: rows.length ? activeTotal / rows.length : 0,
    atTenActive: active.filter((value) => value >= 10).length,
    shareOfActiveInTokenCheck: activeTotal ? inCheck / activeTotal : 0,
    lockWaitSamples: rows.filter((row) => row[4] > 0).length,
  }
}

function perRequest(directory) {
  const path = join(directory, 'k6-summary.json')

  if (!existsSync(path)) {
    return {}
  }

  const metrics = JSON.parse(readFileSync(path, 'utf8')).metrics

  return Object.fromEntries(
    Object.entries(metrics)
      .filter(([name]) => name.startsWith('http_req_duration{name:'))
      .map(([name, value]) => [
        name.slice('http_req_duration{name:'.length, -1),
        { count: value.count, p50: value.med, p95: value['p(95)'], max: value.max },
      ]),
  )
}

export function summarize(directory) {
  const run = JSON.parse(readFileSync(join(directory, 'run.json'), 'utf8'))
  const k6 = parseConsole(readFileSync(join(directory, 'k6-console.log'), 'utf8'))
  const chromiumPath = join(directory, 'chromium.json')
  const chromium = existsSync(chromiumPath)
    ? JSON.parse(readFileSync(chromiumPath, 'utf8'))
    : { samples: [] }
  const windowStart = run.startMs + RAMP_MS + 5_000
  const windowEnd = windowStart + run.windowMs
  const generatorStart = windowStart + run.clockOffsetMs
  const generatorEnd = windowEnd + run.clockOffsetMs

  const ofView = (view) => k6.samples[view] ?? []
  const scenario = Object.fromEntries(
    ['list', 'detail', 'detail_settled', 'list_filtered'].map((view) => {
      const samples = ofView(view)

      return [
        view,
        {
          ...describe(samples.map((sample) => sample.ms)),
          meanBytes: samples.length
            ? Math.round(samples.reduce((total, sample) => total + sample.bytes, 0) / samples.length)
            : 0,
        },
      ]
    }),
  )
  const browser = {
    list: chromiumReading(chromium, 'list'),
    detail: chromiumReading(chromium, 'detail'),
  }

  const reasons = []
  const refusedInWindow = k6.refused.filter(({ at }) => at >= generatorStart && at < generatorEnd)
  const voiding = refusedInWindow.filter(({ status }) => VOIDING(status))
  const browserRefused = (chromium.refused ?? []).filter(
    ({ at, status }) => at >= windowStart && at < windowEnd && VOIDING(status),
  )

  if (voiding.length > 0) {
    reasons.push(`${voiding.length} voiding responses in the window from the scenario`)
  }

  if (browserRefused.length > 0) {
    reasons.push(`${browserRefused.length} voiding responses in the window from Chromium`)
  }

  const signedIn = k6.signIns.filter(({ status }) => status === 200)

  if (signedIn.length !== 50) {
    reasons.push(`${signedIn.length} of 50 scenario users signed in`)
  }

  const signInTimes = signedIn.map(({ at }) => at)

  if (signInTimes.length && Math.max(...signInTimes) - Math.min(...signInTimes) > RAMP_MS) {
    reasons.push('the sign-ins ran past the 30-second ramp')
  }

  if (!chromium.signedInAt || chromium.signedInAt >= run.startMs + RAMP_MS) {
    reasons.push('the Chromium user did not sign in during the ramp')
  }

  for (const view of ['list', 'detail']) {
    if (scenario[view].n < REQUIRED_SCENARIO_SAMPLES) {
      reasons.push(`${scenario[view].n} scenario ${view} samples, AC6 needs ${REQUIRED_SCENARIO_SAMPLES}`)
    }

    if (browser[view].n !== REQUIRED_CHROMIUM_SAMPLES) {
      reasons.push(`${browser[view].n} Chromium ${view} samples, AC6 needs ${REQUIRED_CHROMIUM_SAMPLES}`)
    }
  }

  const times = chromium.samples.map(({ at }) => at)
  const closest = Math.min(...times.slice(1).map((at, index) => at - times[index]))

  if (closest < CHROMIUM_SPACING_MS) {
    reasons.push(`two Chromium samples ${closest} ms apart, AC6 needs ${CHROMIUM_SPACING_MS}`)
  }

  if (chromium.firstAt < windowStart || (chromium.lastAt && chromium.lastAt >= windowEnd)) {
    reasons.push('the Chromium session left the window')
  }

  const everything = [...signInTimes, ...ofView('detail').map(({ at }) => at)]
  const lastRequest = Math.max(0, ...everything, ...(chromium.lastAt ? [chromium.lastAt] : []))

  if (signInTimes.length && lastRequest - Math.min(...signInTimes) >= RUN_LIMIT_MS) {
    reasons.push('the run reached 12 minutes')
  }

  for (const { name, code } of run.exitCodes) {
    if (code !== 0) {
      reasons.push(`${name} exited ${code}`)
    }
  }

  const verdicts = Object.fromEntries(
    ['list', 'detail'].map((view) => {
      const met = scenario[view].p95 <= TARGETS_MS[view] && browser[view].p95 <= TARGETS_MS[view]

      return [view, { targetMs: TARGETS_MS[view], scenarioP95: scenario[view].p95, chromiumP95: browser[view].p95, met }]
    }),
  )

  const summary = {
    mode: run.mode,
    directory,
    commit: run.commit,
    imageTag: run.imageTag,
    void: reasons.length > 0,
    voidReasons: reasons,
    verdicts: run.mode === 'target' ? verdicts : 'not compared with the target',
    scenario,
    chromium: browser,
    signIns: { scenario: signedIn.length, firstToLastMs: signInTimes.length ? Math.max(...signInTimes) - Math.min(...signInTimes) : 0, chromiumAt: chromium.signedInAt },
    statuses: k6.statuses,
    nonSuccess: { inWindow: refusedInWindow.length, total: k6.refused.length, sample: k6.refused.slice(0, 10), chromium: chromium.refused ?? [] },
    runMs: lastRequest - Math.min(...signInTimes, Infinity),
    chromiumClosestSamplesMs: closest,
    shellAssets: chromium.shell?.assets,
    perRequest: perRequest(directory),
    logs: logCounts(directory),
    pool: poolReading(directory, windowStart, windowEnd),
    chromiumVersion: chromium.browserVersion,
  }

  summary.text = JSON.stringify(summary, null, 2)
  writeFileSync(join(directory, 'summary.json'), summary.text)

  return summary
}
