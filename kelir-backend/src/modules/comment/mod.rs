//! Comments — the conversation about a document (SRS §4.11, FR-CMT-*).
//!
//! # This is not the decision comment, and the two are one table apart
//!
//! [#249](https://github.com/sujanto-gaws/kelir/issues/249) AC3 asks for this
//! paragraph before the first row is written, because the next reader will
//! assume one is the other.
//!
//! **FR-TASK-006 shipped in Sprint 11** ([#182](https://github.com/sujanto-gaws/kelir/issues/182))
//! as three columns written in one transaction:
//!
//! | Column | Answers |
//! |---|---|
//! | `workflow_tasks.comment` | what the person holding this task said when they decided it |
//! | `approval_decisions.comment` | what was said with the formal decision about this document |
//! | `workflow_history.comment` | what was said at the transition that moved the process |
//!
//! That is a **record**: the reason an approver gave, captured with the decision
//! and immutable because the decision is. `comments` is a **conversation**:
//! something a person says about a document, which a later sprint lets them
//! reply to, edit and resolve.
//!
//! **Neither is derived from the other, and neither can stand in for the
//! other.** An approver's reason belongs to the decision and stops being
//! evidence the moment somebody can edit it; a colleague's question about a
//! supplier belongs to no decision and would have to invent one to be stored as
//! a decision comment. This is the fourth time this project has drawn a line
//! like this — `documents.status` against `workflow_instances.current_state`
//! ([#178](https://github.com/sujanto-gaws/kelir/issues/178)), the three-way
//! workflow record distinction ([#181](https://github.com/sujanto-gaws/kelir/issues/181)),
//! and `activity_events` against `audit_events` in the item after this one — and
//! each earlier time the cost of *not* drawing it was paid one layer over.
//!
//! # Two permissions, and the document's read is what scopes them
//!
//! `comment:create` and `comment:read` say whether an account comments and
//! reads comments at all. **Which document's** is the document's own question,
//! asked through its module's service, so a comment is exactly as private as the
//! document it is about. That is `modules::attachment`'s rule one table over,
//! and the same reasoning: a thing that hangs on a document cannot be more
//! visible than the document.
//!
//! **And the timeline is inside that rule rather than beside it**
//! ([#292](https://github.com/sujanto-gaws/kelir/issues/292), **D-45**). The
//! `Comment.Added` event never carried the body — `service::add_comment` drew
//! that line in the commit that wrote it — but it carried the body's
//! **length**, which is a
//! measurement of a thing the reader may not read. It now carries the
//! `comment_id` and nothing else, so what a comment says and how much of it
//! there is are both answers this module gives.
//!
//! **`comment:update` and `comment:delete` arrived with the tail**
//! ([#253](https://github.com/sujanto-gaws/kelir/issues/253),
//! `0036_comment_thread.sql`), and each is checked **with authorship**: the
//! permission says whether an account edits or deletes comments at all, and
//! `comments.created_by` says whose. There is deliberately **no moderator
//! code** — nothing in this release lets one account edit or delete another's
//! comment — because a permission row nothing checks is the `delegations`
//! situation **D-13** spent two decisions undoing. When a deployment needs one,
//! it arrives with the surface that uses it.
//!
//! # The conversation is one level deep, and a deleted comment can leave a mark
//!
//! **D-50 / [ADR-0029]: a reply is to a comment, never to a reply.** #253 AC1
//! asks that the depth be decided rather than fall out of a nullable column,
//! and one level is the decision: a conversation about a document is read top
//! to bottom, and arbitrary depth buys a recursive read, a collapsing screen
//! and threads that drift off the point the document is about.
//! `service::add_comment` refuses the second hop, because *my parent has no
//! parent* is a fact about another row and no `CHECK` can read one.
//!
//! **D-51 / [ADR-0030]: deleting is soft, and it does not take the replies.**
//! They are other people's words. A deleted comment that still has undeleted
//! replies is served as a **tombstone** — author, time, no body — so the
//! answers under it still
//! have something to be answers to; one that nobody replied to is not served at
//! all. Both halves are decided in `repository::list_for_document`, the single
//! place every reader comes through, and `service::delete_comment` states the
//! argument.
//!
//! **An edit is visible as an edit** (#253 AC3). `comments.edited_at` is null
//! until the body changes and is not `updated_at`, which moves for any write to
//! the row — the delete included. The previous text is **not kept**: this module
//! stores no revisions, so what survives an edit is that it happened, when and
//! by whom.
//!
//! Both decisions carry their rejected alternatives in the records rather than
//! here — cascade, re-parent and hide for the delete; unbounded and bounded
//! depth for the thread — and both are the first taken *after*
//! `docs/architectures/adr/` existed rather than filed into it retrospectively.
//!
//! [ADR-0029]: ../../../../docs/architectures/adr/0029.%20A%20Comment%20Thread%20Is%20One%20Level%20Deep.md
//! [ADR-0030]: ../../../../docs/architectures/adr/0030.%20A%20Deleted%20Comment%20Leaves%20a%20Tombstone.md
//!
//! # What this module does not do yet
//!
//! **No resolving** (FR-CMT-005) and **no mentions** (FR-CMT-006), both `Could`.
//! `comments.status`, `resolved_by`, `resolved_at` and the two side tables
//! `comment_mentions` and `comment_attachments` exist and are written by
//! nothing; `0032_comment.sql` says so table by table, because a column that
//! exists is not a feature that exists.
//!
//! # Every write here lands on the timeline, in its own transaction
//!
//! `Comment.Added`, `Comment.Replied`, `Comment.Edited` and `Comment.Deleted`
//! ([#248](https://github.com/sujanto-gaws/kelir/issues/248)'s rule, #253 AC5).
//! Each is written by `activity::service::record` inside the transaction that
//! writes the row, so a timeline cannot claim something the conversation never
//! agreed to — that is what `record`'s signature is for, and it is the opposite
//! of the audit path's deliberate tolerance one call below it.
//!
//! **None of the four carries anything about the comment** — not the body, not
//! its length, not the old text on an edit. The entry says what happened to the
//! *document* and links to the comment, which is **D-45** and the only reason
//! the timeline needs no second permission.

pub mod domain;
pub mod handlers;
pub mod repository;
pub mod service;

/// What the audit trail calls a comment (naming convention §7).
pub const COMMENT_OBJECT_TYPE: &str = "COMMENT";

pub const COMMENT_CREATE: &str = "comment:create";
pub const COMMENT_READ: &str = "comment:read";
/// Editing one's **own** comment (FR-CMT-003). Never enough on its own —
/// `service::refuse_unless_author` is the other half.
pub const COMMENT_UPDATE: &str = "comment:update";
/// Deleting one's **own** comment (FR-CMT-004), softly. Same pairing.
pub const COMMENT_DELETE: &str = "comment:delete";
