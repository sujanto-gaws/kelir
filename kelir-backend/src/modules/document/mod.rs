//! Documents — the thing the platform is about (FR-DOC-*).
//!
//! A document is created from a [document type][super::document_type], renders
//! that type's bound form, holds the data somebody filled into it, takes a
//! number when it is submitted, and moves through its own statuses until it is
//! finished. Sprint 9 builds all of that; the workflow that will *drive* the
//! statuses is Phase 5 and is deliberately absent (see [`domain::status`]).
//!
//! Storage is [Database Schema](../../../../docs/design/02.%20Database%20Schema.md)
//! §6.6–§6.10, created by `0015_document.sql` and given its permissions by
//! `0023_document.sql`.
//!
//! # Two status concepts exist in this codebase and this module owns one of them
//!
//! **`record_status` answers "how far has this record got through governance".**
//! A master-data record — a supplier, a facility — is drafted, approved, made
//! active, suspended, archived (FR-MDM-007, [#99]). It is a property of a record
//! *about a real-world thing that keeps existing*, and the question it answers
//! is whether the business currently stands behind that record.
//!
//! **`documents.status` answers "where is this document in its own life".** A
//! document is an *event*: it is drafted, submitted, decided, and then it is
//! history. It has no governance lifecycle because there is no ongoing thing for
//! governance to be about — a purchase requisition is not suspended, it is
//! approved or it is not.
//!
//! A row in `documents` therefore has one and only one lifecycle and it is this
//! one. **Nothing in this module reads or writes `record_status`, and nothing in
//! [`super::master_data`] reads or writes `documents.status`.** Two status
//! concepts on one row is how a codebase gets a field nobody can explain
//! ([#169]), and the way that is prevented is by each one saying which question
//! it answers where a reader will find it.
//!
//! [#99]: https://github.com/sujanto-gaws/kelir/issues/99
//! [#169]: https://github.com/sujanto-gaws/kelir/issues/169

pub mod domain;
pub mod handlers;
pub mod repository;
pub mod service;

/// The permissions `0023_document.sql` seeds, as constants rather than
/// literals — a typo in a permission string is a permission nobody holds, which
/// reads as a working check that refuses everybody (#58).
///
/// The resource segment is omitted because this module manages one resource
/// (naming convention §6). Metadata, versions, relations and the entity link
/// are parts of a document rather than resources beside it, so none of them has
/// a permission of its own.
pub const DOCUMENT_CREATE: &str = "document:create";
pub const DOCUMENT_READ: &str = "document:read";
pub const DOCUMENT_UPDATE: &str = "document:update";
pub const DOCUMENT_DELETE: &str = "document:delete";

/// Submitting is not updating.
///
/// It takes a number the document keeps forever and starts a life a workflow
/// will later drive. A deployment that lets a clerk correct a requisition's
/// line items has not thereby decided that the clerk may commit it.
pub const DOCUMENT_SUBMIT: &str = "document:submit";

/// Transitioning is not updating either.
///
/// #99's AC1, restated for documents: a transition has a from-state, a legal
/// set, its own audit action and its own consequences. Putting it behind the
/// update permission would put approval behind typing.
pub const DOCUMENT_TRANSITION: &str = "document:transition";

/// What the audit trail calls a document (naming convention §7).
pub const OBJECT_TYPE: &str = "DOCUMENT";
