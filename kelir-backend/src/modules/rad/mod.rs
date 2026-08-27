//! RAD — the metadata layer that makes a document type configurable rather
//! than coded (FR-RAD-*).
//!
//! Phase 4 carries the front half of it (decision **D-2**): the JSON Logic
//! evaluator here, the metadata tables (`0014_rad.sql`), and the storage APIs
//! over form and list definitions. The builder UIs and the rule engines around
//! the evaluator stay in Sprints 14–16.

pub mod domain;
pub mod evaluator;
pub mod handlers;
pub mod repository;
pub mod service;

/// The permissions `0014_rad.sql` seeds, as constants rather than literals.
///
/// A permission string is compared against a stored catalogue row, so a typo in
/// one is a permission nobody holds — which reads as a working authorization
/// check that refuses everybody, and is the shape #58 took. Naming them once
/// means the typo is a compile error.
pub const FORM_CREATE: &str = "rad:form:create";
pub const FORM_READ: &str = "rad:form:read";
pub const FORM_UPDATE: &str = "rad:form:update";
pub const FORM_PUBLISH: &str = "rad:form:publish";
pub const FORM_DELETE: &str = "rad:form:delete";
/// Filling in a published form and recording the submission ([#164]).
///
/// **Distinct from [`FORM_READ`] on purpose.** Opening a requisition to read it
/// and raising one are different questions, and a deployment that wants them to
/// be the same person grants both — the same shape [`FORM_PUBLISH`] has beside
/// [`FORM_UPDATE`].
///
/// [#164]: https://github.com/sujanto-gaws/kelir/issues/164
pub const FORM_SUBMIT: &str = "rad:form:submit";
pub const LIST_CREATE: &str = "rad:list:create";
pub const LIST_READ: &str = "rad:list:read";
pub const LIST_UPDATE: &str = "rad:list:update";
pub const LIST_DELETE: &str = "rad:list:delete";
