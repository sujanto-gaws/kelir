//! Opening, listing and ending delegation windows (FR-IDM-006; [#184]).
//!
//! Owns the permission checks and the writes; the routing read those windows
//! feed is [`super::delegation_repository::active_delegate_of`], called from the
//! workflow module's assignment resolver.
//!
//! # Three permissions, and they are not the same question asked three times
//!
//! * `identity:delegation:create` — *may this account hand its own work over*.
//!   It is a permission over the caller's own approvals, which is why
//!   [`create_delegation`] takes no delegator: see
//!   [`super::delegation`], where the escalation that omission prevents is
//!   written down.
//! * `identity:delegation:read` — *may this account see the tenant's windows*.
//!   Administrative: the row somebody has to be able to find is the one whose
//!   owner went on leave without ending it.
//! * `identity:delegation:delete` — *may this account stop one*. Administrative
//!   for the same reason, and the reason `0005`'s catalogue has no
//!   `identity:delegation:update`: a window is opened or ended, never edited
//!   into a different window while approvals are being routed by it.
//!
//! [#184]: https://github.com/sujanto-gaws/kelir/issues/184

use chrono::Utc;
use uuid::Uuid;

use super::delegation::{validate_create, CreateDelegationRequest, Delegation};
use super::delegation_repository as repo;
use crate::error::{AppError, ValidationDetail};
use crate::middleware::auth::Authenticated;
use crate::modules::audit::{self, AuditEntry};
use crate::modules::document_type::repository as document_type_repo;
use crate::response::{PageMeta, Pagination};
use crate::state::AppState;

/// What the audit trail calls a delegation window (naming convention §7).
pub const DELEGATION_OBJECT_TYPE: &str = "DELEGATION";

pub const DELEGATION_CREATE: &str = "identity:delegation:create";
pub const DELEGATION_READ: &str = "identity:delegation:read";
pub const DELEGATION_DELETE: &str = "identity:delegation:delete";

pub async fn list_delegations(
    state: &AppState,
    caller: &Authenticated,
    pagination: &Pagination,
) -> Result<(Vec<Delegation>, PageMeta), AppError> {
    caller.require(DELEGATION_READ)?;

    let tenant_id = caller.tenant_id();
    let total = repo::count_delegations(&state.pool, tenant_id).await?;
    let delegations = repo::list_delegations(
        &state.pool,
        tenant_id,
        pagination.limit(),
        pagination.offset(),
    )
    .await?;

    Ok((delegations, pagination.meta(total.max(0) as u64)))
}

/// Opens a window in the caller's own name ([#184] AC1).
///
/// **Everything checkable is checked before the row is written**, and the two
/// database constraints stay where they are. `ck_delegations_not_self` and
/// `ck_delegations_window` have guarded this table since `0002` with nothing
/// writing to it; they are what makes the refusal true for a row this service
/// did not write, and the checks above are what turn them into a message naming
/// a field instead of a constraint name in a 500.
pub async fn create_delegation(
    state: &AppState,
    caller: &Authenticated,
    request: CreateDelegationRequest,
) -> Result<Delegation, AppError> {
    caller.require(DELEGATION_CREATE)?;

    let tenant_id = caller.tenant_id();
    let delegator = caller.user_id();

    let validated = validate_create(&request, delegator, Utc::now())?;

    // **`ACTIVE`, not merely present.** A window pointing at an account that
    // cannot sign in produces tasks that look assigned and that nobody can
    // reach — `delegation_repository::user_is_available` carries the reasoning,
    // and the same predicate is inside the routing statement so a window that
    // outlives its delegate's account stops rather than stalling work.
    if !repo::user_is_available(&state.pool, tenant_id, validated.delegate_user_id).await? {
        return Err(AppError::validation(vec![ValidationDetail::new(
            "delegateUserId",
            "exists",
            "NOT_AVAILABLE",
            "no active user with that id in this tenant; a delegation has to \
             reach somebody who can sign in and act",
        )]));
    }

    if let Some(document_type_id) = validated.document_type_id {
        // Read through the owning module's repository rather than a second
        // statement over its table (coding standard §2.2), for
        // `check_department`'s reason one field over: the foreign key would
        // otherwise answer a mistyped id with a constraint name.
        if document_type_repo::find_type(&state.pool, tenant_id, document_type_id)
            .await?
            .is_none()
        {
            return Err(AppError::validation(vec![ValidationDetail::new(
                "documentTypeId",
                "exists",
                "NOT_FOUND",
                "No document type with that id in this tenant",
            )]));
        }
    }

    let id = Uuid::now_v7();

    repo::insert_delegation(
        &state.pool,
        &repo::NewDelegation {
            id,
            tenant_id,
            delegator_user_id: delegator,
            delegate_user_id: validated.delegate_user_id,
            scope: validated.scope,
            document_type_id: validated.document_type_id,
            starts_at: validated.starts_at,
            ends_at: validated.ends_at,
            reason: validated.reason.as_deref(),
        },
    )
    .await
    .map_err(constraint_to_refusal)?;

    let created = load(state, tenant_id, id).await?;

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "Identity.DelegationOpened",
            action: "CREATE",
            object_type: DELEGATION_OBJECT_TYPE,
            object_id: id,
            actor_user_id: Some(delegator),
            ip_address: None,
            // **The window's own `reason` is the audit reason**, and it is one of
            // the few places the two are the same field rather than a copy of
            // something private. A delegation's reason is "annual leave" — it is
            // written to explain the arrangement to whoever finds it later,
            // which is exactly who reads an audit row. That is the opposite of a
            // decision comment, which **D-12** and **D-32** keep out of this
            // trail because it is prose about somebody else's document.
            reason: created.reason.as_deref(),
            old_value: None,
            new_value: Some(serde_json::json!({
                "delegatorUserId": created.delegator_user_id,
                "delegateUserId": created.delegate_user_id,
                "scope": created.scope,
                "documentTypeId": created.document_type_id,
                "startsAt": created.starts_at,
                "endsAt": created.ends_at,
            })),
        },
    )
    .await;

    Ok(created)
}

/// Ends a window ([#184] AC6).
///
/// **Audited with both halves**, because this is the write whose absence is the
/// finding: a delegation that was ended and a delegation that expired route
/// nothing in exactly the same way, and only this record says which happened and
/// who decided it.
pub async fn end_delegation(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
) -> Result<(), AppError> {
    caller.require(DELEGATION_DELETE)?;

    let tenant_id = caller.tenant_id();

    let before = repo::find_delegation(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::not_found("Delegation"))?;

    if repo::end_delegation(&state.pool, tenant_id, id, caller.user_id()).await? == 0 {
        return Err(AppError::not_found("Delegation"));
    }

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "Identity.DelegationEnded",
            action: "DELETE",
            object_type: DELEGATION_OBJECT_TYPE,
            object_id: id,
            actor_user_id: Some(caller.user_id()),
            ip_address: None,
            reason: None,
            old_value: Some(serde_json::json!({
                "isActive": before.is_active,
                "isRouting": before.is_routing,
            })),
            new_value: Some(serde_json::json!({
                "isActive": false,
                "isRouting": false,
            })),
        },
    )
    .await;

    Ok(())
}

async fn load(state: &AppState, tenant_id: Uuid, id: Uuid) -> Result<Delegation, AppError> {
    repo::find_delegation(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::Internal {
            source: anyhow::anyhow!("delegation {id} vanished after it was written"),
        })
}

/// Turns `0002`'s two check constraints into the refusals they were written to
/// be.
///
/// Unreachable through this service — `domain::delegation::validate_create`
/// refuses both shapes first, naming the field. Mapped anyway, because
/// "unreachable" is a claim about the code that exists today: the constraints
/// outlived four sprints with no writer at all, and the next writer of this
/// table should get a 422 rather than a 500 with a constraint name in it.
fn constraint_to_refusal(error: sqlx::Error) -> AppError {
    let sqlx::Error::Database(database) = &error else {
        return error.into();
    };

    match database.constraint() {
        Some("ck_delegations_not_self") => AppError::validation(vec![ValidationDetail::new(
            "delegateUserId",
            "notSelf",
            "DELEGATE_IS_DELEGATOR",
            "a delegation hands work to somebody else",
        )]),
        Some("ck_delegations_window") => AppError::validation(vec![ValidationDetail::new(
            "endsAt",
            "window",
            "WINDOW_INVERTED",
            "a delegation window ends after it starts",
        )]),
        _ => error.into(),
    }
}
