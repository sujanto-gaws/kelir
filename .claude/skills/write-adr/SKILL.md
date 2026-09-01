---
name: write-adr
description: Author, supersede, or reject a Kelir architecture decision record in docs/architectures/adr/. Use whenever an architecturally significant choice is being made or reversed — a stack or library choice, a cross-cutting contract (envelope, permission grammar, hook result, outbox), a pattern that constrains code not yet written, or a trade-off against a stated NFR — and whenever the user asks for "an ADR", "a decision record", or to record why something is built the way it is. Also use to check whether a decision needs an ADR at all.
---

# Writing a Kelir ADR

The authority is [docs/standards/06. Architecture Decision Records.md](../../../docs/standards/06.%20Architecture%20Decision%20Records.md). Read it before writing — this skill is the procedure, that document is the rule, and where they differ the standard wins.

## Step 0 — does this need an ADR at all?

Most decisions do not. Apply the significance tests in standard §1 and the exclusion list in §2 before touching the folder:

- Scope (what gets built, when) → `D-n` in [Product Backlog](../../../projects/planning/02.%20Product%20Backlog.md) §6, not an ADR.
- A name, casing, prefix → [standards/02. Naming Convention.md](../../../docs/standards/02.%20Naming%20Convention.md).
- How a shipped subsystem works → the SDD or the architecture documents.
- A column, index, table → [design/02. Database Schema.md](../../../docs/design/02.%20Database%20Schema.md); a JSON shape → its `docs/schema/` specification.

If it fails every significance test, say so and put the reasoning where it belongs instead. A decision with no rejected alternative is not significant — an ADR whose §3 has one row is a note wearing a costume.

## Step 1 — allocate and copy

1. List `docs/architectures/adr/` and take the next free four-digit number (`0000` is the template and is never edited in place).
2. Copy `0000. ADR Template.md` to `NNNN. Title Case.md`, titled with **the decision, not the topic** — `0007. Outbox Before Direct Delivery.md`, not `0007. Event Delivery.md`.
3. Delete the template's preamble, down to and including the `---` rule. Keep the section numbers exactly as they are.

## Step 2 — write it

Fill §1–§7 in the template's order. What separates a usable record from filler:

- **§1 Context** — the forces as they stood, in facts a reader can check: what already existed, which constraints bound the choice, which `FR-*`/`NFR-*` applied, what raised the question now. If the section names the answer, it is not context.
- **§2 Decision** — one paragraph, imperative, unhedged, with the scope explicit: what it covers and what it deliberately leaves open.
- **§3 Options Considered** — every option genuinely on the table, each with a paragraph saying what it would have bought and the specific cost that ruled it out. Do not invent losers to pad the table; find the real ones by reading what the codebase already does.
- **§4 Consequences** — costs as well as benefits. An ADR with no negatives has not been thought through.
- **§4.1 Impacted Documents** — see step 3. This is the load-bearing table.
- **§5 Compliance** — the test, CI check, or migration rule that catches a violation. "Reviewer judgement" only when nothing mechanical can exist, and then say what the reviewer looks at.
- **§6 Revisit When** — an observable trigger, not a mood.

Before writing §1 and §3, read what the decision touches — the SDD section, the architecture document, the code — so the context is the project's, not a generic one.

## Step 3 — change the impacted documents in the same branch

**This is the step that gets skipped, and skipping it defeats the record.** An ADR states *why*; the operative documents state *what* and *how* (authority rule 8). Adoption is not complete until they carry the change:

1. List every affected document and section in §4.1.
2. Make those edits now, in this branch, bumping each document's `Last updated`.
3. If one is genuinely too large for this PR, the row names the issue that carries it — and the ADR stays `Draft` until that issue closes.
4. Add the ADR's row to the `architectures/adr/` table in [docs/README.md](../../../docs/README.md). That table is the only index.

## Step 4 — status and commit

- Status is `Draft` while proposed; `Adopted` on merge with the decision date set to the merge date.
- A declined proposal flips to `Rejected` and **merges anyway** — the analysis is the value; it is never deleted.
- Branch `docs/adr-<slug>`, commit `docs(adr): <the decision>` — type `docs`, scope `adr` (commit convention §2–§3).
- Review is a deliberate pass over §3 and §4 before merging: were the alternatives real, are the costs stated as plainly as the benefits. Approvals on `main` are currently relaxed to zero (git workflow §5), so on this repository that pass is the author's own.

## Superseding an existing ADR

Never edit a decision to reverse it (standard §6):

1. Write a **new** ADR with `**Supersedes:** ADR-NNNN`, whose §1 states which force changed. If none did, say plainly that the first analysis was wrong — that is the more useful record.
2. Set the old one's status to `Superseded`, add `**Superseded by:** ADR-NNNN` to its metadata, bump its `Last updated`, and change nothing else in it.
3. For a partial supersession, the old ADR stays `Adopted` and carries an inline note saying which part still stands.
4. Update both rows in the `docs/README.md` index.

## Before you hand it back

Run the author checklist in standard §10, then report every file touched — the ADR, each impacted document, and the index. In Kelir, documentation changes go through the `doc-consistency` gate; expect the cross-references and `Last updated` dates to be checked.
