# The load test

Measures **NFR-PERF-001** on the release stack: a list view within 1 second and a detail view within 2 seconds at P95, with 10,000 documents and 50 concurrent users ([#777](https://github.com/sujanto-gaws/kelir/issues/777), construction plan 21 row 1). The numbers it produced are in the [System Design Document](../docs/design/01.%20System%20Design%20Document.md) §13.4.

**It measures and does not fix.** It changes no product code, deploy file or migration. A breached target is filed as its own issue, with the numbers.

**It is not a required check.** No CI step runs it. It runs by hand, on a named machine, because the targets are read on the laptop and not on CI's runner, which is about twice as fast (plan 20's thirteenth note).

## What it needs

| Need | Version | Why |
|---|---|---|
| Node | 24.7 or later | The runner, and `crypto.argon2Sync` for the seeded password |
| Docker with Compose | Any current | The stack, and k6 |
| `grafana/k6` | `1.3.0`, by digest, in `run.mjs` | The scenarios. Pulled on first use; never part of a product image |
| Chromium | The build Playwright 1.62.1 names | The view measurement, on the host |

```bash
cd load
npm ci
npx playwright install chromium
```

`load/` is its own workspace. Its Playwright is pinned to the version `e2e/package-lock.json` resolves, and nothing links into `e2e/` or `kelir-frontend/`.

**Run it on an otherwise idle machine.** Stop your own builds first. The runner names what else was running (`background-before.tsv`, `background-mid.tsv`, `background-after.tsv`), and does not stop anything.

## Running it

```bash
node run.mjs up                   # builds the images if missing, starts, seeds, counts
node run.mjs smoke                # two users, one list, one detail: about 5 seconds
node run.mjs measure target       # AC4's run: about 6 minutes
node run.mjs measure saturation   # the same with zero think time: not compared with the target
node run.mjs measure tabs         # the target's window, with AC2's tab calls added
node run.mjs down                 # removes the stack, its volumes and network
```

`up` tags the images `load-<short sha>`. Set `KELIR_LOAD_IMAGE_TAG=load-<sha>` to measure images already on the machine, and `KELIR_LOAD_PROJECT`, `KELIR_LOAD_PORT` and `KELIR_LOAD_HTTPS_PORT` to move it (`kelir-load`, 18097 and 18447 by default).

`check` counts the data set again. `report <directory>` reads a measurement's raw files again without running anything. `node --test` runs the harness's own tests, which need no stack.

Every run writes to `load/.run/<stack>/<mode>-<time>/`, which git ignores. It holds the raw samples, so nothing there is committed.

### What it refuses

Every command checks these first, and exits 2 without starting anything:

- a compose project named `kelir-staging`
- port 8090
- a database named `kelir` or `kelir_dev`

`measure` also exits 1 before starting if the data set differs from AC3 by a single count (`seed/seed.mjs`, `EXPECTED`). A second seed of one database stops on its first statement.

### The smoke mode

`node run.mjs smoke` is #777's AC10, and what [#780](https://github.com/sujanto-gaws/kelir/issues/780) runs. It signs in two users, opens one list and one detail each, and **exits non-zero on any non-2xx**. It takes the document from the list, so it needs no id file. It runs on the compose project's network against `http://caddy:80`.

Against another stack, name the project, and the address if it is not `caddy:80`:

```bash
KELIR_LOAD_PROJECT=kelir-two-backends \
KELIR_LOAD_PASSWORD=... \
KELIR_LOAD_USER_PREFIX=load-user- \
  node run.mjs smoke
```

## Two backends

`node run.mjs two <step>` is [#780](https://github.com/sujanto-gaws/kelir/issues/780): NFR-SCA-001 to 003 on a stack, two backends behind the shipped Caddyfile. It uses the same refusals. The project is `kelir-two-backends`, on 18098 and 18448, and `compose.two-backends.yml` is layered last, over the release file, `compose.overlay.yml` and `docker-compose.e2e.yml`. `load/two-backends.mjs` holds the steps. Unlike `up`, it starts on an empty database with the default tenant `SYSTEM`, so the two backends migrate and bootstrap together.

```bash
node run.mjs two up          # both backends on an empty database
node run.mjs two race 12     # 12 cold starts of the pair, each on a new database, then 2 warm
node run.mjs two e2e         # the browser suite: needs `npm ci` in e2e/
node run.mjs smoke           # #777's scenario, unchanged: after e2e or cross, it needs a document
node run.mjs two traffic     # each instance's count, its control, and Caddy's split
node run.mjs two count       # each instance's count so far
node run.mjs two cross       # AC3's checks, the rate limit, the lockout and the mail count
node run.mjs two spread      # smoke through a Caddy derived to spread, then Caddy put back
node run.mjs two spread control  # the same on the shipped file, to compare: see below
node run.mjs two errors      # a real ERROR on each instance: the control on every zero
node run.mjs down
```

**`race` drops the database each round**, so it comes before anything worth keeping.

- **An instance is addressed by running curl inside it**, so which one served a request is a fact. `X-Forwarded-For` is sent as Caddy would send it, with a source per check, so no check spends another's rate limit.
- **Each instance's own count** is the statement the extractor asks on every authenticated request (ADR-0045), counted in that instance's log. That needs `sqlx::query=trace`, the one setting a deployment would not have. `traffic`'s control sends 23 requests to one and 7 to the other and reads them back.
- **`cross` adds a table and two triggers to the throwaway database while it runs**, which record each status write with the connection's address, so a write names its instance. It removes them after.
- **Every "once" has a control that reads 2.** A file or event put back to `PENDING` is settled again and has to count twice.
- **clamd and mailpit are paused for 12 seconds**, in this project, to hold both workers on the same row.
- **`spread` is not the shipped proxy.** The shipped Caddyfile does not balance ([#854](https://github.com/sujanto-gaws/kelir/issues/854)): each new upstream connection lands where the name resolves, and the split then sticks to Caddy's pool, so a pool that landed on one instance stays there. Smoke through it reaches both by chance. `spread` derives a Caddyfile from the shipped one at each run, with only `reverse_proxy backend:8080` changed, under `.run/`, records the diff, recreates Caddy alone, and exits non-zero unless smoke moved both instances' own counts. It then puts the shipped file back and reads serial traffic on one instance again.
- **`two spread control` usually exits 0, and that is not a failed control.** It is the same step on the shipped file. Both moving is necessary and not sufficient; what round robin adds is an even split. `KELIR_LOAD_BASE_URL=http://kelir-two-backends-backend-2:8080 node run.mjs two spread` forces the red exit.
- **`errors` makes a user of its own with a stored hash that is not a PHC string.** A sign-in logs ERROR at `password.rs:34` and answers 401. It leaves one ERROR per sign-in in each log, so run it after anything that reads "0 ERROR".

It writes `stack-verified.json`, `race.json`, `traffic-*.json`, `cross-*.json`, `spread-*.json`, `Caddyfile.derived-*.diff`, `errors-*.json` and `e2e-*.log` to the run's directory. The two backends share one kernel, so their clocks cannot differ here, and `token` says nothing about skew.

## How to read the output

`measure` prints `summary.json` and exits 1 when the measurement is void.

- **`void` and `voidReasons`.** A void run is not a number. It is void on any 401, 403, 429 or 5xx in the window from either generator; fewer than 50 users, or a Chromium user, signing in during the ramp; fewer than 1,000 scenario samples or anything but 30 Chromium samples of a view; two Chromium samples under 5 seconds apart, or the Chromium session leaving the window; a run of 12 minutes or more; either generator exiting non-zero; or **an empty `backend.log` or a failed control**, below.
- **`verdicts`.** A view meets its target when its **scenario P95 and its Chromium P95 are both at or under it**. Only `target` gives one. `saturation` and `tabs` say so and give none.
- **`scenario`.** k6's reading of each view. `n`, `p50`, `p95` and `max` are milliseconds, and `meanBytes` is uncompressed response bytes.
- **`chromium`.** The Chromium session's reading. `warm` leaves out the view's cold sample, `cold` is the first open of each view in a fresh context, and `apiBytesPerOpen` is what crossed the wire, headers included. `settled` is from the first request sent to the last response finished, with no render, so it can read below the view's time. `warmAssets` is what opening the view again fetched.
- **`perRequest`.** k6's own figures per call, for finding which call a slow view spent its time on. These percentiles are interpolated. They are for diagnosis and are never the reading.
- **`logs`.** The counts of `ERROR` and `WARN` lines, pool timeouts, slow acquires and slow statements in the backend's log, from 5 seconds before the first sign-in until the control starts, and of 5xx in Caddy's. The backend logs no line per request, so Caddy's count is the 5xx count. It is taken live, because Docker Desktop's default log driver keeps about 100 MB.
- **`logs.backendLog`.** What those counts rest on: `capturedLines`, `streams`, and the lines `before`, `measured` and in the `control`. **Read the control first.** Zero means nothing on its own, so the counts are readings only when `control.passed` is true.
- **`pool`.** PostgreSQL's view of the 10 connections, five times a second: how many were active, how often all ten were, and what share of the active ones were running the token check. **ADR-0045 §6's trigger is read here.** sqlx exposes no queue, so all ten active is the sign of queueing, and `poolTimedOut` is the proof of it. `overTen` and `localSessions` name any session over the pool's 10, or any that did not arrive over the network, across the whole run.

### The log's control

`run.mjs` follows the backend's container for the whole run, both streams, into `backend.log`, with each line tagged with its stream and stamped by Docker. A run is void unless that file holds:

- **the startup line**, `listening`, which every backend writes
- **each line the control provokes** after both generators, Caddy's count and the pool sampler have stopped: a slow statement, a slow acquire, a pool timeout, a `WARN` and an `ERROR`

To provoke them it holds `LOCK TABLE documents` on the throwaway database for 7.5 seconds and sends 36 list requests into it, 30 and then 6. Those requests answer 200 or 500 by design, and none of them is in Caddy's count or the pool's. Nothing is written. The backend is left as it was, with its pool refilling once the lock lifts.

| Line | Emitted by | Level | Fires at |
|---|---|---|---|
| `slow statement: execution time exceeded alert threshold` | sqlx 0.8.6, target `sqlx::query` | `WARN` | 1 s, sqlx's default, which `db.rs` leaves alone |
| `acquired connection, but time to acquire exceeded slow threshold` | sqlx 0.8.6, target `sqlx::pool::acquire` | `WARN` | 2 s, sqlx's default |
| `pool timed out while waiting for an open connection` | `error.rs`, `request failed with an internal error` | `ERROR` | 5 s, `acquire_timeout` in `db.rs` |

`KELIR_LOG=info,sqlx=warn` lets the first two through. **Their zeros are bounded by those thresholds**: no slow statement means none reached 1 second, and says nothing about one that took 900 ms.

The log carries ANSI colour, and `ERROR` is written with no space before it, so `report.mjs` strips the colour and reads a level from its field rather than searching the line for the word.

**P95 is nearest-rank**: the value at position ceil(0.95 n) in the sorted samples, the 29th of 30. `report.mjs` computes it from the raw samples.

## What is measured

**A list view** is `GET /documents?page=1&pageSize=20` and `GET /notifications/unread-count`, sent together. It is done in Chromium when `documents-table` shows a row.

**A detail view** is `GET /documents/{id}` with the unread count, then `GET /rad/forms/{formId}`, then `GET /documents/{id}/status-history`. It is done when `document-form` shows a control.

**The tabs are not part of a detail view, and #777's AC2 says they are.** `DocumentWorkspace.vue` mounts each tab with a `v-if` on the open tab, so a tab's calls go only when a person opens it. Chromium confirms it: a detail open sends four API calls. The scenario follows the page. `measure tabs` sends the seven calls AC2 lists, after the content set, for the "settled" figure AC2 asked for.

**The loop** is AC4's: open the list, open one of 500 documents at random, think for 1 to 3 seconds, with a `?status=IN_REVIEW` list recorded apart on one loop in five.

**The Chromium session** is the 51st user. It makes 60 opens, five seconds apart, interleaved. A sample is a route change in the loaded shell, by the `popstate` vue-router listens for, and the page times itself.

## How this stack differs from a deployment

The numbers carry these, and any record of them should too.

- **The database** is a container in the compose project, not PostgreSQL native on the host. It is one Docker bridge hop away in both cases, and shares the machine's cores in both. It runs on the image's default settings. It has no published port.
- **Plain HTTP on HTTP/1.1.** Caddy serves `:80`, because a certificate cannot be issued for an address, so no TLS handshake is paid and Chromium's six-connection limit applies.
- **k6 reaches Caddy on the compose network**, so the scenario's reading has no Docker Desktop port forward in it. Chromium's does, as it reaches the published port from Windows.
- **The attachments are rows, with no objects behind them.** No view downloads one.
- **Everything shares one machine**: the stack, the database, k6 and Chromium. A deployment does not have the last two.

## What each file is

| File | What it is |
|---|---|
| `run.mjs` | The runner: the stack, the machine capture, the measurement, the logs |
| `guard.mjs` | The refusals |
| `report.mjs` | Reads the raw samples: nearest-rank P95, voids, verdicts, log and pool counts |
| `harness.test.mjs` | Tests of the arithmetic, the refusals and the log's control: `node --test` |
| `compose.overlay.yml` | What a run adds to `docker-compose.staging.yml`, and why |
| `compose.two-backends.yml` | What #780 adds over that, and why |
| `two-backends.mjs` | #780's steps: the cold starts, the counts, the cross-instance checks |
| `two-backends.test.mjs` | Tests of what those steps read from a log: `node --test` |
| `seed/seed.sql` | The data set (AC3, AC4) |
| `seed/seed.mjs` | Runs it, and holds the counts it must equal |
| `scenarios/views.js` | What a list and a detail send, shared by the two scenarios |
| `scenarios/load.js` | The 50 users' closed loop |
| `scenarios/smoke.js` | The smoke mode |
| `view/views.spec.ts` | The Chromium session |
