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

    // **Paging counts separately, and that is not the duplication #432 removed.**
    // The page's own `count(*) OVER ()` describes the rows it returned, so a
    // page past the end of the list carries no number at all — `meta.total` has
    // to be the size of the whole list, which is the question
    // `count_for_caller` answers.
    let total = inbox::count_for_caller(&state.pool, tenant_id, user_id, filters).await?;
    let page = inbox::list_for_caller(
        &state.pool,
        tenant_id,
        user_id,
        filters,
        pagination.limit(),
        pagination.offset(),
    )
    .await?;

    let tasks = page
        .rows
        .into_iter()
        .map(|row| to_task(row, user_id))
        .collect();

    Ok((tasks, pagination.meta(total.max(0) as u64)))
}

/// What is waiting for this caller: two numbers, and the top of the queue they
/// describe (FR-RPT-001, [#431]; FR-RPT-002, [#432]).
///
/// # It reads the inbox's own statement, and that is the whole point
///
/// The numbers and the rows come from [`inbox::list_for_caller`] — the
/// statement [`list_inbox`] pages on, under the `WHERE` clause that decides
/// which rows the inbox shows. **A second statement answering "what is waiting
/// for me" would be a second answer to *whose task is this*, and this module's
/// repository is one long argument about what that costs**: the department
/// clause that [#225] added, the delegation rules, the grant's validity window,
/// and [#279](https://github.com/sujanto-gaws/kelir/issues/279), where the one
/// duplication in that file drifted exactly where its own comment warned it
/// would and an inbox said 23 and ended at 19.
///
/// So the dashboard's rows and the inbox's rows cannot disagree, because there
/// is one statement. [#432] AC2 asks for a **divergence** test rather than a
/// presence one — a task the inbox lists and the widget does not, or the
/// reverse — and the reason that test can be written at all is that there is a
/// single predicate for it to catch somebody forking.
///
/// # One read rather than three
///
/// This used to take three: a count of what is open, a count of what is late,
/// and nothing else, because there were no rows to fetch. The widget needed the
/// rows too, and [#432] AC5 asks that **the count and the list come from one
/// statement** — so all three answers now come back from one pass, and a
/// decision landing mid-request can no longer make the card say *5 waiting*
/// over four rows.
///
/// `limit` is how many rows the caller wants; the counts describe the **whole**
/// queue behind them, which is what makes *3 of 12* sayable. The page is read
/// at offset 0, which is the case [`inbox::InboxPage::matching`] is exact for.
///
/// # This function requires no permission, and that is the decision
///
/// [`list_inbox`] and [`get_task`] both open with `caller.require(TASK_READ)`.
/// `workflow:task:read` is the permission for **the inbox surface** — the
/// screen, its paging, its search, its filters.
///
/// **This serves the caller's own queue and nothing else**, and that was true
/// when it returned two integers and is still true now that it returns five
/// rows with them. The predicate is *assigned to this caller, or offered to a
/// role this caller holds*: every row is work this person is being asked to do,
/// and a task's own holder is the last party its name needs keeping from. So
/// the widget is gated where a surface should be — by
/// [`crate::modules::reporting::DASHBOARD_READ`], in the one service that
/// serves it — and asking for `workflow:task:read` on top would be the shape
/// **D-45** found and **D-47** undid one module over, arriving from the other
/// direction: a person holding `reporting:dashboard:read` and nothing else,
/// refused a list of their own waiting work on the one screen built to show it.
///
/// **The line this draws is *whose work*, not *rows versus numbers*.** That
/// distinction is worth stating because the earlier version of this comment
/// leaned on the second one, and [ADR-0039] had already taken the first:
/// FR-RPT-002 "inherits FR-RPT-001's endpoint, its permission and its card".
///
/// **What this does not license.** Tasks this caller does *not* hold are the
/// inbox population, and that is [`TASK_READ`]'s to gate. FR-RPT-007's
/// workload-by-department report is exactly that, and it cannot be served by
/// widening this function.
///
/// [ADR-0039]: ../../../../docs/architectures/adr/0039.%20A%20Dashboard%20Widget%20Is%20a%20Purpose-Built%20Endpoint.md
/// [#225]: https://github.com/sujanto-gaws/kelir/issues/225
/// [#431]: https://github.com/sujanto-gaws/kelir/issues/431
/// [#432]: https://github.com/sujanto-gaws/kelir/issues/432
pub async fn waiting_work(
    state: &AppState,
    caller: &Authenticated,
    limit: i64,
) -> Result<WaitingWork, AppError> {
    let tenant_id = caller.tenant_id();
    let user_id = caller.user_id();

    let page = inbox::list_for_caller(
        &state.pool,
        tenant_id,
        user_id,
        &InboxFilters {
            scope: InboxScope::Open,
            ..InboxFilters::default()
        },
        limit,
        0,
    )
    .await?;

    // **`Overdue` narrows `Open` rather than replacing it** — `InboxScope`'s own
    // documented shape, `overdue ⊂ open ⊂ all` — so the second number is a
    // subset of the first and a screen may say "3 waiting, 1 late" without the
    // two being read as four tasks. Both are counted over the rows this
    // statement matched, so neither can be a count of a different set.
    let waiting = page.matching.unwrap_or(0);
    let overdue = page.matching_overdue.unwrap_or(0);

    Ok(WaitingWork {
        waiting: waiting.max(0),
        overdue: overdue.max(0),
        next: page
            .rows
            .into_iter()
            .map(|row| to_task(row, user_id))
            .collect(),
    })
}

/// One person's queue, as [`waiting_work`] answers it.
#[derive(Debug, Clone)]
pub struct WaitingWork {
    /// Tasks assigned to the caller, or offered to a role they hold, still open.
    pub waiting: i64,
    /// Those of them that are past their date — **a subset of `waiting`**, never
    /// a separate population.
    pub overdue: i64,
    /// The first few of them, in the order the inbox opens on.
    ///
    /// **A prefix of the inbox, not a selection out of it.** The order is
    /// `created_at DESC, id DESC` because that is the order the statement
    /// carries, so a person reading the widget and then opening their inbox
    /// finds the same rows at the top in the same sequence. A widget that
    /// ordered by due date would be a second opinion about which work matters
    /// most, taken by a card rather than by the screen that owns the queue —
    /// and FR-RPT-005's overdue widget is where *late first* is the question
    /// being asked.
    ///
    /// Shorter than [`Self::waiting`] whenever there is more waiting than the
    /// card has room for, which is the ordinary case and the reason the count
    /// is carried beside the rows rather than derived from their length.
    pub next: Vec<InboxTask>,
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
    .rows
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
