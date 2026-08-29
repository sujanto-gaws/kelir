//! The workflow engine — what turns a submitted document into an approval
//! somebody has to make (FR-WF-*).
//!
//! A workflow definition is a finite state machine written in
//! [JWSS](../../../../docs/schema/JSON%20Workflow%20Schema.md): states, the
//! transitions between them, who may act, and which state each one means for
//! the document underneath. Publishing one freezes it. Submitting a document of
//! a type that binds one starts an **instance** of that revision; entering a
//! state that declares a `task` creates the task; approving or rejecting the
//! task moves the instance; and the document's status follows.
//!
//! Storage is [Database Schema](../../../../docs/design/02.%20Database%20Schema.md)
//! §7, created by `0025_workflow.sql`.
//!
//! # Sequential approval only, and it is a constraint rather than an intention
//!
//! An instance is in exactly one `current_state`; a JWSS state declares at most
//! one `task`; so an instance has at most one open task, and
//! `uq_workflow_tasks_open_per_instance` says so in the database. Parallel
//! approval is FR-WF-016, which the [SRS](../../../../docs/requirements/srs.md)
//! §4.8 defers itself — and when it is scheduled, that index is the thing it has
//! to argue with. Which is the point: this design's assumption is written down
//! and dated, rather than discovered later as a property of code that turned out
//! to allow something.
//!
//! # Three status notions exist in this codebase and this module owns one of them
//!
//! This is [#178](https://github.com/sujanto-gaws/kelir/issues/178)'s AC5, and
//! it is stated here rather than in three places because that is the whole point
//! of it. [`super::document`]'s own module documentation draws the first half of
//! the same line; this is where the third notion arrives and where the
//! relationship between all three is settled.
//!
//! **`record_status` answers "how far has this record got through governance".**
//! A master-data record — a supplier, a facility — is drafted, approved, made
//! active, suspended, archived (FR-MDM-007, [#99]). It is a property of a record
//! *about a real-world thing that keeps existing*, and nothing in this module
//! reads or writes it.
//!
//! **`workflow_instances.current_state` answers "where is this process".** It is
//! the **single source of truth** for that ([#175] AC3). It is a state of *this
//! definition*, so it is free-form: `MANAGER_APPROVAL` means whatever the
//! workflow that declared it means.
//!
//! **`documents.status` answers "where is this document in its own life"**
//! ([#169]) — and from Sprint 10 on, **for a document with a live instance it is
//! a projection of the instance's state, not a parallel record.**
//!
//! ## The projection is one-way, and here is what that costs
//!
//! A workflow transition sets the document's status. **Setting the document's
//! status does not move the workflow**, and it is not allowed to try:
//! `PUT /api/v1/documents/{id}/status` refuses a document that has a live
//! instance, naming the instance and the action that would move it.
//!
//! That withdraws something the document surface shipped in Sprint 9, and it is
//! deliberate. Letting it through would produce a document whose status
//! disagrees with the process driving it, which is the exact defect [#178]
//! exists to prevent — and it would produce it silently, on the screen a person
//! is most likely to trust. #178 AC2 allows for a manual override as *"a
//! separate, audited action rather than a side effect"*; nothing has asked for
//! one, so none is built, and the refusal is what makes the need visible if
//! anything ever does.
//!
//! ## The map lives in the definition, never in code
//!
//! [#178] AC4. `mapsToDocumentStatus` on each state is the whole mapping, and
//! [`domain::graph::State`] is where it is read. **A new workflow says what its
//! own states mean for a document, and adding one needs no backend change** — if
//! this module ever grows a `match` from state code to document status, that
//! requirement has been broken.
//!
//! # Two records of one decision, and the third that is not this sprint's
//!
//! `approval_decisions` and `workflow_task_history` are both written when a
//! decision is recorded, and they are not one row twice:
//!
//! * **`approval_decisions` answers "what was decided about this document"** —
//!   the formal record, one row per decision, denormalized for reporting and for
//!   the signature binding Phase 8 will need (§7.8).
//! * **`workflow_task_history` answers "what happened to this task"** —
//!   append-only, one row per task status move, including moves no person made.
//!
//! **Neither is FR-WF-012.** That is the *document's* history — "how did this
//! document get where it is" — and it is
//! [#181](https://github.com/sujanto-gaws/kelir/issues/181) in Sprint 11, whose
//! own text requires it to distinguish itself from the audit trail. It will be
//! the third statement of this distinction, and it should read like the first
//! two.
//!
//! [#99]: https://github.com/sujanto-gaws/kelir/issues/99
//! [#169]: https://github.com/sujanto-gaws/kelir/issues/169
//! [#175]: https://github.com/sujanto-gaws/kelir/issues/175
//! [#178]: https://github.com/sujanto-gaws/kelir/issues/178

pub mod domain;
pub mod handlers;
pub mod repository;
pub mod service;

/// The permissions `0025_workflow.sql` seeds, as constants rather than
/// literals — a typo in a permission string is a permission nobody holds, which
/// reads as a working check that refuses everybody (#58).
///
/// **The resource segment is required here**, because this module manages
/// several resources ([naming convention](../../../../docs/standards/02.%20Naming%20Convention.md)
/// §6) — which is the case that convention names, with `workflow:task:execute`
/// as its own worked example.
pub const DEFINITION_CREATE: &str = "workflow:definition:create";
pub const DEFINITION_READ: &str = "workflow:definition:read";
pub const DEFINITION_UPDATE: &str = "workflow:definition:update";
pub const DEFINITION_DELETE: &str = "workflow:definition:delete";

/// Publishing is not updating.
///
/// It fixes a revision that running instances will execute for as long as they
/// run, which is `rad:form:publish`'s reasoning one artefact over. Who may draft
/// an approval chain and who may make one binding are different questions.
pub const DEFINITION_PUBLISH: &str = "workflow:definition:publish";

pub const INSTANCE_READ: &str = "workflow:instance:read";

/// Reading tasks — the inbox and one task's detail.
///
/// [`super::task_inbox`] requires **this** permission rather than one of its
/// own, because it reads these rows. A `task:read` beside it would let a
/// deployment grant the inbox without granting the task, which is the gap §5.13
/// of the schema refused to create for `rad:lookup:read`.
pub const TASK_READ: &str = "workflow:task:read";

/// Claiming a task, recording a decision on it, and handing it to somebody else.
///
/// **One permission for all three**, and a separate `workflow:task:claim` was
/// the alternative. It is rejected: a permission that lets somebody take a task
/// off the queue and then not act on it is a permission to stall an approval,
/// which is a worse power than the one it was trying to split off. The same
/// argument refused a `workflow:task:delegate` when
/// [#184](https://github.com/sujanto-gaws/kelir/issues/184) added the hand-off —
/// splitting off the ability to stop working on something is the same shape of
/// mistake. *Which* task a caller may act on is not a permission question at
/// all — it is answered against the row, by
/// [`domain::task::refuse_unless_theirs`] and, for a hand-off, by the stricter
/// [`domain::task::refuse_unless_held_by`].
pub const TASK_EXECUTE: &str = "workflow:task:execute";

/// What the audit trail calls a workflow definition (naming convention §7).
pub const DEFINITION_OBJECT_TYPE: &str = "WORKFLOW_DEFINITION";
/// What the audit trail calls a running process.
pub const INSTANCE_OBJECT_TYPE: &str = "WORKFLOW_INSTANCE";
/// What the audit trail calls a user task.
pub const TASK_OBJECT_TYPE: &str = "WORKFLOW_TASK";
