//! The governance lifecycle master-data records move through (FR-MDM-007).
//!
//! `record_status` has existed on `mdm_parties`, `mdm_facilities`,
//! `mdm_products` and `mdm_services` since `0008` and nothing moved it: every
//! record sat at `DRAFT` and always would. A column the product does not honour
//! is the shape of overstatement Sprint 5 had to narrow out of the session
//! contract (#85), so this is the other resolution — the column becomes a
//! controlled transition with a permission and an audit event.
//!
//! **Not the same column as `mdm_parties.status`.** That one is
//! `PARTY_ENABLED` / `PARTY_DISABLED`, the party's own enabled flag, and #80
//! already moves and audits it. Two columns, two meanings: `status` says
//! whether the business deals with this party, `record_status` says how far the
//! record itself has got through governance.
//!
//! # Three decisions this file records
//!
//! **`PENDING_APPROVAL` cannot be entered by a direct edit**, and since
//! [#255](https://github.com/sujanto-gaws/kelir/issues/255) it can be entered.
//! Both halves are still true, and the difference between them is the whole of
//! FR-MDM-010.
//!
//! This paragraph used to say the value was unreachable because *nothing today
//! can approve anything*, and named [`RecordStatus::may_move_to`] as where that
//! would change when the workflow landed. It has. A record parks here when a
//! **governed change document** is submitted against it, and leaves when that
//! document is approved or refused — both writes made by
//! `master_data::service::governance`, inside the transaction that moved the
//! document.
//!
//! **The transition surface still refuses it, by name.** `POST /transition` is a
//! person saying where a record should be; parking is a **process** saying that
//! a record is not theirs to move. A caller who could park a record by hand
//! would create exactly what this file warned about — a record awaiting an
//! approver that does not exist — and a caller who could unpark one would strand
//! the change document still pointing at it. `service::record_status::transition`
//! refuses `PENDING_APPROVAL` at either end and says which door to use.
//!
//! **`ARCHIVED` is terminal.** `ARCHIVED -> DRAFT` is the case worth naming:
//! archiving is what a tenant does to a record it has finished with, and a
//! route back to `DRAFT` would make the archive a filter rather than a
//! decision. A record that must live again is re-created.
//!
//! **The legal set is stated once**, in [`RecordStatus::may_move_to`], rather
//! than implied by match arms in four services. Four copies of a state machine
//! are four state machines.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::error::{AppError, ValidationDetail};

/// Where a master-data record has got to in its own governance lifecycle
/// (concepts/03 §5; the `record_status` `CHECK` in Database Schema §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RecordStatus {
    Draft,
    PendingApproval,
    Active,
    Suspended,
    Inactive,
    Archived,
}

impl RecordStatus {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Draft => "DRAFT",
            Self::PendingApproval => "PENDING_APPROVAL",
            Self::Active => "ACTIVE",
            Self::Suspended => "SUSPENDED",
            Self::Inactive => "INACTIVE",
            Self::Archived => "ARCHIVED",
        }
    }

    /// Reads a value already in the database.
    ///
    /// The column has a `CHECK`, so an unrecognised value cannot be there —
    /// falling back to `Draft` rather than failing is what keeps a schema
    /// change from making every record unreadable, and the `CHECK` is what
    /// makes that fallback unreachable rather than lenient.
    pub fn from_db(value: &str) -> Self {
        match value {
            "PENDING_APPROVAL" => Self::PendingApproval,
            "ACTIVE" => Self::Active,
            "SUSPENDED" => Self::Suspended,
            "INACTIVE" => Self::Inactive,
            "ARCHIVED" => Self::Archived,
            _ => Self::Draft,
        }
    }

    /// Where a record in this state may go next, and nowhere else.
    ///
    /// The whole state machine, in one place. `PENDING_APPROVAL` is reachable
    /// from nothing — see the module documentation — and `ARCHIVED` leads
    /// nowhere.
    ///
    /// A status is never in its own list. Re-sending the state a record is
    /// already in is a caller who believes something happened that did not, and
    /// answering 200 would confirm the belief.
    pub fn may_move_to(self) -> &'static [Self] {
        match self {
            // `PENDING_APPROVAL` is here since #255: a governed change parks a
            // record from either of the two states it can be raised from. It is
            // the *state machine* that permits the move; the transition surface
            // refuses it, because parking is a process's move rather than a
            // person's — see this module's header.
            Self::Draft => &[Self::PendingApproval, Self::Active, Self::Inactive],
            Self::PendingApproval => &[Self::Active, Self::Draft],
            Self::Active => &[Self::PendingApproval, Self::Suspended, Self::Inactive],
            Self::Suspended => &[Self::Active, Self::Inactive],
            Self::Inactive => &[Self::Active, Self::Archived],
            Self::Archived => &[],
        }
    }

    /// Whether this is the state a governed change parks a record in.
    ///
    /// One predicate rather than a `==` in three places, because *the parked
    /// state* is a concept two modules now share and one of them refuses it.
    pub fn is_parked(self) -> bool {
        matches!(self, Self::PendingApproval)
    }

    /// Refuses a move this record cannot make, naming both ends of it.
    ///
    /// A 422 rather than a 409: the request names a status the record cannot
    /// take *from where it is*, which is a property of the payload against the
    /// resource, and the caller needs to be told which transition was rejected
    /// rather than that something conflicted.
    pub fn check_move_to(self, target: Self) -> Result<(), AppError> {
        if self.may_move_to().contains(&target) {
            return Ok(());
        }

        let allowed = self.may_move_to();
        let message = if allowed.is_empty() {
            format!("{} is final; a record cannot leave it", self.as_db())
        } else {
            format!(
                "{} cannot become {}. From here: {}",
                self.as_db(),
                target.as_db(),
                allowed
                    .iter()
                    .map(|status| status.as_db())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };

        Err(AppError::validation(vec![ValidationDetail::new(
            "recordStatusId",
            "transition",
            "ILLEGAL_TRANSITION",
            message,
        )]))
    }
}

/// Which master-data record is being moved.
///
/// The table name reaches SQL, so this is an enum and not a string from the
/// path (coding standard §2.5). There is no route that turns caller input into
/// a table here — `/parties/{id}/transition` is [`TransitionTarget::Party`] by
/// construction.
///
/// `mdm_products` and `mdm_services` carry the same columns and are absent on
/// purpose: they have no endpoints until Sprint 7, and a transition route for a
/// record nothing can create is a control with nothing behind it. Adding them
/// is one variant each.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionTarget {
    Party,
    Facility,
}

impl TransitionTarget {
    /// What the audit trail calls this record (naming convention §7).
    pub fn object_type(self) -> &'static str {
        match self {
            Self::Party => "PARTY",
            Self::Facility => "FACILITY",
        }
    }

    /// The business subject the event is named for, which is not the table.
    pub fn entity(self) -> &'static str {
        match self {
            Self::Party => "Party",
            Self::Facility => "Facility",
        }
    }

    pub fn missing(self) -> &'static str {
        match self {
            Self::Party => "Party",
            Self::Facility => "Facility",
        }
    }
}

/// The body of a transition request.
///
/// `reason` is optional and goes to the audit record's `reason` column, which
/// is what makes "why was this suspended" answerable later. It is not required,
/// because requiring a reason that nobody reads produces reasons nobody wrote.
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TransitionRequest {
    pub record_status_id: RecordStatus,
    pub reason: Option<String>,
}

/// What a transition answers with: where the record was and where it now is.
///
/// Both ends, rather than only the new status. A client that sent
/// `ACTIVE -> SUSPENDED` already knows the target; what it cannot know without
/// being told is what the record was when the transition ran, which is what
/// makes a concurrent change visible.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TransitionResult {
    pub previous_record_status_id: RecordStatus,
    pub record_status_id: RecordStatus,
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [RecordStatus; 6] = [
        RecordStatus::Draft,
        RecordStatus::PendingApproval,
        RecordStatus::Active,
        RecordStatus::Suspended,
        RecordStatus::Inactive,
        RecordStatus::Archived,
    ];

    #[test]
    fn every_status_round_trips_through_the_database_spelling() {
        for status in ALL {
            assert_eq!(RecordStatus::from_db(status.as_db()), status);
        }
    }

    #[test]
    fn the_documented_path_is_walkable_end_to_end() {
        // DRAFT → ACTIVE → SUSPENDED → ACTIVE → INACTIVE → ARCHIVED. If any leg
        // of this is refused, the lifecycle has states nothing can reach.
        let path = [
            RecordStatus::Draft,
            RecordStatus::Active,
            RecordStatus::Suspended,
            RecordStatus::Active,
            RecordStatus::Inactive,
            RecordStatus::Archived,
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
    fn archived_is_final() {
        // The case #99 names. A route back to DRAFT would make the archive a
        // filter rather than a decision.
        for target in ALL {
            assert!(
                RecordStatus::Archived.check_move_to(target).is_err(),
                "ARCHIVED reached {}",
                target.as_db()
            );
        }
    }

    #[test]
    fn a_direct_edit_cannot_put_a_record_into_pending_approval() {
        // Nothing can approve anything until FR-MDM-010, so a record put here
        // would await an approver that does not exist.
        for status in ALL {
            assert!(
                status.check_move_to(RecordStatus::PendingApproval).is_err(),
                "{} reached PENDING_APPROVAL",
                status.as_db()
            );
        }
    }

    #[test]
    fn a_record_cannot_transition_to_the_state_it_is_already_in() {
        // Answering 200 would confirm a belief that nothing happened to
        // justify.
        for status in ALL {
            assert!(
                status.check_move_to(status).is_err(),
                "{} transitioned to itself",
                status.as_db()
            );
        }
    }

    #[test]
    fn a_refusal_names_both_ends_and_what_was_possible() {
        let error = RecordStatus::Draft
            .check_move_to(RecordStatus::Archived)
            .expect_err("DRAFT cannot archive");

        let AppError::Validation { details } = error else {
            panic!("expected a validation failure");
        };

        assert_eq!(details[0].path, "recordStatusId");
        assert_eq!(details[0].code, "ILLEGAL_TRANSITION");
        assert!(details[0].message.contains("DRAFT"), "{details:?}");
        assert!(details[0].message.contains("ARCHIVED"), "{details:?}");
        assert!(details[0].message.contains("ACTIVE"), "{details:?}");
    }

    #[test]
    fn every_status_except_the_terminal_one_leads_somewhere() {
        for status in ALL {
            if status == RecordStatus::Archived {
                continue;
            }
            assert!(
                !status.may_move_to().is_empty(),
                "{} is a dead end that is not documented as one",
                status.as_db()
            );
        }
    }

    #[test]
    fn a_transition_request_with_a_misspelled_field_is_refused() {
        // #62, and it matters here: `recordStatus` silently dropped would leave
        // `recordStatusId` missing and the request refused for the wrong
        // reason, or — worse, if it were optional — accepted as a no-op.
        let refused: Result<TransitionRequest, _> =
            serde_json::from_str(r#"{"recordStatus": "ACTIVE"}"#);

        assert!(refused.is_err());
    }
}
