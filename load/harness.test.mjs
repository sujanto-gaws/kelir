// The harness's arithmetic, refusals and log control, which need no stack:
// `node --test`.
//
// AC6's P95 is defined to the position, AC5's refusals are the only thing
// between a mistyped variable and a real database, and a log nobody captured
// must never read as a log with nothing in it, so all three are held here.

import assert from 'node:assert/strict'
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'

import { portOf, refusals } from './guard.mjs'
import {
  describe,
  nearestRank,
  parseConsole,
  parsePoolSample,
  readBackendLog,
  summarize,
} from './report.mjs'
import { compareCounts, EXPECTED, STATUS_COUNTS } from './seed/seed.mjs'

const clean = { project: 'kelir-load', ports: [18097, 18447], database: 'kelir_load_ab12cd34' }

test('P95 of 30 samples is the 29th, as AC6 states', () => {
  const samples = Array.from({ length: 30 }, (_, index) => 30 - index)

  assert.equal(nearestRank(samples, 95), 29)
})

test('nearest-rank takes ceil(0.95 n), at the boundaries too', () => {
  const ascending = (n) => Array.from({ length: n }, (_, index) => index + 1)

  assert.equal(nearestRank(ascending(1), 95), 1)
  assert.equal(nearestRank(ascending(20), 95), 19)
  assert.equal(nearestRank(ascending(21), 95), 20)
  assert.equal(nearestRank(ascending(1000), 95), 950)
  assert.equal(nearestRank(ascending(1001), 95), 951)
})

test('describe gives n, p50, p95 and max, and survives no samples', () => {
  assert.deepEqual(describe([3, 1, 2]), { n: 3, p50: 2, p95: 3, max: 3 })
  assert.deepEqual(describe([]), { n: 0 })
})

test('a run is refused on the three things AC5 names, and on nothing else', () => {
  assert.deepEqual(refusals(clean), [])
  assert.equal(refusals({ ...clean, project: 'kelir-staging' }).length, 1)
  assert.equal(refusals({ ...clean, project: 'KELIR-STAGING ' }).length, 1)
  assert.equal(refusals({ ...clean, ports: [8090, 18447] }).length, 1)
  assert.equal(refusals({ ...clean, ports: [18097, '8090'] }).length, 1)
  assert.equal(refusals({ ...clean, database: 'kelir' }).length, 1)
  assert.equal(refusals({ ...clean, database: 'kelir_dev' }).length, 1)
  assert.equal(refusals({ ...clean, database: ' Kelir_Dev' }).length, 1)
  assert.equal(
    refusals({ project: 'kelir-staging', ports: [8090], database: 'kelir' }).length,
    3,
  )
})

test('the port of a base URL is read, default ports included', () => {
  assert.equal(portOf('http://127.0.0.1:8090/'), '8090')
  assert.equal(portOf('http://caddy'), 80)
  assert.equal(portOf('https://kelir.example'), 443)
})

test('the status table is AC3 and sums to 10,000', () => {
  const total = Object.values(STATUS_COUNTS).reduce((sum, count) => sum + count, 0)

  assert.equal(total, 10_000)
  assert.equal(STATUS_COUNTS.DRAFT, 3_000)
  assert.equal(STATUS_COUNTS.SUBMITTED, 1_000)
  assert.equal(STATUS_COUNTS.IN_REVIEW, 2_000)
  assert.equal(STATUS_COUNTS.APPROVED, 2_000)
})

test('one wrong count is named, and the right ones are silent', () => {
  const right = EXPECTED.map(([, expected]) => String(expected))

  assert.deepEqual(compareCounts(right), [])

  const wrong = [...right]

  wrong[EXPECTED.findIndex(([name]) => name === 'documents')] = '9999'

  assert.deepEqual(compareCounts(wrong), ['documents: 9999, expected 10000'])
  assert.equal(compareCounts(right.slice(1)).length, 1)
})

test('console lines are read back as samples, sign-ins, refusals and statuses', () => {
  const parsed = parseConsole(
    [
      'time="t" level=info msg="L,1,1000,64,200" source=console',
      'time="t" level=info msg="S,list,2000,31,8842" source=console',
      'time="t" level=info msg="E,document,500,3000,7" source=console',
      'time="t" level=info msg="T,7,200:9 500:1" source=console',
    ].join('\n'),
  )

  assert.deepEqual(parsed.samples.list, [{ at: 2000, ms: 31, bytes: 8842 }])
  assert.deepEqual(parsed.signIns, [{ vu: 1, at: 1000, ms: 64, status: 200 }])
  assert.deepEqual(parsed.refused, [{ name: 'document', status: 500, at: 3000, vu: 7 }])
  assert.deepEqual(parsed.statuses, { 200: 9, 500: 1 })
})

const START_MS = Date.parse('2026-11-02T06:00:00.000Z')
const logRun = {
  mode: 'target',
  startMs: START_MS,
  windowMs: 300_000,
  clockOffsetMs: 0,
  exitCodes: [],
  control: { atMs: START_MS + 400_000, statuses: { 200: 16, 500: 20 } },
}

/** A line as `watchBackend` keeps it, with the subscriber's colours in. */
const logged = (offsetMs, level, text, stream = 'stdout') =>
  `${stream} ${new Date(START_MS + offsetMs).toISOString().replace('Z', '123456Z')} ` +
  `\u001b[2m2026-11-02T06:00:00.000000Z\u001b[0m ${level} \u001b[2mkelir\u001b[0m\u001b[2m:\u001b[0m ${text}`
const INFO = '\u001b[32m INFO\u001b[0m'
const WARN = '\u001b[33m WARN\u001b[0m'
const ERROR = '\u001b[31mERROR\u001b[0m'
const PASSING_CONTROL = [
  logged(-60_000, INFO, 'listening'),
  logged(401_000, WARN, 'slow statement: execution time exceeded alert threshold'),
  logged(402_000, WARN, 'acquired connection, but time to acquire exceeded slow threshold'),
  logged(403_000, ERROR, 'request failed with an internal error error=pool timed out'),
]

test('an empty log fails its control, so no count in it can read as zero', () => {
  const reading = readBackendLog('', logRun)

  assert.equal(reading.capturedLines, 0)
  assert.equal(reading.control.passed, false)
  assert.ok(reading.control.missing.includes('no startup line'))
  assert.ok(reading.control.missing.includes('no provoked slow acquire'))
})

test('a run with an empty log is void, and one with a passing control is not', () => {
  const directory = mkdtempSync(join(tmpdir(), 'kelir-load-'))
  const reasonsWith = (log) => {
    writeFileSync(join(directory, 'backend.log'), log)

    return summarize(directory).voidReasons.filter((reason) => /log/.test(reason))
  }

  try {
    writeFileSync(join(directory, 'run.json'), JSON.stringify(logRun))
    writeFileSync(join(directory, 'k6-console.log'), '')

    assert.equal(reasonsWith('').length, 2)
    assert.deepEqual(reasonsWith(PASSING_CONTROL.join('\n')), [])
  } finally {
    rmSync(directory, { recursive: true })
  }
})

test('a control missing any one provoked line fails, naming the line', () => {
  for (const index of [1, 2, 3]) {
    const log = PASSING_CONTROL.filter((_, line) => line !== index).join('\n')
    const reading = readBackendLog(log, logRun)

    assert.equal(reading.control.passed, false)
    assert.ok(reading.control.missing.length >= 1)
  }

  assert.equal(readBackendLog(PASSING_CONTROL.join('\n'), logRun).control.passed, true)
})

test('a run with no control recorded fails, whatever the log holds', () => {
  const { control, ...withoutOne } = logRun

  assert.ok(control)
  assert.deepEqual(readBackendLog(PASSING_CONTROL.join('\n'), withoutOne).control.missing, [
    'this run provoked nothing',
  ])
})

test('the provoked lines are not counted as the run\'s, and a coloured ERROR is', () => {
  const reading = readBackendLog(
    [
      ...PASSING_CONTROL,
      logged(60_000, ERROR, 'request failed with an internal error'),
      logged(61_000, WARN, 'slow statement: execution time exceeded alert threshold'),
      logged(62_000, INFO, 'a statement mentioning ERROR and WARN is no error'),
      logged(63_000, INFO, "thread 'main' panicked at src/main.rs:1:1", 'stderr'),
    ].join('\n'),
    logRun,
  )

  assert.equal(reading.capturedLines, 8)
  assert.deepEqual(reading.streams, { stdout: 7, stderr: 1 })
  assert.equal(reading.measured.lines, 4)
  assert.equal(reading.measured.errorLines, 1)
  assert.equal(reading.measured.warnLines, 1)
  assert.equal(reading.measured.slowStatements, 1)
  assert.equal(reading.measured.slowAcquire, 0)
  assert.equal(reading.measured.panics, 1)
  assert.equal(reading.control.provoked.lines, 3)
})

test('a pool sample keeps its numbers, and the sessions a comma would split', () => {
  assert.deepEqual(parsePoolSample('1000,6,1,0,0'), {
    at: 1000,
    open: 6,
    active: 1,
    inCheck: 0,
    waiting: 0,
    local: 0,
    sessions: [],
  })
  assert.equal(parsePoolSample('1000,6,1,0,0,0,').sessions.length, 0)

  const sample = parsePoolSample(
    '1000,11,3,1,0,1,[{"pid": 7, "query": "SELECT a, b", "client_addr": null}]',
  )

  assert.equal(sample.open, 11)
  assert.equal(sample.local, 1)
  assert.equal(sample.sessions[0].query, 'SELECT a, b')
  assert.equal(parsePoolSample('not a sample'), null)
})
