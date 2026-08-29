//! A definition in flight, and the variables it carries (FR-WF-014, [#175]).
//!
//! # The instance's state is the only copy of where the process is
//!
//! [#175]'s AC3: *"the instance's current state is the single source of truth
//! for where the process is. Nothing else stores a duplicate of it."*
//! `workflow_instances.current_state` is that value, and
//! [`super::super`]'s module documentation states how the document's own status
//! relates to it — a projection, one-way, mapped by the definition.
//!
//! A `workflow_tasks` row carries a status too, and that is the **task's**
//! status rather than the process's: whether this piece of human work is open,
//! taken or finished. The two move together and answer different questions, and
//! [`super::task`] says so where a reader will be looking at the second one.
//!
//! # The version pin is the foreign key, and there is no column beside it
//!
//! AC1 asks that an instance record the definition version it started against
//! and keep running it. `workflow_instances.workflow_definition_id` names a
//! *revision row*, and a published revision row never changes — so a
//! `definition_version` column beside it would be a second copy of a fact the
//! reference already carries, which is AC3's failure in the other direction. The
//! API answers the question by joining; the database stores it once.
//!
//! [#175]: https://github.com/sujanto-gaws/kelir/issues/175

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::{AppError, ValidationDetail};

/// Where a running process is (§7.4's `CHECK`).
///
/// **`Started` is in the vocabulary and this engine never writes it.** An
/// instance is running from the moment it exists — the transaction that inserts
/// it also enters the initial state — so a status every row would leave inside
/// one statement is a status a reader would spend time on for nothing. The value
/// stays in the column because §7.4 put it there, and this comment is where a
/// reader finds out that nothing produces it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum InstanceStatus {
    Started,
    Running,
    Suspended,
    Completed,
    Cancelled,
    Failed,
}

impl InstanceStatus {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Started => "STARTED",
            Self::Running => "RUNNING",
            Self::Suspended => "SUSPENDED",
            Self::Completed => "COMPLETED",
            Self::Cancelled => "CANCELLED",
            Self::Failed => "FAILED",
        }
    }

    /// An unknown stored value reads as `Failed`.
    ///
    /// Fails closed: a status nobody recognises must not read as `RUNNING`,
    /// because a running instance is one somebody may still be asked to act on.
    pub fn from_db(value: &str) -> Self {
        match value {
            "STARTED" => Self::Started,
            "RUNNING" => Self::Running,
            "SUSPENDED" => Self::Suspended,
            "COMPLETED" => Self::Completed,
            "CANCELLED" => Self::Cancelled,
            _ => Self::Failed,
        }
    }

    /// Whether the process is still going, which is the predicate the "one live
    /// instance per document" rule is written in terms of.
    ///
    /// One predicate rather than a three-way `matches!` in four services: the
    /// question "is this instance live" is asked by the start path, the seam,
    /// the inbox and the document workspace, and four spellings of it are four
    /// chances for one of them to disagree.
    pub fn is_live(self) -> bool {
        matches!(self, Self::Started | Self::Running | Self::Suspended)
    }
}

/// How a process ended (§7.4's `outcome` `CHECK`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum InstanceOutcome {
    Approved,
    Rejected,
    Returned,
    Cancelled,
}

impl InstanceOutcome {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Approved => "APPROVED",
            Self::Rejected => "REJECTED",
            Self::Returned => "RETURNED",
            Self::Cancelled => "CANCELLED",
        }
    }

    pub fn from_db(value: &str) -> Option<Self> {
        Some(match value {
            "APPROVED" => Self::Approved,
            "REJECTED" => Self::Rejected,
            "RETURNED" => Self::Returned,
            "CANCELLED" => Self::Cancelled,
            _ => return None,
        })
    }

    /// The outcome a final state implies, from the document status it maps to.
    ///
    /// **Derived from the definition rather than from the action that got
    /// there**, which is the same rule [#178](https://github.com/sujanto-gaws/kelir/issues/178)
    /// AC4 applies to the document's status: what a state *means* is the
    /// definition's to say. A workflow whose `REJECT` leads to a state mapping
    /// to `COMPLETED` is a strange workflow, and recording its outcome as
    /// `REJECTED` would be this engine overruling it.
    pub fn from_document_status(status: &str) -> Option<Self> {
        Some(match status {
            "APPROVED" | "COMPLETED" => Self::Approved,
            "REJECTED" => Self::Rejected,
            "RETURNED" => Self::Returned,
            "CANCELLED" | "ARCHIVED" => Self::Cancelled,
            _ => return None,
        })
    }
}

/// A workflow variable as the API returns it.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowVariable {
    pub key: String,
    pub data_type: String,
    /// The value, typed back the way the definition declared it.
    ///
    /// `workflow_variables.variable_value` is `TEXT` with a `data_type` beside
    /// it, so the typing is Kelir's; handing a caller the string `"45000000"`
    /// for a `NUMBER` would make every consumer parse it again, differently.
    #[schema(value_type = Object)]
    pub value: Value,
}

/// A running process, as the API returns it.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowInstance {
    pub id: Uuid,
    pub instance_ref: String,
    pub document_id: Uuid,
    pub workflow_definition_id: Uuid,
    pub workflow_key: String,
    pub workflow_name: String,
    /// The revision this instance is running, joined from the definition it
    /// pins rather than stored again (AC1, and the module documentation).
    pub definition_version: i32,
    pub status: InstanceStatus,
    pub current_state: String,
    /// The state's display name from the definition, so a screen does not have
    /// to hold a copy of the workflow to render `MANAGER_APPROVAL` as
    /// "Manager approval".
    pub current_state_name: String,
    pub outcome: Option<InstanceOutcome>,
    pub business_key: Option<String>,
    pub started_by: Option<Uuid>,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub variables: Vec<WorkflowVariable>,
}

/// Parses a stored variable back into a JSON value of its declared type.
///
/// Lenient in one direction only: a value that does not parse comes back as the
/// **string it was stored as** rather than as an error. The strictness is at the
/// write ([#175](https://github.com/sujanto-gaws/kelir/issues/175) AC2, and
/// [`check_variable`]), which is where a person can still fix it; failing a read
/// of a running approval because a variable written months ago no longer parses
/// would hide the whole instance to complain about one field.
pub fn read_variable(value: &str, data_type: &str) -> Value {
    match data_type {
        "NUMBER" => value
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number)
            .unwrap_or_else(|| Value::String(value.to_owned())),
        "BOOLEAN" => value
            .parse::<bool>()
            .map(Value::Bool)
            .unwrap_or_else(|_| Value::String(value.to_owned())),
        "JSON" => serde_json::from_str(value).unwrap_or_else(|_| Value::String(value.to_owned())),
        // `STRING` and `DATE` are both stored and returned as strings. A date is
        // not parsed into a `DateTime` because JSON has no date type and the
        // caller would receive a string either way — parsing it here would only
        // change *which* string.
        _ => Value::String(value.to_owned()),
    }
}

/// Renders a value for storage, or refuses it as the declared type (AC2).
///
/// A `NUMBER` variable that a `source` computed to `"not a number"` is a
/// definition defect, and storing it would make every later read of that
/// variable answer something no consumer expects.
pub fn write_variable(key: &str, value: &Value, data_type: &str) -> Result<String, AppError> {
    let refuse = |expected: &str| {
        Err(AppError::validation(vec![ValidationDetail::new(
            format!("variables.{key}"),
            "type",
            "WRONG_VARIABLE_TYPE",
            format!(
                "`{key}` is declared {data_type} and the value is not {expected}; the \
                 definition's declaration is what a routing condition reads it as"
            ),
        )]))
    };

    match data_type {
        "NUMBER" => match value {
            Value::Number(number) => Ok(number.to_string()),
            _ => refuse("a number"),
        },
        "BOOLEAN" => match value {
            Value::Bool(flag) => Ok(flag.to_string()),
            _ => refuse("a boolean"),
        },
        "JSON" => Ok(value.to_string()),
        // `STRING` and `DATE`. A date is a string in JSON either way, so
        // there is nothing a separate arm would do differently.
        _ => match value {
            Value::String(text) => Ok(text.clone()),
            // A `STRING` variable given a number is rendered rather than
            // refused: JSON Logic has no string coercion the two engines agree
            // on, and `45000000` written into a string variable is what the
            // definition asked for.
            Value::Number(number) => Ok(number.to_string()),
            Value::Bool(flag) => Ok(flag.to_string()),
            _ => refuse("a string"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_live_instance_is_one_a_person_may_still_be_asked_to_act_on() {
        for status in [
            InstanceStatus::Started,
            InstanceStatus::Running,
            InstanceStatus::Suspended,
        ] {
            assert!(status.is_live(), "{} is not live", status.as_db());
        }

        for status in [
            InstanceStatus::Completed,
            InstanceStatus::Cancelled,
            InstanceStatus::Failed,
        ] {
            assert!(!status.is_live(), "{} is live", status.as_db());
        }
    }

    #[test]
    fn an_unknown_stored_status_does_not_read_as_running() {
        assert_eq!(InstanceStatus::from_db("RUNNING"), InstanceStatus::Running);
        assert_eq!(InstanceStatus::from_db("PAUSED"), InstanceStatus::Failed);
        assert!(!InstanceStatus::from_db("PAUSED").is_live());
    }

    #[test]
    fn the_outcome_comes_from_the_state_the_definition_named() {
        // #178 AC4's rule, applied to the outcome: what a state means is the
        // definition's to say, not this engine's to infer from the verb that
        // reached it.
        assert_eq!(
            InstanceOutcome::from_document_status("COMPLETED"),
            Some(InstanceOutcome::Approved)
        );
        assert_eq!(
            InstanceOutcome::from_document_status("REJECTED"),
            Some(InstanceOutcome::Rejected)
        );
        assert_eq!(InstanceOutcome::from_document_status("SUBMITTED"), None);
    }

    #[test]
    fn a_variable_round_trips_through_its_declared_type() {
        for (value, data_type) in [
            (json!(45_000_000.0), "NUMBER"),
            (json!(true), "BOOLEAN"),
            (json!("PROC"), "STRING"),
            (json!({ "a": 1 }), "JSON"),
        ] {
            let stored = write_variable("v", &value, data_type).expect("a storable value");

            assert_eq!(
                read_variable(&stored, data_type),
                value,
                "{data_type} did not round trip"
            );
        }
    }

    #[test]
    fn a_value_of_the_wrong_type_is_refused_at_the_write() {
        let error = write_variable("amount", &json!("lots"), "NUMBER")
            .expect_err("a string is not a number");

        let AppError::Validation { details } = error else {
            panic!("expected a validation failure");
        };

        assert_eq!(details[0].code, "WRONG_VARIABLE_TYPE");
        assert_eq!(details[0].path, "variables.amount");
    }

    #[test]
    fn a_value_that_no_longer_parses_reads_back_as_its_stored_text() {
        // The asymmetry is deliberate: strict at the write, where somebody can
        // still fix it, and lenient at the read, because refusing to show a
        // running approval over one field would hide the whole instance.
        assert_eq!(read_variable("lots", "NUMBER"), json!("lots"));
    }
}

/// One transition, as the document workspace renders it (FR-WF-012; [#181]).
///
/// **`occurredAt` rather than `createdAt`**, though the column is `created_at`.
/// The row's creation and the event are the same instant here — it is written
/// in the transition's own transaction — and naming the field for the event is
/// what stops a reader treating the list as a log of writes rather than as an
/// account of the process.
///
/// `actorUsername` is resolved for display beside the id, because a history
/// showing UUIDs answers *how did this get here* only to somebody who can look
/// them up. It is `None` for an engine action and for a user since deleted.
///
/// [#181]: https://github.com/sujanto-gaws/kelir/issues/181
#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowHistoryEntry {
    pub id: Uuid,
    /// `None` on the first row: the initial state came from nowhere.
    pub from_state: Option<String>,
    pub to_state: String,
    /// `None` when nothing named an action — the start.
    pub action: Option<String>,
    /// The task the decision came from, when a decision moved it.
    pub task_id: Option<Uuid>,
    /// The reason given with the decision (FR-TASK-006,
    /// [#182](https://github.com/sujanto-gaws/kelir/issues/182)).
    ///
    /// **This is the copy a person reads.** The same sentence is on the task
    /// and on the formal decision record; this is the one the document
    /// workspace renders beside the transition it explains, which is why #182
    /// AC2 names the history rather than either of the others.
    ///
    /// `None` on a transition nobody gave a reason for, and on every row no
    /// decision drove.
    pub comment: Option<String>,
    pub actor_user_id: Option<Uuid>,
    pub actor_username: Option<String>,
    /// Whose authority the actor was exercising, where a delegation put the
    /// task in their hands ([#184] AC4).
    ///
    /// **The reason the history is where the pair is recorded** rather than
    /// `approval_decisions`: §7.8 is the formal record and its approver is the
    /// one who signed, while this is the account somebody reads to answer *who
    /// approved this, and on whose authority*. A delegated approval that showed
    /// only the delegate would answer the first half and lose the second, which
    /// is the accountability delegation was supposed to preserve.
    ///
    /// `None` on every row nobody was standing in for.
    ///
    /// [#184]: https://github.com/sujanto-gaws/kelir/issues/184
    pub on_behalf_of_user_id: Option<Uuid>,
    pub on_behalf_of_username: Option<String>,
    /// Why this branch and not the other one (FR-WF-015, [#186] AC5).
    ///
    /// Every transition condition the engine evaluated, in the order S7 puts
    /// them, each with its outcome:
    ///
    /// ```json
    /// [{"to": "DIRECTOR_APPROVAL", "condition": {"…": …}, "outcome": false},
    ///  {"to": "FINANCE_APPROVAL",  "condition": {"…": …}, "outcome": true}]
    /// ```
    ///
    /// **The expression travels and the screen does not have to render it.**
    /// What a person reads is which branch was considered and whether it
    /// applied; the rule itself is in the workflow definition, where somebody
    /// who needs to change it is going anyway. It is on the wire because a
    /// history that answers *why* only for people with database access is a
    /// history that does not answer it.
    ///
    /// `null` on every row where nothing was evaluated — the instance's first
    /// state, and every action leaving one unconditioned edge.
    ///
    /// [#186]: https://github.com/sujanto-gaws/kelir/issues/186
    pub routing: Option<serde_json::Value>,
    pub occurred_at: DateTime<Utc>,
}
