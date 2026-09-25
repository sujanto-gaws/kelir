//! Credential references — **where** a system's secrets live, under
//! `integration:credential:*` (#520, the product owner's answer 3).
//!
//! **Nothing here resolves a reference.** A reference is validated for shape
//! (`domain::credential`), stored, and returned; the secret it points at is
//! never read, so there is none to leak into a response or a log line.

use serde_json::json;
use uuid::Uuid;

use super::require_system;
use crate::error::AppError;
use crate::middleware::auth::Authenticated;
use crate::modules::audit::{self, domain::ObjectType, AuditEntry, ChangeSet};
use crate::modules::integration::domain::credential::{
    validate_create, validate_update, validate_window,
};
use crate::modules::integration::domain::{
    CreateIntegrationCredentialRequest, IntegrationCredential, UpdateIntegrationCredentialRequest,
};
use crate::modules::integration::repository::credential::{
    self as repo, CredentialFields, NewCredential,
};
use crate::modules::integration::{
    CREDENTIAL_CREATE, CREDENTIAL_DELETE, CREDENTIAL_READ, CREDENTIAL_UPDATE,
};
use crate::response::{PageMeta, Pagination};
use crate::state::AppState;

fn credential_not_found() -> AppError {
    AppError::not_found("Integration credential")
}

pub async fn list_credentials(
    state: &AppState,
    caller: &Authenticated,
    system_id: Uuid,
    pagination: &Pagination,
) -> Result<(Vec<IntegrationCredential>, PageMeta), AppError> {
    caller.require(CREDENTIAL_READ)?;

    let tenant_id = caller.tenant_id();
    require_system(state, tenant_id, system_id).await?;

    let total = repo::count_credentials(&state.pool, tenant_id, system_id).await?;
    let credentials = repo::list_credentials(
        &state.pool,
        tenant_id,
        system_id,
        pagination.limit(),
        pagination.offset(),
    )
    .await?;

    Ok((credentials, pagination.meta(total.max(0) as u64)))
}

pub async fn get_credential(
    state: &AppState,
    caller: &Authenticated,
    system_id: Uuid,
    id: Uuid,
) -> Result<IntegrationCredential, AppError> {
    caller.require(CREDENTIAL_READ)?;

    let tenant_id = caller.tenant_id();
    require_system(state, tenant_id, system_id).await?;

    repo::find_credential(&state.pool, tenant_id, system_id, id)
        .await?
        .ok_or_else(credential_not_found)
}

pub async fn create_credential(
    state: &AppState,
    caller: &Authenticated,
    system_id: Uuid,
    request: CreateIntegrationCredentialRequest,
) -> Result<IntegrationCredential, AppError> {
    caller.require(CREDENTIAL_CREATE)?;
    validate_create(&request)?;

    let tenant_id = caller.tenant_id();
    let actor = Some(caller.user_id());
    require_system(state, tenant_id, system_id).await?;

    let id = Uuid::now_v7();

    repo::insert_credential(
        &state.pool,
        &NewCredential {
            id,
            tenant_id,
            external_system_id: system_id,
            credential_type: request.credential_type.as_db(),
            secret_reference: request.secret_reference.trim(),
            valid_from: request.valid_from,
            valid_to: request.valid_to,
            is_active: request.is_active.unwrap_or(true),
            created_by: actor,
        },
    )
    .await?;

    let created = load(state, tenant_id, system_id, id).await?;

    // The reference is in the record, and it is readable there under
    // `integration:credential:read` alone — `ObjectType::readable_by` says so —
    // which is the same permission that shows it on the credential itself.
    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "IntegrationCredential.Created",
            action: "CREATE",
            object_type: ObjectType::IntegrationCredential,
            object_id: id,
            actor_user_id: actor,
            ip_address: caller.ip_address(),
            reason: None,
            old_value: None,
            new_value: Some(json!({
                "externalSystemId": created.external_system_id,
                "credentialType": created.credential_type,
                "secretReference": created.secret_reference,
                "validFrom": created.valid_from,
                "validTo": created.valid_to,
                "isActive": created.is_active,
            })),
        },
    )
    .await;

    Ok(created)
}

pub async fn update_credential(
    state: &AppState,
    caller: &Authenticated,
    system_id: Uuid,
    id: Uuid,
    request: UpdateIntegrationCredentialRequest,
) -> Result<IntegrationCredential, AppError> {
    caller.require(CREDENTIAL_UPDATE)?;
    validate_update(&request)?;

    let tenant_id = caller.tenant_id();
    let actor = Some(caller.user_id());
    require_system(state, tenant_id, system_id).await?;

    let before = repo::find_credential(&state.pool, tenant_id, system_id, id)
        .await?
        .ok_or_else(credential_not_found)?;

    // The window as it would stand after this edit — a request that moves one
    // bound is checked against the stored other.
    validate_window(
        request.valid_from.unwrap_or(before.valid_from),
        request.valid_to.unwrap_or(before.valid_to),
    )?;

    let affected = repo::update_credential(
        &state.pool,
        tenant_id,
        system_id,
        id,
        &CredentialFields {
            credential_type: request.credential_type.map(|kind| kind.as_db()),
            secret_reference: request.secret_reference.as_deref().map(str::trim),
            valid_from: request.valid_from,
            valid_to: request.valid_to,
            is_active: request.is_active,
        },
        actor,
    )
    .await?;

    if affected == 0 {
        return Err(credential_not_found());
    }

    let after = load(state, tenant_id, system_id, id).await?;

    let mut changes = ChangeSet::new();
    changes.field(
        "credentialType",
        &before.credential_type,
        &after.credential_type,
    );
    changes.field(
        "secretReference",
        &before.secret_reference,
        &after.secret_reference,
    );
    changes.field("validFrom", &before.valid_from, &after.valid_from);
    changes.field("validTo", &before.valid_to, &after.valid_to);
    changes.field("isActive", &before.is_active, &after.is_active);
    let (old_value, new_value) = changes.halves();

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "IntegrationCredential.Updated",
            action: "UPDATE",
            object_type: ObjectType::IntegrationCredential,
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

/// Removes a credential reference by soft delete. The secret it pointed at is
/// untouched — it was never Kelir's to delete.
pub async fn delete_credential(
    state: &AppState,
    caller: &Authenticated,
    system_id: Uuid,
    id: Uuid,
) -> Result<(), AppError> {
    caller.require(CREDENTIAL_DELETE)?;

    let tenant_id = caller.tenant_id();
    let actor = Some(caller.user_id());
    require_system(state, tenant_id, system_id).await?;

    let before = repo::find_credential(&state.pool, tenant_id, system_id, id)
        .await?
        .ok_or_else(credential_not_found)?;

    if repo::soft_delete_credential(&state.pool, tenant_id, system_id, id, actor).await? == 0 {
        return Err(credential_not_found());
    }

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "IntegrationCredential.Deleted",
            action: "DELETE",
            object_type: ObjectType::IntegrationCredential,
            object_id: id,
            actor_user_id: actor,
            ip_address: caller.ip_address(),
            reason: None,
            old_value: Some(json!({
                "credentialType": before.credential_type,
                "secretReference": before.secret_reference,
                "isActive": before.is_active,
            })),
            new_value: None,
        },
    )
    .await;

    Ok(())
}

async fn load(
    state: &AppState,
    tenant_id: Uuid,
    system_id: Uuid,
    id: Uuid,
) -> Result<IntegrationCredential, AppError> {
    repo::find_credential(&state.pool, tenant_id, system_id, id)
        .await?
        .ok_or_else(|| AppError::Internal {
            source: anyhow::anyhow!("integration credential {id} vanished after it was written"),
        })
}
