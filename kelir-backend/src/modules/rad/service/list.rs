//! List definition use cases (FR-RAD-003).
//!
//! **A list and its children are written in one transaction.** Columns and
//! filters are replaced wholesale, so a failure halfway through an update would
//! otherwise leave a list with the new columns and the old filters — a shape no
//! caller asked for and none could tell had happened.

use serde_json::json;
use uuid::Uuid;

use super::super::domain::{
    validate_create_list, validate_update_list, CreateListRequest, ListDefinition, ListStatus,
    ListSummary, UpdateListRequest,
};
use super::super::repository::list::{self as repo, ListFields, NewList};
use super::super::{LIST_CREATE, LIST_DELETE, LIST_READ, LIST_UPDATE};
use crate::error::AppError;
use crate::middleware::auth::Authenticated;
use crate::modules::audit::{self, AuditEntry, ChangeSet};
use crate::response::{PageMeta, Pagination};
use crate::state::AppState;

/// What the audit trail calls a list definition (naming convention §7).
const OBJECT_TYPE: &str = "RAD_LIST";

/// `rad_lists.page_size`'s own default, repeated here because a create that
/// omits the field has to send *something* and the column default would
/// otherwise be reachable only by omitting the column from the INSERT.
const DEFAULT_PAGE_SIZE: i32 = 20;

pub async fn list_lists(
    state: &AppState,
    caller: &Authenticated,
    pagination: &Pagination,
) -> Result<(Vec<ListSummary>, PageMeta), AppError> {
    caller.require(LIST_READ)?;

    let tenant_id = caller.tenant_id();
    let total = repo::count_lists(&state.pool, tenant_id).await?;
    let lists = repo::list_lists(
        &state.pool,
        tenant_id,
        pagination.limit(),
        pagination.offset(),
    )
    .await?;

    Ok((lists, pagination.meta(total.max(0) as u64)))
}

pub async fn get_list(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
) -> Result<ListDefinition, AppError> {
    caller.require(LIST_READ)?;

    repo::find_list(&state.pool, caller.tenant_id(), id)
        .await?
        .ok_or_else(|| AppError::not_found("List definition"))
}

pub async fn create_list(
    state: &AppState,
    caller: &Authenticated,
    request: CreateListRequest,
) -> Result<ListDefinition, AppError> {
    caller.require(LIST_CREATE)?;
    validate_create_list(&request)?;

    let tenant_id = caller.tenant_id();
    let actor = Some(caller.user_id());
    let id = Uuid::now_v7();
    let status = request.status.unwrap_or(ListStatus::Active);

    let mut transaction = state.pool.begin().await?;

    repo::insert_list(
        &mut *transaction,
        &NewList {
            id,
            tenant_id,
            list_key: request.list_key.trim(),
            title: request.title.trim(),
            entity_id: request.entity_id,
            default_sort_json: request.default_sort.as_ref(),
            page_size: request.page_size.unwrap_or(DEFAULT_PAGE_SIZE),
            status: status.as_db(),
            created_by: actor,
        },
    )
    .await
    .map_err(duplicate_to_conflict)?;

    repo::replace_columns(&mut transaction, tenant_id, id, &request.columns, actor).await?;
    repo::replace_filters(&mut transaction, tenant_id, id, &request.filters, actor).await?;

    transaction.commit().await?;

    // Read back before the record is written (#135): keys and labels are
    // trimmed on the way in, and the stored order is the array's.
    let created = load(state, tenant_id, id).await?;

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "RadList.Created",
            action: "CREATE",
            object_type: OBJECT_TYPE,
            object_id: id,
            actor_user_id: actor,
            ip_address: None,
            reason: None,
            old_value: None,
            new_value: Some(json!({
                "listKey": created.list_key,
                "title": created.title,
                "entityId": created.entity_id,
                "pageSize": created.page_size,
                "status": created.status,
                "columns": created.columns.len(),
                "filters": created.filters.len(),
            })),
        },
    )
    .await;

    Ok(created)
}

pub async fn update_list(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
    request: UpdateListRequest,
) -> Result<ListDefinition, AppError> {
    caller.require(LIST_UPDATE)?;
    validate_update_list(&request)?;

    let tenant_id = caller.tenant_id();
    let actor = Some(caller.user_id());

    let before = repo::find_list(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::not_found("List definition"))?;

    let mut transaction = state.pool.begin().await?;

    let affected = repo::update_list(
        &mut *transaction,
        tenant_id,
        id,
        &ListFields {
            title: request.title.as_deref().map(str::trim),
            entity_id: request.entity_id,
            default_sort_json: request.default_sort.as_ref().map(Option::as_ref),
            page_size: request.page_size,
            status: request.status.map(ListStatus::as_db),
        },
        actor,
    )
    .await?;

    if affected == 0 {
        // Deleted between the read and the write. Rolling back rather than
        // writing children onto a row that is no longer live.
        return Err(AppError::not_found("List definition"));
    }

    if let Some(columns) = &request.columns {
        repo::replace_columns(&mut transaction, tenant_id, id, columns, actor).await?;
    }

    if let Some(filters) = &request.filters {
        repo::replace_filters(&mut transaction, tenant_id, id, filters, actor).await?;
    }

    transaction.commit().await?;

    let after = load(state, tenant_id, id).await?;

    // What changed, not what was requested (#135). The two collections are
    // compared as they are stored, so re-sending an identical set of columns
    // records nothing — a caller that always sends the whole list would
    // otherwise write a change record on every save.
    let mut changes = ChangeSet::new();
    changes.field("title", &before.title, &after.title);
    changes.field("entityId", &before.entity_id, &after.entity_id);
    changes.field("defaultSort", &before.default_sort, &after.default_sort);
    changes.field("pageSize", &before.page_size, &after.page_size);
    changes.field("status", &before.status, &after.status);
    changes.field("columns", &before.columns, &after.columns);
    changes.field("filters", &before.filters, &after.filters);

    let (old_value, new_value) = changes.halves();

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "RadList.Updated",
            action: "UPDATE",
            object_type: OBJECT_TYPE,
            object_id: id,
            actor_user_id: actor,
            ip_address: None,
            reason: None,
            old_value: Some(old_value),
            new_value: Some(new_value),
        },
    )
    .await;

    Ok(after)
}

pub async fn delete_list(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
) -> Result<(), AppError> {
    caller.require(LIST_DELETE)?;

    let tenant_id = caller.tenant_id();
    let actor = Some(caller.user_id());

    let before = repo::find_list(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::not_found("List definition"))?;

    if repo::soft_delete(&state.pool, tenant_id, id, actor).await? == 0 {
        return Err(AppError::not_found("List definition"));
    }

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "RadList.Deleted",
            action: "DELETE",
            object_type: OBJECT_TYPE,
            object_id: id,
            actor_user_id: actor,
            ip_address: None,
            reason: None,
            old_value: Some(json!({
                "listKey": before.list_key,
                "title": before.title,
                "status": before.status,
            })),
            new_value: None,
        },
    )
    .await;

    Ok(())
}

fn duplicate_to_conflict(error: sqlx::Error) -> AppError {
    match &error {
        sqlx::Error::Database(database) if database.is_unique_violation() => {
            AppError::conflict("a list definition with this key already exists")
        }
        _ => error.into(),
    }
}

async fn load(state: &AppState, tenant_id: Uuid, id: Uuid) -> Result<ListDefinition, AppError> {
    repo::find_list(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::Internal {
            source: anyhow::anyhow!("list {id} vanished after it was written"),
        })
}
