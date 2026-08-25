# Kelir Documentation

**Status:** Living · **Last updated:** 2026-08-25

Kelir is a metadata-driven, document-centric, workflow-enabled full-stack framework for building enterprise business applications rapidly — Rust (Axum + SQLx + PostgreSQL) on the backend, Vue 3 (Vite + Pinia + shadcn-vue + Tailwind CSS v4) on the frontend. This folder is the complete documentation set; this file is its map.

---

## Reading Order

New to the project? Read in this order:

1. [concepts/01. Concept.md](concepts/01.%20Concept.md) — the founding idea: documents + a central workflow engine.
2. [architectures/01. Basic Framework Concept and Architecture.md](architectures/01.%20Basic%20Framework%20Concept%20and%20Architecture.md) — the architecture rationale, document lifecycle, and key decisions.
3. [requirements/srs.md](requirements/srs.md) — what the system shall do (FR/NFR, MVP criteria).
4. [design/01. System Design Document.md](design/01.%20System%20Design%20Document.md) — how the requirements are realized.
5. [design/02. Database Schema.md](design/02.%20Database%20Schema.md) — the column-level schema, when you start touching data.
6. [projects/planning/01. Sprint Plan.md](../projects/planning/01.%20Sprint%20Plan.md) — what gets built when.
7. [operations/01. Installation and Deployment.md](operations/01.%20Installation%20and%20Deployment.md) — how to run it, when you need it running.

Everything else is reference material you pull in when working on that area.

---

## Folder Map

### requirements/

| Document | Content |
|---|---|
| [srs.md](requirements/srs.md) | Software Requirements Specification: functional (FR-*) and non-functional (NFR-*) requirements, MVP acceptance criteria (§9) |

### concepts/

Early exploratory documents. Still valid for intent and examples, but **superseded in detail** by the architecture and design documents (e.g. their `snake_case` JSON samples and flat `mdm_suppliers`-style tables are superseded — see Authority Rules below).

| Document | Content |
|---|---|
| [01. Concept.md](concepts/01.%20Concept.md) | Founding concept: document flow and workflow synchronized through a central engine |
| [02. Handling Attachments Comments and Activity Log.md](concepts/02.%20Handling%20Attachments%20Comments%20and%20Activity%20Log.md) | Attachment, comment, activity, and audit behavior with samples |
| [03. Handling Master Data.md](concepts/03.%20Handling%20Master%20Data.md) | Master data governed by document workflows; record lifecycle statuses |
| [04. Sample Data Structure for System Overview.md](concepts/04.%20Sample%20Data%20Structure%20for%20System%20Overview.md) | End-to-end sample data walkthrough of the whole system |

### architectures/

| Document | Content |
|---|---|
| [01. Basic Framework Concept and Architecture.md](architectures/01.%20Basic%20Framework%20Concept%20and%20Architecture.md) | Framework concept, stack, layering, **document lifecycle and hooks (§12)**, APIs, phases, key decisions |
| [02. Kelir Framework.md](architectures/02.%20Kelir%20Framework.md) | Canonical module breakdown — backend and frontend modules with responsibilities, entities, endpoints |
| [03. Kelir Modules for Interfacing with External Systems.md](architectures/03.%20Kelir%20Modules%20for%20Interfacing%20with%20External%20Systems.md) | Integration layer: connectors, webhooks, event bus/outbox, mapping, sync, secrets, logging |
| [04. Kelir Plugin and Extension Management Concept.md](architectures/04.%20Kelir%20Plugin%20and%20Extension%20Management%20Concept.md) | Plugin platform: registry, lifecycle, permissions, hooks, settings, sandboxing |
| [05. Core - Master Data - Party.md](architectures/05.%20Core%20-%20Master%20Data%20-%20Party.md) | Party master-data model as a JSON Schema aggregate (OFBiz-style) |

### design/

| Document | Content |
|---|---|
| [01. System Design Document.md](design/01.%20System%20Design%20Document.md) | The SDD: architecture overview, stacks, module map, table groups, workflow/RAD/integration/plugin/security design, roadmap |
| [02. Database Schema.md](design/02.%20Database%20Schema.md) | Column-level DDL for all 95 tables across 17 migrations, with conventions, indexes, and enum vocabularies |

### schema/

The JSON standards family. Each specification carries its own version, RFC 2119 conformance language, and an embedded normative meta-schema.

| Standard | Version | Defines |
|---|---|---|
| [JSON Form Schema.md](schema/JSON%20Form%20Schema.md) (JFSS) | 2.0.1 (errata E-1) | Dynamic form definitions: components, validation, conditional logic, calculations |
| [JFSS Validation Rule Registry.md](schema/JFSS%20Validation%20Rule%20Registry.md) | 1.3.0 | Registered validation rules usable in JFSS `rules` |
| [JFSS Calculation Rule Registry.md](schema/JFSS%20Calculation%20Rule%20Registry.md) | 1.4.0 | Registered JSON Logic operators for calculations and all platform conditions |
| [JSON Workflow Schema.md](schema/JSON%20Workflow%20Schema.md) (JWSS) | 1.0.0 | Workflow definitions: states, transitions, tasks, assignment rules, guards/actions |
| [Lifecycle Hook Contract.md](schema/Lifecycle%20Hook%20Contract.md) (LHCS) | 1.0.0 | The hook ABI: registration entry, invocation payload, CONTINUE/MODIFY/REJECT result |
| [Plugin Manifest Schema.md](schema/Plugin%20Manifest%20Schema.md) (PMS) | 1.0.0 | `plugin.json`: identity, entrypoints, permissions, hooks, events, settings, dependencies |
| [Event Envelope Schema.md](schema/Event%20Envelope%20Schema.md) (EES) | 1.0.0 | The event envelope for outbox, plugin subscriptions, and webhook deliveries |
| [Document Type Definition Schema.md](schema/Document%20Type%20Definition%20Schema.md) (DTDS) | 1.0.0 | The document type aggregate: form/list/workflow bindings, numbering, attachment rules, hooks |

### operations/

| Document | Content |
|---|---|
| [01. Installation and Deployment.md](operations/01.%20Installation%20and%20Deployment.md) | Running Kelir: the development stack, deploying to an IP for testing, provisioning and deploying staging on Ubuntu 24.04, the full configuration reference, backup and restore, troubleshooting |

### standards/

| Document | Content |
|---|---|
| [01. Coding Standard.md](standards/01.%20Coding%20Standard.md) | Rust and Vue coding rules, module layout, testing requirements, review checklist |
| [02. Naming Convention.md](standards/02.%20Naming%20Convention.md) | **Single authority for all names**: code, database, API, permissions, events, identifiers, files |
| [03. Commit Message Convention.md](standards/03.%20Commit%20Message%20Convention.md) | Conventional-commit format, types, scopes, breaking changes |
| [04. Release Process.md](standards/04.%20Release%20Process.md) | Versioning, release checklist, tagging, hotfixes, migration compatibility |
| [05. Git Workflow.md](standards/05.%20Git%20Workflow.md) | Branching model (always-releasable `main`), PRs, review |

### ../projects/planning/

Project management lives outside `docs/`, in the repo-root [projects/](../projects/README.md) folder (it has its own index).

| Document | Content |
|---|---|
| [01. Sprint Plan.md](../projects/planning/01.%20Sprint%20Plan.md) | Scope-sequenced sprint plan mapping the SDD roadmap phases onto sprints |
| [02. Product Backlog.md](../projects/planning/02.%20Product%20Backlog.md) | Every FR mapped to an epic, backlog item and sprint; MVP coverage check against SRS §9; the scope decision record (D-1…D-5) |

---

## Authority Rules

When documents disagree, precedence is explicit:

1. **Names** — [standards/02. Naming Convention.md](standards/02.%20Naming%20Convention.md) wins over every other document; older documents with different casing or naming are superseded on that point.
2. **Requirements vs design** — the [SRS](requirements/srs.md) says *what*, the [SDD](design/01.%20System%20Design%20Document.md) says *how*. Scope questions resolve to the SRS.
3. **SDD vs architecture documents** — architecture documents are the detailed authority; where they conflict with the SDD, **the SDD wins** and the architecture document gets fixed.
4. **Module lists** — [architectures/02](architectures/02.%20Kelir%20Framework.md) supersedes document 01 where their module lists differ.
5. **Tables and columns** — the [Database Schema](design/02.%20Database%20Schema.md) wins over the table lists in SDD §6 and over column sketches in older documents; its §14 records every deliberate deviation.
6. **JSON shapes** — within each schema/ specification, the embedded **meta-schema is normative** over its own prose. Illustrative JSON examples in architecture/concept documents predate the specifications and are superseded by them (marked in place).
7. **Concepts folder** — background and intent only; any detail that conflicts with newer documents is superseded.

---

## Document Conventions

- Documentation files are numbered per folder: `NN. Title Case.md`; new documents continue their folder's numbering (naming convention §10).
- Every document carries `**Status:** … · **Last updated:** YYYY-MM-DD` under its H1. Schema specifications use the JFSS house style (`**Version:** … / **Status:** … / **Target Stack:** …`) instead, with the spec version doubling as the change marker.
- `Status` takes one of the values defined in [naming convention §10.1](standards/02.%20Naming%20Convention.md) — `Draft`, `Adopted`, `Living`, `Superseded`, `Retired`, `Template`, `Final`, plus the `… Standard` variants for `schema/`. It answers one question: how much can a reader rely on this? A value needing a qualifier to be understood is the wrong value.
- Cross-references cite the target as `document NN §X` (within a folder) or a relative markdown link with URL-encoded spaces (`[SDD](design/01.%20System%20Design%20Document.md)`).
- Schema meta-schema artifacts are named `<acronym>-meta-vX.Y.Z.json`; until extracted as files, the embedded block in each specification is the artifact.

---

## Current State

| Area | Status |
|---|---|
| Requirements (SRS v0.8) | **Draft** — the scope authority, and §9 is the MVP gate, but approval is still an open action (SDD §16 step 1) |
| Architecture 01–05 | **Adopted** |
| System Design Document v0.1 | **Draft** — approval open, as above |
| Database Schema | **Draft** — 95 tables / 17 migrations; adopted with the SDD |
| Schema standards | JFSS **Final Standard**; rule registries **Active Standard**; JWSS / LHCS / PMS / EES / DTDS **Draft Standard** |
| Standards 01–05 | **Adopted** — CI enforces the coding standard, branch protection implements the git workflow |
| Concepts 01–04 | **Superseded** — background and intent only (authority rule 7) |
| Planning | Complete — sprint plan and product backlog cover all 164 FRs. Scope decisions D-1…D-18 are recorded in [Product Backlog](../projects/planning/02.%20Product%20Backlog.md) §6; all are resolved except **D-15**, and **D-7** is superseded by **D-18** |
| Implementation | Phase 1 released as `v0.1.0` and closed; staging retired by decision D-9. Phase 2 backend done (Sprint 3): Argon2id authentication with rotating refresh tokens, users, roles, permissions enforced per route, and the hash-chained audit write path. Remaining in Phase 2: the frontend auth flow, admin screens, delegation, departments and rate limiting. See [Sprint 3 Status](../projects/status/04.%20Sprint%203%20Status.md) |
