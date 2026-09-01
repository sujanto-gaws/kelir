//! What a comment is, and the one refusal that needs no database (FR-CMT-001;
//! [#249]).
//!
//! [#249]: https://github.com/sujanto-gaws/kelir/issues/249

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::{AppError, ValidationDetail};

/// One comment, as the API reports it.
///
/// **`authorUsername` is joined rather than denormalized**, which is the
/// opposite of what `activity_events.actor_name` will do one item later, and the
/// difference is deliberate: an activity event is a fact about a moment and must
/// still read correctly after a user is renamed, while a comment is a living
/// conversation whose participants are current people. A renamed user's comments
/// should show the new name; a renamed user's *history* should not.
///
/// # `body` is optional, and the null is a tombstone rather than a blank
///
/// **A deleted comment that still has replies is served with `body: null` and
/// `deletedAt` set** ([#253](https://github.com/sujanto-gaws/kelir/issues/253)
/// AC4, **D-51**). The row keeps its text; this boundary withholds it, exactly
/// as [`crate::modules::activity::domain::disclosable`] withholds what belongs
/// to another surface. A deleted comment with no replies is not served at all.
///
/// So the two fields are read together and neither is inferred from the other:
/// `deletedAt` says *this was deleted*, and `body` being null is what that costs
/// the reader. An empty string would have said *somebody commented nothing*,
/// which is a thing this API refuses to store.
///
/// # `editedAt`, and why it is not `updatedAt`
///
/// `comments.updated_at` moves for any write to the row, the soft delete
/// included. **#253 AC3 is about the body**: an edit has to be visible *as* an
/// edit, because a comment whose text changed with nothing saying so is a
/// conversation somebody can rewrite after the fact. `editedAt` is null until
/// the body changes, and the screen renders that null as silence.
///
/// **What is not kept is the previous text.** An edit replaces; there is no
/// revision table and this release does not build one. What survives an edit is
/// that it happened, when, and — through `activity_events` and the audit trail —
/// who did it. That is enough to notice a rewrite and not enough to reconstruct
/// one, which is the honest description of what a `Should` on an editable
/// conversation buys.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Comment {
    pub id: Uuid,
    pub document_id: Uuid,
    /// The comment's text, or **null** on a tombstone — a deleted comment kept
    /// in the conversation because replies hang from it (**D-51**).
    pub body: Option<String>,
    /// The root this comment replies to, or null if it is a root itself.
    /// Threading is **one level** (**D-50**), so a comment carrying this is one
    /// whose own replies cannot exist.
    pub parent_comment_id: Option<Uuid>,
    pub author_user_id: Option<Uuid>,
    pub author_username: Option<String>,
    pub created_at: DateTime<Utc>,
    /// When the body last changed, null on a comment nobody has edited.
    pub edited_at: Option<DateTime<Utc>>,
    /// When it was deleted. Non-null only on a tombstone, and then `body` is
    /// null.
    pub deleted_at: Option<DateTime<Utc>>,
}

/// The body of a request to comment on a document, or to reply to a comment.
///
/// **Two fields.** `comment_type` and `visibility` exist on the row with their
/// defaults, and this surface still does not offer them: a vocabulary a caller
/// can choose from and no reader distinguishes is a vocabulary that will be
/// wrong by the time something reads it.
///
/// **`parentCommentId` arrives with the screen that renders it**
/// ([#253](https://github.com/sujanto-gaws/kelir/issues/253) AC1). It was absent
/// in the release that built this surface for the stated reason that a reply
/// nothing renders as one is not a reply; `CommentsTab.vue` now renders one, so
/// the field is here. Absent or null is a root comment — **a reply and a
/// comment are the same write**, which is the shape
/// [the concept document](../../../../docs/concepts/02.%20Handling%20Attachments%20Comments%20and%20Activity%20Log.md)
/// §12.3 describes and the shape a one-level thread makes true: there is no
/// second surface for replying because there is nothing different about it.
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AddCommentRequest {
    pub body: String,
    #[serde(default)]
    pub parent_comment_id: Option<Uuid>,
}

/// The body of a request to edit a comment (FR-CMT-003).
///
/// **One field, and it is not `AddCommentRequest` reused.** An edit cannot
/// re-parent a comment: moving a reply to another thread would rewrite what a
/// conversation says two people were talking about, and a request type that
/// accepted `parentCommentId` would have to refuse it in the service — a
/// refusal a type can make impossible instead. `deny_unknown_fields` is what
/// turns sending it into a 422 naming the field rather than a silent ignore.
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EditCommentRequest {
    pub body: String,
}

/// The longest comment body this API will store.
///
/// **The same number as a decision comment's**, and named separately rather than
/// imported, because the two bounds answer different questions and one of them
/// will move. `workflow::domain::task::MAX_COMMENT_LENGTH` is bounded by an
/// append-only history row nobody can take back out; this is bounded by a
/// conversation a later sprint lets people edit and delete. They agree today by
/// judgement, not by construction.
pub const MAX_COMMENT_BODY: usize = 4000;

/// The body as the row should hold it, or a refusal.
///
/// **`normalize_comment`'s shape, with one deliberate difference**
/// ([#249](https://github.com/sujanto-gaws/kelir/issues/249) AC4). That function
/// turns whitespace into *absent*, because a decision comment is optional and
/// `"   "` is not a reason. Here the comment **is** the request: whitespace is a
/// refusal rather than an absence, because there is nothing else in the message
/// for it to be missing from.
pub fn normalize_body(body: String) -> Result<String, AppError> {
    let trimmed = body.trim();

    if trimmed.is_empty() {
        return Err(AppError::validation(vec![ValidationDetail::new(
            "body",
            "required",
            "COMMENT_EMPTY",
            "a comment needs something in it; whitespace is not a comment",
        )]));
    }

    if trimmed.chars().count() > MAX_COMMENT_BODY {
        return Err(AppError::validation(vec![ValidationDetail::new(
            "body",
            "maxLength",
            "TOO_LONG",
            format!("a comment is at most {MAX_COMMENT_BODY} characters"),
        )]));
    }

    Ok(trimmed.to_owned())
}

/// The refusal for a `parentCommentId` naming nothing this conversation holds.
///
/// **One refusal for four cases**: no such comment, another document's comment,
/// another tenant's, and one that has been deleted. They are told apart only by
/// a lookup the caller may not make, and enumerating them would answer *there is
/// a comment with that id, elsewhere* — which is `get_document`'s 404 rule one
/// module over, applied to the body of a request instead of its path.
pub fn parent_not_found() -> AppError {
    AppError::validation(vec![ValidationDetail::new(
        "parentCommentId",
        "reference",
        "PARENT_NOT_FOUND",
        "this document has no comment with that id, or it has been deleted",
    )])
}

/// The refusal for a reply to a reply (**D-50**).
///
/// A 422 naming the field rather than a 409, because it is the request that is
/// wrong and the fix is in it: reply to the root instead, whose id the caller
/// already has — every reply the list serves carries its `parentCommentId`.
pub fn reply_to_reply() -> AppError {
    AppError::validation(vec![ValidationDetail::new(
        "parentCommentId",
        "reference",
        "REPLY_TO_REPLY",
        "a conversation here is one level deep: reply to the comment this one \
         replies to, not to the reply",
    )])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_comment_of_whitespace_is_refused_rather_than_stored_empty() {
        let refused = normalize_body("   \n\t ".to_owned());

        assert!(refused.is_err(), "whitespace was accepted as a comment");
    }

    #[test]
    fn a_body_is_stored_trimmed() {
        assert_eq!(
            normalize_body("  is this the right supplier?  ".to_owned()).expect("a body"),
            "is this the right supplier?"
        );
    }

    #[test]
    fn a_body_over_the_bound_is_refused_and_the_bound_is_in_characters() {
        // Multi-byte on purpose: a bound counted in bytes would refuse a third
        // of this, which is the bug a `len()` here would be.
        let long = "é".repeat(MAX_COMMENT_BODY);

        assert!(normalize_body(long.clone()).is_ok(), "a body at the bound");
        assert!(normalize_body(format!("{long}é")).is_err(), "one over");
    }
}
