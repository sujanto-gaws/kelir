# Kelir

Kelir is a document-based business application framework with workflow-driven processing and rapid application development (RAD) capabilities.

This repository is driven by the requirements in [docs/requirements/srs.md](docs/requirements/srs.md) (SRS v0.5) and the design in [docs/design/01. System Design Document.md](docs/design/01.%20System%20Design%20Document.md) (SDD v0.1).

## Vision

Every business transaction is treated as a document, and every document progresses through a controlled workflow. The framework is designed to support business domains such as approvals, procurement, onboarding, compliance, and master data governance.

## Documentation

The `docs/` folder contains the full design documentation set (see [docs/README.md](docs/README.md) for the complete index, reading order, and authority rules):

- `docs/concepts/` — conceptual foundation: the document-based platform concept, attachments/comments/activity-log design, master data governance (hybrid golden-record + workflow model), and a consolidated sample data structure.
- `docs/architectures/` — architecture decisions: framework concept, the Kelir module breakdown (backend and frontend), the external-system integration layer, the plugin/extension platform, and the OFBiz-style Party master-data model (adopted for supplier, customer, and employee master data).
- `docs/requirements/srs.md` — the Software Requirements Specification (FR/NFR IDs, MVP acceptance criteria), v0.5.
- `docs/design/` — the System Design Document (architecture, stack, module structure, database design, workflow/RAD/integration/plugin/security/API/deployment design, roadmap), v0.1 — the successor of the Solution Blueprint formerly bundled in srs.md — and the Database Schema (column-level DDL for all 95 tables across 16 migrations).
- `docs/schema/` — the JSON standards family: the JSON Form Schema Standard (JFSS v2.0.1) with its normative meta-schema and rule registries (Calculation v1.2.0, Validation v1.1.0), plus the Workflow Schema (JWSS), Lifecycle Hook Contract (LHCS), Plugin Manifest Schema (PMS), Event Envelope Schema (EES), and Document Type Definition Schema (DTDS), each v1.0.0.
- `docs/standards/` — the coding standard (Rust backend, Vue 3 frontend, SQL, git), the naming convention (code, database, API, permissions, events, identifiers), the commit message convention (Conventional Commits), the release process (SemVer, changelog, deploy, rollback), and the git workflow (trunk-based branches, PRs, review, squash merges).

Project management lives in `projects/` (see [projects/README.md](projects/README.md)):

- `projects/planning/` — the sprint plan mapping the SRS roadmap phases onto a 2-week sprint cadence with per-phase releases, and the product backlog mapping every requirement to an epic, backlog item and sprint.

## MVP Scope (Phases 1-6)

The MVP focuses on:

- Authentication and session management
- User, role, and permission management
- Master data management
- Document type configuration
- Document creation/submission lifecycle
- Workflow task inbox and approvals (approve/reject)
- Attachments and comments baseline
- Activity and audit logging
- REST API foundation under /api/v1

## Technology Stack

- Backend: Rust, Axum, Tokio, SQLx, Tracing
- Frontend: Vue 3, Vite, TypeScript, Pinia, Vue Router, Axios
- Database: PostgreSQL (primary), MariaDB (optional compatibility later)
- Storage: Local for development, S3-compatible in production
- Deployment: Docker Compose (initial), Kubernetes-ready later

## Repository Structure

```text
.
├── docs/
│   ├── README.md
│   ├── concepts/
│   │   ├── 01. Concept.md
│   │   ├── 02. Handling Attachments Comments and Activity Log.md
│   │   ├── 03. Handling Master Data.md
│   │   └── 04. Sample Data Structure for System Overview.md
│   ├── architectures/
│   │   ├── 01. Basic Framework Concept and Architecture.md
│   │   ├── 02. Kelir Framework.md
│   │   ├── 03. Kelir Modules for Interfacing with External Systems.md
│   │   ├── 04. Kelir Plugin and Extension Management Concept.md
│   │   └── 05. Core - Master Data - Party.md
│   ├── requirements/
│   │   └── srs.md
│   ├── design/
│   │   ├── 01. System Design Document.md
│   │   └── 02. Database Schema.md
│   ├── schema/
│   │   ├── JSON Form Schema.md
│   │   ├── jfss-meta-v2.0.1.json
│   │   ├── JFSS Calculation Rule Registry.md
│   │   ├── JFSS Validation Rule Registry.md
│   │   ├── JSON Workflow Schema.md
│   │   ├── Lifecycle Hook Contract.md
│   │   ├── Plugin Manifest Schema.md
│   │   ├── Event Envelope Schema.md
│   │   └── Document Type Definition Schema.md
│   └── standards/
│       ├── 01. Coding Standard.md
│       ├── 02. Naming Convention.md
│       ├── 03. Commit Message Convention.md
│       ├── 04. Release Process.md
│       └── 05. Git Workflow.md
├── projects/
│   ├── README.md
│   ├── planning/
│   │   ├── 01. Sprint Plan.md
│   │   └── 02. Product Backlog.md
│   ├── status/
│   │   ├── 00. Status Report Template.md
│   │   └── 01. Sprint 0 Status.md
│   ├── retrospectives/
│   │   └── 00. Retrospective Template.md
│   └── releases/
│       └── 00. Release Checklist Template.md
├── kelir-backend/
│   ├── Cargo.toml
│   ├── migrations/
│   │   ├── 0001_core.sql
│   │   ├── 0002_identity.sql
│   │   ├── 0003_audit.sql
│   │   ├── 0004_string_lengths.sql
│   │   ├── 0005_delegation_tenant_permissions.sql
│   │   ├── 0006_password_reset_tokens.sql
│   │   ├── 0007_account_lockout.sql
│   │   ├── 0008_master_data.sql
│   │   ├── 0009_party_role_permissions.sql
│   │   ├── 0010_rad.sql
│   │   ├── 0011_document.sql
│   │   ├── 0012_workflow.sql
│   │   ├── 0013_attachment.sql
│   │   ├── 0014_comment.sql
│   │   ├── 0015_activity_audit.sql
│   │   ├── 0016_notification.sql
│   │   ├── 0017_integration.sql
│   │   └── 0018_plugin.sql
│   └── src/
│       ├── main.rs
│       ├── config.rs
│       ├── db.rs
│       ├── error.rs
│       ├── health.rs
│       ├── router.rs
│       ├── middleware/
│       ├── modules/
│       │   ├── auth/
│       │   ├── identity/
│       │   ├── roles/
│       │   ├── organization/
│       │   ├── master_data/
│       │   ├── rad/
│       │   ├── document_type/
│       │   ├── document/
│       │   ├── workflow/
│       │   ├── task_inbox/
│       │   ├── attachment/
│       │   ├── comment/
│       │   ├── activity/
│       │   ├── audit/
│       │   ├── notification/
│       │   ├── reporting/
│       │   ├── search/
│       │   ├── integration/
│       │   └── plugin/
│       └── utils/
├── kelir-frontend/
│   ├── package.json
│   ├── vite.config.ts
│   ├── tsconfig.json
│   ├── index.html
│   └── src/
│       ├── main.ts
│       ├── App.vue
│       ├── api/
│       ├── components/
│       ├── composables/
│       ├── features/
│       │   ├── auth/
│       │   ├── dashboard/
│       │   ├── documents/
│       │   ├── workflow/
│       │   ├── tasks/
│       │   ├── master-data/
│       │   ├── admin/
│       │   ├── notifications/
│       │   └── settings/
│       ├── layouts/
│       ├── pages/
│       ├── router/
│       ├── stores/
│       ├── styles/
│       ├── types/
│       └── lib/
└── deploy/
    ├── docker/
    │   └── docker-compose.yml
    └── env/
        └── .env.example
```

Migrations `0001`–`0006` exist; `0007` onward are planned. The authoritative
migration list is the mapping table at the top of
[docs/design/02. Database Schema.md](docs/design/02.%20Database%20Schema.md) —
numbers follow the order migrations are applied, so an early arrival shifts
every later file down.

## Local Development

Full instructions — including bringing the release stack up locally from the
release images — are in
[docs/operations/01. Installation and Deployment.md](docs/operations/01.%20Installation%20and%20Deployment.md).

### Option A: Docker Compose

Run from the repository root:

```bash
docker compose -f deploy/docker/docker-compose.yml up --build
```

Services provided:

- frontend: Vue development server
- backend: Rust API service
- postgres: primary relational database
- minio: object storage for attachments
- mailpit: local SMTP capture

Once up, `GET http://localhost:8080/health` returns `{"status":"ok"}`, and the generated OpenAPI document is at `http://localhost:8080/api/docs/openapi.json`.

### Port conflicts

Every host port is configurable, because the defaults collide with common local services. Override any of them in the environment or in a `.env` beside `docker-compose.yml`:

```bash
KELIR_POSTGRES_PORT=55433 KELIR_MINIO_PORT=9100 KELIR_MINIO_CONSOLE_PORT=9101 \
  docker compose -f deploy/docker/docker-compose.yml up
```

The full set is `KELIR_FRONTEND_PORT`, `KELIR_BACKEND_PORT`, `KELIR_POSTGRES_PORT`, `KELIR_MINIO_PORT`, `KELIR_MINIO_CONSOLE_PORT`, `KELIR_SMTP_PORT`, `KELIR_MAILPIT_UI_PORT` — see `deploy/env/.env.example`.

> **A natively installed PostgreSQL is the trap to watch for.** If one is running as a Windows service, it also listens on 5432. Both it and Docker can bind the port, and a host process connecting to `localhost:5432` may reach either — which surfaces as `password authentication failed` rather than a connection error. Publish the container on a free port and point `KELIR_DATABASE_URL` at that instead.

### Option B: Run Individually

Backend:

```bash
cd kelir-backend
cargo run
```

Frontend:

```bash
cd kelir-frontend
npm install
npm run dev
```

## Environment Variables

Use deploy/env/.env.example as the baseline for configuration values.

## Current Status

**Phase 1 is released as `v0.1.0` and closed**, as is Phase 2 at `v0.2.0`. Both were held open for a staging host that never existed; decision **D-9** (2026-08-21) retired that environment for want of infrastructure and replaced the release step with the Docker Compose stack brought up from the release images. See the release records for [v0.1.0](projects/releases/01.%20Release%20v0.1.0.md) and [v0.2.0](projects/releases/02.%20Release%20v0.2.0.md).

> The rest of this section is a Sprint 2 snapshot and understates what exists — Phase 2 shipped authentication, users, roles and permissions. It is due a pass of its own.

Done:

- **Backend foundation** (Sprint 1) — `KELIR_*` configuration, SQLx pool and migration runner, `0001_core.sql` (tenants, system settings, system tenant), `/health`, `/health/live`, `/health/ready`, `/version`, the standard response envelope and `AppError`, pagination, and a generated OpenAPI document at `/api/docs/openapi.json`
- **Frontend foundation** (Sprint 2) — app shell with navigation and a dark theme, a typed API client that unwraps the response envelope and normalises every failure into an `ApiError`, a login page, and the Tailwind v4 + shadcn-vue baseline
- **Working repository** (Sprint 0) — protected `main`, CI running fmt, clippy, tests and builds on both stacks plus commit-message validation, and 29 tracked issues across two phase milestones

- **Documentation set** — 26 documents: SRS v0.5, System Design Document v0.1, the column-level database schema (95 tables across 16 migrations), 8 JSON standards, 5 architecture documents, 5 engineering standards, 4 concept documents
- **Planning** — sprint plan mapping SDD §14 onto 21 sprints, and a product backlog assigning all 164 functional requirements to epics, items and sprints (149 scheduled, 15 explicitly unscheduled)
- **Scope decisions D-1…D-5 resolved** (2026-08-11) — MVP milestone moved to `v0.6.0` to satisfy SRS §9; RAD split so its metadata and form renderer land in Phase 4; priority separated from MVP scope in SRS v0.5; ClamAV chosen for attachment scanning; six proposed NFR targets baselined
- Documentation consistency audit (2026-08-05): unified naming (Kelir), permission format `module:resource:action`, dotted event names, `mdm_*` tables, aligned health endpoints
- Supplier, customer, and employee master data unified under the Party model (SRS v0.3): parties with roles, role-specific profiles, identifications, relationships, and contact mechanisms; facility, product, and service remain dedicated entities
- Repository structure aligned with the SRS and System Design Document; `.env.example` complete
- Docker Compose services defined for the local development baseline

Not done:

- **No staging environment**, and none scheduled — retired by decision **D-9**; releases are verified against the compose stack built from release images (#12 closed as not planned)
- **No master-data UI.** The party endpoints exist; the list and detail screens over them are Sprint 6 with FR-MDM-008
- **No party roles or role profiles.** A party can be created and read, but not yet given a SUPPLIER or CUSTOMER role — that is #81, in the same sprint
- **Most module trees under `src/modules/` are still stubs**, filled in phase by phase. `auth`, `identity`, `organization`, `audit` and `master_data` are real; the rest are not

## Next Implementation Steps

Work is tracked as [GitHub issues](https://github.com/sujanto-gaws/kelir/issues), grouped into phase milestones.

Phase 1 and Phase 2 are closed. Sprint 5 is in progress: the Phase 2 exit debt first, then the Party model opens Phase 3 ([sprint plan](projects/planning/01.%20Sprint%20Plan.md) §5).

Phase 2 (Sprints 3–4) follows: `0002_identity.sql`, Argon2 authentication, the JWT-or-session decision, permission middleware, and user/role management.
