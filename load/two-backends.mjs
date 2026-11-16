// Two backends behind Caddy (#780): NFR-SCA-001 to 003, on a stack.
//
//   node run.mjs two up          starts the stack: both backends, on an empty database
//   node run.mjs two race <n>    n more cold starts of the pair, each on a new database
//   node run.mjs two traffic     what each instance served, with its control
//   node run.mjs two count       each instance's count so far
//   node run.mjs two cross       AC3's checks and AC5's two sentences, each with a control
//   node run.mjs two e2e         the browser suite against this stack
//   node run.mjs two document    one document, so smoke has a row to open
//   node run.mjs two split <file> even|one-sided   the split through that Caddyfile, asserted
//   node run.mjs two failover <file>   one of two stopped under traffic and started, asserted
//   node run.mjs two restart <file>    one backend restarted under the readiness probe, asserted
//   node run.mjs two errors      a real ERROR on each instance, the control on every zero
//
// `smoke` and `down` are `run.mjs`'s own, and the refusals in `guard.mjs` have
// run before anything here does. **A check addresses an instance by running curl
// inside it** (`on`), so which instance served a request is a fact, and not
// something the balancer was trusted with.

import { spawn, spawnSync } from 'node:child_process'
import { createHash, randomBytes } from 'node:crypto'
import { createWriteStream, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

import { hashPassword } from './seed/seed.mjs'

const HERE = dirname(fileURLToPath(import.meta.url))
const INSTANCES = [1, 2]
const pause = (ms) => new Promise((done) => setTimeout(done, ms))
const hex = (bytes) => randomBytes(bytes).toString('hex')
const docker = (args) => spawnSync('docker', args, { encoding: 'utf8', maxBuffer: 1 << 30 })
const name = (ctx, n) => `${ctx.config.project}-backend-${n}`
const sql = (ctx, statement) => ctx.psql(`${statement}\n`).trim()
const total = (pair) => pair[0] + pair[1]

export const countOf = (text, needle) => text.split(needle).length - 1

/** An instance's log, colour removed. Tracing writes to stdout, a panic to stderr. */
function logOf(container, since) {
  const { stdout, stderr } = docker(['logs', '--timestamps', ...(since ? ['--since', since] : []), container])

  // eslint-disable-next-line no-control-regex
  return `${stdout}${stderr}`.replace(/\x1b\[[0-9;]*m/g, '')
}

const logs = (ctx, since) => INSTANCES.map((n) => logOf(name(ctx, n), since))

/** Lines holding every needle, per instance. */
const linesWith = (ctx, ...needles) =>
  logs(ctx).map(
    (log) => log.split('\n').filter((line) => needles.every((needle) => line.includes(needle))).length,
  )

async function until(what, test, seconds = 60) {
  for (let waited = 0; waited < seconds * 2; waited += 1) {
    if (test()) {
      return
    }

    await pause(500)
  }

  throw new Error(`gave up after ${seconds} s waiting for ${what}`)
}

const bothListening = (ctx, since) =>
  until('both backends', () => logs(ctx, since).every((log) => log.includes('listening')), 120)

/** Readers for the smoke scenario, and users to lock out, in the tenant served. */
function seedUsers(ctx) {
  ctx.psql(
    `INSERT INTO roles (id, tenant_id, role_code, name)
     SELECT gen_random_uuid(), id, 'ROLE-LOAD-READER', 'Load test reader' FROM tenants
     WHERE tenant_code = 'SYSTEM';

     INSERT INTO role_permissions (id, tenant_id, role_id, permission_id)
     SELECT gen_random_uuid(), r.tenant_id, r.id, p.id FROM roles r, permissions p
     WHERE r.role_code = 'ROLE-LOAD-READER' AND p.deleted_at IS NULL
       AND p.permission_code IN ('document:read', 'rad:form:read', 'rad:menu:read',
           'notification:read', 'workflow:instance:read', 'workflow:task:read',
           'attachment:read', 'comment:read');

     INSERT INTO users (id, tenant_id, username, email, password_hash, display_name, status,
                        must_change_password)
     SELECT gen_random_uuid(), r.tenant_id, 'load-user-' || lpad(n::text, 3, '0'),
            'load-user-' || n || '@load.kelir.invalid', :'password_hash', 'Load user ' || n,
            'ACTIVE', false
     FROM roles r, generate_series(1, 9) AS n WHERE r.role_code = 'ROLE-LOAD-READER';

     INSERT INTO user_roles (id, tenant_id, user_id, role_id)
     SELECT gen_random_uuid(), u.tenant_id, u.id, r.id
     FROM users u JOIN roles r ON r.tenant_id = u.tenant_id AND r.role_code = 'ROLE-LOAD-READER'
     WHERE u.username LIKE 'load-user-%';\n`,
    ['-v', `password_hash=${hashPassword(ctx.stack().password)}`],
  )
}

async function up(ctx) {
  const stack = ctx.startStack({
    KELIR_BOOTSTRAP_ADMIN_USERNAME: 'admin',
    KELIR_BOOTSTRAP_ADMIN_PASSWORD: `Kelir-${hex(12)}-Aa1!`,
    KELIR_BOOTSTRAP_ADMIN_EMAIL: 'admin@load.kelir.invalid',
    KELIR_E2E_UPSTREAM_TOKEN: hex(16),
    // A stale network from another run can hold the e2e file's default subnet.
    KELIR_E2E_UPSTREAM_SUBNET: '10.89.95.0/29',
    KELIR_E2E_UPSTREAM_POOL: '10.89.95.4/30',
    KELIR_E2E_UPSTREAM_ADDRESS: '10.89.95.2',
    KELIR_E2E_UPSTREAM_PORT: '18089',
  })

  await bothListening(ctx)
  await ctx.waitForReady(stack.url)

  const verified = await ctx.verifyStack(stack.url)
  const hops = INSTANCES.map((n) =>
    docker(['exec', name(ctx, n), 'printenv', 'KELIR_TRUSTED_PROXY_HOPS']).stdout.trim(),
  )
  const started = logs(ctx).map((log) => [bootstrapOf(log), lockWaitMs(log)])

  seedUsers(ctx)
  writeFileSync(
    join(stack.runDirectory, 'stack-verified.json'),
    JSON.stringify({ verified, hops, started, compose: ctx.composeArgs() }, null, 2),
  )
  console.log(`Both listening, hops ${hops.join(', ')}: ${JSON.stringify(started)}`)
}

// ---------------------------------------------------------------------------
// Addressing one instance
// ---------------------------------------------------------------------------

/** A source no other check shares; `X-Forwarded-For` is what Caddy would have set. */
let lastSource = 0
const newSource = () => `198.51.${100 + (++lastSource >> 8)}.${lastSource & 255}`

/** One request, sent from inside instance `n`, so instance `n` is what serves it. */
function on(ctx, n, method, path, { token, json, from = newSource(), file } = {}) {
  const args = ['exec', '-i', name(ctx, n), 'curl', '-sS', '--max-time', '40', '-o', '-']

  args.push('-w', '\n%{http_code}', '-X', method, '-H', `X-Forwarded-For: ${from}`)

  if (token) {
    args.push('-H', `Authorization: Bearer ${token}`)
  }

  if (json) {
    args.push('-H', 'Content-Type: application/json', '--data-binary', '@-')
  }

  if (file) {
    args.push('-F', 'file=@-;filename=row-4.pdf;type=application/pdf')
  }

  args.push(`http://127.0.0.1:8080/api/v1${path}`)

  return new Promise((done, failed) => {
    const child = spawn('docker', args, { stdio: ['pipe', 'pipe', 'inherit'] })
    const chunks = []

    child.on('error', failed)
    child.stdout.on('data', (chunk) => chunks.push(chunk))
    child.on('close', () => {
      const all = Buffer.concat(chunks)
      const cut = all.lastIndexOf(10)
      const bytes = all.subarray(0, cut)
      let data = null

      try {
        data = JSON.parse(bytes.toString()).data
      } catch {
        // A download, or an empty body.
      }

      done({ status: Number(all.subarray(cut + 1).toString()), bytes, data })
    })
    child.stdin.end(json ? JSON.stringify(json) : file)
  })
}

const login = (ctx, n, username, password, from) =>
  on(ctx, n, 'POST', '/auth/login', { json: { username, password }, from })

// ---------------------------------------------------------------------------
// A start of the pair
// ---------------------------------------------------------------------------

/** How an instance's bootstrap went, by the line it wrote. Silent is the pre-check. */
export function bootstrapOf(log) {
  if (log.includes('another instance is creating the first administrator')) {
    return 'deferred at the lock'
  }

  if (log.includes('another instance created the first administrator')) {
    return 'deferred at the re-read'
  }

  return log.includes('created the first administrator') ? 'created' : 'found a user'
}

/** Milliseconds this instance waited for the migrator's advisory lock. */
export function lockWaitMs(log) {
  const line = log.split('\n').find((text) => text.includes('SELECT pg_advisory_lock'))
  const found = /elapsed_secs=([\d.e-]+)/.exec(line ?? '')

  return found ? Math.round(Number(found[1]) * 1000) : null
}

const linesAt = (log, level) => log.split('\n').filter((line) => new RegExp(`Z\\s+${level}\\s`).test(line))

export const levelCount = (log, level) => linesAt(log, level).length

/** From a stopped pair to both listening, read from their own logs and the tables. */
async function startPair(ctx, kind) {
  await Promise.all(
    INSTANCES.map(
      (n) => new Promise((done) => spawn('docker', ['start', name(ctx, n)]).on('close', done)),
    ),
  )

  const pair = INSTANCES.map((n) => JSON.parse(docker(['inspect', '-f', '{{json .}}', name(ctx, n)]).stdout))
  const [first, second] = pair.map((container) => container.State.StartedAt).sort()

  await bothListening(ctx, first)

  const texts = logs(ctx, first)
  const count = (statement) => Number(sql(ctx, `SELECT count(*) FROM ${statement}`))

  return {
    kind,
    startGapMs: Date.parse(second) - Date.parse(first),
    restarts: pair.map((container) => container.RestartCount),
    logBytes: texts.map((text) => text.length),
    bootstrap: texts.map(bootstrapOf),
    migrationLockWaitMs: texts.map(lockWaitMs),
    listening: texts.map((text) => countOf(text, 'listening')),
    errors: texts.map((text) => levelCount(text, 'ERROR')),
    warnings: texts.map((text) => levelCount(text, 'WARN')),
    administrators: count(`users WHERE username = 'admin'`),
    createdAudited: count(`audit_events WHERE event_type = 'User.Created'`),
    migrations: count('_sqlx_migrations WHERE success'),
    usersAll: count('users'),
  }
}

/** `n` cold starts, each on an empty database, then two on a migrated one. */
async function race(ctx, [rounds = '12']) {
  const server = ['exec', '-T', 'postgres', 'psql', '-U', 'kelir_load', '-d', 'postgres', '-qc']
  const database = ctx.config.database
  const results = []
  const start = async (kind) => {
    ctx.compose('stop', '-t', '1', 'backend')

    if (kind === 'empty') {
      ctx.compose(...server, `DROP DATABASE IF EXISTS "${database}" WITH (FORCE)`)
      ctx.compose(...server, `CREATE DATABASE "${database}" OWNER kelir_load`)
    }

    results.push(await startPair(ctx, kind))
    console.log(JSON.stringify(results.at(-1)))
  }

  for (let round = 0; round < Number(rounds); round += 1) {
    await start('empty')
  }

  seedUsers(ctx)
  await start('migrated')
  await start('migrated')
  writeFileSync(join(ctx.stack().runDirectory, 'race.json'), JSON.stringify(results, null, 2))
}

// ---------------------------------------------------------------------------
// Who served what
// ---------------------------------------------------------------------------

/**
 * Each instance's own count of authenticated requests: the extractor asks one
 * statement on every one (ADR-0045), and sqlx logs it in that instance's log.
 */
const served = (ctx) => logs(ctx).map((log) => countOf(log, 'AS tenant_deleted'))
const change = (after, before) => after.map((count, index) => count - before[index])

/** Sends `rounds` rounds of `size` requests through Caddy and reads where they went. */
async function throughCaddy(ctx, token, rounds, size) {
  const before = served(ctx)
  const statuses = {}
  const send = () =>
    fetch(`${ctx.stack().url}/api/v1/auth/me`, { headers: { Authorization: `Bearer ${token}` } }).then(
      async (answer) => {
        await answer.text()
        statuses[answer.status] = (statuses[answer.status] ?? 0) + 1
      },
    )

  for (let round = 0; round < rounds; round += 1) {
    await Promise.all(Array.from({ length: size }, send))
  }

  return { sent: rounds * size, atOnce: size, counted: change(served(ctx), before), statuses }
}

/** The count, proved on requests each instance was sent, then read through Caddy. */
async function traffic(ctx) {
  const { accessToken: token } = (await login(ctx, 1, 'load-user-003', ctx.stack().password)).data
  const result = {
    at: new Date().toISOString(),
    soFar: served(ctx),
    logBytes: logs(ctx).map((log) => log.length),
    logStartHeld: logs(ctx).map((log) => log.includes('starting kelir backend')),
  }
  const before = served(ctx)
  const sent = [23, 7]

  for (const n of INSTANCES) {
    for (let index = 0; index < sent[n - 1]; index += 1) {
      await on(ctx, n, 'GET', '/auth/me', { token })
    }
  }

  result.control = { sent, counted: change(served(ctx), before) }
  result.oneAtATime = await throughCaddy(ctx, token, 300, 1)
  result.tenAtATime = await throughCaddy(ctx, token, 30, 10)
  console.log(JSON.stringify(result, null, 2))
  writeFileSync(join(ctx.stack().runDirectory, `traffic-${Date.now()}.json`), JSON.stringify(result, null, 2))
}

// ---------------------------------------------------------------------------
// AC3: what crosses
// ---------------------------------------------------------------------------

/**
 * On this throwaway database only: each write of a status, with the connection's
 * address, which names the instance that made it. `cross` removes it again.
 */
const INSTRUMENT = `
  CREATE TABLE row4_writes (row_id uuid, became text, client inet DEFAULT inet_client_addr());
  CREATE FUNCTION row4_log() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN
    INSERT INTO row4_writes (row_id, became) VALUES (NEW.id, to_jsonb(NEW) ->> TG_ARGV[0]);
    RETURN NEW; END $$;
  CREATE TRIGGER row4 AFTER UPDATE ON outbox_events FOR EACH ROW
    EXECUTE FUNCTION row4_log('status');
  CREATE TRIGGER row4 AFTER UPDATE OF virus_scan_status ON attachments FOR EACH ROW
    EXECUTE FUNCTION row4_log('virus_scan_status');`

/** `{ id: [writes by 1, writes by 2] }` for the rows that became `status`. */
function writesTo(ctx, status) {
  const address = (n) =>
    JSON.parse(docker(['inspect', '-f', '{{json .NetworkSettings.Networks}}', name(ctx, n)]).stdout)[
      `${ctx.config.project}_default`
    ].IPAddress
  const addresses = INSTANCES.map(address)
  const counted = {}

  for (const line of sql(ctx, `SELECT row_id, host(client) FROM row4_writes
      WHERE became = '${status}' AND client IS NOT NULL`).split(/\r?\n/)) {
    const [id, client] = line.split('|')

    counted[id] ??= [0, 0]
    counted[id][addresses.indexOf(client)] += 1
  }

  return counted
}

/** A workflow of one decision whose edge logs one hook run, and a type bound to it. */
async function arrange(ctx, token) {
  const key = `row4_${hex(3)}`
  const code = key.toUpperCase()
  const form = key.replace('_', '-')
  const post = async (path, json) => {
    const answer = await on(ctx, 1, 'POST', path, { token, json })

    if (answer.status >= 300) {
      throw new Error(`${path}: ${answer.status} ${answer.bytes}`)
    }

    return answer.data
  }
  const definition = {
    workflowKey: key,
    version: '1.0.0',
    name: 'One step',
    initialState: 'SUBMITTED',
    states: [
      { code: 'SUBMITTED', name: 'Waiting', mapsToDocumentStatus: 'PENDING_APPROVAL',
        task: { taskDefinitionKey: 'approve', taskName: 'Approve',
                assignment: { assigneeType: 'ROLE', roleCode: 'ROLE-ADMIN' } } },
      { code: 'APPROVED', name: 'Approved', mapsToDocumentStatus: 'COMPLETED', isFinal: true },
    ],
    transitions: [
      { from: 'SUBMITTED', to: 'APPROVED', action: 'APPROVE', allowedBy: 'ROLE:ROLE-ADMIN',
        actions: [{ handler: 'core:continue_always' }] },
    ],
  }
  const workflow = await post('/workflow/definitions', { workflowKey: key, name: 'One step', definition })

  await post(`/workflow/definitions/${workflow.id}/publication`, {})

  const { id: formId } = await post('/rad/forms', {
    formKey: form,
    title: 'Row 4',
    definition: {
      formId: form,
      version: '2.0.1',
      title: 'Row 4',
      components: [{ id: 'amount-field', role: 'data', type: 'number', key: 'amount',
                     label: 'Amount', validation: { type: 'number', minimum: 0 } }],
    },
  })

  await post(`/rad/forms/${formId}/publish`, {})

  const { id: type } = await post('/document-types', {
    typeCode: code,
    name: code,
    formId,
    workflows: [{ workflowDefinitionId: workflow.id }],
  })

  await on(ctx, 1, 'PUT', `/document-types/${type}/numbering-rule`, {
    token,
    json: { ruleTemplate: `${code}-{year}-{sequence}`, sequenceScope: 'YEAR', gapPolicy: 'GAPLESS' },
  })

  return type
}

const newDocument = async (ctx, n, token, type) =>
  (await on(ctx, n, 'POST', '/documents', {
    token,
    json: { documentTypeId: type, title: 'Row 4', formData: { amount: 100 } },
  })).data.id

/** One document, as `cross` makes its first, so smoke has a row to open. */
async function oneDocument(ctx) {
  const { accessToken: token } = (await login(ctx, 2, 'admin', ctx.stack().env.KELIR_BOOTSTRAP_ADMIN_PASSWORD)).data
  const id = await newDocument(ctx, 1, token, await arrange(ctx, token))

  console.log(`document ${id}`)
}

const pdf = (marker) => Buffer.from(`%PDF-1.7\n% ${marker}\n1 0 obj\n<<>>\nendobj\n%%EOF\n`)
const sha = (bytes) => createHash('sha256').update(bytes).digest('hex')

async function upload(ctx, n, token, document, marker) {
  const answer = await on(ctx, n, 'POST', `/documents/${document}/attachments`, { token, file: pdf(marker) })

  if (answer.status !== 200) {
    throw new Error(`upload on ${n}: ${answer.status} ${answer.bytes}`)
  }

  return answer.data.id
}

const scanOf = (ctx, id) => sql(ctx, `SELECT virus_scan_status FROM attachments WHERE id = '${id}'`)

/** NFR-SCA-002: stored through one, served through the other, both ways round. */
async function fileCrosses(ctx, token, document) {
  const legs = []

  for (const [storedOn, servedOn] of [[1, 2], [2, 1]]) {
    const marker = hex(8)
    const id = await upload(ctx, storedOn, token, document, marker)

    await until(`${id} to be scanned`, () => scanOf(ctx, id) === 'CLEAN')

    const got = await on(ctx, servedOn, 'GET', `/documents/${document}/attachments/${id}`, { token })

    legs.push({
      storedOn,
      servedOn,
      status: got.status,
      bytes: got.bytes.length,
      sameBytes: sha(got.bytes) === sha(pdf(marker)),
      anotherFileWouldDiffer: sha(got.bytes) !== sha(pdf(hex(8))),
    })
  }

  return { pass: legs.every((leg) => leg.status === 200 && leg.sameBytes), legs }
}

/**
 * NFR-SCA-003, the scanner. clamd is paused, so both workers are held mid-scan on
 * the same files and both answer when it resumes: only one may settle. A row put
 * back to PENDING settles a second time, which shows the count can read 2.
 */
async function scanSettlesOnce(ctx, token, document) {
  const clamd = `${ctx.config.project}-clamav-1`
  const ids = []

  docker(['pause', clamd])

  try {
    for (const n of [1, 2, 1]) {
      ids.push(await upload(ctx, n, token, document, hex(8)))
    }

    await pause(12_000)
  } finally {
    docker(['unpause', clamd])
  }

  await until('the files to settle', () => ids.every((id) => scanOf(ctx, id) === 'CLEAN'))
  await pause(6_000)

  const read = (id) => ({
    settled: linesWith(ctx, 'scanned clean', id),
    lostTheRace: linesWith(ctx, 'somebody else had already decided', id),
    writes: writesTo(ctx, 'CLEAN')[id],
  })
  const files = ids.map(read)
  const once = files.every((file) => total(file.settled) === 1 && total(file.writes) === 1)
  const crossed = files.some((file) => total(file.lostTheRace) > 0)

  sql(ctx, `UPDATE attachments SET virus_scan_status = 'PENDING' WHERE id = '${ids[0]}'`)
  await until('the reset file to settle', () => scanOf(ctx, ids[0]) === 'CLEAN')
  await pause(6_000)

  const control = read(ids[0])

  return {
    pass: once && crossed && total(control.settled) === 2 && total(control.writes) === 2,
    once,
    crossed,
    files,
    control,
  }
}

const eventOf = (ctx, document) =>
  sql(ctx, `SELECT id, status, attempt_count FROM outbox_events
            WHERE payload_json->'payload'->>'documentId' = '${document}'`).split('|')

const hookRuns = (ctx, document) =>
  Number(sql(ctx, `SELECT count(*) FROM document_hook_executions WHERE document_id = '${document}'`))

/**
 * NFR-SCA-003, the outbox. Real events, read for one claim and one hook run; then
 * both workers queued on a lock while 100 due rows appear at once, so two
 * processes contend for the claim itself. A real event put back to PENDING is
 * delivered twice, which shows the counts can read 2. Submitting in parallel on
 * both instances is also the numbering check.
 */
async function outboxDeliversOnce(ctx, token, type) {
  const documents = []

  for (let index = 0; index < 24; index += 1) {
    documents.push(await newDocument(ctx, 1 + (index % 2), token, type))
  }

  await Promise.all(
    documents.map((id, index) => on(ctx, 1 + (index % 2), 'POST', `/documents/${id}/submission`, { token })),
  )

  const ids = documents.map((id) => `'${id}'`).join(',')
  const sequence = sql(ctx, `SELECT document_number FROM documents WHERE id IN (${ids})`)
    .split(/\r?\n/)
    .map((number) => Number(number.split('-').at(-1)))
  const tasks = sql(ctx, `SELECT id FROM workflow_tasks WHERE document_id IN (${ids})
                          AND status IN ('CREATED', 'ASSIGNED', 'IN_PROGRESS')`).split(/\r?\n/)

  for (const [index, task] of tasks.entries()) {
    const decided = await on(ctx, 2 - (index % 2), 'POST', `/workflow/tasks/${task}/decision`, {
      token,
      json: { action: 'APPROVE' },
    })

    if (decided.status !== 200) {
      throw new Error(`decision: ${decided.status} ${decided.bytes}`)
    }
  }

  await until('every event to settle', () =>
    documents.every((document) => eventOf(ctx, document)[1] === 'PROCESSED'),
  )

  const claims = writesTo(ctx, 'PROCESSING')
  const events = documents.map((document) => {
    const [id, , attempts] = eventOf(ctx, document)

    return { document, id, attempts: Number(attempts), claims: claims[id], hookRuns: hookRuns(ctx, document) }
  })
  const once = events.every(
    (event) => event.attempts === 1 && total(event.claims) === 1 && event.hookRuns === 1,
  )
  const eventsClaimedBy = [0, 1].map((side) => events.filter((event) => event.claims[side]).length)

  const burstType = `Row4.${hex(3)}`

  sql(
    ctx,
    `BEGIN;
     LOCK TABLE outbox_events IN ACCESS EXCLUSIVE MODE;
     INSERT INTO outbox_events (id, tenant_id, aggregate_type, aggregate_id, event_type, payload_json)
     SELECT gen_random_uuid(), id, 'SYSTEM', gen_random_uuid(), '${burstType}', '{}'
     FROM tenants, generate_series(1, 100) WHERE tenant_code = 'SYSTEM';
     SELECT pg_sleep(3);
     COMMIT;`,
  )
  await until('the burst to settle', () =>
    sql(ctx, `SELECT count(*) FROM outbox_events
              WHERE event_type = '${burstType}' AND status <> 'PROCESSED'`) === '0',
  )

  const burstIds = sql(ctx, `SELECT id FROM outbox_events WHERE event_type = '${burstType}'`).split(/\r?\n/)
  const burstClaims = writesTo(ctx, 'PROCESSING')
  const burst = {
    events: burstIds.length,
    claimedOnce: burstIds.filter((id) => total(burstClaims[id] ?? [0, 0]) === 1).length,
    claimedBy: [0, 1].map((side) => burstIds.filter((id) => burstClaims[id]?.[side]).length),
    consumerLines: linesWith(ctx, 'no consumer in this build', burstType),
  }

  const again = events[0]

  sql(ctx, `UPDATE outbox_events SET status = 'PENDING', processed_at = NULL, attempt_count = 0
            WHERE id = '${again.id}'`)
  await until('the requeued event to settle', () => eventOf(ctx, again.document)[1] === 'PROCESSED')

  const control = { claims: writesTo(ctx, 'PROCESSING')[again.id], hookRuns: hookRuns(ctx, again.document) }

  return {
    pass:
      once &&
      eventsClaimedBy.every(Boolean) &&
      burst.claimedOnce === 100 &&
      burst.claimedBy.every(Boolean) &&
      total(burst.consumerLines) === 100 &&
      total(control.claims) === 2 &&
      control.hookRuns === 2,
    once,
    eventsClaimedBy,
    burst,
    control,
    numbering: {
      numbers: sequence.length,
      distinct: new Set(sequence).size,
      span: Math.max(...sequence) - Math.min(...sequence) + 1,
    },
    events,
  }
}

/** A token one instance signed, on the other; and one tampered with, refused there. */
async function tokenCrosses(ctx) {
  const legs = []

  for (const [issuedOn, usedOn] of [[1, 2], [2, 1]]) {
    const { accessToken: token } = (await login(ctx, issuedOn, 'load-user-003', ctx.stack().password)).data
    const tampered = `${token.slice(0, -2)}${token.at(-2) === 'A' ? 'B' : 'A'}${token.at(-1)}`

    legs.push({
      issuedOn,
      usedOn,
      accepted: (await on(ctx, usedOn, 'GET', '/auth/me', { token })).status,
      tampered: (await on(ctx, usedOn, 'GET', '/auth/me', { token: tampered })).status,
      none: (await on(ctx, usedOn, 'GET', '/auth/me')).status,
    })
  }

  return {
    pass: legs.every((leg) => leg.accepted === 200 && leg.tampered === 401 && leg.none === 401),
    legs,
  }
}

/** Rotated on one, replayed on the other: refused, audited, and the family ended. */
async function refreshReplay(ctx) {
  const password = ctx.stack().password
  const refresh = (n, refreshToken) => on(ctx, n, 'POST', '/auth/refresh', { json: { refreshToken } })
  const audited = () =>
    Number(sql(ctx, `SELECT count(*) FROM audit_events WHERE event_type = 'Security.RefreshTokenReused'`))
  const start = audited()
  const session = (await login(ctx, 1, 'load-user-004', password)).data
  const rotated = await refresh(1, session.refreshToken)
  const replayed = await refresh(2, session.refreshToken)
  const afterReplay = audited() - start
  const descendant = await refresh(1, rotated.data.refreshToken)
  const afterDescendant = audited() - start

  // The control: a live token over the same call, so a 401 above is the reuse
  // and not the path, the body or the instance. Its replay is a third row.
  const live = (await login(ctx, 2, 'load-user-005', password)).data
  const liveRotated = await refresh(1, live.refreshToken)
  const liveReplayed = await refresh(2, live.refreshToken)
  const afterControl = audited() - start

  return {
    pass:
      rotated.status === 200 &&
      replayed.status === 401 &&
      descendant.status === 401 &&
      liveRotated.status === 200 &&
      liveReplayed.status === 401 &&
      [afterReplay, afterDescendant, afterControl].join() === '1,2,3',
    rotatedOn1: rotated.status,
    replayedOn2: replayed.status,
    descendantOn1: descendant.status,
    auditedRows: { afterReplay, afterDescendant, afterControl },
    revokedReasons: sql(ctx, `SELECT revoked_reason, count(*) FROM refresh_tokens t JOIN users u
      ON u.id = t.user_id WHERE u.username = 'load-user-004' GROUP BY 1 ORDER BY 1`),
    control: { liveRotatedAcross: liveRotated.status, replayedBack: liveReplayed.status },
  }
}

// ---------------------------------------------------------------------------
// AC5's two sentences
// ---------------------------------------------------------------------------

/** The rate limit, read from one source on each instance in turn. */
async function rateLimit(ctx) {
  const from = `203.0.${Math.floor(Math.random() * 100) + 100}.40`
  const attempt = (n, source = from) => login(ctx, n, 'nobody-row-4', 'wrong', source)
  const failuresBefore429 = []

  for (const n of INSTANCES) {
    let failures = 0

    while (failures < 30 && (await attempt(n)).status !== 429) {
      failures += 1
    }

    failuresBefore429.push(failures)
  }

  return {
    pass: failuresBefore429.join() === '10,10',
    failuresBefore429,
    stillRefused: [(await attempt(1)).status, (await attempt(2)).status],
    anotherSourceOn1: (await attempt(1, '203.0.113.41')).status,
  }
}

const auditedAs = (ctx, user, reason) =>
  Number(sql(ctx, `SELECT count(*) FROM audit_events e JOIN users u ON u.id = e.object_id
    WHERE e.event_type = 'Security.SignInFailed' AND u.username = '${user}' AND e.reason = '${reason}'`))

/** Guesses sent at once by `lockout`, to one instance or half to each. */
const BURST = 120

/** `count` wrong passwords at once, each from its own source, inside instance `n`. */
function guesses(ctx, n, user, count, seed) {
  const script = `for i in $(seq ${count}); do
    curl -s -o /dev/null -H "X-Forwarded-For: 203.0.${seed}.$i" -H 'Content-Type: application/json' \
      -d '{"username":"${user}","password":"wrong-guess"}' http://127.0.0.1:8080/api/v1/auth/login &
  done; wait`

  return new Promise((done) => spawn('docker', ['exec', name(ctx, n), 'sh', '-c', script]).on('close', done))
}

/**
 * Five failures across the two lock the account on both, and four do not. Then a
 * burst, all to one instance and half to each, reading how many guesses reached
 * the password check: each of those read the row before the lock was on it.
 */
async function lockout(ctx) {
  const password = ctx.stack().password
  const user = 'load-user-006'
  const reset = (who) =>
    sql(ctx, `UPDATE users SET locked_until = NULL, failed_login_count = 0 WHERE username = '${who}'`)
  const fail = (n) => login(ctx, n, user, 'wrong-guess')
  const checked = (who) => auditedAs(ctx, who, 'incorrect password')
  const byLockBefore = auditedAs(ctx, user, 'account is locked out after repeated failures')

  reset(user)

  for (const n of [1, 2, 1, 2]) {
    await fail(n)
  }

  const afterFour = (await login(ctx, 2, user, password)).status

  for (const n of [1, 2, 1, 2, 1]) {
    await fail(n)
  }

  const afterFive = [(await login(ctx, 1, user, password)).status, (await login(ctx, 2, user, password)).status]
  const refusedByLock = auditedAs(ctx, user, 'account is locked out after repeated failures') - byLockBefore
  const burst = { sent: BURST, oneInstance: [], twoInstances: [] }

  for (let round = 0; round < 3; round += 1) {
    reset('load-user-007')
    reset('load-user-008')

    const one = checked('load-user-007')

    await guesses(ctx, 1, 'load-user-007', BURST, 10 + round)
    burst.oneInstance.push(checked('load-user-007') - one)

    const two = checked('load-user-008')

    await Promise.all([
      guesses(ctx, 1, 'load-user-008', BURST / 2, 20 + round),
      guesses(ctx, 2, 'load-user-008', BURST / 2, 30 + round),
    ])
    burst.twoInstances.push(checked('load-user-008') - two)
  }

  return {
    pass: afterFour === 200 && afterFive.join() === '401,401' && refusedByLock === 2,
    afterFour,
    afterFive,
    refusedByLock,
    burst,
  }
}

// ---------------------------------------------------------------------------
// What AC4 says of the notification sender, measured
// ---------------------------------------------------------------------------

/** One row, two senders: started together, held on the relay, and after one restarted. */
async function mailOnce(ctx) {
  const mailpit = `${ctx.config.project}-mailpit-1`
  const mails = (tag) =>
    JSON.parse(docker(['exec', mailpit, 'wget', '-qO-', `http://127.0.0.1:8025/api/v1/search?query=${tag}&limit=1`]).stdout)
      .messages_count
  const pollGapMs = () => {
    const [a, b] = logs(ctx).map((log) =>
      Date.parse(log.split('\n').findLast((line) => line.includes('SELECT n.id, n.tenant_id')).split(' ')[0]),
    )

    return Math.min(Math.abs(a - b) % 5000, 5000 - (Math.abs(a - b) % 5000))
  }
  const measure = async (count, hold = false) => {
    const tag = `row4x${hex(4)}`
    const pollGapBeforeMs = pollGapMs()

    if (hold) {
      docker(['pause', mailpit])
    }

    sql(ctx, `INSERT INTO notifications (id, tenant_id, recipient_user_id, notification_type, title,
                body, channel, status)
              SELECT gen_random_uuid(), u.tenant_id, u.id, 'TASK_ASSIGNED', '${tag} ' || n, '${tag}',
                'IN_APP', 'PENDING'
              FROM users u, generate_series(1, ${count}) AS n WHERE u.username = 'load-user-009'`)

    if (hold) {
      await pause(12_000)
      docker(['unpause', mailpit])
    }

    await until('delivery', () =>
      sql(ctx, `SELECT count(*) FROM notifications WHERE body = '${tag}' AND status = 'PENDING'`) === '0',
    )
    await pause(6_000)

    return {
      notifications: count,
      mails: mails(tag),
      attemptsLogged: Number(sql(ctx, `SELECT count(*) FROM notification_logs l JOIN notifications n
        ON n.id = l.notification_id WHERE n.body = '${tag}'`)),
      pollGapBeforeMs,
      pollGapAfterMs: pollGapMs(),
    }
  }

  const startedTogether = await measure(64)
  const heldOnTheRelay = await measure(3, true)

  docker(['restart', '-t', '1', name(ctx, 2)])
  await pause(8_000)

  const afterOneRestarted = []

  for (let round = 0; round < 4; round += 1) {
    afterOneRestarted.push(await measure(4))
  }

  return { startedTogether, heldOnTheRelay, afterOneRestarted }
}

async function cross(ctx) {
  sql(ctx, INSTRUMENT)

  const stack = ctx.stack()
  const signedIn = await login(ctx, 2, 'admin', stack.env.KELIR_BOOTSTRAP_ADMIN_PASSWORD)
  const token = signedIn.data.accessToken
  const type = await arrange(ctx, token)
  const document = await newDocument(ctx, 1, token, type)
  const checks = {
    tokenCrosses: () => tokenCrosses(ctx),
    refreshReplay: () => refreshReplay(ctx),
    fileCrosses: () => fileCrosses(ctx, token, document),
    scanSettlesOnce: () => scanSettlesOnce(ctx, token, document),
    outboxDeliversOnce: () => outboxDeliversOnce(ctx, token, type),
    rateLimit: () => rateLimit(ctx),
    lockout: () => lockout(ctx),
    mailOnce: () => mailOnce(ctx),
  }
  const results = { at: new Date().toISOString() }

  for (const [check, run] of Object.entries(checks)) {
    console.log(`\n==> ${check}`)

    try {
      results[check] = await run()
    } catch (error) {
      results[check] = { pass: false, error: error.message }
    }

    console.log(JSON.stringify(results[check]).slice(0, 1_000))
    writeFileSync(join(stack.runDirectory, `cross-${results.at.replace(/\D/g, '')}.json`), JSON.stringify(results, null, 2))
  }

  sql(ctx, 'DROP FUNCTION row4_log CASCADE; DROP TABLE row4_writes;')
  process.exitCode = Object.keys(checks).some((check) => results[check].pass === false) ? 1 : 0
}

/** The browser suite, with what each instance served during it. */
async function e2e(ctx, args) {
  const stack = ctx.stack()
  const before = served(ctx)
  const file = join(stack.runDirectory, `e2e-${Date.now()}.log`)
  const suite = join(HERE, '..', 'e2e')

  console.log(`The browser suite, logging to ${file}`)

  const child = spawn(process.execPath, [join(suite, 'node_modules/@playwright/test/cli.js'), 'test', ...args], {
    cwd: suite,
    env: {
      ...process.env,
      KELIR_E2E_BASE_URL: stack.url,
      KELIR_E2E_USERNAME: 'admin',
      KELIR_E2E_PASSWORD: stack.env.KELIR_BOOTSTRAP_ADMIN_PASSWORD,
      KELIR_E2E_UPSTREAM_TOKEN: stack.env.KELIR_E2E_UPSTREAM_TOKEN,
      KELIR_E2E_UPSTREAM_URL: `http://127.0.0.1:${stack.env.KELIR_E2E_UPSTREAM_PORT}`,
    },
    stdio: ['ignore', 'pipe', 'pipe'],
  })
  const log = createWriteStream(file)
  const limit = setTimeout(() => child.kill(), 40 * 60_000)

  child.stdout.pipe(log)
  child.stderr.pipe(log)

  const code = await new Promise((done) => child.on('close', done))

  clearTimeout(limit)
  console.log(JSON.stringify({ exit: code, servedDuringTheSuite: change(served(ctx), before) }))
  process.exitCode = code === 0 ? 0 : 1
}

// ---------------------------------------------------------------------------
// Caddy on a given file, the split through it, and the control on "0 ERROR"
// ---------------------------------------------------------------------------

const caddy = (ctx) => `${ctx.config.project}-caddy-1`
const caddyfileInUse = (ctx) => docker(['exec', caddy(ctx), 'cat', '/etc/caddy/Caddyfile']).stdout

/** Writes `text` to `file` and an overlay that mounts it as Caddy's configuration. */
function caddyOverlay(directory, file, text, label) {
  const overlay = join(directory, `compose.${label}-caddy.yml`)

  writeFileSync(file, text)
  writeFileSync(
    overlay,
    `services:
  caddy:
    volumes:
      - type: bind
        source: ${file.replaceAll('\\', '/')}
        target: /etc/caddy/Caddyfile
        read_only: true
`,
  )

  return overlay
}

/** Recreates Caddy alone, so the backends and their logs are not touched. */
async function recreateCaddy(ctx, ...overlay) {
  const { status, stderr } = spawnSync(
    'docker',
    [...ctx.composeArgs(), ...overlay, 'up', '-d', '--no-deps', '--force-recreate', 'caddy'],
    { encoding: 'utf8', timeout: 120_000 },
  )

  if (status !== 0) {
    throw new Error(`recreating caddy: ${stderr}`)
  }

  await until(
    'caddy',
    () => docker(['exec', caddy(ctx), 'wget', '-qO-', 'http://127.0.0.1:80/version']).status === 0,
    30,
  )
}

// ---------------------------------------------------------------------------
// The split through a Caddyfile as it is, asserted (#854)
// ---------------------------------------------------------------------------

/**
 * How far from half either instance may land, as a share of the requests sent:
 * 5 %, so 135 to 165 of 300.
 *
 * Round robin hands each request to the next upstream, so a serial run and a
 * concurrent one alike should split exactly, and #854's runs on it did: smoke
 * read [8,8] in 8 of 8. The margin is for the `dynamic` upstream's lookup of
 * `backend` being refreshed mid-run and answered in another order, which can
 * hand one instance two requests in a row. Every reading #854 took through the
 * shipped file is outside it: serial traffic read [0,300] or [300,0], and the
 * nearest concurrent one, [118,182], is 32 from even, over twice the margin.
 */
export const EVEN_TOLERANCE = 0.05

/** The margin in requests, whole, so 300 gives 15 and not 15.000000000000002. */
const marginOf = (sent) => Math.round(sent * EVEN_TOLERANCE)

export const isEven = ({ sent, counted }) =>
  sent > 0 &&
  counted.every((count) => Math.abs(count - sent / 2) <= marginOf(sent))

/** Why a split run fails, or nothing. `expect` is `even` or `one-sided`. */
export function splitFailures(expect, { inUse, serial, concurrent, smokeExit }) {
  const failures = []
  const readings = { serial, 'ten at a time': concurrent }

  if (!inUse) {
    failures.push('Caddy is not serving the file under test')
  }

  // A count that does not add up is not a reading of where anything went.
  for (const [label, reading] of Object.entries(readings)) {
    if (total(reading.counted) !== reading.sent) {
      failures.push(`${label}: the instances counted ${total(reading.counted)} of ${reading.sent}`)
    }

    // Requests the caller saw fail are not a split of traffic served.
    if ((reading.statuses?.[200] ?? 0) !== reading.sent) {
      failures.push(`${label}: ${JSON.stringify(reading.statuses)} of ${reading.sent} answered, not all 200`)
    }
  }

  if (smokeExit !== 0) {
    failures.push(`smoke exited ${smokeExit}`)
  }

  if (expect === 'even') {
    for (const [label, reading] of Object.entries(readings)) {
      if (!isEven(reading)) {
        failures.push(
          `${label}: [${reading.counted}] of ${reading.sent} is not even within ` +
            `±${marginOf(reading.sent)} of ${reading.sent / 2} each`,
        )
      }
    }
  } else if (!serial.counted.includes(0)) {
    failures.push(`serial: [${serial.counted}] reached both instances; the control expects one`)
  }

  return failures
}

/**
 * Traffic through Caddy started cold on `file`, unchanged: 300 requests one at a
 * time, 300 ten at a time, then #777's smoke. Caddy is put back after.
 *
 * `even` is #854's assertion on the shipped file. `one-sided` is its control, on
 * a file that does not balance: serial traffic all on one instance, as #854
 * measured it every time, which shows the same reading can tell them apart.
 */
async function split(ctx, [file, expect]) {
  if (!file || !['even', 'one-sided'].includes(expect)) {
    throw new Error('usage: node run.mjs two split <Caddyfile> even|one-sided')
  }

  const directory = ctx.stack().runDirectory
  const text = readFileSync(file, 'utf8')
  const overlay = caddyOverlay(directory, join(directory, `Caddyfile.${expect}`), text, expect)
  const stamp = Date.now()
  const result = {
    at: new Date().toISOString(),
    expect,
    caddyfile: file,
    caddyfileSha256: sha(text),
    tolerance: EVEN_TOLERANCE,
  }

  try {
    await recreateCaddy(ctx, '-f', overlay)
    result.inUse = caddyfileInUse(ctx) === text

    const { accessToken: token } = (await login(ctx, 1, 'load-user-003', ctx.stack().password)).data

    result.serial = await throughCaddy(ctx, token, 300, 1)
    result.concurrent = await throughCaddy(ctx, token, 30, 10)

    const before = served(ctx)
    const smoke = spawnSync(process.execPath, [join(HERE, 'run.mjs'), 'smoke'], {
      encoding: 'utf8',
      timeout: 180_000,
    })

    writeFileSync(join(directory, `split-${expect}-smoke-${stamp}.log`), `${smoke.stdout}${smoke.stderr}`)
    result.smokeExit = smoke.status
    result.smokeMoved = change(served(ctx), before)
  } finally {
    await recreateCaddy(ctx)
  }

  result.failures = splitFailures(expect, result)

  const record = JSON.stringify({ ...result, pass: result.failures.length === 0 }, null, 2)

  console.log(record)
  writeFileSync(join(directory, `split-${expect}-${stamp}.json`), record)
  process.exitCode = result.failures.length === 0 ? 0 : 1
}

// ---------------------------------------------------------------------------
// A backend stopping, starting or restarting behind a Caddyfile (#854)
// ---------------------------------------------------------------------------

/**
 * A started backend is in the name by the next `refresh`, 10 s, and one whose
 * dial failed is benched for `fail_duration`, 2 s: it serves again within
 * both, plus 2 s of polling. A restarted single backend should be served
 * through Caddy within 5 s of its own readiness, as it was before #854.
 */
export const REJOIN_LIMIT_MS = (10 + 2 + 2) * 1000

/**
 * How many requests must be sent while one backend is stopped for a failover
 * run to count. It only shows that traffic flowed through the outage: what
 * shows a retry was exercised is Caddy's own count of a failed dial, checked
 * beside it. The number that lands in the ~4 s outage follows the runner's
 * speed (CI read 225, 115, 110 and 46), so a floor of 50 failed a run whose
 * every request was answered 200; 10 still refuses a run that sent nothing.
 */
export const MIN_REQUESTS_WHILE_STOPPED = 10
export const RESTART_GAP_LIMIT_MS = 5_000

const now = () => Date.now()

/** `docker`, without blocking the requests the event loop is sending beside it. */
const dockerAsync = (args) =>
  new Promise((done) => {
    const child = spawn('docker', args, { stdio: ['ignore', 'pipe', 'pipe'] })
    const [out, err] = [[], []]

    child.stdout.on('data', (chunk) => out.push(chunk))
    child.stderr.on('data', (chunk) => err.push(chunk))
    child.on('error', () => done({ status: -1, stdout: '', stderr: '' }))
    child.on('close', (status) => done({ status, stdout: `${Buffer.concat(out)}`, stderr: `${Buffer.concat(err)}` }))
  })

/** Polls `test` every 250 ms for up to `seconds`; whether it held. */
async function within(seconds, test) {
  for (const end = now() + seconds * 1000; now() < end; await pause(250)) {
    if (await test()) {
      return true
    }
  }

  return false
}

/** `probe`'s status, again `every` ms after each answer until stopped, as `{ start, end, status }`. */
function keepPolling(t0, probe, every) {
  const seen = []
  let running = true
  const loop = (async () => {
    while (running) {
      const start = now() - t0

      seen.push({ start, status: await probe(), end: now() - t0 })
      await pause(every)
    }
  })()

  return { seen, stop: () => ((running = false), loop.then(() => seen)) }
}

/** A status, or 0 for a request refused, reset or timed out, whose body is read whole. */
const statusOf = (url, headers) =>
  fetch(url, { headers, signal: AbortSignal.timeout(10_000) })
    .then(async (answer) => (await answer.arrayBuffer(), answer.status))
    .catch(() => 0)
const readyInside = async (container) =>
  (await dockerAsync(['exec', container, 'curl', '-fsS', '--max-time', '2', '-o', '/dev/null',
    'http://127.0.0.1:8080/health/ready'])).status === 0 ? 200 : 0
const servedBy = async (container) => {
  const { stdout, stderr } = await dockerAsync(['logs', container])

  return countOf(`${stdout}${stderr}`, 'AS tenant_deleted')
}
const servedNow = (ctx) => Promise.all(INSTANCES.map((n) => servedBy(name(ctx, n))))

/** Caddy's own count of failed dials, sampled every 500 ms, so a bench is seen and not inferred. */
const watchUpstreams = (ctx, t0) =>
  keepPolling(t0, async () => {
    const { status, stdout } = await dockerAsync(['exec', caddy(ctx), 'wget', '-qO-',
      'http://127.0.0.1:2019/reverse_proxy/upstreams'])

    try {
      return status === 0 ? JSON.parse(stdout) : null
    } catch {
      return null
    }
  }, 500)

/** Per upstream address, the most failures Caddy held and from when to when it held any. */
export function benchesOf(samples) {
  const benches = {}

  for (const { start, status: upstreams } of samples) {
    for (const { address, fails } of upstreams ?? []) {
      const bench = (benches[address] ??= { maxFails: 0, from: null, until: null })

      bench.maxFails = Math.max(bench.maxFails, fails)

      if (fails > 0) {
        bench.from ??= start
        bench.until = start
      }
    }
  }

  return benches
}

/** Answers by status, and how many were sent from `from` to `to`. */
export const answersOf = (answers, from, to) => ({
  byStatus: answers.reduce((counts, { status }) => ({ ...counts, [status]: (counts[status] ?? 0) + 1 }), {}),
  firstBad: answers.filter(({ status }) => status !== 200).slice(0, 5),
  total: answers.length,
  whileOneServed: answers.filter(({ start }) => start >= from && start < to).length,
})

/** Why a failover run fails, or nothing. */
export function failoverFailures({ inUse, answers, benches, marks, served }) {
  const ok = answers.byStatus[200] ?? 0
  const checks = [
    [inUse, 'Caddy is not serving the file under test'],
    [ok === answers.total, `${answers.total - ok} of ${answers.total} answered other than 200: ${JSON.stringify(answers.byStatus)}`],
    [answers.whileOneServed >= MIN_REQUESTS_WHILE_STOPPED, `${answers.whileOneServed} requests were sent while one backend was stopped, fewer than ${MIN_REQUESTS_WHILE_STOPPED}`],
    [served.beforeStop[1] > 0, 'the backend stopped had served nothing before it was, so nothing failed over'],
    [Object.values(benches).some(({ maxFails }) => maxFails > 0), 'Caddy never counted a failed dial, so no retry was exercised'],
    [marks.ready !== undefined, 'the started backend never reported ready'],
    [marks.rejoined - marks.ready <= REJOIN_LIMIT_MS, `the started backend served nothing through Caddy within ${REJOIN_LIMIT_MS} ms of its readiness`],
    [total(served.all) >= ok, `the instances counted ${total(served.all)} of the ${ok} answered`],
  ]

  return checks.filter(([held]) => !held).map(([, failure]) => failure)
}

/**
 * #854 (b): three callers through `file` while the second backend is stopped,
 * left stopped 4 s and started. Every answer must be 200, so the survivor took
 * what the other could not serve while it drained, was gone and started, and
 * the started one must serve through Caddy within REJOIN_LIMIT_MS of its readiness.
 */
async function failover(ctx, [file]) {
  const second = name(ctx, 2)
  const marks = {}

  await underCaddy(ctx, 'failover', file, { marks }, failoverFailures, async (result, t0) => {
    const mark = (label) => (marks[label] = now() - t0)
    const { accessToken: token } = (await login(ctx, 1, 'load-user-003', ctx.stack().password)).data
    const url = `${ctx.stack().url}/api/v1/auth/me`
    const before = await servedNow(ctx)
    const callers = [1, 2, 3].map(() => keepPolling(t0, () => statusOf(url, { Authorization: `Bearer ${token}` }), 50))

    await pause(3_000)

    const atStop = await servedNow(ctx)

    mark('stop')
    await dockerAsync(['stop', '-t', '10', second])
    mark('stopped')

    const stoppedHad = await servedBy(second)

    await pause(4_000)
    mark('start')
    await dockerAsync(['start', second])

    if (await within(90, async () => (await readyInside(second)) === 200)) {
      mark('ready')

      if (await within(REJOIN_LIMIT_MS / 1000 + 15, async () => (await servedBy(second)) > stoppedHad)) {
        mark('rejoined')
      }
    }

    await pause(1_000)
    result.answers = answersOf((await Promise.all(callers.map((caller) => caller.stop()))).flat(), marks.stop, marks.start)
    await pause(1_000)
    result.served = { beforeStop: change(atStop, before), all: change(await servedNow(ctx), before) }
  })
}

/**
 * From a restart's probes, `{ start, end, status }` in ms: whether the backend
 * was seen down after `restartAt`, when its own probe was answered 200 again,
 * and when Caddy was, for good, on a probe sent after the backend's last refusal.
 */
export function restartGap({ through, inside }, restartAt) {
  const after = inside.filter(({ start }) => start >= restartAt)
  const down = after.findIndex(({ status }) => status !== 200)
  const back = down === -1 ? -1 : after.findIndex(({ status }, index) => index > down && status === 200)
  const readyAt = back === -1 ? null : after[back].end
  // Caddy's first 200 for good: after its last other answer, and sent after the backend's last refusal.
  const lastBad = through.findLastIndex(({ status }) => status !== 200)
  const answered = back === -1 ? undefined : through.find(({ start }, index) => index > lastBad && start >= after[back - 1].start)
  const caddyAt = answered ? answered.end : null

  return {
    wentDown: down !== -1,
    readyAt,
    caddyAt,
    gapMs: readyAt === null || caddyAt === null ? null : Math.max(0, caddyAt - readyAt),
  }
}

/** Why a restart run fails, or nothing. */
export function restartFailures({ inUse, backends, gap }) {
  const failures = [
    [inUse, 'Caddy is not serving the file under test'],
    [backends === 1, `${backends} backends were running, and the shipped stack has one`],
    [gap.wentDown, 'the backend was never seen down, so nothing was restarted'],
    [!gap.wentDown || gap.readyAt !== null, 'the backend never reported ready after its restart'],
    [gap.readyAt === null || gap.caddyAt !== null, 'Caddy never answered /health/ready 200 after the backend was ready'],
    [!(gap.gapMs > RESTART_GAP_LIMIT_MS), `Caddy answered 200 ${gap.gapMs} ms after the backend was ready, past ${RESTART_GAP_LIMIT_MS}`],
  ]

  return failures.filter(([held]) => !held).map(([, failure]) => failure)
}

/**
 * #854 (c): the shipped shape, one backend, stopped for 3 s and started while
 * `deploy.sh`'s probe polls `/health/ready` through Caddy on `file`, and inside it. The
 * second backend is removed first, and stays removed.
 */
async function restart(ctx, [file]) {
  ctx.compose('up', '-d', '--no-deps', '--no-recreate', '--scale', 'backend=1', 'backend')

  const running = docker(['ps', '--format', '{{.Names}}', '--filter', `label=com.docker.compose.project=${ctx.config.project}`,
    '--filter', 'label=com.docker.compose.service=backend']).stdout.split(/\s+/).filter(Boolean)
  const only = running[0]
  const ready = () => statusOf(`${ctx.stack().url}/health/ready`)

  await underCaddy(ctx, 'restart', file, { backends: running.length }, restartFailures, async (result, t0) => {
    if (!(await within(30, async () => (await ready()) === 200))) {
      throw new Error('Caddy never answered /health/ready 200 before the restart')
    }

    const probes = { through: keepPolling(t0, ready, 250), inside: keepPolling(t0, () => readyInside(only), 250) }
    const gap = () => restartGap({ through: probes.through.seen, inside: probes.inside.seen }, result.restartAt)

    await pause(1_000)
    result.restartAt = now() - t0
    // Held down 3 s, as a start that migrates is: `docker restart` here is back
    // in under 300 ms, between two probes, and so proves nothing (#854's first run).
    await dockerAsync(['stop', '-t', '10', only])
    await pause(3_000)
    await dockerAsync(['start', only])
    await within(120, () => gap().readyAt !== null)
    await within(RESTART_GAP_LIMIT_MS / 1000 + 40, () => gap().caddyAt !== null)
    await pause(2_000)
    result.probes = { through: await probes.through.stop(), inside: await probes.inside.stop() }
    result.gap = gap()
  })
}

/**
 * Caddy recreated cold on `file`, unchanged, then `body`, with its upstreams
 * watched; the shipped file is put back after, and the record, judged, is kept.
 */
async function underCaddy(ctx, label, file, start, judge, body) {
  if (!file) {
    throw new Error(`usage: node run.mjs two ${label} <Caddyfile>`)
  }

  const directory = ctx.stack().runDirectory
  const text = readFileSync(file, 'utf8')
  const stamp = Date.now()
  const result = { at: new Date().toISOString(), caddyfile: file, caddyfileSha256: sha(text), ...start }

  try {
    await recreateCaddy(ctx, '-f', caddyOverlay(directory, join(directory, `Caddyfile.${label}-${stamp}`), text, `${label}-${stamp}`))
    result.inUse = caddyfileInUse(ctx) === text

    const t0 = now()
    const upstreams = watchUpstreams(ctx, t0)
    await body(result, t0)
    result.benches = benchesOf(await upstreams.stop())
    result.failures = judge(result)
  } finally {
    await recreateCaddy(ctx)
  }

  const record = JSON.stringify({ ...result, pass: result.failures.length === 0 }, null, 2)

  console.log(record)
  writeFileSync(join(directory, `${label}-${stamp}.json`), record)
  process.exitCode = result.failures.length === 0 ? 0 : 1
}

/**
 * A positive control for every "0 ERROR" on this stack. A stored hash that is not
 * a PHC string logs ERROR at `password.rs:34`, and its caller only sees a 401, so
 * nothing is held or starved. One sign-in on each instance, on a user of its own.
 */
async function errors(ctx) {
  const user = 'row4-error-control'
  const read = () => logs(ctx).map((log) => levelCount(log, 'ERROR'))

  sql(ctx, `INSERT INTO users (id, tenant_id, username, email, password_hash, display_name, status,
              must_change_password)
            SELECT gen_random_uuid(), tenant_id, '${user}', '${user}@load.kelir.invalid',
              'not-a-phc-string', 'Row 4 error control', 'ACTIVE', false
            FROM users WHERE username = 'load-user-009' ON CONFLICT DO NOTHING;
            UPDATE users SET failed_login_count = 0, locked_until = NULL WHERE username = '${user}';`)

  const before = read()
  const legs = []

  for (const n of INSTANCES) {
    const start = read()
    const { status } = await login(ctx, n, user, 'any-password')

    await pause(1_000)
    legs.push({ signedInOn: n, status, moved: change(read(), start) })
  }

  const result = {
    at: new Date().toISOString(),
    before,
    legs,
    after: read(),
    lines: logs(ctx).map((log) => linesAt(log, 'ERROR')),
    pass: legs.every(({ signedInOn, moved }) => moved[signedInOn - 1] > 0 && moved[2 - signedInOn] === 0),
  }

  console.log(JSON.stringify(result, null, 2))
  writeFileSync(join(ctx.stack().runDirectory, `errors-${Date.now()}.json`), JSON.stringify(result, null, 2))
  process.exitCode = result.pass ? 0 : 1
}

export async function twoBackends(step, args, ctx) {
  const steps = {
    up,
    race,
    traffic,
    cross,
    e2e,
    document: oneDocument,
    split,
    failover,
    restart,
    errors,
    count: () => console.log(JSON.stringify(served(ctx))),
  }

  if (!steps[step]) {
    throw new Error(`usage: node run.mjs two ${Object.keys(steps).join(' | ')}`)
  }

  return steps[step](ctx, args)
}
