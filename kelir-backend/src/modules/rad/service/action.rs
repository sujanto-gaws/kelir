//! Which configured actions a caller is offered ([#340]).
//!
//! **One rule, and it is the whole of this file:**
//!
//! > An action the caller could not invoke is not returned.
//!
//! `rad_actions.required_permission` is a column, so the question already has a
//! per-row answer and this is where it is asked. What it is *not* is a gate on
//! the catalogue: there is no `rad:action:read`, because a permission guarding
//! nothing the row's own permission does not guard is one permission too many
//! ([ADR-0011](../../../../../docs/architectures/adr/0011.%20A%20Derived%20Surface%20Requires%20the%20Permission%20of%20What%20It%20Derives%20From.md)'s
//! converse), and because a deployment could then grant the button to somebody
//! the button refuses.
//!
//! **Filtered rather than flagged.** The alternative — return everything and
//! let the renderer disable what the caller may not use — publishes the
//! existence of every configured action to everybody, which is exactly what
//! `required_permission` was set to prevent, and puts a copy of the rule in a
//! client where it can drift. [`Authenticated::holds`] is the right tool here
//! and says so in its own doc: this is a surface that serves *less*, not one
//! that skips a check.
//!
//! [#340]: https://github.com/sujanto-gaws/kelir/issues/340

use super::super::domain::action::{Action, ActionContext};
use super::super::repository::action as repo;
use crate::error::AppError;
use crate::middleware::auth::Authenticated;
use crate::state::AppState;

/// The actions this caller may invoke in `context`.
pub async fn list_actions(
    state: &AppState,
    caller: &Authenticated,
    context: ActionContext,
) -> Result<Vec<Action>, AppError> {
    let stored = repo::actions_for(&state.pool, caller.tenant_id(), context).await?;

    Ok(stored
        .into_iter()
        .filter(|row| match &row.required_permission {
            // A blank string is not "no permission" — it is a configuration
            // mistake that would otherwise be compared against the caller's
            // held permissions and never match, hiding the action from
            // everyone including an administrator. Treated as unset, which is
            // the reading `VARCHAR(64)` with no `NOT NULL` invites and the only
            // one that does not silently retire a row.
            Some(permission) if !permission.trim().is_empty() => caller.holds(permission.trim()),
            _ => true,
        })
        .map(|row| row.action)
        .collect())
}
