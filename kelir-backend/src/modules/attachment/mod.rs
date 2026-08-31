//! Attachments — the files a document is about (SRS §4.10, FR-ATT-*).
//!
//! # Two permissions, and what each one answers
//!
//! * `attachment:create` — *may this account attach files*. Checked with
//!   `document:read`, never instead of it: an attachment is as private as the
//!   document it hangs on, so attaching to a document you cannot see is refused
//!   by the document's own answer rather than by a rule written here.
//! * `attachment:read` — *may this account retrieve them*. Seeded by
//!   `0031_attachment.sql` and **checked by nothing yet**: the download is
//!   [#245](https://github.com/sujanto-gaws/kelir/issues/245), this sprint's
//!   item 2. It is seeded now because both belong to one migration and a second
//!   migration to add one permission row would be a migration for a `VALUES`
//!   line.
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
//! `PENDING` default. The gate is [#246](https://github.com/sujanto-gaws/kelir/issues/246),
//! and until it lands **an attachment is stored and cannot be retrieved** —
//! which is the right order to build the two in, because the alternative is a
//! download that works before anything checks the file.
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
pub mod service;
pub mod storage;

/// What the audit trail calls an attachment (naming convention §7).
pub const ATTACHMENT_OBJECT_TYPE: &str = "ATTACHMENT";

pub const ATTACHMENT_CREATE: &str = "attachment:create";
/// Seeded by `0031_attachment.sql`, checked by #245. See the module note.
#[allow(
    dead_code,
    reason = "the download that checks it is #245, this sprint's item 2"
)]
pub const ATTACHMENT_READ: &str = "attachment:read";
