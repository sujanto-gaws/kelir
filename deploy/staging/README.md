# Release stack

This directory holds the release deployment: the compose file, the single-origin
Caddy configuration, the host provisioning script and the deploy scripts.

**Five of the six deployment files run today, and the three #778 added run in a container (below). One has never run, and one of the five has never run the way it was written for: both describe a host that does not exist.** (Corrected 2026-10-01: this said two of five, and counted `deploy.sh` as unused although `deploy-local.sh` runs it.)

| File | State |
|---|---|
| `docker-compose.staging.yml` | **In use.** The release stack — Caddy, backend, MinIO, Mailpit — running the immutable `kelir-backend` / `kelir-frontend` images selected by `KELIR_VERSION` |
| `docker-compose.e2e.yml` | **In use, by the browser flows only.** An overlay that `deploy.sh` layers over the release stack when `KELIR_COMPOSE_OVERLAY` names it: a stand-in external system on a private network, its one address in `KELIR_INTEGRATION_ALLOWED_CIDRS`, and the system tenant's secret ([#593](https://github.com/sujanto-gaws/kelir/issues/593), [`e2e/README.md`](../../e2e/README.md)). No deployment sets the variable |
| `deploy-local.sh` | **In use.** Brings that stack up locally from release images and runs the smoke test. This is [release process](../../docs/standards/04.%20Release%20Process.md) §4 step 7, and the environment the Definition of Done names |
| `Caddyfile` | In use by the compose file; TLS applies only to a hostname deployment. It sets the bundle's `Cache-Control` ([below](#what-the-browser-may-keep)) |
| `deploy.sh` | **In use through `deploy-local.sh`**, which sets the address and hands over to it: the secret checks, the image build, `compose up`, the overlay when `KELIR_COMPOSE_OVERLAY` names one, the service check and the smoke test are all this script. **Unused as a command of its own**, the per-release deploy to `kelir-staging-01` |
| `provision-ubuntu-24.sh` | **Unused.** One-time setup of `kelir-staging-01` |
| `kelir-backup.sh`, `kelir-restore-objects.sh` | **Unused on a host, run in a container** ([#778](https://github.com/sujanto-gaws/kelir/issues/778)). The provisioner installs them as `/usr/local/bin/kelir-backup` and `kelir-restore-objects`: the encrypted backup pair and the store half's restore. Their settings let them run against any stack, which is how they were rehearsed |
| `encryption-at-rest.probe.sh` | **A probe, run by hand in a Linux container** (#778, ADR-0049 §5): the store's files and both archives read raw, the backup's refusals, and `deploy.sh`'s |

## What the browser may keep

**A hashed file is kept for a year, and everything else the single-page app
serves is revalidated on every use** ([#771](https://github.com/sujanto-gaws/kelir/issues/771)).
Until #771 the Caddyfile sent no `Cache-Control` at all, so a browser guessed
each file's freshness from its `Last-Modified`, and #721's measurement watched
Chromium refetch the lazy *Graph* chunk once the guess ran out.

| Request | `Cache-Control` | Why |
|---|---|---|
| A file under `/assets/` that exists | `public, max-age=31536000, immutable` | Vite names it by a hash of its content, so its URL never answers anything else. A new build makes new names |
| `/`, `/index.html` and every client route, such as `/login` | `no-cache` | The document is the one file that names the hashes, so it has to be fresh after a deploy. `no-cache` keeps it and asks before each use; its ETag makes that a `304` |
| A path under `/assets/` that does not exist | `no-cache` | `try_files` answers it with `index.html`. Kept a year under a removed chunk's URL, that answer would be the stale page the header exists to prevent, so the immutable rule asks Caddy's `file` matcher whether the file is on disk |
| `/version.json` | `no-cache` | Unhashed, and different in every build; `deploy.sh` reads it to tell which image is serving |
| `/api/*`, `/health`, `/health/*`, `/version` | none from Caddy | Both headers are set inside the single-page app's `handle` only, so the backend's answers pass through as the backend sends them. It sends none today |

**`deploy.sh`'s smoke test reads three of these**: `/`, the first `/assets/`
file `/` names, and a made-up path under `/assets/`, which must not answer
`immutable`. That third check is the one a simpler rule fails: matched on
`path /assets/*` alone, the immutable header lands on the fallback too.

**It has no version floor**, unlike its `/version.json` check. The rules are in
this directory's `Caddyfile`, which the compose file mounts over the Caddy base
image's default one; the frontend image adds only the bundle. So they arrive
with the script: an image built before #771, a rollback target included, is
served with them.

## Why the unused ones are still here

`kelir-staging-01` has never existed, and no infrastructure is available to
provision it. It was the outstanding item from Sprint 2 to Sprint 5, blocked the
`v0.1.0` and `v0.2.0` release records, and the Definition of Done clause naming
it was waived four sprints running — twice with an expiry that was extended
rather than met.

Decision **D-9** (2026-08-21, [Product Backlog](../../projects/planning/02.%20Product%20Backlog.md) §6.1)
retired the environment rather than the gate: the Definition of Done and the
release checklist now name the compose stack above, which exists and which
`deploy-local.sh` drives. Issue #12 is closed as not planned.

Staging is **unscheduled, not abandoned.** These two scripts are finished and
reviewed, so if a host ever appears the work is a deploy rather than a redesign —
which is the whole reason keeping them costs nothing. Until then, read them as a
design. ~~They have never run.~~ `provision-ubuntu-24.sh` has never run, and
`deploy.sh` has never run against a host; it runs whenever `deploy-local.sh`
does, CI's browser job included.

**Taking the stack down when the overlay was layered takes both files**:
`docker compose -f docker-compose.staging.yml -f docker-compose.e2e.yml down`,
or `docker compose -p <project> down`. With the release file alone, the
stand-in and its network are left behind ([`e2e/README.md`](../../e2e/README.md)).

What would be needed: a host, DNS for `staging.kelir.gawshub.com`, and the
secrets described in `.env.staging.example`. The procedure is
[release process](../../docs/standards/04.%20Release%20Process.md) §4.1.
