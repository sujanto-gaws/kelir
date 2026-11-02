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

## How to read the output

`measure` prints `summary.json` and exits 1 when the measurement is void.

- **`void` and `voidReasons`.** A void run is not a number. It is void on any 401, 403, 429 or 5xx in the window from either generator; fewer than 50 users, or a Chromium user, signing in during the ramp; fewer than 1,000 scenario samples or anything but 30 Chromium samples of a view; a run of 12 minutes or more; or either generator exiting non-zero.
- **`verdicts`.** A view meets its target when its **scenario P95 and its Chromium P95 are both at or under it**. Only `target` gives one. `saturation` and `tabs` say so and give none.
- **`scenario`.** k6's reading of each view. `n`, `p50`, `p95` and `max` are milliseconds, and `meanBytes` is uncompressed response bytes.
- **`chromium`.** The Chromium session's reading. `warm` leaves out the two cold samples, `cold` is the first open of each view in a fresh context, and `apiBytesPerOpen` is what crossed the wire, headers included. `warmAssets` is what opening the view again fetched.
- **`perRequest`.** k6's own figures per call, for finding which call a slow view spent its time on. These percentiles are interpolated. They are for diagnosis and are never the reading.
- **`logs`.** The counts of `ERROR` and `WARN` lines, `pool timed out` and slow acquires in the backend's log, and of 5xx in Caddy's. The backend logs no line per request, so Caddy's count is the 5xx count. It is taken live, because Docker Desktop's default log driver keeps about 100 MB.
- **`pool`.** PostgreSQL's view of the 10 connections, five times a second: how many were active, how often all ten were, and what share of the active ones were running the token check. **ADR-0045 §6's trigger is read here.** sqlx exposes no queue, so all ten active is the sign of queueing, and `poolTimedOut` is the proof of it.

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
| `harness.test.mjs` | Tests of the arithmetic and the refusals: `node --test` |
| `compose.overlay.yml` | What a run adds to `docker-compose.staging.yml`, and why |
| `seed/seed.sql` | The data set (AC3, AC4) |
| `seed/seed.mjs` | Runs it, and holds the counts it must equal |
| `scenarios/views.js` | What a list and a detail send, shared by the two scenarios |
| `scenarios/load.js` | The 50 users' closed loop |
| `scenarios/smoke.js` | The smoke mode |
| `view/views.spec.ts` | The Chromium session |
