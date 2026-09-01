//! Attachments — the files a document is about (SRS §4.10, FR-ATT-*).
//!
//! # Two permissions, and what each one answers
//!
//! * `attachment:create` — *may this account attach files*. Checked with
//!   `document:read`, never instead of it: an attachment is as private as the
//!   document it hangs on, so attaching to a document you cannot see is refused
//!   by the document's own answer rather than by a rule written here.
//! * `attachment:read` — *may this account retrieve them*. Checked by the list
//!   and the download, both added by
//!   [#245](https://github.com/sujanto-gaws/kelir/issues/245).
//!
//! **The rule covers the name, and it took [#292] to make that true.** An
//! attachment being as private as its document was enforced here and nowhere
//! else, so the activity timeline — which asks for `activity:read` and the
//! document's read and no more — served every file's original name and size to
//! callers holding no `attachment:read` at all. A file name is routinely the
//! sensitive part. **D-45** took the detail out of the event and left the
//! `attachment_id` in its place, so the name comes from here or from nowhere.
//!
//! [#292]: https://github.com/sujanto-gaws/kelir/issues/292
//!
//! There is no `attachment:delete`. Soft-delete is FR-ATT-006 and Sprint 13, and
//! a permission row nothing checks is the `delegations` situation **D-13** spent
//! two decisions undoing.
//!
//! # Where the bytes are, and where they are not
//!
//! [`storage`] is the only thing in this codebase that writes an object, and
//! `attachments.storage_reference` is the only thing that says where one is. The
//! reference is **generated** and never taken from a request
//! ([#244](https://github.com/sujanto-gaws/kelir/issues/244) AC6), and it is not
//! serialized to any caller: knowing the object path buys nothing but the shape
//! of the bucket.
//!
//! # What this module does not do yet
//!
//! **Nothing scans**, and nothing sets `virus_scan_status` to anything but its
//! `PENDING` default. The scanner is
//! [#246](https://github.com/sujanto-gaws/kelir/issues/246).
//!
//! **The download-side gate, however, is already here**, and that is a
//! deliberate departure from the construction plan's item order. #246 AC2 and
//! AC4 — refused unless `CLEAN`, enforced where the bytes are served — landed
//! with [#245](https://github.com/sujanto-gaws/kelir/issues/245) rather than
//! after it, because this paragraph used to say *an attachment is stored and
//! cannot be retrieved* and a download shipped without the gate would have made
//! that false while serving every unscanned byte in the product. Since nothing
//! sets `CLEAN`, **every attachment is currently listed and none is
//! downloadable**, which is the state this module intends until the scanner
//! exists. #246 keeps the scanner, the status transitions, the once-only move
//! out of `INFECTED`, and the behaviour when the scanner is unreachable.
//!
//! **Nothing writes an activity event.** That is
//! [#248](https://github.com/sujanto-gaws/kelir/issues/248), this sprint's item
//! 5, and it lands after the surfaces it records. The audit row is written here,
//! because the audit trail is a control over the action rather than part of what
//! the action produced — the distinction `modules::activity` will have to state
//! in full.

pub mod domain;
pub mod handlers;
pub mod repository;
pub mod scanner;
pub mod service;
pub mod storage;
pub mod worker;

/// What the audit trail calls an attachment (naming convention §7).
pub const ATTACHMENT_OBJECT_TYPE: &str = "ATTACHMENT";

pub const ATTACHMENT_CREATE: &str = "attachment:create";
/// Seeded by `0031_attachment.sql`, checked by #245. See the module note.
#[allow(
    dead_code,
    reason = "the download that checks it is #245, this sprint's item 2"
)]
pub const ATTACHMENT_READ: &str = "attachment:read";
