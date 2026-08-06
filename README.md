# Kelir

Kelir is a document-based business application framework with workflow-driven processing and rapid application development (RAD) capabilities.

This repository is initialized from the SRS and solution blueprint in docs/requirements/srs.md (v0.1, dated 2026-08-05).

## Vision

Every business transaction is treated as a document, and every document progresses through a controlled workflow. The framework is designed to support business domains such as approvals, procurement, onboarding, compliance, and master data governance.

## MVP Scope (Phase 1-5)

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
│   └── requirements/
│       └── srs.md
├── kelir-backend/
│   ├── Cargo.toml
│   ├── migrations/
│   │   ├── 0001_core.sql
│   │   ├── 0002_identity.sql
│   │   ├── 0003_master_data.sql
│   │   ├── 0004_rad.sql
│   │   ├── 0005_document.sql
│   │   ├── 0006_workflow.sql
│   │   ├── 0007_attachment.sql
│   │   ├── 0008_comment.sql
│   │   ├── 0009_activity_audit.sql
│   │   ├── 0010_notification.sql
│   │   ├── 0011_integration.sql
│   │   └── 0012_plugin.sql
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

## Local Development

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

Initialized project skeleton for Phase 1 (foundation):

- Repository structure aligned with SRS blueprint
- Backend module and migration placeholders
- Frontend feature-based folder scaffolding
- Docker Compose services for local development baseline

## Next Implementation Steps

1. Wire backend Axum router and health endpoint.
2. Add SQLx database connection and migration execution.
3. Add frontend app shell, auth page, and route guards.
4. Implement authentication module (login/logout/me).
5. Implement identity and role management APIs.
6. Start master data CRUD (supplier, customer, employee, facility).
