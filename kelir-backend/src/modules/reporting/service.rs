//! The dashboard's one use case (FR-RPT-001; [#431]).
//!
//! **There is no logic here beyond the permission and the assembly**, and that
//! is the design rather than an omission — the same shape
//! [`crate::modules::task_inbox::service`] has, for the same stated reason. Each
//! number belongs to a module that already knows how to count it under its own
//! rule; a function here that filtered, re-scoped or re-derived would be a
//! second answer to a question the owning module has answered, and two answers
//! drift.
//!
//! [#431]: https://github.com/sujanto-gaws/kelir/issues/431

use super::domain::DashboardSummary;
use super::{DASHBOARD_READ, PENDING_TASKS_SHOWN};
use crate::error::AppError;
use crate::middleware::auth::Authenticated;
use crate::modules::document::service::list as document_list;
use crate::modules::workflow::service::inbox as workflow_inbox;
use crate::state::AppState;

/// The dashboard summary for whoever is asking.
///
/// # One `require`, and it is this module's own
///
/// [`DASHBOARD_READ`] gates the **surface**; neither `workflow:task:read` nor
/// `document:read` is asked for, because every number below is about the
/// caller's own work and there is no second surface's data here to protect. The
/// module doc is the full argument, including the two roadmap requirements that
/// will cross that line and why they cannot ship behind this grant alone.
///
/// # The scoping is in the statements, not here
///
/// *Whose task is this* is four clauses in `workflow::repository::inbox`'s
/// `WHERE`, and *which drafts are mine* is three in
/// `document::repository::list`. **Neither is re-stated in this function**,
/// which is [#171] AC2 and [#179] AC3 and the [#106]/[#121] lesson that cost
/// this project three sprints of coverage findings: a rule in a service is one
/// the next caller of the repository can step around, and a test that asserts
/// around a query proves something about the handler and nothing about the
/// rows. `tests/reporting_dashboard.rs` puts a second tenant's and a second
/// user's rows in the database and asserts on what comes back, for that reason.
///
/// # Two reads rather than one
///
/// The task half and the draft count are two round trips, and they are not
/// folded into one statement. Folding them would mean this module writing SQL
/// against two other modules' tables — the thing the module doc's table exists
/// to say it does not do — and the saving is one round trip on a screen that
/// makes one request. **If this becomes the dashboard's cost centre the answer
/// is a statement in each owning module, not a statement here**, so the
/// predicates stay where their rules are.
///
/// It was three until FR-RPT-002 ([#432]). The widget needed the rows behind
/// the waiting count, and the count, the overdue count and the rows now come
/// back from one pass through the inbox's statement — so the read that was
/// added to this screen cost it a round trip *fewer*, and the card's number can
/// no longer disagree with the card's list.
///
/// [#106]: https://github.com/sujanto-gaws/kelir/issues/106
/// [#121]: https://github.com/sujanto-gaws/kelir/issues/121
/// [#171]: https://github.com/sujanto-gaws/kelir/issues/171
/// [#179]: https://github.com/sujanto-gaws/kelir/issues/179
/// [#432]: https://github.com/sujanto-gaws/kelir/issues/432
pub async fn dashboard_summary(
    state: &AppState,
    caller: &Authenticated,
) -> Result<DashboardSummary, AppError> {
    caller.require(DASHBOARD_READ)?;

    let tasks = workflow_inbox::waiting_work(state, caller, PENDING_TASKS_SHOWN).await?;
    let draft_documents = document_list::count_own_drafts(state, caller).await?;

    Ok(DashboardSummary {
        tasks_waiting: tasks.waiting,
        tasks_overdue: tasks.overdue,
        draft_documents,
        pending_tasks: tasks.next,
    })
}
