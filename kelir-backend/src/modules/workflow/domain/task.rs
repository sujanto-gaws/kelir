//! A user task: the piece of human work a transition generates (FR-WF-004,
//! FR-WF-006, FR-WF-007; [#176], [#177]).
//!
//! # A task's status is not the process's status
//!
//! [`super::instance`] holds the process's, and says which is which. This one
//! answers *what has happened to this piece of work* — it was created, somebody
//! took it, somebody finished it. The two move in the same transaction and
//! answer different questions, which is the distinction this codebase has now
//! had to draw three times and should read the same way each time.
//!
//! # Assigned to a user, or offered to a role, and never both
//!
//! [#176] AC2. A task offered to a role has **no assignee** until somebody
//! claims it, and that is the difference [#179] AC1 has to be able to show a
//! person: an unclaimed queue item and work that is already mine are different
//! situations. Writing both — an owner *and* a role — would erase the difference
//! at the moment the task is created, and no screen could recover it.
//!
//! [#176]: https://github.com/sujanto-gaws/kelir/issues/176
//! [#177]: https://github.com/sujanto-gaws/kelir/issues/177
//! [#179]: https://github.com/sujanto-gaws/kelir/issues/179

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use super::graph::TransitionAction;
use crate::error::{AppError, ValidationDetail};

/// Where a task is in its own life (§7.6's `CHECK`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TaskStatus {
    Created,
    Assigned,
    InProgress,
    Completed,
    Delegated,
    Escalated,
    Cancelled,
}

impl TaskStatus {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Created => "CREATED",
            Self::Assigned => "ASSIGNED",
            Self::InProgress => "IN_PROGRESS",
            Self::Completed => "COMPLETED",
            Self::Delegated => "DELEGATED",
            Self::Escalated => "ESCALATED",
            Self::Cancelled => "CANCELLED",
        }
    }

    /// An unknown stored value reads as `Cancelled`.
    ///
    /// Fails closed in the direction that matters here: a task nobody can
    /// classify must not read as open, because an open task is one this engine
    /// will let somebody act on.
    pub fn from_db(value: &str) -> Self {
        match value {
            "CREATED" => Self::Created,
            "ASSIGNED" => Self::Assigned,
            "IN_PROGRESS" => Self::InProgress,
            "COMPLETED" => Self::Completed,
            "DELEGATED" => Self::Delegated,
            "ESCALATED" => Self::Escalated,
            _ => Self::Cancelled,
        }
    }

    /// Whether the task is still open for somebody to act on.
    ///
    /// The one predicate the claim statement, the decision statement, the
    /// "one open task per instance" index and the inbox filter are all written
    /// in terms of. `DELEGATED` is open — Sprint 11's delegation moves a task
    /// to another person rather than finishing it — and `ESCALATED` is too, for
    /// the same reason.
    pub fn is_open(self) -> bool {
        matches!(self, Self::Created | Self::Assigned | Self::InProgress)
    }
}

/// What a caller may do to a task in Sprint 10.
///
/// **Two of them**, and the rest of JWSS's transition vocabulary is deliberately
/// absent: `RETURN` is FR-WF-008 ([#183]) and `DELEGATE` is FR-WF-009 ([#184]),
/// both Sprint 11. Accepting a verb this engine cannot complete would be a 500
/// where the contract promises a refusal, so the request type does not have the
/// variant at all and an unknown value is refused by `serde` at the boundary.
///
/// [#183]: https://github.com/sujanto-gaws/kelir/issues/183
/// [#184]: https://github.com/sujanto-gaws/kelir/issues/184
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DecisionAction {
    Approve,
    Reject,
}

impl DecisionAction {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Approve => "APPROVE",
            Self::Reject => "REJECT",
        }
    }

    /// The transition this decision fires.
    pub fn transition(self) -> TransitionAction {
        match self {
            Self::Approve => TransitionAction::Approve,
            Self::Reject => TransitionAction::Reject,
        }
    }
}

/// The body of a decision request.
///
/// **There is no `comment` field, and its absence is a decision.** FR-TASK-006
/// is [#182](https://github.com/sujanto-gaws/kelir/issues/182) in Sprint 11;
/// `workflow_tasks.comment` and `approval_decisions.comment` exist from `0024`
/// and nothing writes them. The cost is real and belongs where somebody will
/// read it: **a rejection recorded this sprint has no reason on it.**
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DecisionRequest {
    pub action: DecisionAction,
}

/// How a task reached the caller who is looking at it ([#179] AC1).
///
/// Not a nullable assignee for the reader to interpret: a client that has to
/// derive "is this mine or is it going spare" will derive it differently in two
/// places, and the two situations need different words on the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Assignment {
    /// Assigned to the caller, by name.
    Mine,
    /// Unclaimed, and offered to a role the caller holds.
    Role,
}

/// A task as the API returns it.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowTask {
    pub id: Uuid,
    pub task_ref: String,
    pub workflow_instance_id: Uuid,
    pub document_id: Uuid,
    pub task_definition_key: String,
    pub task_name: String,
    pub task_type: String,
    pub status: TaskStatus,
    pub assignee_user_id: Option<Uuid>,
    pub candidate_role_id: Option<Uuid>,
    pub candidate_role_code: Option<String>,
    pub candidate_department_id: Option<Uuid>,
    pub priority: String,
    pub due_at: Option<DateTime<Utc>>,
    pub action: Option<DecisionAction>,
    pub completed_by: Option<Uuid>,
    pub completed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// Refuses an action on a task that is no longer open ([#177] AC2).
///
/// **A 409 naming the current status**, not a 500 and not a silent no-op. The
/// second approver of a task somebody else has already decided has to be able to
/// tell "you were too late" from "the server broke", because those need
/// different things from them.
pub fn refuse_unless_open(status: TaskStatus) -> Result<(), AppError> {
    if status.is_open() {
        return Ok(());
    }

    Err(AppError::conflict(format!(
        "this task is {} and a decision has already been recorded against it; \
         recording a second one would be a signature on a decision somebody else made",
        status.as_db()
    )))
}

/// Refuses a decision the caller is not the one to make ([#177] AC5).
///
/// **This is not the permission check.** `workflow:task:execute` says the caller
/// may work tasks at all; this says whether *this* task is theirs, and the two
/// are different questions. A deployment that grants the permission broadly and
/// relies on this is doing exactly what it should.
pub fn refuse_unless_theirs(
    caller: Uuid,
    assignee: Option<Uuid>,
    holds_candidate_role: bool,
) -> Result<(), AppError> {
    match assignee {
        Some(assignee) if assignee == caller => Ok(()),
        // Unclaimed and offered to a role the caller holds. **They may act
        // without claiming first**, and claiming stays useful: it is how a
        // person tells the rest of the queue they have started. Requiring a
        // claim would add a round trip whose only effect is a round trip.
        None if holds_candidate_role => Ok(()),
        _ => Err(AppError::Forbidden),
    }
}

/// Refuses a claim of a task that is already somebody's ([#176] AC3).
///
/// Raised after the compare-and-swap has updated no rows and the row has been
/// re-read, so it can say which of the two things happened — taken, or finished
/// — and those are different situations for the person who lost.
pub fn claim_lost(status: TaskStatus, assignee: Option<Uuid>) -> AppError {
    if assignee.is_some() {
        AppError::conflict(
            "this task was claimed by somebody else while this claim was being applied",
        )
    } else {
        AppError::conflict(format!(
            "this task is {} and cannot be claimed",
            status.as_db()
        ))
    }
}

/// Refuses an action the definition does not offer from where the instance is.
///
/// A 422 rather than a 409: the request names an action the process cannot take
/// *from the state it is in*, which is a property of the payload against the
/// resource. A 409 is what a **concurrent** change earns, and the service raises
/// that one — the two are different failures and a caller fixes them
/// differently. The same split `DocumentStatus::check_move_to` makes.
pub fn no_such_transition(state: &str, action: DecisionAction, available: &[&str]) -> AppError {
    let message = if available.is_empty() {
        format!("`{state}` is final; nothing moves the process from there")
    } else {
        format!(
            "`{state}` has no {} transition. From here: {}",
            action.as_db(),
            available.join(", ")
        )
    };

    AppError::validation(vec![ValidationDetail::new(
        "action",
        "transition",
        "NO_SUCH_TRANSITION",
        message,
    )])
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [TaskStatus; 7] = [
        TaskStatus::Created,
        TaskStatus::Assigned,
        TaskStatus::InProgress,
        TaskStatus::Completed,
        TaskStatus::Delegated,
        TaskStatus::Escalated,
        TaskStatus::Cancelled,
    ];

    #[test]
    fn every_status_round_trips_through_the_database_spelling() {
        for status in ALL {
            assert_eq!(TaskStatus::from_db(status.as_db()), status);
        }
    }

    #[test]
    fn an_unknown_stored_status_is_not_open() {
        // Fails closed: an unclassifiable task must not be one this engine lets
        // somebody act on.
        assert!(!TaskStatus::from_db("HANDED_OVER").is_open());
    }

    #[test]
    fn only_the_three_working_statuses_are_open() {
        for status in ALL {
            let expected = matches!(
                status,
                TaskStatus::Created | TaskStatus::Assigned | TaskStatus::InProgress
            );

            assert_eq!(
                status.is_open(),
                expected,
                "{} disagreed about being open",
                status.as_db()
            );
        }
    }

    #[test]
    fn a_completed_task_cannot_be_decided_again() {
        let error = refuse_unless_open(TaskStatus::Completed).expect_err("already decided");

        let AppError::Conflict { message } = error else {
            panic!("expected a conflict, not a validation failure or a 500");
        };

        assert!(message.contains("COMPLETED"), "{message}");
    }

    #[test]
    fn the_assignee_may_act_and_a_third_party_may_not() {
        let assignee = Uuid::now_v7();
        let stranger = Uuid::now_v7();

        assert!(refuse_unless_theirs(assignee, Some(assignee), false).is_ok());
        assert!(refuse_unless_theirs(stranger, Some(assignee), false).is_err());
        // Holding the candidate role does not make somebody the assignee of a
        // task that already has one: the person it was assigned to is the
        // person who decides it.
        assert!(refuse_unless_theirs(stranger, Some(assignee), true).is_err());
    }

    #[test]
    fn an_unclaimed_role_task_is_actionable_by_a_role_holder_and_nobody_else() {
        let caller = Uuid::now_v7();

        assert!(refuse_unless_theirs(caller, None, true).is_ok());
        assert!(refuse_unless_theirs(caller, None, false).is_err());
    }

    #[test]
    fn a_refusal_names_the_state_and_what_was_possible() {
        let error = no_such_transition("MANAGER_APPROVAL", DecisionAction::Approve, &["REJECT"]);

        let AppError::Validation { details } = error else {
            panic!("expected a validation failure");
        };

        assert_eq!(details[0].code, "NO_SUCH_TRANSITION");
        assert!(
            details[0].message.contains("MANAGER_APPROVAL"),
            "{details:?}"
        );
        assert!(details[0].message.contains("REJECT"), "{details:?}");
    }

    #[test]
    fn a_decision_request_with_a_misspelled_field_is_refused() {
        // #62, and it matters more here than usual: a decision with an
        // unrecognised field silently dropped is an approval recorded from a
        // request the caller believes said something else.
        let refused: Result<DecisionRequest, _> =
            serde_json::from_str(r#"{"decision": "APPROVE"}"#);
        assert!(refused.is_err());

        // And a verb Sprint 11 owns is refused at the boundary rather than
        // reaching an engine that cannot complete it.
        let refused: Result<DecisionRequest, _> = serde_json::from_str(r#"{"action": "RETURN"}"#);
        assert!(refused.is_err());
    }
}
