# Changelog

All notable changes to Kelir are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html) as applied by the
[release process](docs/standards/04.%20Release%20Process.md).

While the major version is `0`, the public API may change in any release.

## [Unreleased]

Nothing yet.

## [0.1.0] — 2026-08-12

First tagged release: the Phase 1 skeleton — a backend that serves and migrates,
a frontend shell that talks to it, and the means to deploy both.

**This release has not been verified on staging.** The environment is defined and
tested locally but not yet provisioned, so release checklist item 7 is
outstanding. Treat `0.1.0` as cut, not proven.

### Added

- **Backend API.** Axum service under `/api/v1` with the standard response
  envelope (`{success, data}`, `{success, data, meta}`, `{success: false, error}`)
  and a central `AppError` mapping failures to stable machine-readable codes.
  Pagination is available to every list endpoint, clamped so a caller cannot
  request an unbounded scan.
- **Operational endpoints.** `/health`, `/health/live`, `/health/ready` and
  `/version`. Liveness never touches a dependency; readiness reports `503` when
  the database is unreachable, so a load balancer drains the instance instead of
  the orchestrator killing it.
- **Database.** SQLx pool and migration runner; migrations apply at startup.
  `0001_core.sql` creates `tenants` and `system_settings` and seeds the reserved
  system tenant.
- **Configuration.** `KELIR_*` environment loading with typed environments.
  `KELIR_JWT_SECRET` has no default, and staging and production refuse the
  development placeholder.
- **OpenAPI.** Generated document at `/api/docs/openapi.json`, never hand-edited.
- **Frontend shell.** Vue 3 application with navigation, a dark theme, lazy
  routes and a Pinia store; a login page (presentation only until Phase 2); and
  the Tailwind CSS v4 plus shadcn-vue baseline.
- **Typed API client.** Unwraps the response envelope so callers receive `data`,
  and normalises every failure — HTTP, network, timeout, malformed body — into an
  `ApiError` carrying the backend's code and JFSS validation details.
- **Deployment.** Multi-stage release images for both stacks; a staging stack
  behind Caddy serving one origin with automatic TLS; `provision-ubuntu-24.sh`
  for a fresh Ubuntu 24.04 host, including PostgreSQL, firewall and daily
  backups; `deploy.sh` and `deploy-local.sh`, the latter deploying to an IP for
  testing before a tag exists.
- **CI.** Formatting, clippy, tests and builds for both stacks, plus commit
  message validation on pull requests.
- **Documentation.** The full set — requirements, architecture, design, database
  schema, the JSON standards family, engineering standards, and an installation
  and deployment guide.

### Changed

- Renamed every `BHUVARLOKA_*` environment variable to `KELIR_*`.
- Compose host ports are configurable through `KELIR_*_PORT`, so the stack can
  run alongside other projects.

### Fixed

- The backend served nothing: `main.rs` printed one line and exited, so the
  compose stack came up without an API.
- Compose wrote into the host's `node_modules` and `target/`, leaving the working
  tree dirty after every run.
- The frontend could not reach the backend at all — no CORS layer existed, so the
  browser refused every response.

### Removed

- Eleven placeholder migrations (`0002`–`0012`). SQLx records a checksum per
  applied migration, so an empty migration applied now would have refused to run
  once its real DDL was written. Each phase adds its migration when it writes it.

### Known limitations

- No authentication. The login page does not sign in; identity arrives in Phase 2.
- No business endpoints. `/api/v1` is mounted and empty.
- No production environment, image registry, or rehearsed database restore.

[Unreleased]: https://github.com/sujanto-gaws/kelir/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/sujanto-gaws/kelir/releases/tag/v0.1.0
