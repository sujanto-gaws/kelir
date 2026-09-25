//! Registering, reading, editing and deactivating external systems.

use serde_json::json;
use uuid::Uuid;

use super::{duplicate_to_conflict, search_term, system_not_found};
use crate::error::AppError;
use crate::middleware::auth::Authenticated;
use crate::modules::audit::{self, domain::ObjectType, AuditEntry, ChangeSet};
use crate::modules::integration::domain::external_system::{validate_register, validate_update};
use crate::modules::integration::domain::{
    trimmed, ExternalSystem, ExternalSystemQuery, ExternalSystemStatus,
    RegisterExternalSystemRequest, RetryPolicy, UpdateExternalSystemRequest,
};
use crate::modules::integration::repository::external_system::{
    self as repo, ExternalSystemFields, ExternalSystemFilter, NewExternalSystem,
};
use crate::modules::integration::{
    EXTERNAL_SYSTEM_CREATE, EXTERNAL_SYSTEM_DEACTIVATE, EXTERNAL_SYSTEM_READ,
    EXTERNAL_SYSTEM_UPDATE,
};
use crate::response::PageMeta;
use crate::state::AppState;

/// The default `timeout_seconds`, §12.1's column default.
const DEFAULT_TIMEOUT_SECONDS: i32 = 30;

pub async fn list_external_systems(
    state: &AppState,
    caller: &Authenticated,
    query: &ExternalSystemQuery,
) -> Result<(Vec<ExternalSystem>, PageMeta), AppError> {
    caller.require(EXTERNAL_SYSTEM_READ)?;

    let tenant_id = caller.tenant_id();
    let pagination = query.pagination();
    let filter = ExternalSystemFilter {
        search: search_term(query.search.as_deref()),
        status: query.status.map(ExternalSystemStatus::as_db),
        system_type: query.system_type.map(|kind| kind.as_db()),
    };

    let total = repo::count_external_systems(&state.pool, tenant_id, &filter).await?;
    let systems = repo::list_external_systems(
        &state.pool,
        tenant_id,
        &filter,
        pagination.limit(),
        pagination.offset(),
    )
    .await?;

    Ok((systems, pagination.meta(total.max(0) as u64)))
}

pub async fn get_external_system(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
) -> Result<ExternalSystem, AppError> {
    caller.require(EXTERNAL_SYSTEM_READ)?;

    repo::find_external_system(&state.pool, caller.tenant_id(), id)
        .await?
        .ok_or_else(system_not_found)
}

pub async fn register_external_system(
    state: &AppState,
    caller: &Authenticated,
    request: RegisterExternalSystemRequest,
) -> Result<ExternalSystem, AppError> {
    caller.require(EXTERNAL_SYSTEM_CREATE)?;
    validate_register(&request)?;

    let tenant_id = caller.tenant_id();
    let actor = Some(caller.user_id());
    let id = Uuid::now_v7();
    let retry_policy = request.retry_policy.clone().unwrap_or_default();

    repo::insert_external_system(
        &state.pool,
        &NewExternalSystem {
            id,
            tenant_id,
            system_code: request.system_code.trim(),
            system_name: request.system_name.trim(),
            system_type: request.system_type.map(|kind| kind.as_db()),
            base_url: trimmed(request.base_url.as_deref()),
            auth_type: request.auth_type.map(|kind| kind.as_db()),
            timeout_seconds: request.timeout_seconds.unwrap_or(DEFAULT_TIMEOUT_SECONDS),
            retry_policy: retry_policy.to_db(),
            description: trimmed(request.description.as_deref()),
            created_by: actor,
        },
    )
    .await
    .map_err(|error| {
        duplicate_to_conflict(
            error,
            "an external system with this systemCode already exists",
        )
    })?;

    let created = load(state, tenant_id, id).await?;

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "ExternalSystem.Registered",
            action: "CREATE",
            object_type: ObjectType::ExternalSystem,
            object_id: id,
            actor_user_id: actor,
            ip_address: caller.ip_address(),
            reason: None,
            old_value: None,
            new_value: Some(json!({
                "systemCode": created.system_code,
                "systemName": created.system_name,
                "systemType": created.system_type,
                "baseUrl": created.base_url,
                "authType": created.auth_type,
                "timeoutSeconds": created.timeout_seconds,
                "retryPolicy": created.retry_policy,
                "description": created.description,
                "status": created.status,
            })),
        },
    )
    .await;

    Ok(created)
}

pub async fn update_external_system(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
    request: UpdateExternalSystemRequest,
) -> Result<ExternalSystem, AppError> {
    caller.require(EXTERNAL_SYSTEM_UPDATE)?;
    validate_update(&request)?;

    let tenant_id = caller.tenant_id();
    let actor = Some(caller.user_id());

    let before = repo::find_external_system(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(system_not_found)?;

    // A blank string clears a nullable text field, the same as `null` does.
    let base_url = request
        .base_url
        .as_ref()
        .map(|value| trimmed(value.as_deref()));
    let description = request
        .description
        .as_ref()
        .map(|value| trimmed(value.as_deref()));

    let affected = repo::update_external_system(
        &state.pool,
        tenant_id,
        id,
        &ExternalSystemFields {
            system_name: request.system_name.as_deref().map(str::trim),
            system_type: request
                .system_type
                .map(|value| value.map(|kind| kind.as_db())),
            base_url,
            auth_type: request
                .auth_type
                .map(|value| value.map(|kind| kind.as_db())),
            timeout_seconds: request.timeout_seconds,
            retry_policy: request.retry_policy.as_ref().map(RetryPolicy::to_db),
            description,
            status: request.status.map(ExternalSystemStatus::as_db),
        },
        actor,
    )
    .await?;

    if affected == 0 {
        return Err(system_not_found());
    }

    let after = load(state, tenant_id, id).await?;

    // What changed, not what was requested (#135).
    let mut changes = ChangeSet::new();
    changes.field("systemName", &before.system_name, &after.system_name);
    changes.field("systemType", &before.system_type, &after.system_type);
    changes.field("baseUrl", &before.base_url, &after.base_url);
    changes.field("authType", &before.auth_type, &after.auth_type);
    changes.field(
        "timeoutSeconds",
        &before.timeout_seconds,
        &after.timeout_seconds,
    );
    changes.field("retryPolicy", &before.retry_policy, &after.retry_policy);
    changes.field("description", &before.description, &after.description);
    changes.field("status", &before.status, &after.status);
    let (old_value, new_value) = changes.halves();

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "ExternalSystem.Updated",
            action: "UPDATE",
            object_type: ObjectType::ExternalSystem,
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

/// Takes a system out of service: `status` becomes `INACTIVE`.
///
/// **Idempotent.** Deactivating a system that is already inactive returns it
/// unchanged and records nothing — the caller asked for a state the system is
/// in, and an audit row saying it moved would be false.
///
/// **Its endpoints and credentials are left as they are.** They belong to the
/// system and are unusable through it while it is inactive; setting each of
/// them inactive too would make reactivating the system a second, larger job
/// of remembering which ones had been active before.
pub async fn deactivate_external_system(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
) -> Result<ExternalSystem, AppError> {
    caller.require(EXTERNAL_SYSTEM_DEACTIVATE)?;

    let tenant_id = caller.tenant_id();
    let actor = Some(caller.user_id());

    let before = repo::find_external_system(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(system_not_found)?;

    if before.status == ExternalSystemStatus::Inactive {
        return Ok(before);
    }

    if repo::deactivate_external_system(&state.pool, tenant_id, id, actor).await? == 0 {
        // A concurrent deactivation won; the system is in the state asked for.
        return load(state, tenant_id, id).await;
    }

    let after = load(state, tenant_id, id).await?;

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "ExternalSystem.Deactivated",
            action: "STATUS_CHANGE",
            object_type: ObjectType::ExternalSystem,
            object_id: id,
            actor_user_id: actor,
            ip_address: caller.ip_address(),
            reason: None,
            old_value: Some(json!({ "status": before.status })),
            new_value: Some(json!({ "status": after.status })),
        },
    )
    .await;

    Ok(after)
}

async fn load(state: &AppState, tenant_id: Uuid, id: Uuid) -> Result<ExternalSystem, AppError> {
    repo::find_external_system(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::Internal {
            source: anyhow::anyhow!("external system {id} vanished after it was written"),
        })
}
