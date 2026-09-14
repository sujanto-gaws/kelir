//! The dashboard route (FR-RPT-001; [#431]).
//!
//! [#431]: https://github.com/sujanto-gaws/kelir/issues/431

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};

use super::domain::DashboardSummary;
use super::service;
use crate::error::AppError;
use crate::middleware::auth::Authenticated;
use crate::response::ItemEnvelope;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new().route("/summary", get(dashboard_summary))
}

/// **`/dashboard/summary`, and the path is part of the contract.**
///
/// Not `/dashboard` — the dashboard is a screen and this is one of the things
/// on it. **What the path does not leave room for is a second summary**: the
/// later widgets in [ADR-0039]'s scope add fields to [`DashboardSummary`], and
/// a `GET /dashboard/tasks` appearing beside this is the decision being
/// reversed in a diff rather than in a record.
///
/// **FR-RPT-002 was the first test of that, and it held.** The pending-task
/// widget added `pendingTasks` to [`DashboardSummary`] and no route beside this
/// one, so the page still makes a single request on sign-in.
///
/// **FR-RPT-003 was the harder test and it held too**, because it is the row
/// [ADR-0039]'s rejected alternative would have served: its rows genuinely are
/// documents and a RAD list definition would render them. It added
/// `recentDocuments` to [`DashboardSummary`] — no `GET /dashboard/recent`, and
/// no list definition ([#433] AC1). Three of the three widgets this sprint
/// built are on one contract, which is the whole of what the record claimed
/// would be cheaper.
///
/// **FR-RPT-005 is the fourth, and the one whose issue asked for less than it
/// needed.** [#446] AC1 asked for an overdue count, which `tasksOverdue` had
/// carried since FR-RPT-001; what the requirement lacked was the rows. They
/// arrived as `overdueTasks` on [`DashboardSummary`] — the inbox's statement
/// read a second time, longest late first — and no `GET /dashboard/overdue`.
/// Four widgets, one contract.
///
/// **FR-RPT-004 is the fifth, and the one this comment once made room for
/// elsewhere.** It used to leave space for a `/dashboard/status-breakdown`, on
/// the forecast that a status summary would be tenant-wide and so a surface of
/// its own. It was built over the caller's own documents ([#447]), so it
/// arrived as `documentsByStatus` on [`DashboardSummary`] and the path was
/// never needed. Five widgets, one contract.
///
/// [#433]: https://github.com/sujanto-gaws/kelir/issues/433
/// [#446]: https://github.com/sujanto-gaws/kelir/issues/446
/// [#447]: https://github.com/sujanto-gaws/kelir/issues/447
///
/// [ADR-0039]: ../../../../docs/architectures/adr/0039.%20A%20Dashboard%20Widget%20Is%20a%20Purpose-Built%20Endpoint.md
#[utoipa::path(
    get, path = "/api/v1/dashboard/summary", tag = "reporting",
    responses(
        (status = 200, description = "The caller's own work: the counts, the documents they raised in each status, the first few tasks waiting for them, the tasks longest overdue, and the documents they touched most recently", body = DashboardSummary),
        (status = 403, description = "Missing reporting:dashboard:read — the summary asks for nothing else, because every row in it is the caller's own work (ADR-0039)")
    ),
    security(("bearer" = []))
)]
pub async fn dashboard_summary(
    State(state): State<AppState>,
    caller: Authenticated,
) -> Result<Json<ItemEnvelope<DashboardSummary>>, AppError> {
    let summary = service::dashboard_summary(&state, &caller).await?;

    Ok(Json(ItemEnvelope::new(summary)))
}
