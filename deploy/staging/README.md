# Release stack

This directory holds the release deployment: the compose file, the single-origin
Caddy configuration, the host provisioning script and the deploy scripts.

**Two of the five files run today. Three describe a host that does not exist.**

| File | State |
|---|---|
| `docker-compose.staging.yml` | **In use.** The release stack — Caddy, backend, MinIO, Mailpit — running the immutable `kelir-backend` / `kelir-frontend` images selected by `KELIR_VERSION` |
| `deploy-local.sh` | **In use.** Brings that stack up locally from release images and runs the smoke test. This is [release process](../../docs/standards/04.%20Release%20Process.md) §4 step 7, and the environment the Definition of Done names |
| `Caddyfile` | In use by the compose file; TLS applies only to a hostname deployment |
| `deploy.sh` | **Unused.** Per-release deploy to `kelir-staging-01` |
| `provision-ubuntu-24.sh` | **Unused.** One-time setup of `kelir-staging-01` |

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
design. They have never run.

What would be needed: a host, DNS for `staging.kelir.gawshub.com`, and the
secrets described in `.env.staging.example`. The procedure is
[release process](../../docs/standards/04.%20Release%20Process.md) §4.1.
