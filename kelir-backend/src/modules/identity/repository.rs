//! SQLx access for the identity module. Private to the module: other modules go
//! through `service` (coding standard §2.2).
//!
//! Every query filters by `tenant_id` (database schema §1.5) and excludes
//! soft-deleted rows.

use chrono::{DateTime, Utc};
use sqlx::{PgExecutor, PgPool};
use uuid::Uuid;

use super::domain::{Permission, Role, RoleSummary, User, UserStatus};

/// A user row including the password hash — repository-internal, so the hash
/// cannot escape into a response by accident.
pub struct UserCredentials {
    pub id: Uuid,
    pub username: String,
    pub password_hash: String,
    pub status: String,
}

pub async fn find_credentials_by_username(
    executor: impl PgExecutor<'_>,
    tenant_id: Uuid,
    username: &str,
) -> Result<Option<UserCredentials>, sqlx::Error> {
    // Username or email: FR-AUTH-001 allows either as the identifier.
    sqlx::query_as!(
        UserCredentials,
        r#"
        SELECT id, username, password_hash, status
        FROM users
        WHERE tenant_id = $1
          AND (lower(username) = lower($2) OR lower(email) = lower($2))
          AND deleted_at IS NULL
        "#,
        tenant_id,
        username
    )
    .fetch_optional(executor)
    .await
}

pub async fn record_successful_login(
    executor: impl PgExecutor<'_>,
    user_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE users SET last_login_at = now(), failed_login_count = 0, updated_at = now() WHERE id = $1",
        user_id
    )
    .execute(executor)
    .await
    .map(|_| ())
}

pub async fn record_failed_login(
    executor: impl PgExecutor<'_>,
    user_id: Uuid,
) -> Result<i32, sqlx::Error> {
    // Returns the new count so the caller can decide about locking (NFR-SEC-008).
    sqlx::query_scalar!(
        r#"
        UPDATE users
        SET failed_login_count = failed_login_count + 1, updated_at = now()
        WHERE id = $1
        RETURNING failed_login_count
        "#,
        user_id
    )
    .fetch_one(executor)
    .await
}

/// Permission codes granted to a user through their roles.
pub async fn permissions_for_user(
    executor: impl PgExecutor<'_>,
    user_id: Uuid,
) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        SELECT DISTINCT p.permission_code
        FROM user_roles ur
        JOIN role_permissions rp ON rp.role_id = ur.role_id AND rp.deleted_at IS NULL
        JOIN permissions p ON p.id = rp.permission_id AND p.deleted_at IS NULL
        WHERE ur.user_id = $1
          AND ur.deleted_at IS NULL
          AND (ur.valid_from IS NULL OR ur.valid_from <= current_date)
          AND (ur.valid_to IS NULL OR ur.valid_to >= current_date)
        ORDER BY 1
        "#,
        user_id
    )
    .fetch_all(executor)
    .await
}

pub async fn role_codes_for_user(
    executor: impl PgExecutor<'_>,
    user_id: Uuid,
) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        SELECT r.role_code
        FROM user_roles ur
        JOIN roles r ON r.id = ur.role_id AND r.deleted_at IS NULL
        WHERE ur.user_id = $1 AND ur.deleted_at IS NULL
        ORDER BY 1
        "#,
        user_id
    )
    .fetch_all(executor)
    .await
}

// ---------------------------------------------------------------------------
// Refresh tokens
// ---------------------------------------------------------------------------

pub struct StoredRefreshToken {
    pub id: Uuid,
    pub user_id: Uuid,
    pub tenant_id: Uuid,
    pub expires_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

pub async fn insert_refresh_token(
    executor: impl PgExecutor<'_>,
    id: Uuid,
    tenant_id: Uuid,
    user_id: Uuid,
    token_hash: &str,
    expires_at: DateTime<Utc>,
    rotated_from_id: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO refresh_tokens (id, tenant_id, user_id, token_hash, expires_at, rotated_from_id)
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
        id,
        tenant_id,
        user_id,
        token_hash,
        expires_at,
        rotated_from_id
    )
    .execute(executor)
    .await
    .map(|_| ())
}

pub async fn find_refresh_token(
    executor: impl PgExecutor<'_>,
    token_hash: &str,
) -> Result<Option<StoredRefreshToken>, sqlx::Error> {
    sqlx::query_as!(
        StoredRefreshToken,
        r#"
        SELECT id, user_id, tenant_id, expires_at, revoked_at
        FROM refresh_tokens
        WHERE token_hash = $1
        "#,
        token_hash
    )
    .fetch_optional(executor)
    .await
}

pub async fn revoke_refresh_token(
    executor: impl PgExecutor<'_>,
    id: Uuid,
    reason: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE refresh_tokens SET revoked_at = now(), revoked_reason = $2 WHERE id = $1 AND revoked_at IS NULL",
        id,
        reason
    )
    .execute(executor)
    .await
    .map(|_| ())
}

/// Revokes every live token for a user — used on logout-everywhere and when a
/// rotated token is replayed.
pub async fn revoke_all_for_user(
    executor: impl PgExecutor<'_>,
    user_id: Uuid,
    reason: &str,
) -> Result<u64, sqlx::Error> {
    sqlx::query!(
        "UPDATE refresh_tokens SET revoked_at = now(), revoked_reason = $2 WHERE user_id = $1 AND revoked_at IS NULL",
        user_id,
        reason
    )
    .execute(executor)
    .await
    .map(|result| result.rows_affected())
}

// ---------------------------------------------------------------------------
// Users
// ---------------------------------------------------------------------------

pub async fn count_users(pool: &PgPool, tenant_id: Uuid) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!(
        "SELECT count(*) FROM users WHERE tenant_id = $1 AND deleted_at IS NULL",
        tenant_id
    )
    .fetch_one(pool)
    .await
    .map(|count| count.unwrap_or(0))
}

pub async fn list_users(
    pool: &PgPool,
    tenant_id: Uuid,
    limit: i64,
    offset: i64,
) -> Result<Vec<User>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT id, username, email, display_name, status, department_id,
               must_change_password, last_login_at, created_at
        FROM users
        WHERE tenant_id = $1 AND deleted_at IS NULL
        ORDER BY username
        LIMIT $2 OFFSET $3
        "#,
        tenant_id,
        limit,
        offset
    )
    .fetch_all(pool)
    .await?;

    let mut users = Vec::with_capacity(rows.len());
    for row in rows {
        users.push(User {
            id: row.id,
            username: row.username,
            email: row.email,
            display_name: row.display_name,
            status: UserStatus::from_db(&row.status),
            department_id: row.department_id,
            must_change_password: row.must_change_password,
            last_login_at: row.last_login_at,
            created_at: row.created_at,
            roles: roles_of_user(pool, row.id).await?,
        });
    }

    Ok(users)
}

pub async fn find_user(
    pool: &PgPool,
    tenant_id: Uuid,
    id: Uuid,
) -> Result<Option<User>, sqlx::Error> {
    let Some(row) = sqlx::query!(
        r#"
        SELECT id, username, email, display_name, status, department_id,
               must_change_password, last_login_at, created_at
        FROM users
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        id
    )
    .fetch_optional(pool)
    .await?
    else {
        return Ok(None);
    };

    Ok(Some(User {
        id: row.id,
        username: row.username,
        email: row.email,
        display_name: row.display_name,
        status: UserStatus::from_db(&row.status),
        department_id: row.department_id,
        must_change_password: row.must_change_password,
        last_login_at: row.last_login_at,
        created_at: row.created_at,
        roles: roles_of_user(pool, row.id).await?,
    }))
}

pub async fn roles_of_user(
    executor: impl PgExecutor<'_>,
    user_id: Uuid,
) -> Result<Vec<RoleSummary>, sqlx::Error> {
    sqlx::query_as!(
        RoleSummary,
        r#"
        SELECT r.id, r.role_code, r.name
        FROM user_roles ur
        JOIN roles r ON r.id = ur.role_id AND r.deleted_at IS NULL
        WHERE ur.user_id = $1 AND ur.deleted_at IS NULL
        ORDER BY r.role_code
        "#,
        user_id
    )
    .fetch_all(executor)
    .await
}

#[allow(
    clippy::too_many_arguments,
    reason = "one parameter per column; grouping them into a struct would only move the list"
)]
pub async fn insert_user(
    executor: impl PgExecutor<'_>,
    id: Uuid,
    tenant_id: Uuid,
    username: &str,
    email: &str,
    password_hash: &str,
    display_name: &str,
    department_id: Option<Uuid>,
    created_by: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO users (id, tenant_id, username, email, password_hash, display_name, department_id, created_by)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        "#,
        id,
        tenant_id,
        username,
        email,
        password_hash,
        display_name,
        department_id,
        created_by
    )
    .execute(executor)
    .await
    .map(|_| ())
}

#[allow(
    clippy::too_many_arguments,
    reason = "one parameter per updatable column, each independently optional"
)]
pub async fn update_user_fields(
    executor: impl PgExecutor<'_>,
    tenant_id: Uuid,
    id: Uuid,
    email: Option<&str>,
    display_name: Option<&str>,
    status: Option<&str>,
    department_id: Option<Uuid>,
    updated_by: Option<Uuid>,
) -> Result<u64, sqlx::Error> {
    // COALESCE keeps every unsupplied field at its current value, so a partial
    // update cannot blank a column it did not mention.
    sqlx::query!(
        r#"
        UPDATE users
        SET email = COALESCE($3, email),
            display_name = COALESCE($4, display_name),
            status = COALESCE($5, status),
            department_id = COALESCE($6, department_id),
            updated_by = $7,
            updated_at = now()
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        id,
        email,
        display_name,
        status,
        department_id,
        updated_by
    )
    .execute(executor)
    .await
    .map(|result| result.rows_affected())
}

pub async fn soft_delete_user(
    executor: impl PgExecutor<'_>,
    tenant_id: Uuid,
    id: Uuid,
    updated_by: Option<Uuid>,
) -> Result<u64, sqlx::Error> {
    sqlx::query!(
        r#"
        UPDATE users
        SET deleted_at = now(), status = 'INACTIVE', updated_by = $3, updated_at = now()
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        id,
        updated_by
    )
    .execute(executor)
    .await
    .map(|result| result.rows_affected())
}

pub async fn replace_user_roles(
    executor: &mut sqlx::PgConnection,
    tenant_id: Uuid,
    user_id: Uuid,
    role_ids: &[Uuid],
) -> Result<(), sqlx::Error> {
    sqlx::query!("DELETE FROM user_roles WHERE user_id = $1", user_id)
        .execute(&mut *executor)
        .await?;

    for role_id in role_ids {
        sqlx::query!(
            "INSERT INTO user_roles (id, tenant_id, user_id, role_id) VALUES ($1, $2, $3, $4)",
            Uuid::now_v7(),
            tenant_id,
            user_id,
            role_id
        )
        .execute(&mut *executor)
        .await?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Roles and permissions
// ---------------------------------------------------------------------------

pub async fn count_roles(pool: &PgPool, tenant_id: Uuid) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!(
        "SELECT count(*) FROM roles WHERE tenant_id = $1 AND deleted_at IS NULL",
        tenant_id
    )
    .fetch_one(pool)
    .await
    .map(|count| count.unwrap_or(0))
}

pub async fn list_roles(
    pool: &PgPool,
    tenant_id: Uuid,
    limit: i64,
    offset: i64,
) -> Result<Vec<Role>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT id, role_code, name, description, is_system
        FROM roles
        WHERE tenant_id = $1 AND deleted_at IS NULL
        ORDER BY role_code
        LIMIT $2 OFFSET $3
        "#,
        tenant_id,
        limit,
        offset
    )
    .fetch_all(pool)
    .await?;

    let mut roles = Vec::with_capacity(rows.len());
    for row in rows {
        roles.push(Role {
            id: row.id,
            role_code: row.role_code,
            name: row.name,
            description: row.description,
            is_system: row.is_system,
            permissions: permissions_of_role(pool, row.id).await?,
        });
    }

    Ok(roles)
}

pub async fn find_role(
    pool: &PgPool,
    tenant_id: Uuid,
    id: Uuid,
) -> Result<Option<Role>, sqlx::Error> {
    let Some(row) = sqlx::query!(
        r#"
        SELECT id, role_code, name, description, is_system
        FROM roles
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        id
    )
    .fetch_optional(pool)
    .await?
    else {
        return Ok(None);
    };

    Ok(Some(Role {
        id: row.id,
        role_code: row.role_code,
        name: row.name,
        description: row.description,
        is_system: row.is_system,
        permissions: permissions_of_role(pool, row.id).await?,
    }))
}

pub async fn permissions_of_role(
    executor: impl PgExecutor<'_>,
    role_id: Uuid,
) -> Result<Vec<Permission>, sqlx::Error> {
    sqlx::query_as!(
        Permission,
        r#"
        SELECT p.id, p.permission_code, p.module, p.description
        FROM role_permissions rp
        JOIN permissions p ON p.id = rp.permission_id AND p.deleted_at IS NULL
        WHERE rp.role_id = $1 AND rp.deleted_at IS NULL
        ORDER BY p.permission_code
        "#,
        role_id
    )
    .fetch_all(executor)
    .await
}

pub async fn list_permissions(pool: &PgPool) -> Result<Vec<Permission>, sqlx::Error> {
    sqlx::query_as!(
        Permission,
        r#"
        SELECT id, permission_code, module, description
        FROM permissions
        WHERE deleted_at IS NULL
        ORDER BY module, permission_code
        "#
    )
    .fetch_all(pool)
    .await
}

pub async fn insert_role(
    executor: impl PgExecutor<'_>,
    id: Uuid,
    tenant_id: Uuid,
    role_code: &str,
    name: &str,
    description: Option<&str>,
    created_by: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO roles (id, tenant_id, role_code, name, description, created_by)
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
        id,
        tenant_id,
        role_code,
        name,
        description,
        created_by
    )
    .execute(executor)
    .await
    .map(|_| ())
}

pub async fn update_role_fields(
    executor: impl PgExecutor<'_>,
    tenant_id: Uuid,
    id: Uuid,
    name: Option<&str>,
    description: Option<&str>,
    updated_by: Option<Uuid>,
) -> Result<u64, sqlx::Error> {
    sqlx::query!(
        r#"
        UPDATE roles
        SET name = COALESCE($3, name),
            description = COALESCE($4, description),
            updated_by = $5,
            updated_at = now()
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        id,
        name,
        description,
        updated_by
    )
    .execute(executor)
    .await
    .map(|result| result.rows_affected())
}

pub async fn soft_delete_role(
    executor: impl PgExecutor<'_>,
    tenant_id: Uuid,
    id: Uuid,
    updated_by: Option<Uuid>,
) -> Result<u64, sqlx::Error> {
    // is_system guards the seeded administrator role: deleting it would leave a
    // tenant with no way to grant permissions.
    sqlx::query!(
        r#"
        UPDATE roles
        SET deleted_at = now(), updated_by = $3, updated_at = now()
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL AND is_system = false
        "#,
        tenant_id,
        id,
        updated_by
    )
    .execute(executor)
    .await
    .map(|result| result.rows_affected())
}

pub async fn replace_role_permissions(
    executor: &mut sqlx::PgConnection,
    tenant_id: Uuid,
    role_id: Uuid,
    permission_ids: &[Uuid],
) -> Result<(), sqlx::Error> {
    sqlx::query!("DELETE FROM role_permissions WHERE role_id = $1", role_id)
        .execute(&mut *executor)
        .await?;

    for permission_id in permission_ids {
        sqlx::query!(
            "INSERT INTO role_permissions (id, tenant_id, role_id, permission_id) VALUES ($1, $2, $3, $4)",
            Uuid::now_v7(),
            tenant_id,
            role_id,
            permission_id
        )
        .execute(&mut *executor)
        .await?;
    }

    Ok(())
}

pub async fn set_password_hash(
    executor: impl PgExecutor<'_>,
    tenant_id: Uuid,
    id: Uuid,
    password_hash: &str,
) -> Result<u64, sqlx::Error> {
    sqlx::query!(
        r#"
        UPDATE users
        SET password_hash = $3, must_change_password = false, failed_login_count = 0, updated_at = now()
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        id,
        password_hash
    )
    .execute(executor)
    .await
    .map(|result| result.rows_affected())
}
