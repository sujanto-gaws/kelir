# Kelir Documentation

**Status:** Living · **Last updated:** 2026-09-01

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

### architectures/adr/

Architecture decision records — one file per architecturally significant choice, recording the alternatives that lost and what would reopen the question. They record *why*; the design documents remain the statement of *what* and *how* (authority rule 8). Rules in [standards/06. Architecture Decision Records.md](standards/06.%20Architecture%20Decision%20Records.md); this table is the only index, and a new ADR adds its row here in the same change.

| ADR | Decision | Status |
|---|---|---|
| [0000. ADR Template.md](architectures/adr/0000.%20ADR%20Template.md) | The form every record is copied from | Template |
| [0001](architectures/adr/0001.%20Rust%20and%20Axum%20for%20the%20Backend.md) | Rust and Axum for the backend — one backend language, and it is Rust | Adopted |
| [0002](architectures/adr/0002.%20SQLx%20with%20Compile-Time%20Verified%20Queries.md) | SQLx with compile-time verified queries; no run-time query assembly | Adopted |
| [0003](architectures/adr/0003.%20PostgreSQL%20as%20the%20Only%20Database.md) | PostgreSQL as the only database | Adopted |
| [0004](architectures/adr/0004.%20A%20Modular%20Monolith%20with%20a%20Flat%20Module%20Layout.md) | A modular monolith with a flat `modules/` layout | Adopted |
| [0005](architectures/adr/0005.%20An%20Internal%20Workflow%20Engine%20Instead%20of%20BPMN.md) | An internal workflow engine instead of BPMN | Adopted |
| [0006](architectures/adr/0006.%20Metadata-Driven%20Forms%20and%20Lists.md) | Metadata-driven forms and lists rendered from JFSS | Adopted |
| [0007](architectures/adr/0007.%20Explicit%20Endpoints%20for%20Core%20Master%20Data.md) | Explicit typed endpoints for the six core master-data entities | Adopted |
| [0008](architectures/adr/0008.%20One%20JSON%20Logic%20Engine%20on%20Both%20Sides.md) | One JSON Logic engine on both sides — `datalogic-rs` and its WASM build | Adopted |
| [0009](architectures/adr/0009.%20Per-Request%20Tenancy%20Deferred%20Past%201.0.md) | Per-request tenancy deferred past 1.0 | **Superseded** by 0010 |
| [0010](architectures/adr/0010.%20Multi-Tenant%20Mode%20Runs%20and%20Roles%20Are%20Tenant-Scoped.md) | Multi-tenant mode runs, and roles are tenant-scoped | Adopted |
| [0011](architectures/adr/0011.%20A%20Derived%20Surface%20Requires%20the%20Permission%20of%20What%20It%20Derives%20From.md) | A derived surface discloses only what its subject's permission allows | Adopted |
| [0012](architectures/adr/0012.%20The%20Audit%20Hash%20Chain%20Covers%20the%20Payload.md) | The audit hash chain covers the payload, length-prefixed | Adopted |
| [0013](architectures/adr/0013.%20A%20Business%20Code%20Is%20Never%20Released.md) | A business code is never released, even by a deleted party | Adopted |
| [0014](architectures/adr/0014.%20Kelir%20Extends%20JFSS%20Through%20Settings.md) | Kelir extends JFSS through `settings`, never by editing the standard | Adopted |
| [0015](architectures/adr/0015.%20Calculations%20Run%20Before%20Conditional%20Stripping.md) | Calculations run before conditional stripping — a security property | Adopted |
| [0016](architectures/adr/0016.%20An%20Unenforceable%20Rule%20Refuses%20Rather%20Than%20Passes.md) | A rule this build cannot enforce refuses rather than passes | Adopted |
| [0017](architectures/adr/0017.%20Field%20Refusals%20Answer%20422%20with%20the%20S10.3%20Envelope.md) | Field-level refusals answer 422 with the S10.3 envelope | Adopted |
| [0018](architectures/adr/0018.%20Division%20by%20Zero%20Is%20an%20Evaluation%20Error.md) | Division by zero is an evaluation error on both sides | Adopted |
| [0019](architectures/adr/0019.%20A%20Workflow%20Definition%20Is%20Narrowed%20at%20Save%20Time.md) | A workflow definition the engine could never run is refused at save | Adopted |
| [0020](architectures/adr/0020.%20The%20Workflow%20Owns%20the%20Document%20Status.md) | The workflow owns the document's status while a process is live | Adopted |
| [0021](architectures/adr/0021.%20AllowedBy%20Authorizes%20and%20Does%20Not%20Select.md) | `allowedBy` authorizes the chosen edge; it does not select one | Adopted |
| [0022](architectures/adr/0022.%20Numbering%20Counters%20Live%20in%20Per-Scope%20Buckets.md) | Numbering counters live in per-scope buckets, allocated off the transaction | Adopted |
| [0023](architectures/adr/0023.%20Resubmission%20Enters%20Through%20the%20Document%20Submit.md) | Resubmission enters through the document's own submit endpoint | Adopted |
| [0024](architectures/adr/0024.%20A%20Role%20Task%20Notifies%20Every%20Holder.md) | A role task notifies every current holder, unbounded | Adopted |
| [0025](architectures/adr/0025.%20A%20Detached%20Send%20and%20a%20Residual%20Timing%20Oracle.md) | A detached mail send, and a 16 ms enumeration oracle accepted and stated | Adopted |
| [0026](architectures/adr/0026.%20ClamAV%20in%20the%20Stack,%20Scanning%20Asynchronously.md) | ClamAV in the compose stack, scanning asynchronously | Adopted |
| [0027](architectures/adr/0027.%20A%20Document%20Pins%20the%20Form%20Revision%20It%20Was%20Filled%20Against.md) | A document pins its form revision; re-pointing is refused only for unpinned documents | Adopted |
| [0028](architectures/adr/0028.%20A%20Definition%20Is%20Refused%20at%20Save%20Rather%20Than%20at%20Render.md) | A malformed definition is refused at save rather than at render | Adopted |

**0001–0028 were written retrospectively on 2026-09-01** from the documents that already held these decisions — a one-time pass directed by the product owner, not a standing practice (standard §9). Each says so in its §1 and names its source; the source keeps authority. Scope and planning decisions stayed `D-n` in [Product Backlog](../projects/planning/02.%20Product%20Backlog.md) §6, which carries the full `D-n` → ADR map.

### design/

| Document | Content |
|---|---|
| [01. System Design Document.md](design/01.%20System%20Design%20Document.md) | The SDD: architecture overview, stacks, module map, table groups, workflow/RAD/integration/plugin/security design, roadmap |
| [02. Database Schema.md](design/02.%20Database%20Schema.md) | Column-level DDL for every table, with conventions, indexes, and enum vocabularies. **Its own §4 mapping table is the authoritative migration list, and its own table count is the authoritative total** — both are deliberately not restated here, because the pair written on this line went stale twice (Sprint 4 to 2026-08-27, and again by Sprint 10) and a number that is only ever adjusted by whoever remembers is a number nobody can check |

### schema/

The JSON standards family. Each specification carries its own version, RFC 2119 conformance language, and an embedded normative meta-schema.

**Two of those meta-schemas are also extracted as files**, once a validator exists to compile them: `jfss-meta-v2.0.1.json` and — since 2026-08-28 — [`jwss-meta-v1.0.0.json`](schema/jwss-meta-v1.0.0.json). The extracted file and the specification's own fenced block are **one document**, and a test in `kelir-backend/tests/` compares them: a specification describing a schema the product does not enforce is worse than no specification, because an author would write to it and be refused.

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
| [06. Architecture Decision Records.md](standards/06.%20Architecture%20Decision%20Records.md) | What earns an ADR, its required structure, status lifecycle, supersession, and the impacted-documents rule |

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
8. **Decision records** — an [ADR](architectures/adr/) records *why* a choice was made and never states *what* or *how*. Where an ADR conflicts with the SDD, the Database Schema, or a schema specification, **the operative document wins** and the ADR is stale — repaired by a superseding ADR, never by editing the record ([standards/06](standards/06.%20Architecture%20Decision%20Records.md) §7).

---

## Document Conventions

- Documentation files are numbered per folder: `NN. Title Case.md`; new documents continue their folder's numbering (naming convention §10). `architectures/adr/` runs its own four-digit series, `NNNN. Title Case.md`, where the number is the record's permanent `ADR-NNNN` identity.
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
| Database Schema | **Draft** — adopted with the SDD. Counts live in the document itself (§4 and its closing total), not here |
| Schema standards | JFSS **Final Standard**; rule registries **Active Standard**; JWSS / LHCS / PMS / EES / DTDS **Draft Standard** |
| Standards 01–06 | **Adopted** — CI enforces the coding standard, branch protection implements the git workflow. Standard 06 (ADRs) was adopted 2026-09-01 |
| Decision records | **28 adopted, one superseded** (0009 by 0010) — all written in the 2026-09-01 retrofit pass from the architecture documents and the `D-n` table. The next decision taken is the first written as one |
| Concepts 01–04 | **Superseded** — background and intent only (authority rule 7) |
| Planning | Complete — sprint plan and product backlog cover all 164 FRs. Scope decisions **D-1…D-44** are recorded in [Product Backlog](../projects/planning/02.%20Product%20Backlog.md) §6; all are resolved except **D-15**, and **D-7** is superseded by **D-18**. Sprints 0–13 are detailed in [Sprint Plan](../projects/planning/01.%20Sprint%20Plan.md) §5; 14–21 remain an outline in §6, detailed at each phase boundary |
| Implementation | **Phases 1–4 released** — `v0.1.0` through `v0.4.0`; staging retired by **D-9**. **Phase 5's scope is complete and the phase is not closed**: all six of Sprint 11's construction items are merged, so a submitted document runs an approval end to end — definitions, instances, tasks, an inbox, approve/reject/return, delegation, due dates, conditional routing and a history that says how the document got here. What remains before `v0.5.0` is the exit rather than the work: the demo, an independent verification pass, and the release rehearsal. **The pass has started and is partial** — three of the six items were read by somebody who did not write them and three were not ([record 09](../projects/verifications/09.%20Sprint%2011%20Independent%20Pass.md) §1) — and it has already produced one blocking defect, [#259](https://github.com/sujanto-gaws/kelir/issues/259): a definition could publish and then fail at run time. That is fixed, and it moved the JWSS to **R-5**. **`v0.5.0` is not the MVP** (**D-1**) — that is `v0.6.0` at the end of Phase 6. See [Sprint 11 Status](../projects/status/12.%20Sprint%2011%20Status.md) |
