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
//! There is no `comment:update` and no `comment:delete`. Editing and deleting
//! are FR-CMT-003 and Sprint 13, and a permission row nothing checks is the
//! `delegations` situation **D-13** spent two decisions undoing.
//!
//! # What this module does not do yet
//!
//! **No threading** (FR-CMT-002), **no editing or deleting** (FR-CMT-003), **no
//! resolving** (FR-CMT-004), **no mentions** (FR-CMT-005/006) — all Sprint 13.
//! `comments.parent_comment_id`, `comments.status` and both side tables exist
//! and are written by nothing; `0032_comment.sql` says which sprint fills each,
//! because a column that exists is not a feature that exists.
//!
//! **No activity event.** [#249](https://github.com/sujanto-gaws/kelir/issues/249)
//! AC6 asks for one in the same transaction as the comment, and
//! `activity_events` does not exist in this release — it is item 4, and the
//! events for comments and attachments are
//! [#248](https://github.com/sujanto-gaws/kelir/issues/248), item 5. AC6 is
//! **discharged by #248**, which the construction plan §6 sequences last so that
//! an event writer lands after its subjects rather than before them.

pub mod domain;
pub mod handlers;
pub mod repository;
pub mod service;

/// What the audit trail calls a comment (naming convention §7).
pub const COMMENT_OBJECT_TYPE: &str = "COMMENT";

pub const COMMENT_CREATE: &str = "comment:create";
pub const COMMENT_READ: &str = "comment:read";
