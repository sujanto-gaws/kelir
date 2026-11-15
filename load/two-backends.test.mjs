// What `two-backends.mjs` reads out of a log, and that its steps pass the refusals.

import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'

import {
  bootstrapOf,
  countOf,
  isEven,
  levelCount,
  lockWaitMs,
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
const reading = ([a, b]) => ({ sent: a + b, counted: [a, b] })
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
})
