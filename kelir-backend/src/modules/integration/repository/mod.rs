//! Queries for `external_systems`, `integration_endpoints` and
//! `integration_credentials` (Database Schema §12.1–§12.3).
//!
//! **Every statement filters `tenant_id`**, and every read of a child row
//! filters its `external_system_id` as well, so an endpoint or a credential is
//! found only under the system it belongs to. `0046_integration.sql` makes a
//! cross-tenant child row unwritable as well (its composite keys); the
//! predicates here are what make one unreadable, and neither depends on the
//! other.
//!
//! Soft-deleted rows are excluded everywhere. Only a credential is ever
//! soft-deleted by this module: a system is deactivated, and an endpoint set
//! `INACTIVE`, instead.

pub mod credential;
pub mod endpoint;
pub mod external_system;
pub mod log;

/// The escaping rule every searchable list shares (`utils::search`, #525).
use crate::utils::search::like_contains;

/// `(was it sent, the value)` for a nullable column an update may set, leave
/// or clear.
fn split<T>(field: Option<Option<T>>) -> (bool, Option<T>) {
    match field {
        None => (false, None),
        Some(value) => (true, value),
    }
}
