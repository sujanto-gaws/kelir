//! Searching the trail (FR-AUD-004; [#252]).
//!
//! [#252]: https://github.com/sujanto-gaws/kelir/issues/252

use super::domain::{readable_by, AuditEvent, AuditSearch};
use super::repository as repo;
use super::AUDIT_READ;
use crate::error::{AppError, ValidationDetail};
use crate::middleware::auth::Authenticated;
use crate::response::{PageMeta, Pagination};
use crate::state::AppState;

/// One page of the audit trail, newest first.
///
/// # Two rules, and only the first one refuses
///
/// **`audit:read` opens the surface** — may this account ask who did what.
/// **The object's own read permission opens a row's values** — and a caller
/// without it gets the row with `valuesWithheld: true` rather than no row
/// (#252 AC2).
///
/// That split is **D-12** generalized, which is the whole of this item.
/// D-12 found `master-data:audit:read` handing back a party's field values
/// through its change history to a caller refused `GET /parties/{id}`, and
/// settled it by requiring the record's own read *alongside* the audit
/// permission. Here the same rule has to hold for nineteen object types at once,
/// and a search cannot take D-12's shape: requiring every one of them would mean
/// a compliance reviewer needs read on the whole product to search anything, and
/// requiring none would be the defect D-12 fixed, nineteen times over.
///
/// **So the row is the trail and the values are the object.** *Somebody updated
/// party X at 09:05* is what an audit trail is for and is not the party's
/// content; `{"statusId": "SUSPENDED"}` is.
///
/// # Why withholding beats hiding
///
/// A search that dropped the rows would be a search that lies about the shape
/// of the trail — an auditor counting events would count what they may read and
/// take it for what happened. #252 AC2 says so in as many words, and
/// `values_withheld` is what makes the difference legible rather than something
/// a reader has to infer from a null.
///
/// # What this surface does not do
///
/// **It does not verify the hash chain** (#252 AC5). Reading the trail and
/// proving it unbroken are different questions, and a search that implied the
/// second would be claiming something it has not checked — the chain is
/// verified by `tests/audit_hash_chain.rs` over the rows, and a caller-facing
/// verification endpoint is not this item.
pub async fn search_audit(
    state: &AppState,
    caller: &Authenticated,
    filter: &AuditSearch,
    pagination: &Pagination,
) -> Result<(Vec<AuditEvent>, PageMeta), AppError> {
    caller.require(AUDIT_READ)?;

    if !repo::range_is_ordered(filter.from, filter.to) {
        return Err(AppError::validation(vec![ValidationDetail::new(
            "to",
            "range",
            "RANGE_INVERTED",
            "`to` is before `from`, so this range selects nothing",
        )]));
    }

    let tenant_id = caller.tenant_id();

    let total = repo::count(&state.pool, tenant_id, filter).await?;
    let rows = repo::search(
        &state.pool,
        tenant_id,
        filter,
        pagination.limit(),
        pagination.offset(),
    )
    .await?;

    let events = rows
        .into_iter()
        .map(|row| redact_for(caller, row))
        .collect();

    Ok((events, pagination.meta(total.max(0) as u64)))
}

/// Strips a row's values when the caller may not read the object they describe.
///
/// **`holds` rather than `require`**, because this decides how much of a row to
/// serve and not whether to serve it. A `?` here would refuse the whole page
/// because one row happened to name an object the caller cannot read — which
/// would make the search's answer depend on what is in it.
///
/// **An object type with no entry withholds** ([`readable_by`] says why), so a
/// row written by a later release or a plugin is served as an event with no
/// contents rather than as contents nobody decided about.
fn redact_for(caller: &Authenticated, row: AuditEvent) -> AuditEvent {
    let may_read = readable_by(&row.object_type).is_some_and(|permission| caller.holds(permission));

    if may_read {
        return row;
    }

    AuditEvent {
        old_value: None,
        new_value: None,
        values_withheld: true,
        ..row
    }
}

/// The object types this tenant's trail actually holds, for a filter control.
pub async fn object_types(
    state: &AppState,
    caller: &Authenticated,
) -> Result<Vec<String>, AppError> {
    caller.require(AUDIT_READ)?;

    Ok(repo::object_types(&state.pool, caller.tenant_id()).await?)
}
