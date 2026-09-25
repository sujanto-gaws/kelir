//! A system's endpoints, under the system's own permissions (#520, the product
//! owner's answer 1).

use serde_json::json;
use uuid::Uuid;

use super::{duplicate_to_conflict, require_system};
use crate::error::AppError;
use crate::middleware::auth::Authenticated;
use crate::modules::audit::{self, domain::ObjectType, AuditEntry, ChangeSet};
use crate::modules::integration::domain::endpoint::{validate_create, validate_update};
use crate::modules::integration::domain::{
    trimmed, CreateIntegrationEndpointRequest, EndpointStatus, HttpMethod, IntegrationEndpoint,
    UpdateIntegrationEndpointRequest,
};
use crate::modules::integration::repository::endpoint::{
    self as repo, EndpointFields, NewEndpoint,
};
use crate::modules::integration::{EXTERNAL_SYSTEM_READ, EXTERNAL_SYSTEM_UPDATE};
use crate::response::{PageMeta, Pagination};
use crate::state::AppState;

fn endpoint_not_found() -> AppError {
    AppError::not_found("Integration endpoint")
}

pub async fn list_endpoints(
    state: &AppState,
    caller: &Authenticated,
    system_id: Uuid,
    pagination: &Pagination,
) -> Result<(Vec<IntegrationEndpoint>, PageMeta), AppError> {
    caller.require(EXTERNAL_SYSTEM_READ)?;

    let tenant_id = caller.tenant_id();
    require_system(state, tenant_id, system_id).await?;

    let total = repo::count_endpoints(&state.pool, tenant_id, system_id).await?;
    let endpoints = repo::list_endpoints(
        &state.pool,
        tenant_id,
        system_id,
        pagination.limit(),
        pagination.offset(),
    )
    .await?;

    Ok((endpoints, pagination.meta(total.max(0) as u64)))
}

pub async fn get_endpoint(
    state: &AppState,
    caller: &Authenticated,
    system_id: Uuid,
    id: Uuid,
) -> Result<IntegrationEndpoint, AppError> {
    caller.require(EXTERNAL_SYSTEM_READ)?;

    let tenant_id = caller.tenant_id();
    require_system(state, tenant_id, system_id).await?;

    repo::find_endpoint(&state.pool, tenant_id, system_id, id)
        .await?
        .ok_or_else(endpoint_not_found)
}

pub async fn create_endpoint(
    state: &AppState,
    caller: &Authenticated,
    system_id: Uuid,
    request: CreateIntegrationEndpointRequest,
) -> Result<IntegrationEndpoint, AppError> {
    caller.require(EXTERNAL_SYSTEM_UPDATE)?;
    validate_create(&request)?;

    let tenant_id = caller.tenant_id();
    let actor = Some(caller.user_id());
    require_system(state, tenant_id, system_id).await?;

    let id = Uuid::now_v7();

    repo::insert_endpoint(
        &state.pool,
        &NewEndpoint {
            id,
            tenant_id,
            external_system_id: system_id,
            endpoint_code: request.endpoint_code.trim(),
            name: request.name.trim(),
            method: request.method.as_db(),
            path: request.path.trim(),
            description: trimmed(request.description.as_deref()),
            created_by: actor,
        },
    )
    .await
    .map_err(|error| {
        duplicate_to_conflict(
            error,
            "this external system already has an endpoint with this endpointCode",
        )
    })?;

    let created = load(state, tenant_id, system_id, id).await?;

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "IntegrationEndpoint.Created",
            action: "CREATE",
            object_type: ObjectType::IntegrationEndpoint,
            object_id: id,
            actor_user_id: actor,
            ip_address: caller.ip_address(),
            reason: None,
            old_value: None,
            new_value: Some(json!({
                "externalSystemId": created.external_system_id,
                "endpointCode": created.endpoint_code,
                "name": created.name,
                "method": created.method,
                "path": created.path,
                "description": created.description,
                "status": created.status,
            })),
        },
    )
    .await;

    Ok(created)
}

/// Edits an endpoint; `status: INACTIVE` is how one is retired.
pub async fn update_endpoint(
    state: &AppState,
    caller: &Authenticated,
    system_id: Uuid,
    id: Uuid,
    request: UpdateIntegrationEndpointRequest,
) -> Result<IntegrationEndpoint, AppError> {
    caller.require(EXTERNAL_SYSTEM_UPDATE)?;
    validate_update(&request)?;

    let tenant_id = caller.tenant_id();
    let actor = Some(caller.user_id());
    require_system(state, tenant_id, system_id).await?;

    let before = repo::find_endpoint(&state.pool, tenant_id, system_id, id)
        .await?
        .ok_or_else(endpoint_not_found)?;

    let affected = repo::update_endpoint(
        &state.pool,
        tenant_id,
        system_id,
        id,
        &EndpointFields {
            name: request.name.as_deref().map(str::trim),
            method: request.method.map(HttpMethod::as_db),
            path: request.path.as_deref().map(str::trim),
            description: request
                .description
                .as_ref()
                .map(|value| trimmed(value.as_deref())),
            status: request.status.map(EndpointStatus::as_db),
        },
        actor,
    )
    .await?;

    if affected == 0 {
        return Err(endpoint_not_found());
    }

    let after = load(state, tenant_id, system_id, id).await?;

    let mut changes = ChangeSet::new();
    changes.field("name", &before.name, &after.name);
    changes.field("method", &before.method, &after.method);
    changes.field("path", &before.path, &after.path);
    changes.field("description", &before.description, &after.description);
    changes.field("status", &before.status, &after.status);
    let (old_value, new_value) = changes.halves();

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "IntegrationEndpoint.Updated",
            action: "UPDATE",
            object_type: ObjectType::IntegrationEndpoint,
            object_id: id,
            actor_user_id: actor,
            ip_address: caller.ip_address(),
            reason: None,
            old_value: Some(old_value),
            new_value: Some(new_value),
        },
    )
    .await;

    Ok(after)
}

async fn load(
    state: &AppState,
    tenant_id: Uuid,
    system_id: Uuid,
    id: Uuid,
) -> Result<IntegrationEndpoint, AppError> {
    repo::find_endpoint(&state.pool, tenant_id, system_id, id)
        .await?
        .ok_or_else(|| AppError::Internal {
            source: anyhow::anyhow!("integration endpoint {id} vanished after it was written"),
        })
}
