//! The inbox's use cases — which are the workflow module's, called through its
//! service (FR-TASK-001, 002, 003).
//!
//! There is no logic here beyond parsing the query, and that is the design
//! rather than an omission: the visibility rule is one statement in
//! [`workflow::repository::inbox`][r] and the permission is
//! `workflow:task:read`. A function here that filtered, re-checked or reshaped
//! would be a second answer to a question the owning module has already
//! answered — which is what `mod.rs` is about.
//!
//! [r]: crate::modules::workflow::repository::inbox

use uuid::Uuid;

use super::domain::InboxQuery;
use crate::error::AppError;
use crate::middleware::auth::Authenticated;
use crate::modules::workflow::service::inbox::{self, InboxTask, TaskDetail};
use crate::response::PageMeta;
use crate::state::AppState;

pub async fn list_tasks(
    state: &AppState,
    caller: &Authenticated,
    query: &InboxQuery,
) -> Result<(Vec<InboxTask>, PageMeta), AppError> {
    inbox::list_inbox(state, caller, &query.pagination(), &query.filters()?).await
}

pub async fn get_task(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
) -> Result<TaskDetail, AppError> {
    inbox::get_task(state, caller, id).await
}
