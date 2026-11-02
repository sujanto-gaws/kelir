#!/usr/bin/env node
// The load test's runner (#777). `README.md` says how to use it.
//
//   node run.mjs up                      the stack, seeded and checked
//   node run.mjs check                   counts the data set against AC3
//   node run.mjs smoke                   AC10: two users, one list, one detail
//   node run.mjs measure target          AC4's window, with the Chromium session
//   node run.mjs measure saturation      the same, with zero think time
//   node run.mjs measure tabs            the window, with AC2's tab calls added
//   node run.mjs report <measurement>    reads a measurement's directory again
//   node run.mjs down                    removes the stack, its volumes and network
//
// **Every command passes the refusals first** (`guard.mjs`), before anything
// else is read or started. Exit code 2 is a refusal; 1 is a failure or a void.

import { spawn, spawnSync } from 'node:child_process'
import { randomBytes } from 'node:crypto'
import {
  appendFileSync,
  createWriteStream,
  existsSync,
  mkdirSync,
  readFileSync,
  rmdirSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { createInterface } from 'node:readline'
import { fileURLToPath } from 'node:url'

import { portOf, refusals } from './guard.mjs'
import { summarize } from './report.mjs'
import { checkDataset, seedDataset } from './seed/seed.mjs'

const HERE = dirname(fileURLToPath(import.meta.url))
const ROOT = resolve(HERE, '..')
const RUNS = join(HERE, '.run')
const STATE = join(RUNS, 'stack.json')

/** The k6 release, by version and digest (AC9). Not a product image. */
const K6_IMAGE =
  'grafana/k6:1.3.0@sha256:3ddc8b1a33a2c3d8edc6e99b6a762ae36cba08788463458f5e6a7703e14eb77d'

const MODES = {
  target: { think: '1', tabs: '0' },
  saturation: { think: '0', tabs: '0' },
  tabs: { think: '1', tabs: '1' },
}

const LEAD_IN_MS = 10_000
const WINDOW_MS = Number(process.env.KELIR_LOAD_WINDOW_MS || 300_000)

/**
 * The provoked half of the log's control, sized against the pool's two limits:
 * sqlx warns of an acquire over 2 s, and `db.rs` gives up on one at 5 s.
 *
 * The lock is seen up to a second after it is taken. The first wave's requests
 * take all 10 connections, which block on the lock and so run past sqlx's 1 s
 * slow-statement line, and the rest of that wave wait 5 s and time out. The
 * second wave arrives 3.5 s after the lock is seen and is served when it lifts,
 * 3 to 4 s later: slower than 2 s, and short of 5 s.
 */
const CONTROL = { holdMs: 7_500, firstWave: 30, secondWave: 6, secondWaveAtMs: 3_500 }

let state = existsSync(STATE) ? JSON.parse(readFileSync(STATE, 'utf8')) : null

const config = {
  project: process.env.KELIR_LOAD_PROJECT || state?.project || 'kelir-load',
  httpPort: Number(process.env.KELIR_LOAD_PORT || state?.httpPort || 18097),
  httpsPort: Number(process.env.KELIR_LOAD_HTTPS_PORT || state?.httpsPort || 18447),
  database:
    process.env.KELIR_LOAD_DATABASE ||
    state?.database ||
    `kelir_load_${randomBytes(4).toString('hex')}`,
  imageTag:
    process.env.KELIR_LOAD_IMAGE_TAG ||
    state?.imageTag ||
    `load-${git('rev-parse', '--short', 'HEAD')}`,
}

function git(...args) {
  return run('git', args, { cwd: ROOT }).trim()
}

function say(message) {
  console.log(`\n==> ${message}`)
}

/** Runs a command to its end and returns its stdout; a failure is thrown. */
function run(command, args, options = {}) {
  const result = spawnSync(command, args, { encoding: 'utf8', maxBuffer: 1 << 30, ...options })

  if (result.status !== 0) {
    throw new Error(`${command} ${args.slice(0, 6).join(' ')}: ${result.stderr || result.error}`)
  }

  return result.stdout
}

function composeArgs() {
  return [
    'compose',
    '-p',
    config.project,
    '--env-file',
    join(state.runDirectory, 'stack.env'),
    '-f',
    join(ROOT, 'deploy/staging/docker-compose.staging.yml'),
    '-f',
    join(HERE, 'compose.overlay.yml'),
  ]
}

function psqlArgs(extra = []) {
  return [
    ...composeArgs(),
    'exec',
    '-T',
    'postgres',
    'psql',
    '-U',
    'kelir_load',
    '-d',
    config.database,
    '-v',
    'ON_ERROR_STOP=1',
    '-qAt',
    ...extra,
  ]
}

const psql = (sql, extra = []) => run('docker', psqlArgs(extra), { input: sql })

function requireStack() {
  if (!state) {
    throw new Error('no stack is up: run `node run.mjs up` first')
  }
}

function buildImagesIfMissing() {
  const commit = config.imageTag.replace(/^load-/, '')

  for (const side of ['backend', 'frontend']) {
    const image = `kelir-${side}:${config.imageTag}`

    if (spawnSync('docker', ['image', 'inspect', image], { stdio: 'ignore' }).status === 0) {
      continue
    }

    say(`Building ${image}`)
    run(
      'docker',
      [
        'build',
        '-f',
        join(ROOT, `deploy/docker/${side}.Dockerfile`),
        '--build-arg',
        `KELIR_BUILD_SHA=${commit}`,
        '-t',
        image,
        join(ROOT, `kelir-${side}`),
      ],
      { stdio: ['ignore', 'inherit', 'inherit'] },
    )
  }
}

/** Real generated secrets, and a staging-shaped environment (AC5). */
function writeStackEnv(directory) {
  const secret = (bytes) => randomBytes(bytes).toString('base64')
  const hex = (bytes) => randomBytes(bytes).toString('hex')
  const lines = {
    COMPOSE_PROJECT_NAME: config.project,
    KELIR_VERSION: config.imageTag,
    KELIR_APP_ENV: 'staging',
    KELIR_SITE_ADDRESS: ':80',
    KELIR_HTTP_PORT: config.httpPort,
    KELIR_HTTPS_PORT: config.httpsPort,
    KELIR_PUBLIC_URL: `http://127.0.0.1:${config.httpPort}`,
    KELIR_DB_PASSWORD: hex(24),
    KELIR_LOAD_DATABASE: config.database,
    KELIR_JWT_SECRET: secret(36),
    KELIR_MINIO_USER: `load-${hex(6)}`,
    KELIR_MINIO_PASSWORD: hex(24),
    KELIR_MINIO_KMS_SECRET_KEY: `kelir-store:${secret(32)}`,
  }

  writeFileSync(
    join(directory, 'stack.env'),
    Object.entries(lines)
      .map(([key, value]) => `${key}=${value}`)
      .join('\n'),
  )
}

const pause = (ms) => new Promise((done) => setTimeout(done, ms))

async function waitForMigrations() {
  for (let attempt = 0; attempt < 90; attempt += 1) {
    if (run('docker', [...composeArgs(), 'logs', '--no-color', 'backend']).includes('migrations applied')) {
      return
    }

    await pause(2_000)
  }

  throw new Error('the backend never applied its migrations')
}

/** clamd spends a minute at a full core loading signatures: not part of a window. */
async function waitForHealthy() {
  for (let attempt = 0; attempt < 90; attempt += 1) {
    if (!run('docker', [...composeArgs(), 'ps', '--format', '{{.Status}}']).includes('starting')) {
      return
    }

    await pause(2_000)
  }

  throw new Error('a service never came healthy')
}

async function waitForReady(url) {
  for (let attempt = 0; attempt < 90; attempt += 1) {
    try {
      if ((await fetch(`${url}/health/ready`, { signal: AbortSignal.timeout(3_000) })).ok) {
        return
      }
    } catch {
      // Not answering yet is the normal state of this loop.
    }

    await pause(2_000)
  }

  throw new Error(`${url}/health/ready never answered`)
}

/** What `deploy.sh`'s checks are for: this stack, in this configuration. */
async function verifyStack(url) {
  const services = run('docker', [...composeArgs(), 'ps', '-a', '--format', '{{.Service}} {{.State}} {{.ExitCode}}'])
    .trim()
    .split(/\r?\n/)
    .map((line) => line.split(' '))
  const unwell = services.filter(
    ([, status, code]) => status !== 'running' && !(status === 'exited' && code === '0'),
  )

  if (unwell.length > 0) {
    throw new Error(`services not in a good state: ${unwell.join('; ')}`)
  }

  const backend = await (await fetch(`${url}/version`)).json()
  const frontend = await (await fetch(`${url}/version.json`)).json()
  const commit = config.imageTag.replace(/^load-/, '')

  if (backend.environment !== 'staging' || !String(backend.commit).startsWith(commit)) {
    throw new Error(`/version is not this stack: ${JSON.stringify(backend)}`)
  }

  return { backend, frontend, services: services.map(([name, status]) => `${name} ${status}`) }
}

async function up() {
  if (state) {
    throw new Error(`a stack is already up, at ${state.runDirectory}: run \`down\` first`)
  }

  buildImagesIfMissing()

  const directory = join(RUNS, new Date().toISOString().replace(/[:.]/g, '-'))

  mkdirSync(directory, { recursive: true })

  writeStackEnv(directory)

  const password = randomBytes(18).toString('base64url')
  const url = `http://127.0.0.1:${config.httpPort}`
  const stack = { ...config, runDirectory: directory, password, url }

  writeFileSync(STATE, JSON.stringify(stack, null, 2))
  state = stack

  say(`Starting ${config.project} on ${url}, database ${config.database}`)
  run('docker', [...composeArgs(), 'up', '-d', '--remove-orphans'], {
    stdio: ['ignore', 'inherit', 'inherit'],
  })

  // **The backend migrates, then stops**: it will not serve a default tenant
  // nobody created, and the tenant is the seeder's. So the order is migrations,
  // seed, restart, and the stack is only ever verified with its tenant in place.
  await waitForMigrations()

  say('Seeding')
  const seeded = seedDataset(psql, password, directory)

  run('docker', [...composeArgs(), 'restart', 'backend'], { stdio: ['ignore', 'inherit', 'inherit'] })
  await waitForReady(url)

  const verified = await verifyStack(url)

  writeFileSync(join(directory, 'stack-verified.json'), JSON.stringify({ verified, seeded }, null, 2))
  console.log(`Seeded and counted: ${seeded.checked} counts match, ${seeded.detailIds} detail ids.`)
}

function machineCapture() {
  // `wsl.exe` writes UTF-16, which reads as text with a NUL in every other place.
  const powershell = (command) =>
    spawnSync('powershell.exe', ['-NoProfile', '-Command', command], { encoding: 'utf8' })
      .stdout?.replace(/\0/g, '')
      .trim()
  const docker = (...args) => spawnSync('docker', args, { encoding: 'utf8' }).stdout?.trim()
  const vm = docker('info', '--format', '{{.NCPU}} cpus, {{.MemTotal}} bytes, kernel {{.KernelVersion}}, {{.OperatingSystem}}')

  return {
    capturedAt: new Date().toISOString(),
    cpu: powershell('(Get-CimInstance Win32_Processor).Name'),
    logicalCores: powershell('(Get-CimInstance Win32_Processor).NumberOfLogicalProcessors'),
    ramBytes: powershell('(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory'),
    osBuild: powershell('$o = Get-CimInstance Win32_OperatingSystem; "$($o.Caption) $($o.Version)"'),
    powerPlan: powershell('powercfg /getactivescheme'),
    powerLine: powershell(
      'Add-Type -AssemblyName System.Windows.Forms; [System.Windows.Forms.SystemInformation]::PowerStatus.PowerLineStatus',
    ),
    dockerDesktop: docker('version', '--format', '{{.Server.Platform.Name}}'),
    dockerEngine: docker('version', '--format', '{{.Server.Version}}'),
    wsl2Vm: vm,
    wslConfig: powershell('Get-Content "$env:USERPROFILE\\.wslconfig" -ErrorAction SilentlyContinue'),
    wslVersion: powershell('wsl --version'),
    node: process.version,
    k6: K6_IMAGE,
    generators: 'k6 in the Docker VM and Chromium on the host, both on this machine',
  }
}

/** Background load, in the VM and on the host, named in the record (AC5). */
function backgroundLoad(file) {
  const stats = spawnSync(
    'docker',
    ['stats', '--no-stream', '--format', '{{.Name}}\t{{.CPUPerc}}\t{{.MemUsage}}'],
    { encoding: 'utf8' },
  ).stdout
  const host = spawnSync(
    'powershell.exe',
    [
      '-NoProfile',
      '-Command',
      `typeperf '\\Processor(_Total)\\% Processor Time' -sc 5 | Select-String '^"\\d';
       Get-Process | Sort-Object CPU -Descending | Select-Object -First 8 Name,
         @{n='cpuSeconds';e={[int]$_.CPU}} | Format-Table -AutoSize -HideTableHeaders`,
    ],
    { encoding: 'utf8' },
  ).stdout

  writeFileSync(file, `${stats}\nHost: five seconds of CPU, then the busiest processes\n${host}`)
}

/** How far the Docker VM's clock is from this host's, in milliseconds. */
function clockOffsetMs() {
  const before = Date.now()
  const vm = Number(run('docker', [...composeArgs(), 'exec', '-T', 'postgres', 'date', '+%s%3N']).trim())

  return vm - (before + Date.now()) / 2
}

/**
 * Samples PostgreSQL's view of the pool five times a second (ADR-0045 §6).
 *
 * sqlx exposes no queue, so this reads what the 10 connections are doing: how
 * many are active at each sample, and how many of those are the token check.
 */
function startPoolSampler(directory) {
  const out = createWriteStream(join(directory, 'pool-samples.csv'))
  // A count rather than a kill: stopping the client outside the container would
  // leave psql running inside it.
  const samples = Math.ceil((LEAD_IN_MS + WINDOW_MS + 70_000) / 200)
  // The last two columns name a session that is not the pool's: more than 10
  // client connections, or any that did not arrive over the network. `jsonb`
  // and not `json`, whose array puts a line break between its elements.
  const query = `SELECT (extract(epoch FROM now()) * 1000)::bigint,
    count(*),
    count(*) FILTER (WHERE state = 'active'),
    count(*) FILTER (WHERE state = 'active' AND query LIKE '%tenant_deleted%'),
    count(*) FILTER (WHERE wait_event_type = 'Lock'),
    count(*) FILTER (WHERE client_addr IS NULL),
    CASE WHEN count(*) > 10 OR count(*) FILTER (WHERE client_addr IS NULL) > 0 THEN
      jsonb_agg(jsonb_build_object(
        'pid', pid, 'usename', usename, 'application_name', application_name,
        'client_addr', client_addr::text, 'state', state,
        'backend_start', backend_start, 'query', left(query, 200)))::text
    END
  FROM pg_stat_activity
  WHERE datname = current_database() AND backend_type = 'client backend'
    AND pid <> pg_backend_pid() \\watch i=0.2 c=${samples}\n`
  const sampler = spawn('docker', psqlArgs(['-F', ',']), { stdio: ['pipe', 'pipe', 'inherit'] })

  sampler.stdout.pipe(out)
  sampler.stdin.write(query)

  return sampler
}

/**
 * Counts Caddy's responses by status as they are logged.
 *
 * Read live, because reading the log afterwards undercounts: Docker Desktop's
 * default `local` driver keeps about 100 MB, which held 88,047 of the 258,830
 * responses of the first saturation run. Keeps the non-2xx lines and no others.
 */
function watchCaddy(directory, startMs) {
  const statuses = {}
  // `docker logs` and not `docker compose logs`: killing the compose client
  // leaves its plugin process holding this pipe, and the runner never exits.
  const follower = spawn(
    'docker',
    [
      'logs',
      '--follow',
      '--since',
      new Date(startMs - 5_000).toISOString(),
      `${config.project}-caddy-1`,
    ],
    { stdio: ['ignore', 'pipe', 'ignore'] },
  )
  const refused = createWriteStream(join(directory, 'caddy-refused.log'))

  createInterface({ input: follower.stdout }).on('line', (line) => {
    const status = /"status":\s*(\d{3})/.exec(line)?.[1]

    if (status) {
      statuses[status] = (statuses[status] ?? 0) + 1

      if (!status.startsWith('2')) {
        refused.write(`${line}\n`)
      }
    }
  })

  return async () => {
    await pause(2_000)
    follower.kill()
    follower.stdout.destroy()
    refused.end()
    writeFileSync(join(directory, 'caddy-status.json'), JSON.stringify(statuses, null, 2))
  }
}

/**
 * Keeps all the backend's container has written, as Caddy's log is read.
 *
 * Both streams, each line tagged with the one it came from and stamped by
 * Docker, and from the container's first line and not from the window's: the
 * startup line is half of the control, and `report.mjs` splits the file by time.
 */
function watchBackend(directory) {
  const out = createWriteStream(join(directory, 'backend.log'))
  const follower = spawn(
    'docker',
    ['logs', '--follow', '--timestamps', `${config.project}-backend-1`],
    { stdio: ['ignore', 'pipe', 'pipe'] },
  )
  let open = true
  const keep = (stream, input) =>
    createInterface({ input }).on('line', (line) => {
      if (open) {
        out.write(`${stream} ${line}\n`)
      }
    })

  keep('stdout', follower.stdout)
  keep('stderr', follower.stderr)

  return async () => {
    await pause(2_000)
    follower.kill()
    await Promise.race([finished(follower, 'backend log'), pause(3_000)])
    follower.stdout.destroy()
    follower.stderr.destroy()
    open = false
    await new Promise((done) => out.end(done))
  }
}

async function waitForTheLock() {
  const lock = `SELECT count(*) FROM pg_locks
    WHERE relation = 'documents'::regclass AND mode = 'AccessExclusiveLock' AND granted`

  for (let attempt = 0; attempt < 20; attempt += 1) {
    if (psql(lock).trim() === '1') {
      return
    }

    await pause(100)
  }

  throw new Error('the control never saw its lock')
}

/**
 * Makes the product write the lines this run counts, after the window has shut.
 *
 * A lock on the throwaway database's `documents`, and requests that wait on it.
 * Nothing is written. A run that reports "0 slow acquires" has then shown, in
 * the same file, that a slow acquire reaches that file and is counted.
 */
async function provokeControl() {
  const atMs = Date.now()
  const login = await fetch(`${state.url}/api/v1/auth/login`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ username: 'load-user-050', password: state.password }),
  })
  const token = (await login.json()).data?.accessToken

  if (!token) {
    throw new Error(`the control could not sign in: ${login.status}`)
  }

  const holder = spawn('docker', psqlArgs(), { stdio: ['pipe', 'ignore', 'inherit'] })
  // Attached now: the holder exits as the lock lifts, which is also when the
  // requests answer, and a listener added after its `close` waits for ever.
  const held = finished(holder, 'lock holder')

  holder.stdin.end(`BEGIN;
    LOCK TABLE documents IN ACCESS EXCLUSIVE MODE;
    SELECT pg_sleep(${CONTROL.holdMs / 1000});
    COMMIT;\n`)
  await waitForTheLock()

  const open = () =>
    fetch(`${state.url}/api/v1/documents?page=1&pageSize=20`, {
      headers: { Authorization: `Bearer ${token}` },
    })
      .then((response) => response.status)
      .catch(() => 0)
  const wave = (size) => Array.from({ length: size }, open)
  const first = wave(CONTROL.firstWave)

  await pause(CONTROL.secondWaveAtMs)

  const statuses = await Promise.all([...first, ...wave(CONTROL.secondWave)])
  const { code } = await held

  if (code !== 0) {
    throw new Error(`the control's lock holder exited ${code}`)
  }

  return {
    atMs,
    ...CONTROL,
    statuses: statuses.reduce(
      (counts, status) => ({ ...counts, [status]: (counts[status] ?? 0) + 1 }),
      {},
    ),
  }
}

function finished(child, name) {
  return new Promise((done) => child.on('close', (code) => done({ name, code })))
}

async function measure(mode) {
  requireStack()

  const settings = MODES[mode]

  if (!settings) {
    throw new Error(`unknown measurement ${mode}: one of ${Object.keys(MODES).join(', ')}`)
  }

  say('Checking the data set before measuring')
  checkDataset(psql)
  await waitForHealthy()

  const directory = join(state.runDirectory, `${mode}-${Date.now()}`)

  mkdirSync(directory, { recursive: true })
  writeFileSync(join(directory, 'machine.json'), JSON.stringify(machineCapture(), null, 2))
  backgroundLoad(join(directory, 'background-before.tsv'))

  const offset = clockOffsetMs()
  const startMs = Date.now() + LEAD_IN_MS
  const environment = {
    ...process.env,
    KELIR_LOAD_PASSWORD: state.password,
    KELIR_LOAD_WINDOW_MS: String(WINDOW_MS),
    KELIR_LOAD_THINK: settings.think,
    KELIR_LOAD_TABS: settings.tabs,
    KELIR_LOAD_RUN_DIRECTORY: directory,
    KELIR_LOAD_BASE_URL: state.url,
    KELIR_LOAD_START_MS: String(startMs),
    KELIR_LOAD_DETAIL_IDS: join(state.runDirectory, 'detail-ids.json'),
  }
  const stopWatchingBackend = watchBackend(directory)
  const sampler = startPoolSampler(directory)
  const stopWatchingCaddy = watchCaddy(directory, startMs)
  // Halfway through the window: who holds the cores while the load is on.
  const midway = setTimeout(
    () => backgroundLoad(join(directory, 'background-mid.tsv')),
    startMs + 35_000 + WINDOW_MS / 2 - Date.now(),
  )

  say(`${mode}: first sign-in at ${new Date(startMs).toISOString()}, window ${WINDOW_MS / 1000} s`)

  const k6 = spawn(
    'docker',
    [
      'run',
      '--rm',
      '--name',
      `${config.project}-k6`,
      '--network',
      `${config.project}_default`,
      '-v',
      `${join(HERE, 'scenarios')}:/scenarios:ro`,
      '-v',
      `${state.runDirectory}:/stack:ro`,
      '-v',
      `${directory}:/out`,
      '-e',
      'KELIR_LOAD_PASSWORD',
      '-e',
      'KELIR_LOAD_WINDOW_MS',
      '-e',
      'KELIR_LOAD_THINK',
      '-e',
      'KELIR_LOAD_TABS',
      '-e',
      'KELIR_LOAD_BASE_URL=http://caddy:80',
      '-e',
      `KELIR_LOAD_START_MS=${Math.round(startMs + offset)}`,
      '-e',
      'KELIR_LOAD_DETAIL_IDS=/stack/detail-ids.json',
      K6_IMAGE,
      'run',
      '--quiet',
      '--no-usage-report',
      '--console-output=/out/k6-console.log',
      '--summary-export=/out/k6-summary.json',
      '/scenarios/load.js',
    ],
    { env: environment, stdio: ['ignore', 'pipe', 'pipe'] },
  )
  const k6Log = createWriteStream(join(directory, 'k6-output.log'))

  k6.stdout.pipe(k6Log)
  k6.stderr.pipe(k6Log)

  const chromium = spawn(
    process.execPath,
    [join(HERE, 'node_modules/@playwright/test/cli.js'), 'test', '-c', 'view/playwright.config.ts'],
    { cwd: HERE, env: environment, stdio: ['ignore', 'pipe', 'pipe'] },
  )
  const chromiumLog = createWriteStream(join(directory, 'chromium-output.log'))

  chromium.stdout.pipe(chromiumLog)
  chromium.stderr.pipe(chromiumLog)

  const results = await Promise.all([finished(k6, 'k6'), finished(chromium, 'chromium')])

  clearTimeout(midway)
  await stopWatchingCaddy()
  backgroundLoad(join(directory, 'background-after.tsv'))
  sampler.stdin.end()
  await finished(sampler, 'pool sampler')

  // Last, once both generators, Caddy's count and the sampler have all stopped.
  const control = await provokeControl()

  await stopWatchingBackend()

  writeFileSync(
    join(directory, 'run.json'),
    JSON.stringify(
      {
        mode,
        startMs,
        windowMs: WINDOW_MS,
        clockOffsetMs: offset,
        imageTag: config.imageTag,
        commit: state.imageTag.replace(/^load-/, ''),
        harnessCommit: git('rev-parse', 'HEAD'),
        harnessDirty: git('status', '--porcelain', '--', 'load') !== '',
        database: config.database,
        project: config.project,
        exitCodes: results,
        control,
      },
      null,
      2,
    ),
  )

  return report(directory)
}

function report(directory) {
  const summary = summarize(directory)

  console.log(summary.text)
  appendFileSync(join(RUNS, 'measurements.log'), `${directory}\t${summary.void ? 'VOID' : 'valid'}\n`)

  if (summary.void) {
    process.exitCode = 1
  }
}

function smoke() {
  const password = process.env.KELIR_LOAD_PASSWORD || state?.password
  const url = process.env.KELIR_LOAD_BASE_URL || 'http://caddy:80'

  if (!password) {
    throw new Error('set KELIR_LOAD_PASSWORD, or run `node run.mjs up` first')
  }

  const result = spawnSync(
    'docker',
    [
      'run',
      '--rm',
      '--network',
      `${config.project}_default`,
      '-v',
      `${join(HERE, 'scenarios')}:/scenarios:ro`,
      '-e',
      'KELIR_LOAD_PASSWORD',
      '-e',
      `KELIR_LOAD_BASE_URL=${url}`,
      '-e',
      `KELIR_LOAD_USER_PREFIX=${process.env.KELIR_LOAD_USER_PREFIX || 'load-user-'}`,
      K6_IMAGE,
      'run',
      '--quiet',
      '--no-usage-report',
      '/scenarios/smoke.js',
    ],
    { env: { ...process.env, KELIR_LOAD_PASSWORD: password }, stdio: 'inherit' },
  )

  console.log(`smoke: k6 exited ${result.status}`)
  process.exitCode = result.status === 0 ? 0 : 1
}

function down() {
  requireStack()
  say(`Removing ${config.project}, its volumes and its network`)
  run('docker', [...composeArgs(), 'down', '-v', '--remove-orphans'], {
    stdio: ['ignore', 'inherit', 'inherit'],
  })

  // Compose makes this bind-mount source when it is missing. It is empty, and
  // no part of the release: leave `deploy/` as it was found.
  try {
    rmdirSync(join(ROOT, 'deploy/staging/db-tls'))
  } catch {
    // Already absent, or an operator's: either way it is not this run's.
  }

  rmSync(STATE)
  rmSync(join(state.runDirectory, 'stack.env'), { force: true })
}

async function main() {
  const [command, argument] = process.argv.slice(2)
  const ports = [config.httpPort, config.httpsPort]

  if (process.env.KELIR_LOAD_BASE_URL) {
    ports.push(portOf(process.env.KELIR_LOAD_BASE_URL))
  }

  const reasons = refusals({ project: config.project, ports, database: config.database })

  if (reasons.length > 0) {
    console.error(`refused:\n  ${reasons.join('\n  ')}`)
    process.exit(2)
  }

  switch (command) {
    case 'up':
      return up()
    case 'check':
      requireStack()
      console.log(`${checkDataset(psql)} counts match #777 AC3.`)
      return undefined
    case 'smoke':
      return smoke()
    case 'measure':
      return measure(argument)
    case 'report':
      return report(resolve(argument))
    case 'down':
      return down()
    default:
      throw new Error('usage: node run.mjs up | check | smoke | measure <mode> | report <dir> | down')
  }
}

main().catch((error) => {
  console.error(error.message)
  process.exit(1)
})
