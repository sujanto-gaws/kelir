// What `two-backends.mjs` reads out of a log, and that its steps pass the refusals.

import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { test } from 'node:test'

import { bootstrapOf, countOf, levelCount, lockWaitMs } from './two-backends.mjs'

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
