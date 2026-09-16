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
use super::{
    APPROVAL_TIME_WINDOW_DAYS, DASHBOARD_READ, OVERDUE_TASKS_SHOWN, PENDING_TASKS_SHOWN,
    RECENT_DOCUMENTS_SHOWN,
};
use crate::error::AppError;
use crate::middleware::auth::Authenticated;
use crate::modules::document::domain::DocumentStatus;
use crate::modules::document::service::list as document_list;
use crate::modules::workflow::service::inbox as workflow_inbox;
use crate::modules::workflow::service::instance as workflow_instance;
use crate::state::AppState;

/// The dashboard summary for whoever is asking.
///
/// # One `require`, and it is this module's own
///
/// [`DASHBOARD_READ`] gates the **surface**; neither `workflow:task:read` nor
/// `document:read` is asked for, because every number below is about the
/// caller's own work and there is no second surface's data here to protect. The
/// module doc is the full argument, including the roadmap requirement that will
/// cross that line, the tenant-wide reading of FR-RPT-004 that would have, and
/// why neither can ship behind this grant alone.
///
/// # The scoping is in the statements, not here
///
/// *Whose task is this* is four clauses in `workflow::repository::inbox`'s
/// `WHERE`, and *which documents are mine* is three in
/// `document::repository::list`. **Neither is re-stated in this function**,
/// which is [#171] AC2 and [#179] AC3 and the [#106]/[#121] lesson that cost
/// this project three sprints of coverage findings: a rule in a service is one
/// the next caller of the repository can step around, and a test that asserts
/// around a query proves something about the handler and nothing about the
/// rows. `tests/reporting_dashboard.rs` puts a second tenant's and a second
/// user's rows in the database and asserts on what comes back, for that reason.
///
/// # Five reads, and why none is folded into another
///
/// The task half and the document counts are separate round trips, and they are
/// not folded into one statement. Folding them would mean this module writing SQL
/// against two other modules' tables — the thing the module doc's table exists
/// to say it does not do — and the saving is one round trip on a screen that
/// makes one request. **If this becomes the dashboard's cost centre the answer
/// is a statement in each owning module, not a statement here**, so the
/// predicates stay where their rules are.
///
/// It was three until FR-RPT-002 ([#432]). The widget needed the rows behind
/// the waiting count, and the count, the overdue count and the rows came back
/// from one pass through the inbox's statement — so the read that was added to
/// this screen cost it a round trip *fewer*, and the card's number could no
/// longer disagree with the card's list.
///
/// **FR-RPT-003 ([#433]) put it back to three, and that one is a real third
/// read.** The recent-documents widget could not be folded into the draft count
/// the way the task rows folded into the task count: the two touch different
/// tables under different predicates — one counts `documents` the caller
/// authored (then in `DRAFT` alone, by status since [#447]), the other lists
/// `documents` joined to the caller's own `activity_events` — and a statement
/// covering both would be
/// this module writing SQL across two modules' tables, which is the thing the
/// module doc's table exists to say it does not do.
///
/// **FR-RPT-005 ([#446]) makes it four, and the fourth is the inbox's statement
/// read a second time.** The waiting card wants the queue newest first and the
/// overdue card wants the late tasks longest late first. One `ORDER BY` cannot
/// serve both, and five rows of the open queue cannot promise to contain the
/// five longest late. So [`workflow_inbox::late_work`] reads the same `WHERE`
/// narrowed to `InboxScope::Overdue`, and `tasksOverdue` is taken from that read
/// — the count over the rows the overdue card lists — rather than from the
/// waiting pass it used to ride on.
///
/// **The trade, stated.** `tasksOverdue ⊂ tasksWaiting` used to hold by
/// construction, one pass counting both. It now spans two reads, so a task
/// changing hands between them — a decision landing, a reassignment, a grant
/// arriving — can leave the two numbers describing different sets for that one
/// request. **What was chosen over it is the card agreeing with itself**: the
/// overdue card's count and its rows are one answer from one snapshot, which is
/// [#432] AC5's rule and the disagreement a viewer can see on the face of a
/// single card. Two cards a moment apart settle on the next load.
///
/// **Four reads on one screen every session loads is a stated cost, not an
/// oversight** (NFR-PERF-002). Each is a `(tenant_id, …)`-scoped read with a
/// small `LIMIT`, and the fourth matches a subset of what the first already
/// matched — the same `WHERE` with one more predicate — so it is never the
/// larger of the two task reads. If this does become the dashboard's cost
/// centre, the answer remains a statement in each owning module rather than one
/// here.
///
/// **FR-RPT-004 ([#447]) adds a widget and no read.** The per-status count
/// *replaced* the drafts count rather than joining it: one `GROUP BY` over the
/// same author, tenant and soft delete, and `draftDocuments` is its `DRAFT`
/// entry. A fifth statement counting drafts beside a fourth counting every
/// status would be two answers to *how many drafts* on one screen — the card's
/// number and the chart's first bar — which is [#279]'s shape in miniature.
/// Reading the number out of the one result makes them equal by construction
/// rather than by two predicates staying in step.
///
/// **The cost, stated:** that read now covers everything the caller ever raised
/// rather than their drafts. It grows with one person's history and not with
/// the tenant's, and `document::repository::list::count_own_by_status` says what
/// the index does and does not cover.
///
/// **FR-RPT-006 ([#461]) makes it five, and the fifth is a read no other card
/// could carry.** The approval time is over `workflow_instances` — each
/// document's first start and its deciding instance's end — for the documents
/// the caller raised. Folding it into the status count would put the workflow
/// module's outcomes into the document module's `GROUP BY`, and folding it into
/// the inbox's statement would join tasks the caller holds to documents the
/// caller raised, which are different rows. **It is a statement in the owning
/// module, as the paragraph above says the answer always is**, and it grows
/// with one person's ninety days of decisions rather than with the tenant.
///
/// [#106]: https://github.com/sujanto-gaws/kelir/issues/106
/// [#121]: https://github.com/sujanto-gaws/kelir/issues/121
/// [#171]: https://github.com/sujanto-gaws/kelir/issues/171
/// [#179]: https://github.com/sujanto-gaws/kelir/issues/179
/// [#279]: https://github.com/sujanto-gaws/kelir/issues/279
/// [#432]: https://github.com/sujanto-gaws/kelir/issues/432
/// [#433]: https://github.com/sujanto-gaws/kelir/issues/433
/// [#446]: https://github.com/sujanto-gaws/kelir/issues/446
/// [#447]: https://github.com/sujanto-gaws/kelir/issues/447
/// [#461]: https://github.com/sujanto-gaws/kelir/issues/461
pub async fn dashboard_summary(
    state: &AppState,
    caller: &Authenticated,
) -> Result<DashboardSummary, AppError> {
    caller.require(DASHBOARD_READ)?;

    let tasks = workflow_inbox::waiting_work(state, caller, PENDING_TASKS_SHOWN).await?;
    let late = workflow_inbox::late_work(state, caller, OVERDUE_TASKS_SHOWN).await?;
    let documents_by_status = document_list::count_own_by_status(state, caller).await?;
    let recent_documents =
        document_list::recent_documents(state, caller, RECENT_DOCUMENTS_SHOWN).await?;
    let approval_time =
        workflow_instance::approval_time(state, caller, APPROVAL_TIME_WINDOW_DAYS).await?;

    // The `DRAFT` entry of the read above, not a second count — see "FR-RPT-004
    // adds a widget and no read".
    let draft_documents = documents_by_status
        .iter()
        .find(|entry| entry.status == DocumentStatus::Draft)
        .map_or(0, |entry| entry.count);

    Ok(DashboardSummary {
        tasks_waiting: tasks.waiting,
        tasks_overdue: late.overdue,
        draft_documents,
        documents_by_status,
        pending_tasks: tasks.next,
        overdue_tasks: late.next,
        recent_documents,
        approval_time,
    })
}
