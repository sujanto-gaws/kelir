//! Where a document is in its own life, and where it may go next (FR-DOC-007,
//! [#169]).
//!
//! **Not `record_status`.** [`super::super`]'s module documentation states which
//! question each of the two answers and why a reader of `documents` can tell
//! them apart; this file is the one that is a document's.
//!
//! # Five of the ten values belong to a workflow that does not exist yet
//!
//! `documents.status`'s `CHECK` names ten values, because Database Schema §6.6
//! wrote the whole lifecycle at once. Sprint 9 has no workflow — FR-DOC-012 and
//! the whole of FR-WF-* are Phase 5 — and [#169]'s AC5 forbids this item from
//! pre-empting what that link will mean. So the legality table is written over
//! all ten and the transitions *into* the workflow-owned state are simply
//! absent, which is the shape [`RecordStatus::may_move_to`][record] used for
//! `PENDING_APPROVAL` and for the identical reason: a document put there today
//! would await an approver that does not exist.
//!
//! **`PENDING_APPROVAL` is reachable from nothing.** It is the state a running
//! approval puts a document in, and nothing can run an approval.
//!
//! **`ARCHIVED` is reachable from nothing either**, and that is the honest
//! encoding of a cut. FR-DOC-010 (cancel and archive) is a `Should` on Sprint
//! 9's tail and **D-16** expects the tail to go; the value exists in the column
//! because §6.6 put it there, the product moves nothing into it, and this table
//! is where a reader finds out which.
//!
//! **`CANCELLED` *is* reachable**, and it is not the same decision. Cancelling
//! is the ordinary end of a request that was withdrawn — it needs no retention
//! policy, no storage tier and no purge schedule, which is what FR-DOC-009's
//! archive half is actually about. A lifecycle whose only exits are `COMPLETED`
//! and nothing traps every abandoned document forever, and a product that
//! cannot say "never mind" is a product people work around.
//!
//! # `DRAFT -> SUBMITTED` is not this route's to make
//!
//! It is [#168]'s, and [`DocumentStatus::check_move_to`] refuses it by name.
//! Submitting is not a status change with a number as a side effect; it is one
//! transaction that re-evaluates a payload, takes a number and moves a status,
//! and reaching the status half through a different door would produce a
//! submitted document with no number — which #168's own text calls
//! unrecoverable by the user.
//!
//! [#168]: https://github.com/sujanto-gaws/kelir/issues/168
//! [#169]: https://github.com/sujanto-gaws/kelir/issues/169
//! [record]: crate::modules::master_data::domain::RecordStatus::may_move_to

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::error::{AppError, ValidationDetail};

/// Where a document is in its own life (Database Schema §6.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DocumentStatus {
    Draft,
    Submitted,
    InReview,
    PendingApproval,
    Approved,
    Rejected,
    Returned,
    Completed,
    Archived,
    Cancelled,
}

impl DocumentStatus {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Draft => "DRAFT",
            Self::Submitted => "SUBMITTED",
            Self::InReview => "IN_REVIEW",
            Self::PendingApproval => "PENDING_APPROVAL",
            Self::Approved => "APPROVED",
            Self::Rejected => "REJECTED",
            Self::Returned => "RETURNED",
            Self::Completed => "COMPLETED",
            Self::Archived => "ARCHIVED",
            Self::Cancelled => "CANCELLED",
        }
    }

    /// Reads a value already in the database.
    ///
    /// The column has a `CHECK`, so an unrecognised value cannot be there —
    /// falling back to `Draft` rather than failing is what keeps a schema change
    /// from making every document unreadable, and the `CHECK` is what makes the
    /// fallback unreachable rather than lenient. The same trade
    /// `RecordStatus::from_db` takes, for the same reason.
    pub fn from_db(value: &str) -> Self {
        match value {
            "SUBMITTED" => Self::Submitted,
            "IN_REVIEW" => Self::InReview,
            "PENDING_APPROVAL" => Self::PendingApproval,
            "APPROVED" => Self::Approved,
            "REJECTED" => Self::Rejected,
            "RETURNED" => Self::Returned,
            "COMPLETED" => Self::Completed,
            "ARCHIVED" => Self::Archived,
            "CANCELLED" => Self::Cancelled,
            _ => Self::Draft,
        }
    }

    /// Whether the document may still be edited and discarded.
    ///
    /// One predicate rather than `== Draft` written in four services: the
    /// question "may this be edited" is asked by the update path, the delete
    /// path, the submit path and the renderer's mode, and four spellings of it
    /// are four chances for one of them to disagree.
    pub fn is_editable(self) -> bool {
        matches!(self, Self::Draft)
    }

    /// Where a document in this state may go next **through the transition
    /// route**, and nowhere else.
    ///
    /// The whole state machine, in one place. See the module documentation for
    /// why `PENDING_APPROVAL` and `ARCHIVED` appear in no list, and why
    /// `SUBMITTED` does not appear in `DRAFT`'s.
    ///
    /// A status is never in its own list. Re-sending the state a document is
    /// already in is a caller who believes something happened that did not, and
    /// answering 200 would confirm the belief.
    pub fn may_move_to(self) -> &'static [Self] {
        match self {
            // Empty, and the emptiness is the decision. The one move a draft
            // can make is the submit, and that is #168's transaction rather
            // than this route's — `check_move_to` says so by name.
            Self::Draft => &[],
            Self::Submitted => &[
                Self::InReview,
                Self::Approved,
                Self::Rejected,
                Self::Returned,
                Self::Cancelled,
            ],
            Self::InReview => &[
                Self::Approved,
                Self::Rejected,
                Self::Returned,
                Self::Cancelled,
            ],
            // A returned document goes back to its author, who corrects it and
            // sends it again. It re-enters `SUBMITTED` rather than `DRAFT`
            // because it has a number, and a numbered draft is a state nothing
            // else in this file can produce.
            Self::Returned => &[Self::Submitted, Self::Cancelled],
            Self::Approved => &[Self::Completed, Self::Cancelled],
            Self::Rejected => &[Self::Cancelled],
            // Phase 5's. Nothing may enter it, so nothing leaves it.
            Self::PendingApproval => &[],
            Self::Completed => &[],
            Self::Archived => &[],
            Self::Cancelled => &[],
        }
    }

    /// Refuses a move this document cannot make, naming both ends of it.
    ///
    /// A 422 rather than a 409: the request names a status the document cannot
    /// take *from where it is*, which is a property of the payload against the
    /// resource. A 409 is what a *concurrent* change earns, and the service
    /// raises that one — the two are different failures and a caller fixes them
    /// differently.
    pub fn check_move_to(self, target: Self) -> Result<(), AppError> {
        if self.may_move_to().contains(&target) {
            return Ok(());
        }

        let message = if self == Self::Draft && target == Self::Submitted {
            // Named rather than lumped in with the rest, because it is the one
            // illegal transition a caller is right to expect to work and the
            // fix is a different endpoint rather than a different status.
            "a draft is submitted through POST /documents/{id}/submission, which \
             takes its number in the same transaction; moving the status alone \
             would leave a submitted document with no number"
                .to_owned()
        } else if self.may_move_to().is_empty() {
            format!("{} is final; a document cannot leave it", self.as_db())
        } else {
            format!(
                "{} cannot become {}. From here: {}",
                self.as_db(),
                target.as_db(),
                self.may_move_to()
                    .iter()
                    .map(|status| status.as_db())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };

        Err(AppError::validation(vec![ValidationDetail::new(
            "status",
            "transition",
            "ILLEGAL_TRANSITION",
            message,
        )]))
    }
}

/// The body of a transition request.
///
/// `reason` is optional and goes to the audit record's `reason` column and to
/// `document_status_history.reason`, which is what makes "why was this
/// rejected" answerable later. It is not required, because requiring a reason
/// nobody reads produces reasons nobody wrote.
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TransitionRequest {
    pub status: DocumentStatus,
    pub reason: Option<String>,
}

/// What a transition answers with: where the document was and where it now is.
///
/// Both ends rather than only the new status. A client that sent
/// `SUBMITTED -> APPROVED` already knows the target; what it cannot know without
/// being told is what the document was when the transition ran, which is what
/// makes a concurrent change visible.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TransitionResult {
    pub previous_status: DocumentStatus,
    pub status: DocumentStatus,
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [DocumentStatus; 10] = [
        DocumentStatus::Draft,
        DocumentStatus::Submitted,
        DocumentStatus::InReview,
        DocumentStatus::PendingApproval,
        DocumentStatus::Approved,
        DocumentStatus::Rejected,
        DocumentStatus::Returned,
        DocumentStatus::Completed,
        DocumentStatus::Archived,
        DocumentStatus::Cancelled,
    ];

    #[test]
    fn every_status_round_trips_through_the_database_spelling() {
        for status in ALL {
            assert_eq!(DocumentStatus::from_db(status.as_db()), status);
        }
    }

    #[test]
    fn the_documented_path_is_walkable_end_to_end() {
        // SUBMITTED → IN_REVIEW → RETURNED → SUBMITTED → APPROVED → COMPLETED.
        // If any leg is refused, the lifecycle has states nothing can reach and
        // the demo path stops somewhere.
        let path = [
            DocumentStatus::Submitted,
            DocumentStatus::InReview,
            DocumentStatus::Returned,
            DocumentStatus::Submitted,
            DocumentStatus::Approved,
            DocumentStatus::Completed,
        ];

        for pair in path.windows(2) {
            assert!(
                pair[0].check_move_to(pair[1]).is_ok(),
                "{} cannot reach {}",
                pair[0].as_db(),
                pair[1].as_db()
            );
        }
    }

    #[test]
    fn a_workflow_state_cannot_be_entered_by_a_direct_transition() {
        // #169 AC5. Nothing can approve anything until Phase 5, so a document
        // put in PENDING_APPROVAL would await an approver that does not exist —
        // the overstatement #99 removed from `record_status`, reintroduced one
        // module over.
        for status in ALL {
            assert!(
                status
                    .check_move_to(DocumentStatus::PendingApproval)
                    .is_err(),
                "{} reached PENDING_APPROVAL",
                status.as_db()
            );
        }
    }

    #[test]
    fn archiving_is_not_reachable_while_its_requirement_is_cut() {
        // FR-DOC-010 is Sprint 9's cut tail. The value is in the column because
        // §6.6 put it there; nothing moves a document into it, and this is
        // where a reader finds that out rather than by grepping for the string.
        for status in ALL {
            assert!(
                status.check_move_to(DocumentStatus::Archived).is_err(),
                "{} reached ARCHIVED",
                status.as_db()
            );
        }
    }

    #[test]
    fn a_draft_is_not_submitted_through_the_transition_route() {
        let error = DocumentStatus::Draft
            .check_move_to(DocumentStatus::Submitted)
            .expect_err("the transition route does not submit");

        let AppError::Validation { details } = error else {
            panic!("expected a validation failure");
        };

        assert_eq!(details[0].code, "ILLEGAL_TRANSITION");
        // The refusal names the endpoint that does it. A caller told only
        // "DRAFT cannot become SUBMITTED" would reasonably conclude the product
        // cannot submit documents.
        assert!(details[0].message.contains("/submission"), "{details:?}");
    }

    #[test]
    fn a_document_cannot_transition_to_the_state_it_is_already_in() {
        for status in ALL {
            assert!(
                status.check_move_to(status).is_err(),
                "{} transitioned to itself",
                status.as_db()
            );
        }
    }

    #[test]
    fn every_live_status_can_be_ended() {
        // A lifecycle that traps a document is a lifecycle people work around
        // by creating a second document. Every status a document can actually
        // be in either leads somewhere or is one of the four stated ends.
        let ends = [
            DocumentStatus::Completed,
            DocumentStatus::Cancelled,
            DocumentStatus::Archived,
            DocumentStatus::PendingApproval,
        ];

        for status in ALL {
            if ends.contains(&status) || status == DocumentStatus::Draft {
                continue;
            }

            assert!(
                status.may_move_to().contains(&DocumentStatus::Cancelled),
                "{} cannot be cancelled and is not an end",
                status.as_db()
            );
        }
    }

    #[test]
    fn a_refusal_names_both_ends_and_what_was_possible() {
        let error = DocumentStatus::Submitted
            .check_move_to(DocumentStatus::Completed)
            .expect_err("a submitted document is not completed directly");

        let AppError::Validation { details } = error else {
            panic!("expected a validation failure");
        };

        assert_eq!(details[0].path, "status");
        assert!(details[0].message.contains("SUBMITTED"), "{details:?}");
        assert!(details[0].message.contains("COMPLETED"), "{details:?}");
        assert!(details[0].message.contains("APPROVED"), "{details:?}");
    }

    #[test]
    fn only_a_draft_is_editable() {
        for status in ALL {
            assert_eq!(
                status.is_editable(),
                status == DocumentStatus::Draft,
                "{} disagreed about being editable",
                status.as_db()
            );
        }
    }

    #[test]
    fn a_transition_request_with_a_misspelled_field_is_refused() {
        // #62. `statusId` silently dropped would leave `status` missing and the
        // request refused for the wrong reason.
        let refused: Result<TransitionRequest, _> =
            serde_json::from_str(r#"{"statusId": "APPROVED"}"#);

        assert!(refused.is_err());
    }
}
