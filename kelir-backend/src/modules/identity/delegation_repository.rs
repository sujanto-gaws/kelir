//! Queries over `delegations` (§3.8).
//!
//! # The routing read lives here and is called from the workflow module
//!
//! [`active_delegate_of`] is the statement
//! [`crate::modules::workflow::service::assignment`] runs at its seam, and it is
//! **in the identity module** because `delegations` is the identity module's
//! table — coding standard §2.2. A resolver that wrote its own predicate would
//! be a second definition of *is this window open*, and the two would disagree
//! about the row somebody is relying on the day they differ. It is the
//! arrangement `identity::service` already uses in the other direction, reaching
//! `organization::department_repository` to check a department before writing a
//! user.
//!
//! It takes an executor rather than a pool for the same reason
//! `workflow::repository::history::record` does: it runs **inside the
//! transition's transaction**, so the window it reads is the window that was
//! open when the task was written, and coding standard §2.5's one-pooled-
//! connection rule is kept by there being nothing here to open a second one
//! with.

use chrono::{DateTime, Utc};
use sqlx::{PgExecutor, PgPool};
use uuid::Uuid;

use super::delegation::{Delegation, DelegationScope};

/// A window, as the create statement writes it.
pub struct NewDelegation<'a> {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub delegator_user_id: Uuid,
    pub delegate_user_id: Uuid,
    pub scope: DelegationScope,
    pub document_type_id: Option<Uuid>,
    pub starts_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
    pub reason: Option<&'a str>,
}

pub async fn insert_delegation(
    executor: impl PgExecutor<'_>,
    delegation: &NewDelegation<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO delegations
            (id, tenant_id, created_by, delegator_user_id, delegate_user_id,
             scope, document_type_id, starts_at, ends_at, reason)
        VALUES ($1, $2, $3, $3, $4, $5, $6, $7, $8, $9)
        "#,
        delegation.id,
        delegation.tenant_id,
        delegation.delegator_user_id,
        delegation.delegate_user_id,
        delegation.scope.as_db(),
        delegation.document_type_id,
        delegation.starts_at,
        delegation.ends_at,
        delegation.reason,
    )
    .execute(executor)
    .await
    .map(|_| ())
}

/// The tenant's windows, newest first.
///
/// **Newest first rather than by `starts_at`.** The question this list is opened
/// with is *what has been set up*, and a window created this morning for next
/// month belongs at the top of that answer; ordering by the window's own start
/// would bury it behind cover that is already over.
///
/// `is_routing` is computed here, in the same statement, against the same `now()`
/// the resolver uses. Deriving it in Rust from the three columns would be a
/// second definition of *open* beside [`active_delegate_of`]'s — the thing this
/// module is arranged to avoid.
pub async fn list_delegations(
    pool: &PgPool,
    tenant_id: Uuid,
    limit: i64,
    offset: i64,
) -> Result<Vec<Delegation>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT d.id, d.delegator_user_id,
               dor.display_name AS "delegator_display_name!",
               d.delegate_user_id,
               dee.display_name AS "delegate_display_name!",
               d.scope, d.document_type_id, d.starts_at, d.ends_at, d.reason,
               d.is_active,
               (d.is_active AND d.starts_at <= now() AND now() < d.ends_at) AS "is_routing!",
               d.created_at
        FROM delegations d
        JOIN users dor ON dor.id = d.delegator_user_id
        JOIN users dee ON dee.id = d.delegate_user_id
        WHERE d.tenant_id = $1 AND d.deleted_at IS NULL
        ORDER BY d.created_at DESC, d.id DESC
        LIMIT $2 OFFSET $3
        "#,
        tenant_id,
        limit,
        offset
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| Delegation {
            id: row.id,
            delegator_user_id: row.delegator_user_id,
            delegator_display_name: row.delegator_display_name,
            delegate_user_id: row.delegate_user_id,
            delegate_display_name: row.delegate_display_name,
            scope: DelegationScope::from_db(&row.scope),
            document_type_id: row.document_type_id,
            starts_at: row.starts_at,
            ends_at: row.ends_at,
            reason: row.reason,
            is_active: row.is_active,
            is_routing: row.is_routing,
            created_at: row.created_at,
        })
        .collect())
}

pub async fn count_delegations(pool: &PgPool, tenant_id: Uuid) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        SELECT count(*) AS "count!"
        FROM delegations
        WHERE tenant_id = $1 AND deleted_at IS NULL
        "#,
        tenant_id
    )
    .fetch_one(pool)
    .await
}

/// One window by id.
pub async fn find_delegation(
    pool: &PgPool,
    tenant_id: Uuid,
    id: Uuid,
) -> Result<Option<Delegation>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT d.id, d.delegator_user_id,
               dor.display_name AS "delegator_display_name!",
               d.delegate_user_id,
               dee.display_name AS "delegate_display_name!",
               d.scope, d.document_type_id, d.starts_at, d.ends_at, d.reason,
               d.is_active,
               (d.is_active AND d.starts_at <= now() AND now() < d.ends_at) AS "is_routing!",
               d.created_at
        FROM delegations d
        JOIN users dor ON dor.id = d.delegator_user_id
        JOIN users dee ON dee.id = d.delegate_user_id
        WHERE d.tenant_id = $1 AND d.id = $2 AND d.deleted_at IS NULL
        "#,
        tenant_id,
        id
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|row| Delegation {
        id: row.id,
        delegator_user_id: row.delegator_user_id,
        delegator_display_name: row.delegator_display_name,
        delegate_user_id: row.delegate_user_id,
        delegate_display_name: row.delegate_display_name,
        scope: DelegationScope::from_db(&row.scope),
        document_type_id: row.document_type_id,
        starts_at: row.starts_at,
        ends_at: row.ends_at,
        reason: row.reason,
        is_active: row.is_active,
        is_routing: row.is_routing,
        created_at: row.created_at,
    }))
}

/// Switches a window off (FR-IDM-006; [#184] AC6).
///
/// **`is_active = false` rather than a soft delete**, and the two are different
/// records. A window that covered somebody's leave is part of how a month of
/// approvals were routed; `deleted_at` would take it out of the list that
/// explains them. What ending it has to do is stop it routing, which this does
/// and which [`active_delegate_of`] reads in the same breath as the dates.
///
/// **Unconditional on the current value**, so ending a window that is already
/// ended is a no-op rather than a refusal. `DELETE` is the verb, and a caller
/// who sends it twice wanted the same end state both times; the row still exists
/// either way, so there is no second question to answer.
///
/// [#184]: https://github.com/sujanto-gaws/kelir/issues/184
pub async fn end_delegation(
    pool: &PgPool,
    tenant_id: Uuid,
    id: Uuid,
    actor: Uuid,
) -> Result<u64, sqlx::Error> {
    let affected = sqlx::query!(
        r#"
        UPDATE delegations SET
            is_active  = false,
            updated_by = $3,
            updated_at = now()
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        id,
        actor
    )
    .execute(pool)
    .await?
    .rows_affected();

    Ok(affected)
}

/// Who this delegator's work reaches right now, if anybody ([#184] AC2, AC6).
///
/// **The predicate, and every clause in it is load-bearing:**
///
/// * `is_active` and the two dates — a window that has been switched off or
///   whose end has passed stops routing in the same statement that would have
///   applied it, rather than at the next time somebody runs a sweep. AC6 asks
///   for immediately, and *immediately* is only true if nothing is cached
///   between the window and the task.
/// * The `users` join — a window pointing at somebody who has left, or whose
///   account has been deactivated, **does not match**, and routing falls back to
///   the delegator. Sending work to an account that cannot sign in is the one
///   outcome worse than not delegating it, and it would be invisible: the task
///   would look assigned. Deactivating an account is therefore also a way to
///   stop a window, which is the direction this should fail in.
/// * `scope` — `DOCUMENT_TYPE` matches only its own type. `$3` arrives `NULL`
///   from a caller that has no document in hand, and `document_type_id = NULL`
///   is `NULL` rather than true, so such a caller sees only `ALL` windows
///   without needing a second statement to say so.
/// * The ordering — **most specific first, then most recent**. Two windows can
///   legitimately overlap ("everything to Budi, but purchase requisitions to
///   Citra"), so there has to be an answer to which one applies, and it has to
///   be the same answer every time or a task's routing would depend on the plan
///   PostgreSQL happened to pick. Same scope, both open: the one opened last
///   wins, because it is the more recent instruction.
///
/// **One hop.** If the delegate has a window of their own, it is not followed.
/// A chain can be a cycle — `ck_delegations_not_self` forbids A → A and nothing
/// forbids A → B → A — and following it would make *on whose behalf* a list
/// rather than a name. Cover for a delegate who is themselves away is arranged
/// by ending the window or re-pointing it, which is a thing a person does and
/// can see.
pub async fn active_delegate_of<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    delegator_user_id: Uuid,
    document_type_id: Option<Uuid>,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        SELECT d.delegate_user_id AS "delegate_user_id!"
        FROM delegations d
        JOIN users u ON u.id = d.delegate_user_id
                    AND u.tenant_id = d.tenant_id
                    AND u.deleted_at IS NULL
                    AND u.status = 'ACTIVE'
        WHERE d.tenant_id = $1
          AND d.delegator_user_id = $2
          AND d.deleted_at IS NULL
          AND d.is_active
          AND d.starts_at <= now()
          AND now() < d.ends_at
          AND (d.scope = 'ALL'
               OR (d.scope = 'DOCUMENT_TYPE' AND d.document_type_id = $3))
        ORDER BY (d.scope = 'DOCUMENT_TYPE') DESC, d.created_at DESC, d.id DESC
        LIMIT 1
        "#,
        tenant_id,
        delegator_user_id,
        document_type_id
    )
    .fetch_optional(executor)
    .await
}

/// Whether this account can be handed work.
///
/// **Live and `ACTIVE`, which is stricter than "not deleted".** Every other
/// existence check in this codebase asks whether a row is there; this one asks
/// whether there is somebody at the other end of it, because what follows is
/// putting an approval in their hands. A deactivated account cannot sign in, so
/// a window or a hand-off pointing at one produces a task that looks assigned
/// and that nobody can reach.
///
/// The same predicate is inside [`active_delegate_of`]'s join, deliberately: one
/// is the check that refuses a window at the moment somebody opens it, and the
/// other is what stops an already-open window routing to an account that has
/// since been closed. Neither can stand in for the other, and they have to agree
/// about what "available" means.
///
/// Callable from the workflow module, which uses it for the same question one
/// route over — see this file's header on why the statement lives here.
pub async fn user_is_available<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    user_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let found = sqlx::query_scalar!(
        r#"
        SELECT 1 AS "found!"
        FROM users
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL AND status = 'ACTIVE'
        "#,
        tenant_id,
        user_id
    )
    .fetch_optional(executor)
    .await?;

    Ok(found.is_some())
}
