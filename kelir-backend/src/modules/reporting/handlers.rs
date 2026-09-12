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
/// on it, so the noun that pages leaves room for FR-RPT-004's aggregate to be
/// `/dashboard/status-breakdown` rather than a second thing also called the
/// dashboard. **What it does not leave room for is a second summary**: the
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
/// [#433]: https://github.com/sujanto-gaws/kelir/issues/433
///
/// [ADR-0039]: ../../../../docs/architectures/adr/0039.%20A%20Dashboard%20Widget%20Is%20a%20Purpose-Built%20Endpoint.md
#[utoipa::path(
    get, path = "/api/v1/dashboard/summary", tag = "reporting",
    responses(
        (status = 200, description = "The caller's own work: the counts, the first few tasks waiting for them, and the documents they touched most recently", body = DashboardSummary),
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
