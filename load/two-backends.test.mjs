// What `two-backends.mjs` reads out of a log, and that its steps pass the refusals.

import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'

import {
  answersOf,
  benchesOf,
  bootstrapOf,
  countOf,
  failoverFailures,
  isEven,
  levelCount,
  lockWaitMs,
  restartFailures,
  restartGap,
  splitFailures,
} from './two-backends.mjs'

test('each bootstrap line reads as its own outcome, and "created" is not the deferred one', () => {
  const says = (message) => bootstrapOf(`INFO bootstrap: ${message}`)

  assert.equal(says('created the first administrator; it must change its password'), 'created')
  assert.equal(says('another instance created the first administrator; standing down'), 'deferred at the re-read')
  assert.equal(says('another instance is creating the first administrator; standing down'), 'deferred at the lock')
  assert.equal(bootstrapOf('INFO listening'), 'found a user')
})

test('the lock wait is read from the migrator\'s own statement, and absent is not zero', () => {
  const log = 'x\nTRACE sqlx::query: summary="SELECT pg_advisory_lock($1)" elapsed=2.4s elapsed_secs=2.2834\ny'

  assert.equal(lockWaitMs(log), 2283)
  assert.equal(lockWaitMs('no such line'), null)
})

test('a level is read from its field and not from the words of a line', () => {
  const log = [
    '2026-11-02T12:46:07Z  WARN sqlx::query: slow',
    '2026-11-02T12:46:07Z  INFO kelir: the word ERROR in a message',
    '2026-11-02T12:46:07Z ERROR kelir: a failure',
  ].join('\n')

  assert.equal(levelCount(log, 'ERROR'), 1)
  assert.equal(levelCount(log, 'WARN'), 1)
  assert.equal(countOf('AS tenant_deleted, AS tenant_deleted', 'AS tenant_deleted'), 2)
})

test('every `two` step passes the refusals before it touches anything', () => {
  const refused = (environment) =>
    spawnSync(process.execPath, ['run.mjs', 'two', 'up'], {
      env: { ...process.env, ...environment },
      encoding: 'utf8',
    })

  for (const environment of [
    { KELIR_LOAD_PROJECT: 'kelir-staging' },
    { KELIR_LOAD_PORT: '8090' },
    { KELIR_LOAD_DATABASE: 'kelir_dev' },
  ]) {
    const { status, stderr } = refused(environment)

    assert.equal(status, 2, stderr)
    assert.match(stderr, /refused/)
  }
})

// The split itself is CI's `Caddy spreads two backends`; this names its cause.
test('the shipped Caddyfile has one upstream, a dynamic one in round robin (#854)', () => {
  const shipped = readFileSync(new URL('../deploy/staging/Caddyfile', import.meta.url), 'utf8')

  assert.equal(countOf(shipped, 'reverse_proxy'), countOf(shipped, 'reverse_proxy {'))
  assert.equal(countOf(shipped, 'reverse_proxy {'), 1)
  assert.match(shipped, /^\t\t\tdynamic a \{\r?\n\t\t\t\tname backend\r?\n\t\t\t\tport 8080\r?$/m)
  assert.match(shipped, /^\t\t\tlb_policy round_robin\r?$/m)
  assert.doesNotMatch(shipped, /reverse_proxy backend:8080/)
})

// #854's readings through the shipped Caddyfile, and what round robin should read.
const reading = ([a, b]) => ({ sent: a + b, counted: [a, b], statuses: { 200: a + b } })
const run = (serial, concurrent, rest = {}) => ({
  inUse: true,
  serial: reading(serial),
  concurrent: reading(concurrent),
  smokeExit: 0,
  ...rest,
})

test('even is within 5 % of half, and every reading #854 took on the shipped file is not', () => {
  assert.equal(isEven(reading([150, 150])), true)
  assert.equal(isEven(reading([135, 165])), true)
  assert.equal(isEven(reading([165, 135])), true)
  assert.equal(isEven(reading([134, 166])), false)
  assert.equal(isEven(reading([0, 0])), false)

  for (const shipped of [[0, 300], [300, 0], [93, 207], [118, 182], [201, 99]]) {
    assert.equal(isEven(reading(shipped)), false, `[${shipped}]`)
  }
})

test('the even run fails on either reading, and passes only when both are even', () => {
  assert.deepEqual(splitFailures('even', run([150, 150], [149, 151])), [])
  assert.match(splitFailures('even', run([0, 300], [150, 150])).join('\n'), /^serial: \[0,300\] of 300 is not even/)
  assert.match(splitFailures('even', run([150, 150], [118, 182])).join('\n'), /^ten at a time: \[118,182\]/)
})

test('the control holds only when serial traffic stayed on one instance', () => {
  assert.deepEqual(splitFailures('one-sided', run([0, 300], [118, 182])), [])
  assert.deepEqual(splitFailures('one-sided', run([300, 0], [150, 150])), [])
  assert.match(splitFailures('one-sided', run([150, 150], [150, 150])).join('\n'), /reached both/)
})

test('a split is not read from a count that does not add up, a failed smoke or another file', () => {
  const short = { sent: 300, counted: [150, 149] }

  assert.match(splitFailures('even', { ...run([150, 150], [150, 150]), serial: short }).join('\n'), /counted 299 of 300/)
  assert.match(splitFailures('even', run([150, 150], [150, 150], { smokeExit: 1 })).join('\n'), /smoke exited 1/)
  assert.match(splitFailures('one-sided', run([0, 300], [0, 300], { inUse: false })).join('\n'), /not serving/)

  const failed = { ...reading([150, 150]), statuses: { 200: 299, 502: 1 } }

  assert.match(splitFailures('even', { ...run([150, 150], [150, 150]), concurrent: failed }).join('\n'), /not all 200/)
})

// #854's failover and restart, judged from their records.
const probe = (start, status) => ({ start, end: start + 100, status })
const answers = Array.from({ length: 300 }, (_, index) => probe(index * 40, 200))
const failoverRun = (rest = {}) => ({
  inUse: true,
  answers: answersOf(answers, 3_000, 9_000),
  benches: benchesOf([{ start: 3_100, status: [{ address: '10.89.0.3:8080', fails: 1 }] }]),
  marks: { stop: 3_000, stopped: 4_000, start: 9_000, ready: 12_000, rejoined: 40_000 },
  served: { beforeStop: [40, 40], all: [200, 100] },
  ...rest,
})

test('a failover passes on 200s alone, a counted refusal and a rejoin in time, and fails without each', () => {
  assert.deepEqual(failoverRun().answers.whileOneServed, 150)
  assert.deepEqual(failoverRun().benches, { '10.89.0.3:8080': { maxFails: 1, from: 3_100, until: 3_100 } })
  assert.deepEqual(failoverFailures(failoverRun()), [])

  const fails = (rest, reason) => assert.match(failoverFailures(failoverRun(rest)).join('\n'), reason)

  fails({ answers: answersOf([...answers, probe(5_000, 502)], 3_000, 9_000) }, /1 of 301 answered other than 200/)
  fails({ answers: answersOf([...answers, probe(5_000, 0)], 3_000, 9_000) }, /\{"0":1,"200":300\}/)
  fails({ answers: answersOf(answers, 20_000, 20_500) }, /while one backend was stopped/)
  fails({ benches: benchesOf([{ start: 0, status: [{ address: '10.89.0.3:8080', fails: 0 }] }, { start: 1, status: null }]) }, /never counted a failed dial/)
  fails({ marks: { stop: 3_000, start: 9_000, ready: 12_000 } }, /served nothing through Caddy within 42000 ms/)
  fails({ marks: { stop: 3_000, start: 9_000, ready: 12_000, rejoined: 54_001 } }, /within 42000 ms/)
  fails({ marks: { stop: 3_000, start: 9_000 } }, /never reported ready/)
  fails({ served: { beforeStop: [80, 0], all: [200, 100] } }, /had served nothing/)
  fails({ served: { beforeStop: [40, 40], all: [200, 99] } }, /counted 299 of the 300/)
  fails({ inUse: false }, /not serving/)
})

test('a restart gap runs from the backend\'s own readiness to Caddy\'s first 200 for good', () => {
  const inside = [probe(0, 200), probe(1_000, 0), probe(2_000, 0), probe(3_000, 200)]
  const gapOf = (...through) => restartGap({ through: [probe(0, 200), probe(1_500, 502), ...through], inside }, 500)

  // A 200 sent before the backend's last refusal, or followed by a 503, is not Caddy back.
  assert.deepEqual(gapOf(probe(1_900, 200), probe(3_200, 503), probe(33_000, 200)), {
    wentDown: true,
    readyAt: 3_100,
    caddyAt: 33_100,
    gapMs: 30_000,
  })
  assert.deepEqual(gapOf(probe(2_500, 200), probe(3_200, 200)).gapMs, 0)
  assert.deepEqual(gapOf(probe(3_200, 200)).gapMs, 200)
  assert.deepEqual(gapOf(probe(3_200, 503)).caddyAt, null)
  assert.deepEqual(restartGap({ through: [], inside: [probe(0, 200), probe(1_000, 200)] }, 500).wentDown, false)
  assert.deepEqual(restartGap({ through: [], inside: [probe(1_000, 0)] }, 500).readyAt, null)
})

test('a restart fails past 5 s, on a backend never down, and on more than one backend', () => {
  const gap = { wentDown: true, readyAt: 3_100, caddyAt: 8_100, gapMs: 5_000 }
  const fails = (rest) => restartFailures({ inUse: true, backends: 1, gap, ...rest }).join('\n')

  assert.equal(fails(), '')
  assert.match(fails({ gap: { ...gap, caddyAt: 8_101, gapMs: 5_001 } }), /5001 ms after the backend was ready/)
  assert.match(fails({ gap: { ...gap, caddyAt: null, gapMs: null } }), /never answered \/health\/ready 200/)
  assert.match(fails({ gap: { wentDown: false, readyAt: null, caddyAt: null, gapMs: null } }), /^the backend was never seen down/)
  assert.match(fails({ gap: { wentDown: true, readyAt: null, caddyAt: null, gapMs: null } }), /^the backend never reported ready/)
  assert.match(fails({ backends: 2 }), /2 backends were running/)
  assert.match(fails({ inUse: false }), /not serving/)
})
