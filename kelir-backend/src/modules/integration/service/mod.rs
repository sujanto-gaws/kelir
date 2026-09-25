//! Use cases over the external system registry (FR-INT-001).
//!
//! **Permission first, then existence** — every function calls
//! `caller.require` before it reads a row, so a caller without the permission
//! gets 403 whether or not the id exists, and a caller with it gets 404 for an
//! id that is not a live row in their own tenant (`Authenticated::require`
//! states why: an authenticated caller learns nothing useful from a 404 they
//! were not entitled to anyway).
//!
//! **A child route checks its system before its child.** An endpoint or a
//! credential under a system that does not exist in this tenant answers
//! `External system not found`, not `Endpoint not found`, so a caller is told
//! which half of the path was wrong.
//!
//! **Audit, not activity.** These are configuration changes, and coding
//! standard §2.8 puts configuration changes in the hash-chained audit trail;
//! the activity timeline is a document's, and nothing here is on one.

pub mod credential;
pub mod endpoint;
pub mod external_system;

use uuid::Uuid;

use super::repository::external_system as system_repo;
use crate::error::AppError;
use crate::state::AppState;

/// The 404 every route under `/external-systems/{id}` gives for an `id` that is
/// not a live system in the caller's tenant.
fn system_not_found() -> AppError {
    AppError::not_found("External system")
}

/// Refuses a child route whose system is not a live row in this tenant.
async fn require_system(state: &AppState, tenant_id: Uuid, id: Uuid) -> Result<(), AppError> {
    if system_repo::external_system_exists(&state.pool, tenant_id, id).await? {
        Ok(())
    } else {
        Err(system_not_found())
    }
}

/// A unique-index violation, as the 409 a caller can act on; anything else
/// passes through.
fn duplicate_to_conflict(error: sqlx::Error, message: &str) -> AppError {
    match &error {
        sqlx::Error::Database(database) if database.is_unique_violation() => {
            AppError::conflict(message)
        }
        _ => error.into(),
    }
}

/// A search term with its whitespace trimmed; blank is no search.
fn search_term(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}
