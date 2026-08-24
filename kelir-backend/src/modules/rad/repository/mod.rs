//! Queries for the RAD metadata tables (§5).
//!
//! Two conventions hold across every statement in here and are stated once:
//!
//! - **Tenant scoping.** Every statement filters `tenant_id`, taken from the
//!   caller's claims rather than from the request. A read that forgets it
//!   returns another tenant's configuration.
//! - **Soft delete.** Every read filters `deleted_at IS NULL`, and a delete is
//!   an update. A read that forgets it puts a retired definition in front of a
//!   renderer.

pub mod form;
pub mod list;
