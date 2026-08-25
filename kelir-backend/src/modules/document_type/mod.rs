//! Document types — the configuration that makes a document renderable,
//! routable and numbered (FR-DTYPE-*).
//!
//! Sprint 7 builds the type and its bindings (#157) and its numbering rule
//! (#158). Documents themselves are Sprint 9 under decision **D-16**.

pub mod domain;
pub mod handlers;
pub mod repository;
pub mod service;

/// The permissions `0015_document.sql` seeds, as constants rather than
/// literals — a typo in a permission string is a permission nobody holds,
/// which reads as a working check that refuses everybody (#58).
///
/// The resource segment is omitted because this module manages one resource
/// (naming convention §6); the numbering rule and the workflow bindings are
/// parts of a type, not resources beside it.
pub const TYPE_CREATE: &str = "document-type:create";
pub const TYPE_READ: &str = "document-type:read";
pub const TYPE_UPDATE: &str = "document-type:update";
pub const TYPE_DELETE: &str = "document-type:delete";
