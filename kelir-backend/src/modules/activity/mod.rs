//! Activity — what happened to a document, for the people looking at it
//! (SRS §4.12, FR-ACT-*).
//!
//! # Four records, and this is the fourth
//!
//! [#247](https://github.com/sujanto-gaws/kelir/issues/247) AC3 asks for this
//! before the first row is written, because this project has drawn the line
//! three times already and paid one layer over each time it did not.
//!
//! | Record | Answers | Behind | Read by |
//! |---|---|---|---|
//! | `activity_events` | *what happened to this document* | the document's own read | whoever is looking at the document |
//! | `audit_events` | *was this tampered with* | `master-data:audit:read` | somebody investigating |
//! | `workflow_history` | *how did this document get here* | `workflow:...:read` | whoever is following the approval |
//! | `approval_decisions` | *what was formally decided about this document* | the workflow surface | whoever needs the signature |
//!
//! **No one of them is derived from any other.** `modules::activity` reads no
//! audit table and no workflow table; the workflow records were traced the same
//! way in [record 09](../../../../projects/verifications/09.%20Sprint%2011%20Independent%20Pass.md)
//! §6.3, by following every `INSERT` site rather than by believing the prose.
//!
//! # The distinction a signature holds, rather than a paragraph
//!
//! The sharpest difference is between the first two, and it is visible in two
//! function signatures one module apart:
//!
//! ```ignore
//! audit::record(pool: &PgPool, entry: AuditEntry<'_>)              // its own connection
//! activity::record(transaction: &mut PgTransaction<'_>, …)         // the caller's
//! ```
//!
//! **An audit row is a control over an action**, so it must survive the action
//! failing — `record_or_warn` even swallows its own error, deliberately.
//! **An activity event is part of what the action produced**, so an approval
//! that rolled back must leave no trace saying it happened. #247 AC2 states that
//! as a rule about transactions; the signature is what makes it one nobody can
//! step around, because there is no way to reach the insert with a pool.
//!
//! # Who may see what, for the third time — and this time in a function
//!
//! [#292](https://github.com/sujanto-gaws/kelir/issues/292) found the timeline
//! serving an attachment's **original file name** to a caller holding
//! `activity:read` and `document:read` and neither `attachment:read` nor
//! `comment:read`, alongside a comment's length and the second party to a
//! delegation. Not a leak across tenants or documents — both are in the
//! statement — but a **missing second permission**, and
//! [#263](https://github.com/sujanto-gaws/kelir/issues/263)'s shape one sprint
//! on and pointing the other way: that was a screen showing too little to a
//! caller lacking a permission, this was a surface showing too much to one.
//!
//! #292 offered two shapes. **D-45 took the second, and the reasoning is here
//! because this is the third time this module has had to say who may see
//! what** — after the four records above and the two permissions in
//! [`service::list_activity`].
//!
//! | | **Filter the entries** | **Carry no subject detail** (taken) |
//! |---|---|---|
//! | What a caller without `attachment:read` sees | nothing where the upload was | *a file was attached, by Sara, at 09:05* — and a link |
//! | The page and `meta.total` | disagree, or the count learns a second predicate | unchanged; redaction never removes an entry |
//! | A module that adds an event type | must remember to extend the filter, and forgets silently | carries no subject detail because it never had a field for one |
//! | What is lost | the fact of the event, to some readers | nothing — every fact is still where it was already guarded |
//!
//! **The third row is the argument.** A forgotten second permission is exactly
//! what #292 *is*; a fix whose correctness depends on the next author
//! remembering the same thing is the defect with a longer fuse. And the fourth
//! is why nothing is given up: the file's name is in `attachments` behind
//! `attachment:read`, the comment and its length in `comments` behind
//! `comment:read`, the delegation's second party in `workflow_history` behind
//! the workflow's read. **The timeline was answering three other records'
//! questions**, which the table above says it does not do — so this is the
//! four-record distinction being enforced rather than a new rule.
//!
//! Two halves, and they are not redundant:
//!
//! * **The writers stopped producing the keys.** `Attachment.Added`,
//!   `Attachment.Downloaded`, `Comment.Added` and `Workflow.TaskDelegated`
//!   carry `{}` and their link column; `Workflow.Decided` keeps `action`,
//!   `from` and `to` — what moved *this document* — and drops
//!   `onBehalfOfUserId`.
//! * **[`domain::disclosable`] holds it at the read**, because a fix to a
//!   writer reaches no row already written and this table is append-only. It is
//!   an allow-list by event type, `{}` for one it does not know.
//!
//! **What D-45 does not settle:** a deployment that wants the file name on the
//! timeline for readers who *do* hold `attachment:read` has no way to ask for
//! it. That would be the first shape, per entry, and it is a feature request
//! against FR-ACT-005's screen rather than a gap in this fix — the screen has
//! the link and can go and ask.
//!
//! # And a fourth time, which is D-45 being taken to its conclusion (D-47)
//!
//! [#250](https://github.com/sujanto-gaws/kelir/issues/250) AC2 asked for the
//! timeline to read through **the document's own permission and no other**, and
//! that is now what it does: [`service::list_activity`] requires nothing but
//! `document_service::get_document`.
//!
//! **The four-record table above always said so.** So does
//! `0033_activity.sql`'s own `COMMENT ON TABLE` — *behind that document's own
//! read permission* — and so does [Database Schema] §10. Three statements of
//! the rule and one implementation that asked for a second permission anyway.
//!
//! **D-45 is what made the documents right rather than optimistic.** While an
//! entry carried an attachment's file name, `activity:read` was the only thing
//! between a document's reader and another surface's data — accidentally, and
//! badly, since it was one grant for all three. Once `details` says only what
//! happened to the document, the second permission guards nothing the first
//! does not, and all it can still do is refuse the person who raised the
//! document a view of their own document's history.
//!
//! **And this module now declares no permission of its own**
//! ([#301](https://github.com/sujanto-gaws/kelir/issues/301),
//! `0041_activity_read_dropped.sql`). `ACTIVITY_READ` outlived the check by one
//! release and no longer exists: the [release process] §6 N−1 rule deprecates
//! in release N and removes in N+1, `v0.6.0` is the N−1 and it checks nothing,
//! so the constant, the row and its grants went together. What was left in the
//! interval was a permission row nothing checks — the `delegations` shape
//! **D-13** spent two decisions undoing, and the reason `modules::attachment`
//! and `modules::comment` both declare no `delete` permission. **A grep for
//! `activity:` in this crate now finds event types and nothing that gates.**
//!
//! [Database Schema]: ../../../../docs/design/02.%20Database%20Schema.md
//! [release process]: ../../../../docs/standards/04.%20Release%20Process.md
//!
//! # What is not here
//!
//! **The timeline screen arrived in Sprint 13**
//! ([#250](https://github.com/sujanto-gaws/kelir/issues/250), FR-ACT-005, MVP
//! criterion 12), one sprint after the events. The order was the point: a
//! timeline with four sources and no screen is still worth having, and a screen
//! over one source is not — so the sprint plan separated them and the screen
//! waited until attachments and comments were writing too.
//!
//! **No event this module writes on its own.** Every `record` call site is in
//! another module, because an event is part of what an action produced and the
//! action is somewhere else. The document lifecycle and the workflow actions
//! came with [#247](https://github.com/sujanto-gaws/kelir/issues/247);
//! attaching, downloading and commenting came with
//! [#248](https://github.com/sujanto-gaws/kelir/issues/248), **after** the
//! surfaces they describe rather than before them — and #292 is what that
//! ordering costs when the surface's permission does not travel with the event.
//!
//! **No `ip_address`.** The column exists because §10.1 declares it, and it
//! stays null: a timeline does not show an address. FR-AUD-005 is about the
//! *audit* row, and **D-44** carries it into #248.

pub mod domain;
pub mod handlers;
pub mod repository;
pub mod service;
