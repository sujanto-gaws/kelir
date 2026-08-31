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
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Comment {
    pub id: Uuid,
    pub document_id: Uuid,
    pub body: String,
    pub author_user_id: Option<Uuid>,
    pub author_username: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// The body of a request to comment on a document.
///
/// **One field.** `comment_type` and `visibility` exist on the row with their
/// defaults, and this surface does not offer them: a vocabulary a caller can
/// choose from and no reader distinguishes is a vocabulary that will be wrong by
/// the time something reads it. `parentCommentId` is absent for the same reason
/// and a stronger one — threading is FR-CMT-002 and Sprint 13, so a reply this
/// release accepted would be a reply nothing renders as one.
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AddCommentRequest {
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
