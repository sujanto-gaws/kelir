//! The workflow definition as the **engine** reads it (FR-WF-002, FR-WF-003).
//!
//! [`jwss`][super::jwss] answers *is this document a valid workflow*. This file
//! answers *what does it say*, and it is a separate concern: a validator walks
//! JSON reporting every problem, and an engine needs typed states, transitions
//! and assignment rules with the shorthand normalized away.
//!
//! **It parses `definition_json`, not the projections.** `workflow_states` and
//! `workflow_transitions` are regenerated from the JSON on publish and exist for
//! the designer and for the foreign key on `workflow_instances.current_state`;
//! JWSS §1 calls the JSON the single source of truth. An engine reading the
//! projection would be executing a copy — two answers to "what does this
//! workflow do", which is the failure this module's `mod.rs` is about one layer
//! up.
//!
//! Parsing is **total and lenient**, because a stored definition has already
//! passed the validator: a field that cannot be read is absent rather than an
//! error, and the engine's own refusals (no such transition, an assignment that
//! resolves to nobody) are what a caller sees. A parser that could fail would
//! put a second class of failure on the approval path for a document that was
//! valid when it was published.

use serde_json::Value;

/// The action that fires a transition (JWSS §4).
///
/// Sprint 10 issues `SUBMIT`, `APPROVE` and `REJECT`. The rest are in the
/// vocabulary because the definition may declare them and the engine has to be
/// able to *read* a definition it cannot yet be asked to fire — refusing to
/// parse a `RETURN` would make Sprint 11's definitions unpublishable a sprint
/// early.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionAction {
    Submit,
    Approve,
    Reject,
    Return,
    Resubmit,
    Delegate,
    Escalate,
    Cancel,
    Complete,
    /// Fires without a caller. Nothing in Sprint 10 fires one — see
    /// [`super::super::service::engine`], which says so where it would.
    Auto,
}

impl TransitionAction {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Submit => "SUBMIT",
            Self::Approve => "APPROVE",
            Self::Reject => "REJECT",
            Self::Return => "RETURN",
            Self::Resubmit => "RESUBMIT",
            Self::Delegate => "DELEGATE",
            Self::Escalate => "ESCALATE",
            Self::Cancel => "CANCEL",
            Self::Complete => "COMPLETE",
            Self::Auto => "AUTO",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "SUBMIT" => Self::Submit,
            "APPROVE" => Self::Approve,
            "REJECT" => Self::Reject,
            "RETURN" => Self::Return,
            "RESUBMIT" => Self::Resubmit,
            "DELEGATE" => Self::Delegate,
            "ESCALATE" => Self::Escalate,
            "CANCEL" => Self::Cancel,
            "COMPLETE" => Self::Complete,
            "AUTO" => Self::Auto,
            _ => return None,
        })
    }
}

/// Who may act, with the §5.2 shorthand already normalized to the object form.
///
/// **Normalization happens once, here.** JWSS §5.2 permits `"OWNER"`,
/// `"ROLE:X"` and `"USER:X"` wherever an assignment rule is accepted, and
/// requires the engine to normalize before evaluating. Doing it at parse time
/// means nothing downstream — the resolver, the task writer, the authorization
/// check — has to know the shorthand exists, which is what stops one of the
/// three forgetting it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssignmentRule {
    pub assignee_type: AssigneeType,
    pub user_id: Option<String>,
    pub role_code: Option<String>,
    pub department_scope: Option<String>,
}

/// The assignee types this engine resolves.
///
/// Four of JWSS §5.1's six. `MANAGER_OF_OWNER` and `EXPRESSION` are refused at
/// **save** by [`jwss::assignment_errors`][super::jwss], so a stored definition
/// cannot contain one and this enum has no variant for them — which is the
/// point: an unrepresentable state cannot be forgotten about in a `match`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssigneeType {
    User,
    Role,
    DepartmentRole,
    Owner,
}

impl AssigneeType {
    fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "USER" => Self::User,
            "ROLE" => Self::Role,
            "DEPARTMENT_ROLE" => Self::DepartmentRole,
            "OWNER" => Self::Owner,
            _ => return None,
        })
    }
}

impl AssignmentRule {
    /// Reads a rule in either of JWSS's two forms.
    fn parse(value: &Value) -> Option<Self> {
        if let Some(shorthand) = value.as_str() {
            return Self::shorthand(shorthand);
        }

        let assignee_type = AssigneeType::parse(value.get("assigneeType")?.as_str()?)?;

        Some(Self {
            assignee_type,
            user_id: string(value, "userId"),
            role_code: string(value, "roleCode"),
            department_scope: string(value, "departmentScope"),
        })
    }

    /// §5.2's three shorthand strings.
    fn shorthand(value: &str) -> Option<Self> {
        if value == "OWNER" {
            return Some(Self {
                assignee_type: AssigneeType::Owner,
                user_id: None,
                role_code: None,
                department_scope: None,
            });
        }

        let (kind, name) = value.split_once(':')?;

        match kind {
            "ROLE" => Some(Self {
                assignee_type: AssigneeType::Role,
                user_id: None,
                role_code: Some(name.to_owned()),
                department_scope: None,
            }),
            "USER" => Some(Self {
                assignee_type: AssigneeType::User,
                user_id: Some(name.to_owned()),
                role_code: None,
                department_scope: None,
            }),
            _ => None,
        }
    }
}

/// What entering a state asks a person to do (JWSS §3.1).
#[derive(Debug, Clone)]
pub struct TaskSpec {
    pub task_definition_key: String,
    pub task_name: String,
    pub task_type: String,
    pub assignment: AssignmentRule,
    pub priority: String,
    /// How long the person generating this task has to do it (JWSS §3.1,
    /// FR-WF-011; [#185]).
    ///
    /// **Relative, never absolute**, which is the specification's choice and
    /// [#185] AC1's: a definition outlives every instance that runs it, so an
    /// absolute date in one is wrong for every instance after the first.
    ///
    /// Read by [`Self::due_in_seconds`], which is the only thing that turns it
    /// into a number the engine will use — see there for why the arithmetic
    /// itself does not happen in this process.
    ///
    /// [#185]: https://github.com/sujanto-gaws/kelir/issues/185
    pub due_in_hours: Option<f64>,
}

impl TaskSpec {
    /// The declared window in seconds, or `None` when there is no usable one.
    ///
    /// # Why this returns seconds rather than a due date
    ///
    /// **The stamp is computed by the database, not here** ([#185] AC4). The
    /// due date and the *overdue* comparison have to come from one clock, and
    /// the only clock both a write and every later read can share is the
    /// database's — an application fleet whose members' clocks drift would
    /// otherwise stamp two tasks generated a second apart as due hours apart,
    /// and no screen could tell. So this hands `insert_task` a duration and the
    /// statement adds it to `now()`.
    ///
    /// # And why an unusable value is dropped rather than refused
    ///
    /// A number that cannot be turned into an interval — not finite, not
    /// positive, or too large for one — produces `None`, and the caller warns
    /// and writes a task with no due date. **The alternative is failing the
    /// transition**, which would land a definition author's mistake on whoever
    /// happened to submit the next document; that is **D-24**'s direction,
    /// applied to a field that decorates an approval rather than deciding it.
    ///
    /// The meta-schema already refuses a non-positive `dueInHours` at save
    /// (`exclusiveMinimum: 0`), so this guard is about totality rather than
    /// validation: `Value::as_f64` on a row somebody wrote by hand can produce
    /// anything, and `parse` is documented as lenient.
    ///
    /// **The ceiling is a representability bound and not a product rule.**
    /// Nothing in Kelir says how far ahead a due date may be; what this says is
    /// that a hundred years of seconds is the most `make_interval` will be
    /// handed, so the statement cannot fail on arithmetic.
    pub fn due_in_seconds(&self) -> Option<f64> {
        /// A hundred years, in seconds. Far outside any deadline and far inside
        /// what an `interval` holds.
        const CEILING: f64 = 100.0 * 365.25 * 24.0 * 3600.0;

        let seconds = self.due_in_hours? * 3600.0;

        (seconds.is_finite() && seconds > 0.0 && seconds <= CEILING).then_some(seconds)
    }
}

/// A named step of the process.
#[derive(Debug, Clone)]
pub struct State {
    pub code: String,
    pub name: String,
    /// The map JWSS §1.1 calls the fixed platform spine, and
    /// [#178](https://github.com/sujanto-gaws/kelir/issues/178) AC4 requires to
    /// live in the definition rather than in code: a new workflow says what its
    /// own states mean for a document, and adding one needs no backend change.
    pub maps_to_document_status: String,
    pub is_final: bool,
    pub task: Option<TaskSpec>,
}

/// A directed edge, with the condition that gates it.
#[derive(Debug, Clone)]
pub struct Transition {
    pub from: String,
    pub to: String,
    pub action: TransitionAction,
    pub allowed_by: Option<AssignmentRule>,
    pub condition: Option<Value>,
    /// Whether firing this edge needs a reason (JWSS §4.1; FR-TASK-006).
    ///
    /// **Per edge, and that is the whole point of it being here rather than in
    /// code**: an approval explains itself and a refusal does not, so a workflow
    /// author marks `REJECT` and `RETURN` and leaves `APPROVE` alone. A
    /// hard-coded *rejections always need a reason* would decide that for every
    /// deployment.
    ///
    /// Absent reads as `false`, which is what a lenient parser must do with a
    /// definition published before the property existed — and is the same answer
    /// the meta-schema's `default` gives.
    pub requires_comment: bool,
}

/// A workflow variable's declaration (JWSS §6.3).
#[derive(Debug, Clone)]
pub struct VariableDeclaration {
    pub key: String,
    pub data_type: String,
    pub source: Option<Value>,
}

/// A parsed definition.
#[derive(Debug, Clone)]
pub struct Graph {
    pub workflow_key: String,
    pub initial_state: String,
    pub states: Vec<State>,
    pub transitions: Vec<Transition>,
    pub variables: Vec<VariableDeclaration>,
}

impl Graph {
    pub fn parse(definition: &Value) -> Self {
        Self {
            workflow_key: string(definition, "workflowKey").unwrap_or_default(),
            initial_state: string(definition, "initialState").unwrap_or_default(),
            states: array(definition, "states")
                .iter()
                .filter_map(parse_state)
                .collect(),
            transitions: array(definition, "transitions")
                .iter()
                .filter_map(parse_transition)
                .collect(),
            variables: array(definition, "variables")
                .iter()
                .filter_map(parse_variable)
                .collect(),
        }
    }

    pub fn state(&self, code: &str) -> Option<&State> {
        self.states.iter().find(|state| state.code == code)
    }

    /// Every transition leaving `from` on `action`, **fallback last**.
    ///
    /// S7's second half, which the validator cannot enforce and the engine must:
    /// *"at most one of them MAY omit `condition` (the fallback), and it is
    /// evaluated last regardless of document order."* Ordering here rather than
    /// at each call site is what makes that true of every caller there will
    /// ever be — including Sprint 11's, which has not been written.
    pub fn candidates(&self, from: &str, action: TransitionAction) -> Vec<&Transition> {
        let mut matching: Vec<&Transition> = self
            .transitions
            .iter()
            .filter(|transition| transition.from == from && transition.action == action)
            .collect();

        // A stable sort, so conditioned transitions keep their document order
        // relative to each other. Only the unconditioned one moves.
        matching.sort_by_key(|transition| transition.condition.is_none());
        matching
    }

    /// Every action that leaves `from`, in document order and without repeats.
    ///
    /// What a task detail shows the person deciding
    /// ([#179](https://github.com/sujanto-gaws/kelir/issues/179) AC4), and what
    /// a refusal names when an action does not apply.
    pub fn actions_from(&self, from: &str) -> Vec<&Transition> {
        self.transitions
            .iter()
            .filter(|transition| transition.from == from)
            .collect()
    }
}

fn parse_state(value: &Value) -> Option<State> {
    Some(State {
        code: string(value, "code")?,
        name: string(value, "name").unwrap_or_default(),
        maps_to_document_status: string(value, "mapsToDocumentStatus")?,
        is_final: value
            .get("isFinal")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        task: value.get("task").and_then(parse_task),
    })
}

fn parse_task(value: &Value) -> Option<TaskSpec> {
    Some(TaskSpec {
        task_definition_key: string(value, "taskDefinitionKey")?,
        task_name: string(value, "taskName").unwrap_or_default(),
        task_type: string(value, "taskType").unwrap_or_else(|| "APPROVAL_TASK".to_owned()),
        assignment: AssignmentRule::parse(value.get("assignment")?)?,
        priority: string(value, "priority").unwrap_or_else(|| "NORMAL".to_owned()),
        due_in_hours: value.get("dueInHours").and_then(Value::as_f64),
    })
}

fn parse_transition(value: &Value) -> Option<Transition> {
    Some(Transition {
        from: string(value, "from")?,
        to: string(value, "to")?,
        action: TransitionAction::parse(value.get("action")?.as_str()?)?,
        allowed_by: value.get("allowedBy").and_then(AssignmentRule::parse),
        condition: value.get("condition").cloned(),
        requires_comment: value
            .get("requiresComment")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

fn parse_variable(value: &Value) -> Option<VariableDeclaration> {
    Some(VariableDeclaration {
        key: string(value, "key")?,
        data_type: string(value, "dataType").unwrap_or_else(|| "STRING".to_owned()),
        source: value.get("source").cloned(),
    })
}

fn string(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn array<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn definition() -> Value {
        json!({
            "workflowKey": "purchase_requisition_standard",
            "version": "1.0.0",
            "name": "Standard",
            "initialState": "SUBMITTED",
            "states": [
                { "code": "SUBMITTED", "name": "Submitted", "mapsToDocumentStatus": "SUBMITTED",
                  "task": { "taskDefinitionKey": "manager_approval", "taskName": "Manager approval",
                            "assignment": { "assigneeType": "ROLE", "roleCode": "APPROVER" } } },
                { "code": "COMPLETED", "name": "Completed", "mapsToDocumentStatus": "COMPLETED",
                  "isFinal": true },
                { "code": "REJECTED", "name": "Rejected", "mapsToDocumentStatus": "REJECTED",
                  "isFinal": true }
            ],
            "transitions": [
                { "from": "SUBMITTED", "to": "COMPLETED", "action": "APPROVE",
                  "allowedBy": "ROLE:APPROVER" },
                { "from": "SUBMITTED", "to": "REJECTED", "action": "REJECT",
                  "allowedBy": "ROLE:APPROVER" }
            ]
        })
    }

    #[test]
    fn reads_the_states_and_their_task_specifications() {
        let graph = Graph::parse(&definition());

        assert_eq!(graph.states.len(), 3);
        assert_eq!(graph.initial_state, "SUBMITTED");

        let submitted = graph.state("SUBMITTED").expect("the initial state");
        let task = submitted.task.as_ref().expect("a task specification");

        assert_eq!(task.task_definition_key, "manager_approval");
        assert_eq!(task.assignment.assignee_type, AssigneeType::Role);
        assert_eq!(task.assignment.role_code.as_deref(), Some("APPROVER"));
        assert!(graph.state("COMPLETED").expect("a final state").is_final);
    }

    #[test]
    fn the_shorthand_forms_normalize_to_the_object_form() {
        // JWSS §5.2. Normalizing once, at parse time, is what stops the
        // resolver, the task writer and the authorization check each having to
        // remember that a string is also a rule.
        let owner = AssignmentRule::parse(&json!("OWNER")).expect("OWNER is a rule");
        assert_eq!(owner.assignee_type, AssigneeType::Owner);

        let role = AssignmentRule::parse(&json!("ROLE:FINANCE")).expect("ROLE:X is a rule");
        assert_eq!(role.assignee_type, AssigneeType::Role);
        assert_eq!(role.role_code.as_deref(), Some("FINANCE"));

        let user = AssignmentRule::parse(&json!("USER:abc")).expect("USER:X is a rule");
        assert_eq!(user.assignee_type, AssigneeType::User);
        assert_eq!(user.user_id.as_deref(), Some("abc"));

        assert!(AssignmentRule::parse(&json!("DEPARTMENT:X")).is_none());
    }

    #[test]
    fn the_unconditioned_transition_is_evaluated_last() {
        // S7's second half, which is the engine's obligation rather than the
        // validator's. The fallback is written *first* in the document here on
        // purpose: if `candidates` returned document order, the condition below
        // would never be reached and every approval would take the same branch.
        let definition = json!({
            "workflowKey": "w", "version": "1.0.0", "name": "n", "initialState": "A",
            "states": [
                { "code": "A", "name": "A", "mapsToDocumentStatus": "SUBMITTED" },
                { "code": "B", "name": "B", "mapsToDocumentStatus": "COMPLETED", "isFinal": true },
                { "code": "C", "name": "C", "mapsToDocumentStatus": "APPROVED" }
            ],
            "transitions": [
                { "from": "A", "to": "B", "action": "APPROVE", "allowedBy": "OWNER" },
                { "from": "A", "to": "C", "action": "APPROVE", "allowedBy": "OWNER",
                  "condition": { "==": [1, 1] } }
            ]
        });

        let graph = Graph::parse(&definition);
        let candidates = graph.candidates("A", TransitionAction::Approve);

        assert_eq!(candidates.len(), 2);
        assert_eq!(
            candidates[0].to, "C",
            "the conditioned transition comes first"
        );
        assert_eq!(candidates[1].to, "B", "the fallback is evaluated last");
    }

    #[test]
    fn an_action_that_leaves_nowhere_has_no_candidates() {
        let graph = Graph::parse(&definition());

        assert!(graph
            .candidates("COMPLETED", TransitionAction::Approve)
            .is_empty());
        assert!(graph
            .candidates("SUBMITTED", TransitionAction::Return)
            .is_empty());
    }

    #[test]
    fn every_action_of_the_vocabulary_round_trips() {
        // The engine issues three of these in Sprint 10 and must be able to
        // *read* all ten: refusing to parse a RETURN would make Sprint 11's
        // definitions unpublishable a sprint early.
        for action in [
            "SUBMIT", "APPROVE", "REJECT", "RETURN", "RESUBMIT", "DELEGATE", "ESCALATE", "CANCEL",
            "COMPLETE", "AUTO",
        ] {
            let parsed = TransitionAction::parse(action).expect("a declared action");
            assert_eq!(parsed.as_db(), action);
        }

        assert!(TransitionAction::parse("APPROVE_ALL").is_none());
    }

    fn spec(due_in_hours: Option<f64>) -> TaskSpec {
        TaskSpec {
            task_definition_key: "manager_approval".to_owned(),
            task_name: "Manager approval".to_owned(),
            task_type: "APPROVAL_TASK".to_owned(),
            assignment: AssignmentRule::parse(&json!("ROLE:APPROVER")).expect("a rule"),
            priority: "NORMAL".to_owned(),
            due_in_hours,
        }
    }

    #[test]
    fn a_declared_window_becomes_seconds_for_the_statement_to_add() {
        // Hours are what a definition author writes and seconds are what
        // `make_interval` takes; the conversion is here so the statement carries
        // no arithmetic of its own beyond the addition.
        assert_eq!(spec(Some(48.0)).due_in_seconds(), Some(172_800.0));
        assert_eq!(spec(Some(0.5)).due_in_seconds(), Some(1800.0));
    }

    #[test]
    fn a_task_that_declares_no_window_has_no_deadline() {
        // Distinct from a window of zero, which is refused below: this is the
        // ordinary case, and the task is created with `due_at` null.
        assert_eq!(spec(None).due_in_seconds(), None);
    }

    #[test]
    fn a_window_that_is_not_a_duration_is_dropped_rather_than_stamped() {
        // The meta-schema refuses these at save (`exclusiveMinimum: 0`), so
        // reaching them means a row somebody wrote by hand — and `parse` is
        // documented as lenient, so this has to be total rather than trusting.
        assert_eq!(spec(Some(0.0)).due_in_seconds(), None);
        assert_eq!(spec(Some(-4.0)).due_in_seconds(), None);
        assert_eq!(spec(Some(f64::NAN)).due_in_seconds(), None);
        assert_eq!(spec(Some(f64::INFINITY)).due_in_seconds(), None);
    }

    #[test]
    fn a_window_too_large_for_an_interval_is_dropped_rather_than_failing_a_submit() {
        // A representability bound, not a product rule: nothing in Kelir says
        // how far ahead a due date may be. What it guarantees is that the
        // statement cannot fail on arithmetic, so a definition author's silly
        // number never lands on whoever submits the next document.
        assert_eq!(spec(Some(f64::MAX)).due_in_seconds(), None);
        assert_eq!(spec(Some(1e30)).due_in_seconds(), None);

        // A century is inside it, so the bound refuses nothing anybody means.
        assert!(spec(Some(100.0 * 365.0 * 24.0)).due_in_seconds().is_some());
    }

    #[test]
    fn a_definition_that_declares_a_window_carries_it_to_the_task_spec() {
        let mut definition = definition();
        definition["states"][0]["task"]["dueInHours"] = json!(48);

        let graph = Graph::parse(&definition);
        let task = graph
            .state("SUBMITTED")
            .expect("the state")
            .task
            .as_ref()
            .expect("its task");

        assert_eq!(task.due_in_hours, Some(48.0));
        assert_eq!(task.due_in_seconds(), Some(172_800.0));
    }
}
