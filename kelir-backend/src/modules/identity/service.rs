//! Identity use cases. Owns transactions and permission checks (coding
//! standard §2.2/§2.6): handlers call these, never the repository.

use uuid::Uuid;

use super::domain::{
    validate_create_user, validate_password_value, CreateRoleRequest, CreateUserRequest,
    Permission, Role, UpdateRoleRequest, UpdateUserRequest, User, UserStatus,
};
use super::repository as repo;
use crate::error::{AppError, ValidationDetail};
use crate::middleware::auth::Authenticated;
use crate::modules::audit::{self, AuditEntry};
use crate::modules::auth::password::hash_password;
use crate::modules::organization::department_repository as department_repo;
use crate::response::{PageMeta, Pagination};
use crate::state::AppState;

pub async fn list_users(
    state: &AppState,
    caller: &Authenticated,
    pagination: &Pagination,
) -> Result<(Vec<User>, PageMeta), AppError> {
    caller.require("identity:user:read")?;

    let tenant_id = caller.tenant_id();
    let total = repo::count_users(&state.pool, tenant_id).await?;
    let users = repo::list_users(
        &state.pool,
        tenant_id,
        pagination.limit(),
        pagination.offset(),
    )
    .await?;

    Ok((users, pagination.meta(total.max(0) as u64)))
}

pub async fn get_user(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
) -> Result<User, AppError> {
    caller.require("identity:user:read")?;

    repo::find_user(&state.pool, caller.tenant_id(), id)
        .await?
        .ok_or_else(|| AppError::not_found("User"))
}

pub async fn create_user(
    state: &AppState,
    caller: &Authenticated,
    request: CreateUserRequest,
) -> Result<User, AppError> {
    caller.require("identity:user:create")?;
    validate_create_user(&request)?;

    let tenant_id = caller.tenant_id();
    let id = Uuid::now_v7();

    // Hashing is deliberately slow, so it runs off the async runtime.
    let password = request.password.clone();
    let password_hash = tokio::task::spawn_blocking(move || hash_password(&password))
        .await
        .map_err(|error| AppError::Internal {
            source: anyhow::anyhow!("password hashing task failed: {error}"),
        })??;

    // The department is checked before anything is written. Without this the
    // reference reaches the foreign key and a caller who mistyped a UUID gets a
    // 500 rather than a 422 naming the field (FR-IDM-008, decision D-8).
    check_department(state, tenant_id, request.department_id).await?;

    // The user and its role grants are one unit: a user created without the
    // roles that were asked for would silently have no access.
    let mut transaction = state.pool.begin().await?;

    repo::insert_user(
        &mut *transaction,
        id,
        tenant_id,
        request.username.trim(),
        request.email.trim(),
        &password_hash,
        request.display_name.trim(),
        request.department_id,
        // The API does not force a first-sign-in password change: the caller
        // chose this password for the account and hands it over out of band.
        false,
        Some(caller.user_id()),
    )
    .await
    .map_err(duplicate_user_to_conflict)?;

    repo::replace_user_roles(&mut transaction, tenant_id, id, &request.role_ids).await?;
    transaction.commit().await?;

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "User.Created",
            action: "CREATE",
            object_type: "USER",
            object_id: id,
            actor_user_id: Some(caller.user_id()),
            ip_address: None,
            reason: None,
            old_value: None,
            new_value: Some(serde_json::json!({
                "username": request.username,
                "email": request.email,
                "roleIds": request.role_ids,
            })),
        },
    )
    .await;

    get_user_unchecked(state, tenant_id, id).await
}

pub async fn update_user(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
    request: UpdateUserRequest,
) -> Result<User, AppError> {
    caller.require("identity:user:update")?;

    let tenant_id = caller.tenant_id();
    let before = repo::find_user(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::not_found("User"))?;

    // Only a department the caller is *setting* is checked, and only when it
    // names one. `Some(None)` clears the column and has nothing to resolve.
    check_department(state, tenant_id, request.department_id.flatten()).await?;

    let mut transaction = state.pool.begin().await?;

    let updated = repo::update_user_fields(
        &mut *transaction,
        tenant_id,
        id,
        request.email.as_deref().map(str::trim),
        request.display_name.as_deref().map(str::trim),
        request.status.map(UserStatus::as_db),
        request.department_id,
        Some(caller.user_id()),
    )
    .await
    .map_err(duplicate_user_to_conflict)?;

    if updated == 0 {
        return Err(AppError::not_found("User"));
    }

    if let Some(role_ids) = &request.role_ids {
        repo::replace_user_roles(&mut transaction, tenant_id, id, role_ids).await?;
    }

    transaction.commit().await?;

    // Deactivating an account must end its sessions, not merely block new
    // sign-ins: an access token issued a minute ago is still valid otherwise.
    if matches!(request.status, Some(status) if !status.can_sign_in()) {
        let revoked = repo::revoke_all_for_user(&state.pool, id, "account deactivated").await?;
        tracing::info!(user_id = %id, revoked, "revoked sessions for a deactivated account");
    }

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "User.Updated",
            action: "UPDATE",
            object_type: "USER",
            object_id: id,
            actor_user_id: Some(caller.user_id()),
            ip_address: None,
            reason: None,
            old_value: Some(serde_json::json!({
                "email": before.email,
                "displayName": before.display_name,
                "status": before.status,
            })),
            new_value: Some(serde_json::json!({
                "email": request.email,
                "displayName": request.display_name,
                "status": request.status,
            })),
        },
    )
    .await;

    get_user_unchecked(state, tenant_id, id).await
}

pub async fn deactivate_user(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
) -> Result<(), AppError> {
    caller.require("identity:user:delete")?;

    if id == caller.user_id() {
        // Removing your own access mid-request leaves nobody able to undo it.
        return Err(AppError::bad_request(
            "You cannot deactivate your own account",
        ));
    }

    let tenant_id = caller.tenant_id();
    let removed =
        repo::soft_delete_user(&state.pool, tenant_id, id, Some(caller.user_id())).await?;

    if removed == 0 {
        return Err(AppError::not_found("User"));
    }

    repo::revoke_all_for_user(&state.pool, id, "account deleted").await?;

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "User.Deactivated",
            action: "DELETE",
            object_type: "USER",
            object_id: id,
            actor_user_id: Some(caller.user_id()),
            ip_address: None,
            reason: None,
            old_value: None,
            new_value: None,
        },
    )
    .await;

    Ok(())
}

pub async fn set_password(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
    new_password: &str,
) -> Result<(), AppError> {
    caller.require("identity:user:update")?;
    validate_password_value(new_password)?;

    let password = new_password.to_owned();
    let hash = tokio::task::spawn_blocking(move || hash_password(&password))
        .await
        .map_err(|error| AppError::Internal {
            source: anyhow::anyhow!("password hashing task failed: {error}"),
        })??;

    let updated = repo::set_password_hash(&state.pool, caller.tenant_id(), id, &hash).await?;

    if updated == 0 {
        return Err(AppError::not_found("User"));
    }

    // A password change ends every existing session: if the change was prompted
    // by a suspected compromise, leaving sessions alive defeats the point.
    repo::revoke_all_for_user(&state.pool, id, "password changed").await?;

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id: caller.tenant_id(),
            event_type: "User.PasswordChanged",
            action: "UPDATE",
            object_type: "USER",
            object_id: id,
            actor_user_id: Some(caller.user_id()),
            ip_address: None,
            reason: None,
            old_value: None,
            new_value: None,
        },
    )
    .await;

    Ok(())
}

// ---------------------------------------------------------------------------
// Roles and permissions
// ---------------------------------------------------------------------------

pub async fn list_roles(
    state: &AppState,
    caller: &Authenticated,
    pagination: &Pagination,
) -> Result<(Vec<Role>, PageMeta), AppError> {
    caller.require("identity:role:read")?;

    let tenant_id = caller.tenant_id();
    let total = repo::count_roles(&state.pool, tenant_id).await?;
    let roles = repo::list_roles(
        &state.pool,
        tenant_id,
        pagination.limit(),
        pagination.offset(),
    )
    .await?;

    Ok((roles, pagination.meta(total.max(0) as u64)))
}

pub async fn get_role(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
) -> Result<Role, AppError> {
    caller.require("identity:role:read")?;

    repo::find_role(&state.pool, caller.tenant_id(), id)
        .await?
        .ok_or_else(|| AppError::not_found("Role"))
}

pub async fn list_permissions(
    state: &AppState,
    caller: &Authenticated,
) -> Result<Vec<Permission>, AppError> {
    caller.require("identity:role:read")?;

    Ok(repo::list_permissions(&state.pool).await?)
}

pub async fn create_role(
    state: &AppState,
    caller: &Authenticated,
    request: CreateRoleRequest,
) -> Result<Role, AppError> {
    caller.require("identity:role:create")?;

    let tenant_id = caller.tenant_id();
    let id = Uuid::now_v7();

    let mut transaction = state.pool.begin().await?;

    repo::insert_role(
        &mut *transaction,
        id,
        tenant_id,
        request.role_code.trim(),
        request.name.trim(),
        request.description.as_deref(),
        Some(caller.user_id()),
    )
    .await
    .map_err(duplicate_role_to_conflict)?;

    repo::replace_role_permissions(&mut transaction, tenant_id, id, &request.permission_ids)
        .await?;
    transaction.commit().await?;

    audit_permission_change(
        state,
        caller,
        id,
        "Role.Created",
        "CREATE",
        &request.permission_ids,
    )
    .await;

    repo::find_role(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::not_found("Role"))
}

pub async fn update_role(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
    request: UpdateRoleRequest,
) -> Result<Role, AppError> {
    caller.require("identity:role:update")?;

    let tenant_id = caller.tenant_id();
    let mut transaction = state.pool.begin().await?;

    let updated = repo::update_role_fields(
        &mut *transaction,
        tenant_id,
        id,
        request.name.as_deref().map(str::trim),
        request.description.as_deref(),
        Some(caller.user_id()),
    )
    .await?;

    if updated == 0 {
        return Err(AppError::not_found("Role"));
    }

    if let Some(permission_ids) = &request.permission_ids {
        repo::replace_role_permissions(&mut transaction, tenant_id, id, permission_ids).await?;
    }

    transaction.commit().await?;

    audit_permission_change(
        state,
        caller,
        id,
        "Role.Updated",
        "PERMISSION_CHANGE",
        request.permission_ids.as_deref().unwrap_or(&[]),
    )
    .await;

    repo::find_role(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::not_found("Role"))
}

pub async fn delete_role(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
) -> Result<(), AppError> {
    caller.require("identity:role:delete")?;

    let removed =
        repo::soft_delete_role(&state.pool, caller.tenant_id(), id, Some(caller.user_id())).await?;

    if removed == 0 {
        // Either it does not exist or it is a system role. The repository guards
        // is_system, so tell the caller which rather than a bare 404.
        let exists = repo::find_role(&state.pool, caller.tenant_id(), id).await?;

        return match exists {
            Some(role) if role.is_system => Err(AppError::conflict(
                "System roles cannot be deleted; a tenant would be left unable to grant permissions",
            )),
            _ => Err(AppError::not_found("Role")),
        };
    }

    audit_permission_change(state, caller, id, "Role.Deleted", "DELETE", &[]).await;

    Ok(())
}

async fn get_user_unchecked(state: &AppState, tenant_id: Uuid, id: Uuid) -> Result<User, AppError> {
    repo::find_user(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::not_found("User"))
}

/// Permission grants are the changes most worth being able to reconstruct
/// later (SRS FR-AUD-002), so they are always audited with their new set.
async fn audit_permission_change(
    state: &AppState,
    caller: &Authenticated,
    role_id: Uuid,
    event_type: &str,
    action: &str,
    permission_ids: &[Uuid],
) {
    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id: caller.tenant_id(),
            event_type,
            action,
            object_type: "ROLE",
            object_id: role_id,
            actor_user_id: Some(caller.user_id()),
            ip_address: None,
            reason: None,
            old_value: None,
            new_value: Some(serde_json::json!({ "permissionIds": permission_ids })),
        },
    )
    .await;
}

/// A unique-violation on users is a duplicate username or email, which is the
/// caller's problem to fix — not an internal error.
fn duplicate_user_to_conflict(error: sqlx::Error) -> AppError {
    if is_unique_violation(&error) {
        AppError::conflict("That username or email address is already in use")
    } else {
        AppError::from(error)
    }
}

fn duplicate_role_to_conflict(error: sqlx::Error) -> AppError {
    if is_unique_violation(&error) {
        AppError::conflict("That role code is already in use")
    } else {
        AppError::from(error)
    }
}

/// Refuses a department reference that names nothing in this tenant.
///
/// **The edge decision D-8 left to identity** — FR-IDM-008 is re-scoped to
/// exactly this, the user-to-department assignment, while the department entity
/// itself belongs to the organization module (#28).
///
/// Checked here rather than left to the foreign key, because the foreign key
/// answers with a 500 that names a constraint. It is also not a substitute for
/// the constraint: this read and the insert are not one transaction, so a
/// department deleted in between is still caught by the key — as a 500, but a
/// correct one, and the window is a rare administrative race rather than the
/// ordinary case of a mistyped id.
async fn check_department(
    state: &AppState,
    tenant_id: Uuid,
    department_id: Option<Uuid>,
) -> Result<(), AppError> {
    let Some(department_id) = department_id else {
        return Ok(());
    };

    if department_repo::department_is_live(&state.pool, tenant_id, department_id).await? {
        return Ok(());
    }

    Err(AppError::validation(vec![ValidationDetail::new(
        "departmentId",
        "exists",
        "NOT_FOUND",
        "No department with that id in this tenant",
    )]))
}

fn is_unique_violation(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Database(db) if db.code().as_deref() == Some("23505"))
}
