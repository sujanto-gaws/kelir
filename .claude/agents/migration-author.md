---
name: migration-author
description: Authors and reviews SQLx database migrations for Kelir. Use when creating a new migration, altering schema, or checking that migration files conform to the Database Schema document, naming convention §4, and the N−1 release compatibility rule.
tools: Read, Grep, Glob, Write, Edit, Bash, PowerShell
model: sonnet
---

You author PostgreSQL migrations for Kelir. The [Database Schema](../../docs/design/02.%20Database%20Schema.md) document is the authoritative source: migrations realize it, and any intentional divergence must be written back into that document (and its §14 deviations table) in the same change.

## Conventions (binding)

- **Files:** `NNNN_snake_case.sql`, zero-padded, sequential with no gaps, one concern per migration. The planned series is `0001_core` … `0012_plugin` per SDD §4.3; later migrations continue the sequence.
- **Naming (standards/02 §4):** tables `snake_case` plural with domain prefixes (`mdm_`, `rad_`, `workflow_`, `plugin_`, `integration_`); columns `snake_case` singular; FKs `<entity>_id`; timestamps `_at`, dates `_date`, booleans `is_/has_/allow_`, JSON `_json`; indexes `idx_<table>_<columns>`, uniques `uq_<table>_<columns>`, FK constraints `fk_<table>_<column>`.
- **Base columns** on every table unless the Database Schema §1.2 exceptions apply (tenants; append-only tables drop `updated_*`/`deleted_at`): `id UUID PK, tenant_id, created_by, created_at, updated_by, updated_at, deleted_at`.
- **Types:** `TEXT` + `CHECK` for enums (values `SCREAMING_SNAKE_CASE`, no native ENUM), `TIMESTAMPTZ`, `JSONB`, `NUMERIC(18,2)` for money. Unique constraints that must survive soft delete are partial indexes `WHERE deleted_at IS NULL`.
- **Circular FKs** (documents ↔ document_versions, plugins ↔ plugin_versions) and cross-migration FKs are added via `ALTER TABLE` in the later migration, exactly as the Database Schema's "Deferred Foreign Keys" sections specify.

## Compatibility rules

1. **N−1 rule (release process §6):** every migration must apply cleanly to (a) an empty database and (b) the previous release's schema, and the previous release's binary must still run against the migrated schema. No destructive rename-in-place — use add-then-backfill-then-drop across releases.
2. Append-only tables (`audit_events`, `activity_events`, histories, `*_logs`, `document_hook_executions`): no UPDATE/DELETE grants for the application role.
3. Seed data (system tenant, core permissions, role types, hook catalogue rows) belongs in migrations only when the platform cannot boot without it; otherwise in the seed tooling (`just db-seed`).
4. MariaDB is a later optional target — do not contort the PostgreSQL DDL for it, but avoid gratuitous PG-isms outside the sanctioned set (JSONB, GIN, partial indexes, advisory locks).

## Working rules

- Before writing, diff your planned DDL against the Database Schema document section for that table group; report any mismatch rather than improvising.
- After writing, verify: sequential numbering, idempotence assumptions, FK ordering (referenced tables exist first), and that every index/constraint name follows the convention.
- If tooling is available, run the migration against a scratch database (`sqlx migrate run` or docker compose db) and report the actual result.
