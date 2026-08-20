# Kelir Project Management

**Status:** Living · **Last updated:** 2026-08-13

This folder holds Kelir's **project management artifacts** — the documents that govern *when and in what order* things get built. They live outside [docs/](../docs/) deliberately: `docs/` describes the system (requirements, architecture, design, standards — see its [README](../docs/README.md) for the full index and authority rules); `projects/` describes the work.

---

## Structure

```text
projects/
├── planning/           what gets built, in what order
├── status/             per-sprint status reports (decision-oriented, one page)
├── retrospectives/     per-sprint and per-phase retrospectives with committed actions
└── releases/           one evidence checklist per tagged release
```

Each folder is a numbered series (`NN. Title Case.md`); `00.` files are templates — copy them and continue the numbering from `01.`.

### planning/

| Document | Content |
|---|---|
| [01. Sprint Plan.md](planning/01.%20Sprint%20Plan.md) | Scope-sequenced sprint plan mapping the SDD roadmap phases (§14, Phases 1–9) onto a sprint cadence. Sprint contents are the commitment; calendar dates shift with measured velocity. |
| [02. Product Backlog.md](planning/02.%20Product%20Backlog.md) | The scope inventory behind that sequence: all 164 SRS functional requirements mapped to epics, backlog items and target sprints, with the Definition of Ready, the MVP coverage check against SRS §9, and the scope decisions D-1…D-5 (all resolved 2026-08-11, with what each one changed and where it was applied). |

### status/

| Document | Content |
|---|---|
| [00. Status Report Template.md](status/00.%20Status%20Report%20Template.md) | One-page end-of-sprint report: goal verdict, scope status per FR with evidence, blockers with recommendations, scope changes, next-sprint entry criteria |
| [01. Sprint 0 Status.md](status/01.%20Sprint%200%20Status.md) | Sprint 0, closed 2026-08-11: repository, branch protection, CI, compose stack, tracker seeded. Retained as the baseline record |
| [02. Sprint 1 Status.md](status/02.%20Sprint%201%20Status.md) | Sprint 1, closed 2026-08-12: the Phase 1 backend — config, pool and migrations, health endpoints, response envelope and `AppError`, pagination, generated OpenAPI |
| [03. Sprint 2 Status.md](status/03.%20Sprint%202%20Status.md) | Sprint 2, 2026-08-12: the Phase 1 frontend — app shell, envelope-aware API client, login page, UI baseline, plus the CORS layer without which the two halves could not talk. #12 (staging) still open |
| [04. Sprint 3 Status.md](status/04.%20Sprint%203%20Status.md) | Sprint 3, closed 2026-08-12: the Phase 2 backend — authentication with rotating refresh tokens, users, roles, permission enforcement, and the audit write path |
| [05. Sprint 4 Status.md](status/05.%20Sprint%204%20Status.md) | Sprint 4, closed 2026-08-13: the Phase 2 `Must` scope built and merged, and two items recorded as delivered that failed Definition-of-Done verification. Five open decisions, two of which gate the `v0.2.0` tag |

### retrospectives/

| Document | Content |
|---|---|
| [00. Retrospective Template.md](retrospectives/00.%20Retrospective%20Template.md) | Per-sprint and per-phase retrospective: worked/didn't/root causes, committed actions with owners, previous-actions review. Convention changes flow into `docs/standards/`, plan changes re-baseline the sprint plan |
| [01. Sprints 0-1 Retrospective.md](retrospectives/01.%20Sprints%200-1%20Retrospective.md) | Combined, because the two sprints ran without a boundary and their lessons interleave. Six committed actions, chiefly: scaffolds must run rather than merely compile, and acceptance criteria state outcomes rather than mechanisms |
| [02. Sprints 2-4 Retrospective.md](retrospectives/02.%20Sprints%202-4%20Retrospective.md) | Combined because Sprints 2 and 3 closed without one — itself a finding. Eight actions, chiefly: a security-control test is not accepted until the defect it claims to catch has been reintroduced and seen to fail, and a status report does not go Final without its retrospective |

### releases/

| Document | Content |
|---|---|
| [00. Release Checklist Template.md](releases/00.%20Release%20Checklist%20Template.md) | Per-release evidence record for the [release process](../docs/standards/04.%20Release%20Process.md): pre-flight (scope, tests, migrations, docs, version), staging pass with rollback rehearsal, ship steps, aftermath |
| [01. Release v0.1.0.md](releases/01.%20Release%20v0.1.0.md) | Phase 1. Cut 2026-08-12 — pre-flight and ship steps done, staging deploy outstanding and blocking |

---

## How this folder relates to docs/

- **Scope** comes from the [SRS](../docs/requirements/srs.md) (FR/NFR IDs, Must/Should/Could priorities, MVP criteria §9).
- **Sequence** comes from the [SDD](../docs/design/01.%20System%20Design%20Document.md) roadmap (§14); the sprint plan implements it.
- **Definition of Done** references the [standards](../docs/standards/): git workflow for merging, coding standard for tests, release process for tagging. Every phase ends with a tagged release.
- Plans here never redefine requirements or design — when a plan and a `docs/` document disagree, `docs/` wins and the plan is re-baselined.

---

## Conventions

- Documents are numbered per subfolder (`NN. Title Case.md`) and carry the standard header line `**Status:** … · **Last updated:** YYYY-MM-DD`, same as `docs/` (naming convention §10). `00.` is reserved for the folder's template.
- `Status` takes a value from the [vocabulary](../docs/standards/02.%20Naming%20Convention.md) (naming convention §10.1). Three of them do the work here: `Living` for the plan and the backlog, which are revised every sprint; `Final` for a status report or retrospective, which records a moment and is never revised afterwards; `Template` for the `00.` files. A later report does not supersede an earlier one — it follows it.
- Relative links to documentation use `../../docs/…` from inside a subfolder (one level deeper than this file).
- New document types get their own numbered-series subfolder and a row in this index; don't mix types within a folder.
