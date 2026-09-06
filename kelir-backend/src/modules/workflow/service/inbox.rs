//! The task inbox, as this module serves it to [`crate::modules::task_inbox`]
//! (FR-TASK-001, 002, 003; [#179]).
//!
//! **The inbox lives in `task_inbox` and its SQL lives here**, which is coding
//! standard §2.2: a repository is private to its module, and cross-module access
//! goes through the owning module's service. `workflow_tasks` is this module's
//! table. A second module writing its own statement against these rows would be
//! a second implementation of the visibility rule
//! [`super::super::repository::inbox`] states, and the two would drift — which
//! is the failure this codebase has already paid for at the status layer.
//!
//! [#179]: https://github.com/sujanto-gaws/kelir/issues/179

use uuid::Uuid;

use super::super::domain::{Assignment, Graph, TaskStatus, TransitionAction};
use super::super::repository::inbox::{self, InboxFilters, InboxScope};
use super::super::repository::{definition as definition_repo, instance as instance_repo};
use super::super::TASK_READ;
use crate::error::AppError;
use crate::middleware::auth::Authenticated;
use crate::response::{PageMeta, Pagination};
use crate::state::AppState;

/// One row of somebody's inbox.
#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InboxTask {
    pub id: Uuid,
    pub task_ref: String,
    pub task_name: String,
    pub task_type: String,
    pub status: TaskStatus,
    pub priority: String,
    pub due_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Whether this task is late (FR-TASK-007, [#185] AC4).
    ///
    /// **The server's answer, not a date for the client to judge.** `dueAt` is
    /// beside it so a screen can say *when*; this says *whether*, computed by
    /// the database in the statement that read the row and against the clock
    /// that stamped it. A browser comparing `dueAt` to its own clock would be a
    /// second opinion, and a task late on one machine and not on another is the
    /// bug report AC4 names as impossible to reproduce.
    ///
    /// `false` for a task with no due date, and for one already finished.
    ///
    /// [#185]: https://github.com/sujanto-gaws/kelir/issues/185
    pub is_overdue: bool,
    /// **Mine, or going spare.** [#179] AC1, and it is a field rather than
    /// something a client derives from a null assignee: two clients deriving it
    /// would derive it differently, and the two situations need different words
    /// on the screen.
    pub assignment: Assignment,
    pub candidate_role_code: Option<String>,
    /// Whose work this is, when the holder is standing in for somebody
    /// ([#184] AC2).
    ///
    /// **A field beside `assignment` rather than a third variant of it.**
    /// `Assignment` answers *is this mine or is it going spare*, and a delegated
    /// task is unambiguously mine — it is assigned to me, I am the one who has
    /// to decide it, and the queue behaves for me exactly as it would for work
    /// the definition had named me for. What is different is whose approval it
    /// is, which is a second sentence on the same row rather than a different
    /// answer to the first question. Folding it into the enum would make every
    /// existing client's `MINE` branch quietly wrong for these tasks.
    ///
    /// [#184]: https://github.com/sujanto-gaws/kelir/issues/184
    pub delegated_from_user_id: Option<Uuid>,
    pub delegated_from_display_name: Option<String>,
    pub workflow_instance_id: Uuid,
    pub workflow_name: String,
    pub current_state: String,
    pub document_id: Uuid,
    pub document_ref: String,
    pub document_number: Option<String>,
    pub document_title: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// What was decided, on a task that has been ([#256] AC5).
    ///
    /// `null` while it is still waiting, which is what makes the completed view
    /// readable without a second call: the row that says *finished* is the row
    /// that says what was decided and why.
    ///
    /// [#256]: https://github.com/sujanto-gaws/kelir/issues/256
    pub action: Option<String>,
    /// **The reason FR-TASK-006 recorded in Sprint 11**, visible until now only
    /// on the document's own history.
    ///
    /// This is the decision comment — the immutable record, not the
    /// conversation `modules::comment` holds — and it is served here because the
    /// person reading it is the task's own holder or a role holder the
    /// visibility rule already admits. It is the same rule the document's
    /// history applies, asked from the other end; what it is **not** is the
    /// audit trail, which is read by people holding no permission over the
    /// document and where **D-12** and **D-32** keep this text out.
    pub decision_comment: Option<String>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// What a task detail says for itself ([#179] AC4).
///
/// *"A task that says only 'approve?' is a task its holder cannot responsibly
/// action."* So the detail carries the task, the document it is about, the
/// process it belongs to, and **the decision being asked** — the transitions
/// available from the current state, with the definition's own name for each
/// target.
#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TaskDetail {
    #[serde(flatten)]
    pub task: InboxTask,
    pub workflow_key: String,
    /// The state's display name from the definition, so the screen renders
    /// "Manager approval" rather than `MANAGER_APPROVAL`.
    pub current_state_name: String,
    pub decisions: Vec<AvailableDecision>,
}

/// One thing the holder of this task may do, and where it leads.
#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AvailableDecision {
    pub action: String,
    pub to_state: String,
    pub to_state_name: String,
    /// Whether Sprint 10's API can actually perform it.
    ///
    /// A definition may declare `RETURN` — [#183] is Sprint 11 — and a screen
    /// that offered a button for it would produce a 422 from a control the
    /// product drew. Saying so in the payload is the honest encoding, and it is
    /// what lets the screen show the transition without offering it.
    ///
    /// [#183]: https://github.com/sujanto-gaws/kelir/issues/183
    pub supported: bool,
    /// Whether the definition requires a reason with this decision
    /// (JWSS §4.1; FR-TASK-006, [#182]).
    ///
    /// **The screen must not derive this.** A client that decided for itself
    /// which actions need a comment — *rejections do* — would be a second rule,
    /// and the two would drift the first time a workflow marked an `APPROVE`.
    /// Where they drifted, the screen would either refuse a decision the server
    /// would have taken, or send one the server refuses from a control it drew.
    /// [#182] AC4 is that both ends agree; the way they agree is that there is
    /// one rule and this field is it.
    ///
    /// **It is the property of the edge, and `condition` can still choose a
    /// different one.** Where a state offers two transitions for one action the
    /// engine picks between them when the decision arrives, so this is what the
    /// definition declares rather than a promise about which edge fires. The
    /// engine checks again, against the edge it actually chose.
    ///
    /// [#182]: https://github.com/sujanto-gaws/kelir/issues/182
    pub requires_comment: bool,
}

pub async fn list_inbox(
    state: &AppState,
    caller: &Authenticated,
    pagination: &Pagination,
    filters: &InboxFilters,
) -> Result<(Vec<InboxTask>, PageMeta), AppError> {
    caller.require(TASK_READ)?;

    let tenant_id = caller.tenant_id();
    let user_id = caller.user_id();

    let total = inbox::count_for_caller(&state.pool, tenant_id, user_id, filters).await?;
    let rows = inbox::list_for_caller(
        &state.pool,
        tenant_id,
        user_id,
        filters,
        pagination.limit(),
        pagination.offset(),
    )
    .await?;

    let tasks = rows.into_iter().map(|row| to_task(row, user_id)).collect();

    Ok((tasks, pagination.meta(total.max(0) as u64)))
}

pub async fn get_task(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
) -> Result<TaskDetail, AppError> {
    caller.require(TASK_READ)?;

    let tenant_id = caller.tenant_id();
    let user_id = caller.user_id();

    // **The same predicate the list filters on**, reaching the rows rather than
    // being re-derived here. A task somebody else holds answers 404 from the
    // visibility rule itself, so the two can never disagree about what this
    // caller may see.
    if !inbox::is_visible_to(&state.pool, tenant_id, user_id, id).await? {
        return Err(AppError::not_found("Task"));
    }

    // **Filtered in the statement rather than picked out of a page.** This read
    // used to ask for a thousand rows and `find` the one it wanted, which
    // answered 404 for the oldest task of anybody holding more than a thousand.
    let row = inbox::list_for_caller(
        &state.pool,
        tenant_id,
        user_id,
        &InboxFilters {
            // `All`, and the detail view is why the axis has that point at all:
            // it is reached for a task by id, and a decided one is still worth
            // reading — narrowing to what is open, late or finished here would
            // answer 404 for a task somebody opened to see what happened to it.
            scope: InboxScope::All,
            document_id: None,
            task_id: Some(id),
            search: None,
        },
        1,
        0,
    )
    .await?
    .into_iter()
    .next()
    .ok_or_else(|| AppError::not_found("Task"))?;

    let instance = instance_repo::find_instance(&state.pool, tenant_id, row.workflow_instance_id)
        .await?
        .ok_or_else(|| AppError::not_found("Workflow instance"))?;

    let graph = definition_repo::definition_of_instance(
        &state.pool,
        tenant_id,
        instance.workflow_definition_id,
    )
    .await?
    .map(|definition| Graph::parse(&definition.definition_json, definition.version));

    let (current_state_name, decisions) = match &graph {
        Some(graph) => (
            graph
                .state(&instance.current_state)
                .map(|state| state.name.clone())
                .unwrap_or_else(|| instance.current_state.clone()),
            graph
                .actions_from(&instance.current_state)
                .into_iter()
                // **`AUTO` is not a decision, so it is not in this list**
                // ([#264]). `actions_from` answers *every transition out of
                // this state*, which is the graph's question; this field
                // answers *what may the person holding this task do*, which is
                // a narrower one. JWSS §4 forbids `allowedBy` on an `AUTO`
                // transition precisely because there is no caller, so an `AUTO`
                // edge is the one case where the two questions have provably
                // different answers.
                //
                // **Filtered here rather than on the client**, which [#264] AC1
                // asked be decided and recorded. The alternative was every
                // consumer of this list knowing that one member of the action
                // vocabulary is never theirs — the designer, the task screen,
                // and anything built on the API later — and the task inbox's
                // own rule applies: two places deriving a thing derive it
                // differently. `supported` marks what this release can fire;
                // this filter marks what nobody can.
                //
                // [#264]: https://github.com/sujanto-gaws/kelir/issues/264
                .filter(|transition| transition.action != TransitionAction::Auto)
                .map(|transition| AvailableDecision {
                    action: transition.action.as_db().to_owned(),
                    to_state: transition.to.clone(),
                    to_state_name: graph
                        .state(&transition.to)
                        .map(|state| state.name.clone())
                        .unwrap_or_else(|| transition.to.clone()),
                    supported: matches!(transition.action.as_db(), "APPROVE" | "REJECT" | "RETURN"),
                    requires_comment: transition.requires_comment,
                })
                .collect(),
        ),
        None => (instance.current_state.clone(), Vec::new()),
    };

    Ok(TaskDetail {
        task: to_task(row, user_id),
        workflow_key: instance.workflow_key,
        current_state_name,
        decisions,
    })
}

fn to_task(row: inbox::InboxRow, caller: Uuid) -> InboxTask {
    InboxTask {
        // Derived from the row rather than from the task's status, because the
        // status answers a different question: a task can be `ASSIGNED` to
        // somebody else and still be visible to a role holder in a future
        // sprint. What the screen needs is *whose is it, relative to me*.
        assignment: if row.assignee_user_id == Some(caller) {
            Assignment::Mine
        } else {
            Assignment::Role
        },
        id: row.id,
        task_ref: row.task_ref,
        task_name: row.task_name,
        task_type: row.task_type,
        status: TaskStatus::from_db(&row.status),
        priority: row.priority,
        due_at: row.due_at,
        is_overdue: row.is_overdue,
        candidate_role_code: row.candidate_role_code,
        delegated_from_user_id: row.delegated_from_user_id,
        delegated_from_display_name: row.delegated_from_display_name,
        workflow_instance_id: row.workflow_instance_id,
        workflow_name: row.workflow_name,
        current_state: row.current_state,
        document_id: row.document_id,
        document_ref: row.document_ref,
        document_number: row.document_number,
        document_title: row.document_title,
        created_at: row.created_at,
        action: row.action,
        decision_comment: row.decision_comment,
        completed_at: row.completed_at,
    }
}
