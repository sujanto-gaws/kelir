// The harness's arithmetic and refusals, which need no stack: `node --test`.
//
// AC6's P95 is defined to the position, and AC5's refusals are the only thing
// between a mistyped variable and a real database, so both are held here.

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { portOf, refusals } from './guard.mjs'
import { describe, nearestRank, parseConsole } from './report.mjs'
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
