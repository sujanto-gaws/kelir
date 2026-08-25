//! Test data.
//!
//! Seeded through the application's own repository functions wherever one
//! exists, so a fixture cannot construct a row the application could not — a
//! hand-written `INSERT` that quietly disagrees with `insert_user` would make
//! the tests above it prove nothing. Only rows with no repository behind them
//! yet (tenants) are inserted directly.
//!
//! Business identifiers follow naming convention §8: `TNT-001`, `DEPT-PROC`.

use sqlx::PgPool;
use uuid::Uuid;

use kelir_backend::modules::auth::password::hash_password;
use kelir_backend::modules::identity::repository as identity_repo;

use super::harness_failure;

/// The **system tenant's** `ROLE-ADMIN`, seeded by `0002_identity.sql` with
/// every permission.
///
/// It is no longer the id the bootstrap grants: under decision **D-18** roles
/// are tenant-scoped, so the bootstrap looks its role up by code inside the
/// tenant it is creating the account in (#65). Every tenant has a `ROLE-ADMIN`
/// and this id names one of them — the seeded one.
pub const ADMIN_ROLE_ID: Uuid = uuid::uuid!("00000000-0000-0000-0002-000000000001");

/// The reserved system tenant, seeded by `0001_core.sql`.
///
/// The tenant a sign-in resolves to when the harness leaves
/// `default_tenant_code` at `SYSTEM`, which every test but the tenancy ones
/// does — so a fixture user who must be able to authenticate lives here.
pub const SYSTEM_TENANT_ID: Uuid = uuid::uuid!("00000000-0000-0000-0000-000000000001");

/// Creates an active user with exactly the roles given — `&[]` for a user who
/// holds none.
///
/// Returns the new user's id.
pub async fn create_user(
    pool: &PgPool,
    tenant_id: Uuid,
    username: &str,
    email: &str,
    password: &str,
    role_ids: &[Uuid],
) -> Uuid {
    let hash = hash_password(password).unwrap_or_else(|error| {
        harness_failure("hash a fixture password", &error.to_string(), username)
    });

    let id = Uuid::now_v7();
    let mut transaction = pool.begin().await.unwrap_or_else(|error| {
        harness_failure("open a fixture transaction", &error.to_string(), username)
    });

    identity_repo::insert_user(
        &mut *transaction,
        id,
        tenant_id,
        username,
        email,
        &hash,
        username,
        None,
        // A fixture user signs in and exercises endpoints; a pending password
        // change is a state a test asks for deliberately, never a default.
        false,
        None,
    )
    .await
    .unwrap_or_else(|error| {
        harness_failure(
            &format!("insert the fixture user '{username}'"),
            &error.to_string(),
            username,
        )
    });

    identity_repo::replace_user_roles(&mut transaction, tenant_id, id, role_ids)
        .await
        .unwrap_or_else(|error| {
            harness_failure(
                &format!("grant roles to the fixture user '{username}'"),
                &error.to_string(),
                username,
            )
        });

    transaction.commit().await.unwrap_or_else(|error| {
        harness_failure("commit a fixture transaction", &error.to_string(), username)
    });

    id
}

/// Creates a role holding exactly the permissions named, and returns its id.
///
/// Takes permission *codes* rather than ids for two reasons. A test that says
/// `identity:user:create` says what it means; and a code that is not in the
/// catalogue fails here, loudly, instead of quietly granting nothing — which is
/// precisely the failure that would make an authorization test pass for the
/// wrong reason. The lookup goes through `list_permissions`, the same catalogue
/// read the API serves, so a test cannot grant a permission the product does
/// not have.
pub async fn create_role_with_permissions(
    pool: &PgPool,
    tenant_id: Uuid,
    role_code: &str,
    permission_codes: &[&str],
) -> Uuid {
    let catalogue = identity_repo::list_permissions(pool)
        .await
        .unwrap_or_else(|error| {
            harness_failure(
                "read the permission catalogue",
                &error.to_string(),
                role_code,
            )
        });

    let permission_ids: Vec<Uuid> = permission_codes
        .iter()
        .map(|code| {
            catalogue
                .iter()
                .find(|permission| permission.permission_code == *code)
                .unwrap_or_else(|| {
                    harness_failure(
                        "find a permission in the catalogue",
                        &format!("'{code}' is not a seeded permission"),
                        role_code,
                    )
                })
                .id
        })
        .collect();

    let id = Uuid::now_v7();
    let mut transaction = pool.begin().await.unwrap_or_else(|error| {
        harness_failure("open a fixture transaction", &error.to_string(), role_code)
    });

    identity_repo::insert_role(
        &mut *transaction,
        id,
        tenant_id,
        role_code,
        role_code,
        None,
        // A fixture role is an ordinary role: `is_system` would make it
        // undeletable, and a test that needed to delete one would then be
        // asserting against the wrong refusal.
        false,
        None,
    )
    .await
    .unwrap_or_else(|error| {
        harness_failure(
            &format!("insert the fixture role '{role_code}'"),
            &error.to_string(),
            role_code,
        )
    });

    identity_repo::replace_role_permissions(&mut transaction, tenant_id, id, &permission_ids)
        .await
        .unwrap_or_else(|error| {
            harness_failure(
                &format!("grant permissions to the fixture role '{role_code}'"),
                &error.to_string(),
                role_code,
            )
        });

    transaction.commit().await.unwrap_or_else(|error| {
        harness_failure(
            "commit a fixture transaction",
            &error.to_string(),
            role_code,
        )
    });

    id
}

/// Creates a second tenant, so tenant-scoped queries can be probed with another
/// tenant's data actually present rather than assumed absent.
pub async fn create_tenant(pool: &PgPool, tenant_code: &str, name: &str) -> Uuid {
    let id = Uuid::now_v7();

    // Direct on purpose, now that `organization::repository` does own `tenants`.
    // A fixture tenant is a *bare* tenant — no roles, no users — which is
    // precisely what `create_tenant` refuses to produce (D-18), so a test that
    // needs one cannot get it from the surface under test. Tests asserting what
    // the API creates go through the API.
    sqlx::query(
        "INSERT INTO tenants (id, tenant_code, name, status) VALUES ($1, $2, $3, 'ACTIVE')",
    )
    .bind(id)
    .bind(tenant_code)
    .bind(name)
    .execute(pool)
    .await
    .unwrap_or_else(|error| {
        harness_failure(
            &format!("insert the fixture tenant '{tenant_code}'"),
            &error.to_string(),
            tenant_code,
        )
    });

    id
}

/// Fails loudly if the migration no longer seeds `ROLE-ADMIN` in the system
/// tenant under the id above.
///
/// The bootstrap no longer depends on that id (**D-18**), but plenty else
/// assumes the seeded row exists and is a *system* role — `delete_role`'s
/// refusal, and every fixture that grants it. Drift here would not fail a unit
/// test; it would fail at startup on a fresh deployment.
pub async fn assert_admin_role_exists(pool: &PgPool) {
    let found: Option<(String, bool)> = sqlx::query_as(
        "SELECT role_code, is_system FROM roles WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(ADMIN_ROLE_ID)
    .fetch_optional(pool)
    .await
    .unwrap_or_else(|error| {
        harness_failure("read the seeded admin role", &error.to_string(), "roles")
    });

    let (role_code, is_system) =
        found.expect("0002_identity.sql must seed ROLE-ADMIN under the id the bootstrap grants");

    assert_eq!(role_code, "ROLE-ADMIN");
    assert!(is_system, "ROLE-ADMIN must be a system role");
}
