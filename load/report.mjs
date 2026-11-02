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
/** How long before the first sign-in the measured part of the log begins. */
const LOG_LEAD_MS = 5_000
/** The tracing subscriber's colours reach the log, and `ERROR` has no space before it. */
const ANSI = /\x1b\[[0-9;]*m/g

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

/** What one part of the backend's log says. A level is read from its field. */
function countLines(lines) {
  const having = (pattern) => lines.filter((line) => pattern.test(line)).length

  return {
    lines: lines.length,
    errorLines: having(/^\S+\s+ERROR\s/),
    warnLines: having(/^\S+\s+WARN\s/),
    poolTimedOut: having(/pool timed out/i),
    slowAcquire: having(/time to acquire exceeded/i),
    slowStatements: having(/slow statement/i),
    panics: having(/panicked at/),
  }
}

/**
 * Splits `backend.log`, as `run.mjs` writes it, into what came before the run,
 * during it and during the control, and says whether the capture can be read.
 *
 * **Zero is a reading only beside a passing control.** The file must hold the
 * startup line the backend always writes, and every kind of line the control
 * provoked. Otherwise every count here is a count over nothing.
 */
export function readBackendLog(text, run) {
  const parts = { before: [], measured: [], control: [] }
  const streams = { stdout: 0, stderr: 0 }
  const start = run.startMs - LOG_LEAD_MS

  for (const raw of text.split(/\r?\n/)) {
    const match = /^(stdout|stderr) (\S+) ?(.*)$/.exec(raw.replace(ANSI, ''))

    if (!match) {
      continue
    }

    const [, stream, stamp, line] = match
    // Docker's stamp is in nanoseconds and on the VM's clock. A line whose time
    // cannot be read stays among the measured lines, where it will be seen.
    const at = Date.parse(`${stamp.slice(0, 23)}Z`) - (run.clockOffsetMs ?? 0)

    streams[stream] += 1

    if (at < start) {
      parts.before.push(line)
    } else if (run.control && at >= run.control.atMs) {
      parts.control.push(line)
    } else {
      parts.measured.push(line)
    }
  }

  const provoked = countLines(parts.control)
  const startupLines = parts.before.filter((line) => /\bINFO\b.*\blistening\b/.test(line)).length
  const expected = [
    ['slow statement', provoked.slowStatements],
    ['slow acquire', provoked.slowAcquire],
    ['pool timeout', provoked.poolTimedOut],
    ['WARN line', provoked.warnLines],
    ['ERROR line', provoked.errorLines],
  ]
  const missing = [
    ...(startupLines === 0 ? ['no startup line'] : []),
    ...(run.control
      ? expected.filter(([, lines]) => lines === 0).map(([name]) => `no provoked ${name}`)
      : ['this run provoked nothing']),
  ]

  return {
    capturedLines: streams.stdout + streams.stderr,
    streams,
    before: countLines(parts.before),
    measured: countLines(parts.measured),
    control: {
      passed: missing.length === 0,
      missing,
      startupLines,
      provoked,
      responses: run.control?.statuses ?? null,
    },
  }
}

function logCounts(directory, run) {
  const read = (name) => {
    const path = join(directory, name)

    return existsSync(path) ? readFileSync(path, 'utf8') : ''
  }
  const backend = readBackendLog(read('backend.log'), run)
  const statuses = JSON.parse(read('caddy-status.json') || '{}')
  const entries = Object.entries(statuses)

  return {
    backendLog: backend,
    backendErrorLines: backend.measured.errorLines,
    backendWarnLines: backend.measured.warnLines,
    poolTimedOut: backend.measured.poolTimedOut,
    slowAcquire: backend.measured.slowAcquire,
    slowStatements: backend.measured.slowStatements,
    caddy: statuses,
    caddyRequests: entries.reduce((total, [, value]) => total + value, 0),
    caddy5xx: entries
      .filter(([status]) => status.startsWith('5'))
      .reduce((total, [, value]) => total + value, 0),
  }
}

/** One line of `pool-samples.csv`. Its last column is JSON, and holds commas. */
export function parsePoolSample(line) {
  const match = /^(\d+),(\d+),(\d+),(\d+),(\d+)(?:,(\d+),(.*))?$/.exec(line)

  if (!match) {
    return null
  }

  const [, at, open, active, inCheck, waiting, local, sessions] = match

  return {
    at: Number(at),
    open: Number(open),
    active: Number(active),
    inCheck: Number(inCheck),
    waiting: Number(waiting),
    local: Number(local ?? 0),
    sessions: sessions ? JSON.parse(sessions) : [],
  }
}

/** What the five-a-second samples of the connections say (ADR-0045 §6). */
function poolReading(directory, windowStart, windowEnd) {
  const path = join(directory, 'pool-samples.csv')

  if (!existsSync(path)) {
    return { samples: 0 }
  }

  const all = readFileSync(path, 'utf8').split(/\r?\n/).map(parsePoolSample).filter(Boolean)
  const rows = all.filter(({ at }) => at >= windowStart && at < windowEnd)
  const active = rows.map((row) => row.active)
  const inCheck = rows.reduce((total, row) => total + row.inCheck, 0)
  const activeTotal = active.reduce((total, value) => total + value, 0)

  return {
    samples: rows.length,
    connectionsOpenMax: Math.max(0, ...rows.map((row) => row.open)),
    active: describe(active),
    activeMean: rows.length ? activeTotal / rows.length : 0,
    atTenActive: active.filter((value) => value >= 10).length,
    shareOfActiveInTokenCheck: activeTotal ? inCheck / activeTotal : 0,
    lockWaitSamples: rows.filter((row) => row.waiting > 0).length,
    // The whole run and not the window: these name a session, they time nothing.
    samplesInRun: all.length,
    overTen: all
      .filter((row) => row.open > 10)
      .map(({ at, open, sessions }) => ({ at, open, sessions })),
    localSessions: all
      .filter((row) => row.local > 0)
      .map(({ at, open, sessions }) => ({
        at,
        open,
        sessions: sessions.filter((session) => session.client_addr === null),
      })),
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

  const logs = logCounts(directory, run)

  if (logs.backendLog.capturedLines === 0) {
    reasons.push('backend.log holds no line, so its counts are counts over nothing')
  }

  if (!logs.backendLog.control.passed) {
    reasons.push(`the log's control failed: ${logs.backendLog.control.missing.join(', ')}`)
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
    logs,
    pool: poolReading(directory, windowStart, windowEnd),
    chromiumVersion: chromium.browserVersion,
  }

  summary.text = JSON.stringify(summary, null, 2)
  writeFileSync(join(directory, 'summary.json'), summary.text)

  return summary
}
