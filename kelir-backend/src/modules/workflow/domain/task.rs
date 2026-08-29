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
    /// in terms of.
    ///
    /// **`DELEGATED` and `ESCALATED` are outside it, and nothing writes
    /// either.** This comment used to say the opposite about `DELEGATED` — that
    /// Sprint 11's delegation would leave a task open under that status — and
    /// [#184](https://github.com/sujanto-gaws/kelir/issues/184) is the sprint
    /// that found out it cannot. `uq_workflow_tasks_open_per_instance` and the
    /// inbox's open filter are `CHECK`- and `WHERE`-clause copies of this list
    /// in the database; a delegated task carrying a status outside it would
    /// leave its instance with no open task, stop guarding the instance against
    /// a second one, and vanish from the inbox of the person who had just been
    /// given it.
    ///
    /// So delegation does not move the status at all: it moves
    /// `assignee_user_id` and records `delegated_from_user_id`. **Who holds a
    /// task and where the work has got to are two questions**, and putting the
    /// first into a column that answers the second is the failure this schema
    /// has refused before. `DELEGATED` keeps `STARTED`'s standing on
    /// `workflow_instances`: it is in §7.6's `CHECK` because it was specified,
    /// and the product does not produce it.
    pub fn is_open(self) -> bool {
        matches!(self, Self::Created | Self::Assigned | Self::InProgress)
    }
}

/// What a caller may do to a task.
///
/// **Three of them, and `DELEGATE` is still not one** — which is the shape of
/// [#184] rather than an omission it left behind. A decision is an answer about
/// the document that moves the process; handing a task to somebody else answers
/// nothing and moves nothing, so it is its own route
/// (`POST /workflow/tasks/{id}/delegation`) with its own request type rather
/// than a fourth variant here. A `Delegate` in this enum would be a verb
/// `POST /decision` had to accept and then refuse, at the layer below the one
/// that knows why.
///
/// `ESCALATE` is FR-WF-010 and unscheduled. Accepting a verb this engine cannot
/// complete would be a 500 where the contract promises a refusal, so the request
/// type does not have the variant at all and an unknown value is refused by
/// `serde` at the boundary.
///
/// # `RETURN` is here and `RESUBMIT` is not, and that asymmetry is the design
///
/// A return is taken **on a task**, by the approver holding it, which is what
/// makes it a decision like the other two. A resubmission is taken on the
/// *document*, by its owner, from a state that declares no task at all — JWSS's
/// own §10 example has `RETURNED` stateless with a `RESUBMIT` edge `allowedBy`
/// the owner. So it arrives through `POST /documents/{id}/submission`
/// ([`crate::modules::document::service::submit`]), and a `Resubmit` variant
/// here would be a verb with no task to name in the path.
///
/// [#183]: https://github.com/sujanto-gaws/kelir/issues/183
/// [#184]: https://github.com/sujanto-gaws/kelir/issues/184
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DecisionAction {
    Approve,
    Reject,
    /// Send it back for correction (FR-WF-008, [#183]).
    ///
    /// **Not a rejection with a softer name.** Reject is terminal and return is
    /// not: the document goes to the state the definition's `RETURN` edge names,
    /// becomes editable again, and comes back with the same number, the same
    /// history and the same place in the queue.
    Return,
}

impl DecisionAction {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Approve => "APPROVE",
            Self::Reject => "REJECT",
            Self::Return => "RETURN",
        }
    }

    /// The transition this decision fires.
    pub fn transition(self) -> TransitionAction {
        match self {
            Self::Approve => TransitionAction::Approve,
            Self::Reject => TransitionAction::Reject,
            Self::Return => TransitionAction::Return,
        }
    }
}

/// Longest decision comment this API accepts.
///
/// `workflow_tasks.comment` and the two columns beside it are `TEXT`, so nothing
/// in the schema bounds this and the bound has to be here. Four thousand
/// characters is far more than a reason for a decision needs and far less than a
/// body somebody could use to fill an append-only table: the history row is
/// never edited or deleted ([#181] AC6), so an unbounded comment is an unbounded
/// row nobody can take back out.
///
/// [#181]: https://github.com/sujanto-gaws/kelir/issues/181
pub const MAX_COMMENT_LENGTH: usize = 4000;

/// The body of a decision request (FR-TASK-006; [#182]).
///
/// **`comment` is optional here and may be mandatory there.** Whether a reason
/// is required is the *definition's* to say, per transition
/// ([JWSS](../../../../../docs/schema/JSON%20Workflow%20Schema.md) §4.1) — an
/// approval explains itself and a refusal does not — so this type cannot know,
/// and [`super::super::service::engine`] refuses against the edge that was
/// actually chosen. A `#[serde(default)]` rather than a required field is what
/// keeps an `APPROVE` on an unmarked edge one field long.
///
/// [#182]: https://github.com/sujanto-gaws/kelir/issues/182
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DecisionRequest {
    pub action: DecisionAction,
    #[serde(default)]
    pub comment: Option<String>,
}

/// The comment as the record should hold it, or a refusal.
///
/// Two normalizations, and each has a consequence rather than a tidiness
/// motive:
///
/// * **Trimmed, and whitespace becomes absent.** `"   "` is not a reason, and a
///   client that sends one has an empty box on the screen. Storing it would
///   satisfy a `requiresComment` edge with nothing, which is the requirement
///   defeated by a space bar.
/// * **Bounded**, by [`MAX_COMMENT_LENGTH`], as a 422 naming the field rather
///   than a `sqlx` error — the reason `master_data::domain` gives for checking a
///   length the database would otherwise report in its own words.
pub fn normalize_comment(comment: Option<String>) -> Result<Option<String>, AppError> {
    let Some(comment) = comment else {
        return Ok(None);
    };

    let trimmed = comment.trim();

    if trimmed.is_empty() {
        return Ok(None);
    }

    if trimmed.chars().count() > MAX_COMMENT_LENGTH {
        return Err(AppError::validation(vec![ValidationDetail::new(
            "comment",
            "maxLength",
            "TOO_LONG",
            format!("a decision comment is at most {MAX_COMMENT_LENGTH} characters"),
        )]));
    }

    Ok(Some(trimmed.to_owned()))
}

/// Refuses a decision on an edge whose definition requires a reason
/// ([#182](https://github.com/sujanto-gaws/kelir/issues/182) AC4).
///
/// A **422 naming `comment`**, which is what makes the client's half and this
/// half the same rule: the screen refuses an empty box before sending, and a
/// caller that is not the screen gets the same refusal against the same field
/// path. A 409 would be wrong — nothing has changed underneath the caller — and
/// a 403 would be wrong twice, because they may take this edge; they have not
/// said why.
pub fn comment_required(state: &str, action: &str) -> AppError {
    AppError::validation(vec![ValidationDetail::new(
        "comment",
        "requiresComment",
        "COMMENT_REQUIRED",
        format!(
            "the `{action}` transition out of `{state}` requires a comment; \
             this workflow asks for the reason to be recorded with the decision"
        ),
    )])
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
    /// Whose authority the assignee is exercising ([#184] AC2, AC4).
    ///
    /// Set when a delegation window routed this task past the person the
    /// definition named, and when its holder handed it over. `None` on a task
    /// nobody is standing in for, which is almost all of them.
    pub delegated_from_user_id: Option<Uuid>,
    pub priority: String,
    pub due_at: Option<DateTime<Utc>>,
    pub action: Option<DecisionAction>,
    pub completed_by: Option<Uuid>,
    pub completed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// The body of a delegation request (FR-WF-009, FR-TASK-008; [#184]).
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DelegateRequest {
    /// Who is to hold the task instead.
    pub delegate_user_id: Uuid,
    /// Why, if the person handing it over wants to say.
    ///
    /// Lands on the task's own history row and nowhere else. It is not a
    /// decision comment — nothing was decided — so the three columns
    /// FR-TASK-006 fills stay untouched, and `workflow_task_history.comment`
    /// gets its first writer.
    #[serde(default)]
    pub comment: Option<String>,
}

/// Refuses a hand-off of a task the caller does not personally hold ([#184]).
///
/// **Stricter than [`refuse_unless_theirs`], deliberately.** That one lets an
/// unclaimed role task be decided by any holder of the role, because deciding it
/// is what the queue is for. Handing it on is different: an unclaimed task is
/// still being offered to everybody who holds the role, and giving it to one
/// named person would be one holder taking it out of everybody else's queue and
/// assigning it — a claim and a delegation at once, neither of them asked for.
///
/// The refusal names the way out, because there is one and it is one request:
/// claim it, then hand it on.
pub fn refuse_unless_held_by(caller: Uuid, assignee: Option<Uuid>) -> Result<(), AppError> {
    match assignee {
        Some(assignee) if assignee == caller => Ok(()),
        Some(_) => Err(AppError::Forbidden),
        None => Err(AppError::conflict(
            "this task is offered to a role and nobody has taken it, so there is \
             nothing yet to hand over; claim it first, then delegate it",
        )),
    }
}

/// Refuses a delegation to the person already holding the task.
///
/// `ck_delegations_not_self` says the same thing about a window, one module
/// over; this is the point-in-time hand-off's copy of it, and the reason is the
/// same — a delegation to yourself changes nothing and reads, to whoever finds
/// the record, as cover that is not there.
pub fn refuse_self_delegation(caller: Uuid, delegate: Uuid) -> Result<(), AppError> {
    if caller != delegate {
        return Ok(());
    }

    Err(AppError::validation(vec![ValidationDetail::new(
        "delegateUserId",
        "notSelf",
        "DELEGATE_IS_HOLDER",
        "this task is already yours; a delegation hands it to somebody else",
    )]))
}

/// Refuses a delegation to somebody who cannot act on it.
///
/// **`ACTIVE`, not merely present** — `identity::delegation_repository::user_is_available`
/// carries the reasoning, and this is the same refusal at the other end of the
/// same feature: a task handed to an account that cannot sign in is an approval
/// that has stopped, and it looks assigned the whole time.
pub fn delegate_unavailable() -> AppError {
    AppError::validation(vec![ValidationDetail::new(
        "delegateUserId",
        "exists",
        "NOT_AVAILABLE",
        "no active user with that id in this tenant; a task has to be handed to \
         somebody who can sign in and act on it",
    )])
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
    fn a_comment_of_whitespace_is_no_comment_at_all() {
        // The load-bearing case, and the reason this is a function rather than
        // an `Option` passed through: an edge that requires a reason must not be
        // satisfied by a box the person tabbed past.
        assert_eq!(
            normalize_comment(Some("   \n\t ".to_owned())).unwrap(),
            None
        );
        assert_eq!(normalize_comment(Some(String::new())).unwrap(), None);
        assert_eq!(normalize_comment(None).unwrap(), None);
    }

    #[test]
    fn a_comment_is_stored_without_the_whitespace_around_it() {
        assert_eq!(
            normalize_comment(Some("  over budget for Q3  ".to_owned())).unwrap(),
            Some("over budget for Q3".to_owned())
        );
    }

    #[test]
    fn a_comment_is_bounded_and_the_refusal_names_the_field() {
        // Counted in characters rather than bytes, so a reason written in a
        // non-Latin script is not refused at a quarter of the stated length.
        let long = "\u{4e00}".repeat(MAX_COMMENT_LENGTH + 1);

        let AppError::Validation { details } = normalize_comment(Some(long)).expect_err("too long")
        else {
            panic!("expected a validation failure");
        };

        assert_eq!(details[0].path, "comment");
        assert_eq!(details[0].code, "TOO_LONG");

        // And the boundary itself is accepted, so the limit is the limit.
        let at_limit = "\u{4e00}".repeat(MAX_COMMENT_LENGTH);
        assert!(normalize_comment(Some(at_limit)).is_ok());
    }

    #[test]
    fn a_required_comment_is_refused_as_a_422_naming_the_edge() {
        // #182 AC4's server half. It names the state and the action because the
        // person reading it is looking at one task among several, and "a
        // comment is required" does not say which decision wanted one.
        let error = comment_required("MANAGER_APPROVAL", "REJECT");

        let AppError::Validation { details } = error else {
            panic!("expected a validation failure, not a conflict or a 403");
        };

        assert_eq!(details[0].path, "comment");
        assert_eq!(details[0].code, "COMMENT_REQUIRED");
        assert!(
            details[0].message.contains("MANAGER_APPROVAL"),
            "{details:?}"
        );
        assert!(details[0].message.contains("REJECT"), "{details:?}");
    }

    #[test]
    fn a_decision_may_carry_a_comment_and_may_omit_it() {
        // Both shapes deserialize, which is what keeps an APPROVE on an
        // unmarked edge one field long while a REJECT on a marked one is two.
        let with: DecisionRequest =
            serde_json::from_str(r#"{"action": "REJECT", "comment": "over budget"}"#)
                .expect("a decision with a reason");
        assert_eq!(with.comment.as_deref(), Some("over budget"));

        let without: DecisionRequest =
            serde_json::from_str(r#"{"action": "APPROVE"}"#).expect("a decision without one");
        assert_eq!(without.comment, None);
    }

    #[test]
    fn a_decision_request_with_a_misspelled_field_is_refused() {
        // #62, and it matters more here than usual: a decision with an
        // unrecognised field silently dropped is an approval recorded from a
        // request the caller believes said something else.
        let refused: Result<DecisionRequest, _> =
            serde_json::from_str(r#"{"decision": "APPROVE"}"#);
        assert!(refused.is_err());

        // And a verb this engine cannot complete is refused at the boundary
        // rather than reaching an engine that cannot complete it. `RETURN`
        // moved out of this list when #183 built it; `DELEGATE` is #184 and is
        // what the assertion is about now.
        let refused: Result<DecisionRequest, _> = serde_json::from_str(r#"{"action": "DELEGATE"}"#);
        assert!(refused.is_err());

        let accepted: Result<DecisionRequest, _> = serde_json::from_str(r#"{"action": "RETURN"}"#);
        assert!(accepted.is_ok(), "RETURN is a decision this engine takes");
    }
}
