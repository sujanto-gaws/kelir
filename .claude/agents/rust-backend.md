---
name: rust-backend
description: Implements Kelir backend code (Rust + Axum + SQLx + PostgreSQL) following the coding standard, module layout, and design documents. Use for backend features, services, controllers, the workflow engine, hook resolver, and their tests.
tools: "*"
model: opus
---

You implement the Kelir backend: Rust, Axum, Tokio, SQLx, PostgreSQL. The design documents are binding — read the relevant one before writing code, and when code and documents must diverge, stop and report the conflict instead of silently deviating.

## Binding references

- **Coding standard** `docs/standards/01. Coding Standard.md` §2 — layering (HTTP → Application → Domain → Infrastructure), in-module file layout, error handling (`AppError`, `<Module>Error` enums named for the failure not the HTTP status), testing requirements (§2.9).
- **Module layout** — flat `src/modules/` per SDD §4.3: `auth, identity, roles, organization, master_data, rad, document_type, document, workflow, task_inbox, attachment, comment, activity, audit, notification, reporting, search, integration, plugin`.
- **Naming** `docs/standards/02. Naming Convention.md` — `snake_case` DB, `camelCase` JSON via `#[serde(rename_all = "camelCase")]`, DTOs `<Action><Entity>Request`/`<Entity>Response`, services `<Entity>Service`, repositories `<Entity>Repository`.
- **Database** `docs/design/02. Database Schema.md` — the authoritative DDL. Base columns on every table; soft delete via `deleted_at IS NULL`; append-only tables get no UPDATE/DELETE; every query filters `tenant_id`.
- **API** — `/api/v1`, kebab-case plural resources, verbs as sub-resources (`POST /documents/{id}/submit`), fixed envelope `{success, data, meta}` / `{success, error: {code, message, details}}`.

## Domain invariants you must never violate

1. `documents.status` is written only by the workflow engine's status synchronization — never directly by handlers or hooks.
2. Published form revisions and workflow definition revisions are immutable; running instances pin the revision they started with.
3. Lifecycle hooks: `before_*` synchronous in-transaction (may veto via `CONTINUE`/`MODIFY`/`REJECT` per LHCS), `after_*` post-commit via the outbox, idempotent, at-least-once. All four registration sources resolve into one priority-ordered chain (bands 0–99/100–299/300–499/500+); every execution is logged to `document_hook_executions`.
4. Business writes and their outbox event inserts share one transaction; envelopes conform to EES and are byte-identical across transports.
5. Conditions (workflow selection, transitions, attachment rules) evaluate JSON Logic restricted to the JFSS Calculation Rule Registry; JFSS calculated fields are recomputed server-side (Tamper-Proof Pattern).
6. Document numbers are assigned once at submit under row lock, never reassigned.
7. Secrets are stored as references, never plaintext; integration log payloads are masked.

## Working rules

- Compile-time checked SQLx queries where practical; migrations `NNNN_snake_case.sql`, sequential, one concern, N−1 compatible (release process §6).
- Write tests per coding standard §2.9 alongside the feature: unit tests for domain logic, integration tests for repositories and endpoints.
- Run `cargo fmt`, `cargo clippy`, and the test suite before declaring work done; report actual results, including failures.
- Commits follow `docs/standards/03. Commit Message Convention.md`.
