//! Commenting on a document, and reading what has been said (FR-CMT-001;
//! [#249]).
//!
//! [#249]: https://github.com/sujanto-gaws/kelir/issues/249

use serde_json::json;
use uuid::Uuid;

use super::domain::{self, AddCommentRequest, Comment};
use super::repository as repo;
use super::{COMMENT_CREATE, COMMENT_OBJECT_TYPE, COMMENT_READ};
use crate::error::AppError;
use crate::middleware::auth::Authenticated;
use crate::modules::audit::{self, AuditEntry};
use crate::modules::document::service::document as document_service;
use crate::response::{PageMeta, Pagination};
use crate::state::AppState;

/// Adds a comment to a document ([#249] AC2).
///
/// **Two permissions, and the document's own read is the one that scopes it.**
/// `comment:create` is *may this account comment at all*; whether it may comment
/// on *this* document is the document's question, answered by its own module's
/// service (coding standard §2.2) — which requires `document:read` and answers
/// 404 for a document that is not this tenant's, is deleted, or does not exist.
/// A comment is as private as the document it is about, and reusing that answer
/// is what keeps the two from drifting.
///
/// # No activity event yet, and #249 AC6 is where it will come from
///
/// AC6 asks that adding a comment write an activity event, in the same
/// transaction. **`activity_events` does not exist in this release**: it is
/// `0033_activity.sql`, item 4, and the events for comments and attachments are
/// [#248](https://github.com/sujanto-gaws/kelir/issues/248), item 5. The
/// [construction plan](../../../../projects/planning/07.%20Sprint%2012%20Collaboration%20Construction%20Plan.md)
/// §6 sequences item 5 last for exactly this reason — an event writer that lands
/// before its subjects is a writer with nothing to write — so **AC6 is
/// discharged by #248 rather than here**, and this paragraph is that fact
/// written down rather than an omission somebody has to notice.
///
/// The audit row *is* written here, because the audit trail is a control over
/// the action rather than part of what the action produced.
pub async fn add_comment(
    state: &AppState,
    caller: &Authenticated,
    document_id: Uuid,
    request: AddCommentRequest,
) -> Result<Comment, AppError> {
    caller.require(COMMENT_CREATE)?;

    // Normalized before the document is read, for `decide`'s reason one module
    // over: a body of four thousand and one characters is too long whatever the
    // document turns out to be, and finding that out after a query is a query
    // spent on a request that was never going to commit.
    let body = domain::normalize_body(request.body)?;

    let document = document_service::get_document(state, caller, document_id).await?;

    let tenant_id = caller.tenant_id();
    let actor = caller.user_id();
    let id = Uuid::now_v7();

    repo::insert_comment(
        &state.pool,
        &repo::NewComment {
            id,
            tenant_id,
            document_id: document.id,
            body: &body,
            created_by: Some(actor),
        },
    )
    .await?;

    let stored = repo::find_comment(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::Internal {
            source: anyhow::anyhow!("comment {id} vanished after it was written"),
        })?;

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "Comment.Added",
            action: "CREATE",
            object_type: COMMENT_OBJECT_TYPE,
            object_id: id,
            actor_user_id: Some(actor),
            ip_address: None,
            // **Not the body.** This trail is read through
            // `master-data:audit:read` by people who hold no permission over the
            // document, and a comment is prose about somebody else's work —
            // **D-12** and **D-32**'s line, and the same one `decide` and the
            // task hand-off both draw. That the comment exists is auditable;
            // what it says is behind `comment:read`.
            reason: None,
            old_value: None,
            new_value: Some(json!({
                "documentId": document.id,
                "length": stored.body.chars().count(),
            })),
        },
    )
    .await;

    Ok(stored)
}

/// A document's conversation, oldest first.
///
/// Scoped the same way the write is: `comment:read` says whether this account
/// reads comments, and the document's own read says which document's.
pub async fn list_comments(
    state: &AppState,
    caller: &Authenticated,
    document_id: Uuid,
    pagination: &Pagination,
) -> Result<(Vec<Comment>, PageMeta), AppError> {
    caller.require(COMMENT_READ)?;

    let document = document_service::get_document(state, caller, document_id).await?;
    let tenant_id = caller.tenant_id();

    let total = repo::count_for_document(&state.pool, tenant_id, document.id).await?;
    let comments = repo::list_for_document(
        &state.pool,
        tenant_id,
        document.id,
        pagination.limit(),
        pagination.offset(),
    )
    .await?;

    Ok((comments, pagination.meta(total.max(0) as u64)))
}
