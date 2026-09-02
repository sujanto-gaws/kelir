//! Commenting on a document, replying, editing, deleting, and reading what has
//! been said (FR-CMT-001 to FR-CMT-004; [#249], [#253]).
//!
//! [#249]: https://github.com/sujanto-gaws/kelir/issues/249
//! [#253]: https://github.com/sujanto-gaws/kelir/issues/253

use serde_json::json;
use uuid::Uuid;

use super::domain::{self, AddCommentRequest, Comment, EditCommentRequest};
use super::repository as repo;
use super::{COMMENT_CREATE, COMMENT_DELETE, COMMENT_OBJECT_TYPE, COMMENT_READ, COMMENT_UPDATE};
use crate::error::AppError;
use crate::middleware::auth::Authenticated;
use crate::modules::activity::domain::EventCategory;
use crate::modules::activity::service::{record as record_activity, Happening};
use crate::modules::audit::{self, AuditEntry};
use crate::modules::document::service::document as document_service;
use crate::response::{PageMeta, Pagination};
use crate::state::AppState;

/// Adds a comment to a document, or a reply to one of its comments ([#249] AC2,
/// [#253] AC1).
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
///
/// # One level, and this function is where the level is held (**D-50**)
///
/// A reply names a comment; that comment must be one of *this* document's, must
/// still be there, and must not itself be a reply. The third is the depth rule,
/// and it lives here rather than in the schema because *my parent has no parent*
/// is a fact about another row, which no `CHECK` can read —
/// `0026_form_section_not_its_own_parent.sql` states the same limit about the
/// same shape. `ck_comments_not_its_own_parent` holds the one hop a constraint
/// can see; this holds the rest.
///
/// **The parent is read on the pool rather than in the transaction**, and that
/// is not an oversight: nothing can turn a root into a reply — `parent_comment_id`
/// is written once, at insert — so the only race left is a parent deleted
/// between the read and the write, which leaves a reply under a tombstone. That
/// is the state **D-51** designs for, not a state it has to prevent.
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

    if let Some(parent_id) = request.parent_comment_id {
        match repo::find_parent(&state.pool, tenant_id, document.id, parent_id).await? {
            None => return Err(domain::parent_not_found()),
            Some(Some(_)) => return Err(domain::reply_to_reply()),
            Some(None) => {}
        }
    }

    // #249 AC6, discharged here rather than in that item: the comment and its
    // activity event land in one transaction, so a timeline cannot disagree
    // with the conversation it describes.
    let mut transaction = state.pool.begin().await?;

    repo::insert_comment(
        &mut *transaction,
        &repo::NewComment {
            id,
            tenant_id,
            document_id: document.id,
            parent_comment_id: request.parent_comment_id,
            body: &body,
            created_by: Some(actor),
        },
    )
    .await?;

    // **A reply is its own event type**, because the timeline's sentence is
    // different: *somebody replied* is a thing that happened to a conversation
    // this document carries, and *somebody commented* is not the same thing
    // said twice. Neither carries anything about the comment itself — D-45.
    let (event_type, summary) = if request.parent_comment_id.is_some() {
        ("Comment.Replied", "Replied to a comment on the document")
    } else {
        ("Comment.Added", "Commented on the document")
    };

    record_activity(
        &mut transaction,
        &Happening {
            tenant_id,
            document_id: Some(document.id),
            workflow_instance_id: None,
            task_id: None,
            attachment_id: None,
            comment_id: Some(id),
            event_type,
            category: EventCategory::Comment,
            actor_user_id: Some(actor),
            actor_name: Some(caller.username()),
            action_summary: summary,
            // **Not the body, and since #292 not its length either.** The
            // first half was already right — a timeline is read by everyone who
            // may read the document, and the comment is behind `comment:read`,
            // which is the line D-12 and D-32 drew for the decision comment.
            // The length was the same disclosure in a smaller quantity: it is a
            // measurement of a thing this caller may not read, and **D-45**
            // says the timeline reports that a comment happened and links to it.
            details: json!({}),
        },
    )
    .await?;

    transaction.commit().await?;

    let stored = repo::find_comment(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::Internal {
            source: anyhow::anyhow!("comment {id} vanished after it was written"),
        })?;

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type,
            action: "CREATE",
            object_type: COMMENT_OBJECT_TYPE,
            object_id: id,
            actor_user_id: Some(actor),
            ip_address: caller.ip_address(),
            // **Not the body.** This trail is read through
            // `master-data:audit:read` by people who hold no permission over the
            // document, and a comment is prose about somebody else's work —
            // **D-12** and **D-32**'s line, and the same one `decide` and the
            // task hand-off both draw. That the comment exists is auditable;
            // what it says is behind `comment:read`.
            //
            // **`parentCommentId` is a reference and not content**, which is the
            // same test `activity_events`'s link columns pass: it says which
            // conversation this belongs to, and reaching the words it points at
            // still needs `comment:read`.
            reason: None,
            old_value: None,
            new_value: Some(json!({
                "documentId": document.id,
                "parentCommentId": request.parent_comment_id,
                "length": body.chars().count(),
            })),
        },
    )
    .await;

    Ok(stored)
}

/// Replaces what a comment says (FR-CMT-003; [#253] AC2, AC3).
///
/// # Three questions, and all three have to be yes
///
/// `comment:update` is *may this account edit comments at all*. The document's
/// own read is *may it see this conversation* — asked through that module's
/// service, so a comment stays exactly as private as the document it is about.
/// **Authorship is the third**, and it is not a permission: no code in this
/// release lets one account edit another's comment, which is why
/// `0036_comment_thread.sql` grants no moderator permission rather than granting
/// one nothing checks (**D-13**).
///
/// # The edit is stamped, and the previous text is not kept
///
/// AC3 asks that an edit be visible as an edit, and `edited_at` is what the read
/// serves for that. **There is no revision history**: this replaces the body.
/// What survives is that an edit happened, when, and who did it — enough for a
/// reader to know the words have moved and not enough to see what they were.
/// A conversation with versions is a different feature and would need a table;
/// this says so rather than implying more than it stores.
pub async fn edit_comment(
    state: &AppState,
    caller: &Authenticated,
    document_id: Uuid,
    comment_id: Uuid,
    request: EditCommentRequest,
) -> Result<Comment, AppError> {
    caller.require(COMMENT_UPDATE)?;

    // Normalized before anything is read, for `add_comment`'s stated reason: a
    // body over the bound is over it whatever the comment turns out to be.
    let body = domain::normalize_body(request.body)?;

    let document = document_service::get_document(state, caller, document_id).await?;

    let tenant_id = caller.tenant_id();
    let actor = caller.user_id();

    let mut transaction = state.pool.begin().await?;

    let existing = repo::lock_comment(&mut transaction, tenant_id, document.id, comment_id)
        .await?
        .ok_or_else(|| AppError::not_found("Comment"))?;

    refuse_unless_author(&existing, actor)?;

    let previous_length = existing.body.chars().count();

    if repo::update_body(
        &mut *transaction,
        tenant_id,
        document.id,
        comment_id,
        &body,
        Some(actor),
    )
    .await?
        == 0
    {
        // Unreachable: the row was found and locked three lines above under the
        // same predicate, so nothing can have deleted it. Reported as the broken
        // invariant it would be rather than as a 404, which would tell the
        // caller something false about a row that is still there.
        return Err(AppError::Internal {
            source: anyhow::anyhow!("comment {comment_id} was locked and then not updated"),
        });
    }

    record_activity(
        &mut transaction,
        &Happening {
            tenant_id,
            document_id: Some(document.id),
            workflow_instance_id: None,
            task_id: None,
            attachment_id: None,
            comment_id: Some(comment_id),
            event_type: "Comment.Edited",
            category: EventCategory::Comment,
            actor_user_id: Some(actor),
            actor_name: Some(caller.username()),
            action_summary: "Edited a comment on the document",
            // Neither the old text nor the new, and not their lengths: D-45's
            // rule does not soften because the words changed.
            details: json!({}),
        },
    )
    .await?;

    transaction.commit().await?;

    let stored = repo::find_comment(&state.pool, tenant_id, comment_id)
        .await?
        .ok_or_else(|| AppError::Internal {
            source: anyhow::anyhow!("comment {comment_id} vanished after it was edited"),
        })?;

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "Comment.Edited",
            action: "UPDATE",
            object_type: COMMENT_OBJECT_TYPE,
            object_id: comment_id,
            actor_user_id: Some(actor),
            ip_address: caller.ip_address(),
            reason: None,
            // **Lengths, on both sides, and no text.** The trail records that
            // the words changed and by how much; what they were and what they
            // became are behind `comment:read`, as they are on the way in.
            old_value: Some(json!({ "length": previous_length })),
            new_value: Some(json!({
                "documentId": document.id,
                "length": body.chars().count(),
            })),
        },
    )
    .await;

    Ok(stored)
}

/// Deletes a comment, softly, and leaves the conversation standing
/// (FR-CMT-004; [#253] AC2, AC4).
///
/// # What happens to the replies is decided, not defaulted (**D-51**)
///
/// **Nothing happens to them.** They are not deleted, not re-parented and not
/// hidden: they are other people's words, and a delete that took them would let
/// one person end a conversation they only started. The deleted comment stays in
/// the list as a **tombstone** — its author, its time, and no body — for as long
/// as an undeleted reply hangs from it, so a reader sees *somebody said something
/// here and took it back*, and the answers still make sense.
///
/// Once nothing replies to it, or it was a reply itself, it leaves the list
/// altogether. A tombstone with nothing under it holds no shape.
///
/// **`repository::list_for_document` is where both halves are decided**, because
/// it is the one place every reader comes through.
///
/// # The row keeps its text
///
/// A soft delete, so `body` is still on the row and the read boundary withholds
/// it. That is what makes the audit trail's length meaningful and what would let
/// a later release offer an undo; scrubbing the column here would make the
/// delete a one-way door that this item was not asked to build.
pub async fn delete_comment(
    state: &AppState,
    caller: &Authenticated,
    document_id: Uuid,
    comment_id: Uuid,
) -> Result<(), AppError> {
    caller.require(COMMENT_DELETE)?;

    let document = document_service::get_document(state, caller, document_id).await?;

    let tenant_id = caller.tenant_id();
    let actor = caller.user_id();

    let mut transaction = state.pool.begin().await?;

    let existing = repo::lock_comment(&mut transaction, tenant_id, document.id, comment_id)
        .await?
        .ok_or_else(|| AppError::not_found("Comment"))?;

    refuse_unless_author(&existing, actor)?;

    if repo::soft_delete(
        &mut *transaction,
        tenant_id,
        document.id,
        comment_id,
        Some(actor),
    )
    .await?
        == 0
    {
        // Unreachable under the lock, for `edit_comment`'s reason.
        return Err(AppError::Internal {
            source: anyhow::anyhow!("comment {comment_id} was locked and then not deleted"),
        });
    }

    record_activity(
        &mut transaction,
        &Happening {
            tenant_id,
            document_id: Some(document.id),
            workflow_instance_id: None,
            task_id: None,
            attachment_id: None,
            comment_id: Some(comment_id),
            event_type: "Comment.Deleted",
            category: EventCategory::Comment,
            actor_user_id: Some(actor),
            actor_name: Some(caller.username()),
            action_summary: "Deleted a comment on the document",
            // **The link outlives the comment**, and that is deliberate: the
            // timeline says a comment was deleted and points at a row the
            // comment surface will answer for as a tombstone or as a 404. What
            // it said is what a delete takes away.
            details: json!({}),
        },
    )
    .await?;

    transaction.commit().await?;

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "Comment.Deleted",
            action: "DELETE",
            object_type: COMMENT_OBJECT_TYPE,
            object_id: comment_id,
            actor_user_id: Some(actor),
            ip_address: caller.ip_address(),
            reason: None,
            old_value: Some(json!({
                "documentId": document.id,
                "parentCommentId": existing.parent_comment_id,
                "length": existing.body.chars().count(),
            })),
            new_value: None,
        },
    )
    .await;

    Ok(())
}

/// The half of *may I* that a permission cannot answer ([#253] AC2).
///
/// **A comment with no author is nobody's to edit.** `comments.created_by` is
/// nullable — a user can be removed, and `ON DELETE` leaves the row — and a null
/// here must not compare equal to anything. `Some(actor)` on both sides is what
/// makes that true by construction rather than by remembering.
///
/// A bare 403, the same one a missing permission produces, and deliberately not
/// a message naming the author: whose comment it is, is something the list
/// already told this caller, and a refusal that explained itself differently for
/// *not yours* and *not permitted* would be a way to ask the second question by
/// asking the first.
fn refuse_unless_author(comment: &repo::CommentRow, actor: Uuid) -> Result<(), AppError> {
    if comment.created_by == Some(actor) {
        return Ok(());
    }

    Err(AppError::Forbidden)
}

/// A document's conversation, oldest first, replies under what they answer.
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
