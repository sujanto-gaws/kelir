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
//! # What is not here
//!
//! **No timeline screen.** FR-ACT-005 is Sprint 13. This release writes the
//! events and serves them; a screen over one source would have been worth less
//! than the events themselves, which is why the sprint plan separated them.
//!
//! **No attachment or comment events.** They are
//! [#248](https://github.com/sujanto-gaws/kelir/issues/248), the next item, and
//! they land after the surfaces they describe rather than before them. This
//! module writes the document lifecycle and the workflow actions.
//!
//! **No `ip_address`.** The column exists because §10.1 declares it, and it
//! stays null: a timeline does not show an address. FR-AUD-005 is about the
//! *audit* row, and **D-44** carries it into #248.

pub mod domain;
pub mod handlers;
pub mod repository;
pub mod service;

pub const ACTIVITY_READ: &str = "activity:read";
