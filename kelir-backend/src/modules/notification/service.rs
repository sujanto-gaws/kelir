//! Writing notifications, and reading your own (FR-NTF-001/002/003; [#251]).
//!
//! [#251]: https://github.com/sujanto-gaws/kelir/issues/251

use uuid::Uuid;

use super::domain::{Notification, NotificationType, UnreadCount};
use super::repository as repo;
use super::NOTIFICATION_READ;
use crate::error::AppError;
use crate::middleware::auth::Authenticated;
use crate::response::{PageMeta, Pagination};
use crate::state::AppState;

/// What a module hands over when somebody needs telling.
///
/// **Nothing here is looked up**, which is `activity::service::Happening`'s rule
/// and holds for the same reason: the caller has the recipient, the subject and
/// the transaction already, and a service that went back to the database to
/// enrich a notification would be one that can fail *after* the action it
/// announces has been decided.
pub struct Telling<'a> {
    pub tenant_id: Uuid,
    pub recipient_user_id: Uuid,
    pub document_id: Option<Uuid>,
    pub workflow_instance_id: Option<Uuid>,
    pub task_id: Option<Uuid>,
    pub notification_type: NotificationType,
    /// One line. The centre shows it in a list, so it is a subject and not a
    /// sentence.
    pub title: &'a str,
    /// What happened, in the product's own words.
    pub body: &'a str,
    pub actor: Option<Uuid>,
}

/// Tells one person one thing, **in the caller's transaction** (#251 AC3).
///
/// # The same signature as `activity::record`, and the second half of the reason
///
/// An activity event must not outlive the action it describes. A notification
/// must not either — *your approval was rejected* about an approval that rolled
/// back is worse than silence, because the person acts on it. **And it must not
/// be lost when the action commits**, which is the half `modules::audit` gets to
/// give up: `record_or_warn` swallows its own failure deliberately, because an
/// audit row is a control and a missing one is a gap somebody can find. A
/// missing notification is a person who never heard, and nothing anywhere
/// records that they did not.
///
/// So this returns its error and the caller's transaction carries it. **A
/// failure here fails the action**, and that is the trade: an approval that
/// cannot tell anybody it happened is refused rather than completed in silence.
///
/// **The alternative, considered and rejected:** an outbox row and a worker.
/// That is the right shape for email (#257), where delivery is somebody else's
/// network and retry is the whole problem. In-app delivery *is* this insert —
/// there is nothing after it to retry — so an outbox would add a queue between
/// a write and the same write.
pub async fn notify(
    transaction: &mut sqlx::PgTransaction<'_>,
    telling: &Telling<'_>,
) -> Result<(), AppError> {
    repo::insert(
        &mut **transaction,
        &repo::NewNotification {
            tenant_id: telling.tenant_id,
            recipient_user_id: telling.recipient_user_id,
            document_id: telling.document_id,
            workflow_instance_id: telling.workflow_instance_id,
            task_id: telling.task_id,
            notification_type: telling.notification_type,
            title: telling.title,
            body: telling.body,
            actor: telling.actor,
        },
    )
    .await?;

    Ok(())
}

/// Everybody a role task has reached (**D-48**).
///
/// # A role task reaches a set of people, and it tells all of them
///
/// #251 AC2 says *a task reaching somebody produces a notification for them*,
/// and a `ROLE` or `DEPARTMENT_ROLE` task reaches whoever holds the role — the
/// inbox offers it to all of them, and any one of them may claim it. So this
/// fans out, one row per holder.
///
/// **The alternative was to notify nobody until the task is claimed**, and it
/// guts the feature: role assignment is the commonest approval shape in this
/// product's own fixtures, so the release would ship a notification centre that
/// stays empty for the case it was built for. A claim is somebody having
/// *already* noticed.
///
/// **Rows go stale, and that is what a notification is.** Once one holder
/// claims the task, the other notifications describe something no longer
/// waiting for them — but they were true when written, and this is a record of
/// what reached you rather than a live to-do list. The **inbox** is the live
/// view, and it is one click away. Retracting them would mean deleting rows
/// from under a person who is reading them, and would make *what was I told*
/// unanswerable.
///
/// **Unbounded, deliberately and with the limit named.** A role held by two
/// hundred people makes two hundred rows for one task. That is correct — all
/// two hundred can act on it — and it is also the shape that would hurt first
/// at scale. FR-NTF-005 (preferences) is unscheduled and is where a person
/// turns this off; a cap here would silently pick which holders matter.
pub async fn role_recipients(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    role_id: Uuid,
    department: Option<Uuid>,
) -> Result<Vec<Uuid>, AppError> {
    Ok(repo::holders_of_role(&mut **transaction, tenant_id, role_id, department).await?)
}

/// One page of the caller's own notifications.
///
/// **Two independent rules, and neither stands in for the other.**
/// `notification:read` says whether this account has a notification centre;
/// `recipient_user_id = caller` says which rows are in it, and it lives in the
/// statement (#251 AC7). Dropping the permission would leave every account with
/// a centre; dropping the predicate would put everybody's notifications in
/// everybody's centre. **D-47** removed a permission that duplicated another
/// rule — this one duplicates nothing.
pub async fn list_mine(
    state: &AppState,
    caller: &Authenticated,
    pagination: &Pagination,
) -> Result<(Vec<Notification>, PageMeta), AppError> {
    caller.require(NOTIFICATION_READ)?;

    let tenant_id = caller.tenant_id();
    let user_id = caller.user_id();

    let total = repo::count_for_recipient(&state.pool, tenant_id, user_id).await?;
    let items = repo::list_for_recipient(
        &state.pool,
        tenant_id,
        user_id,
        pagination.limit(),
        pagination.offset(),
    )
    .await?;

    Ok((items, pagination.meta(total.max(0) as u64)))
}

/// How many are waiting for the caller — the badge.
pub async fn unread_count(
    state: &AppState,
    caller: &Authenticated,
) -> Result<UnreadCount, AppError> {
    caller.require(NOTIFICATION_READ)?;

    let unread = repo::count_unread(&state.pool, caller.tenant_id(), caller.user_id()).await?;

    Ok(UnreadCount { unread })
}

/// Marks one of the caller's notifications read (#251 AC5).
///
/// **404 for somebody else's**, not 403. The row exists and is not this
/// caller's, and *not found* is the answer that says nothing about whether it
/// exists — the same choice `attachment::service::download` makes and for the
/// same reason: a refusal that distinguishes *there is no such notification*
/// from *there is one and it is not yours* is an oracle for other people's ids.
pub async fn mark_read(state: &AppState, caller: &Authenticated, id: Uuid) -> Result<(), AppError> {
    caller.require(NOTIFICATION_READ)?;

    let found = repo::mark_read(&state.pool, caller.tenant_id(), caller.user_id(), id).await?;

    if !found {
        return Err(AppError::not_found("Notification"));
    }

    Ok(())
}

/// Marks everything waiting for the caller read, and says how many that was.
pub async fn mark_all_read(
    state: &AppState,
    caller: &Authenticated,
) -> Result<UnreadCount, AppError> {
    caller.require(NOTIFICATION_READ)?;

    repo::mark_all_read(&state.pool, caller.tenant_id(), caller.user_id()).await?;

    // **What is left, not what was cleared.** A client updates a badge from
    // this, and a badge showing the number it just dismissed is worse than one
    // showing nothing. It is `0` unless something arrived mid-request, which is
    // the honest answer to *how many are waiting now*.
    let unread = repo::count_unread(&state.pool, caller.tenant_id(), caller.user_id()).await?;

    Ok(UnreadCount { unread })
}
