//! Role administration and what a grant actually means (#59).
//!
//! Sprint 3 recorded the seeded-role guard and the audit chain as verified by
//! hand. Neither had a test. The permission-matching claim had one, on
//! hand-built claims — never on a permission granted through the database and
//! carried into a real token, which is the path that decides what a role is
//! worth.

mod common;

use axum::http::StatusCode;
use common::{fixtures, TestApp};
use serde_json::json;
use uuid::Uuid;

const PASSWORD: &str = "correct horse battery";

async fn permission_id(app: &TestApp, code: &str) -> Uuid {
    sqlx::query_scalar("SELECT id FROM permissions WHERE permission_code = $1")
        .bind(code)
        .fetch_one(&app.pool)
        .await
        .unwrap_or_else(|error| panic!("`{code}` is not in the seeded catalogue: {error}"))
}

async fn role_id(app: &TestApp, role_code: &str) -> Uuid {
    sqlx::query_scalar("SELECT id FROM roles WHERE role_code = $1")
        .bind(role_code)
        .fetch_one(&app.pool)
        .await
        .unwrap_or_else(|error| panic!("role `{role_code}` is missing: {error}"))
}

/// The permission codes actually granted to a role, read from the join table.
async fn granted_codes(app: &TestApp, role: Uuid) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT p.permission_code
           FROM role_permissions rp
           JOIN permissions p ON p.id = rp.permission_id
          WHERE rp.role_id = $1
          ORDER BY p.permission_code",
    )
    .bind(role)
    .fetch_all(&app.pool)
    .await
    .expect("query runs")
}

#[tokio::test]
async fn a_system_role_cannot_be_deleted() {
    // `ROLE-ADMIN` is seeded with `is_system`. Deleting it would leave the
    // tenant with no way to grant permissions and no account able to restore
    // one — recoverable only by editing the database directly.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let admin_role = role_id(&app, "ROLE-ADMIN").await;

    let response = app
        .delete(
            &format!("/api/v1/identity/roles/{admin_role}"),
            Some(&token),
        )
        .await;

    assert_eq!(
        response.status,
        StatusCode::CONFLICT,
        "expected the system-role guard, got {}: {}",
        response.status,
        response.body
    );
    assert_eq!(response.error_code(), Some("CONFLICT"));

    // A 409 that had already soft-deleted the row would be the worst outcome:
    // the right status over the wrong state.
    let deleted_at: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT deleted_at FROM roles WHERE id = $1")
            .bind(admin_role)
            .fetch_one(&app.pool)
            .await
            .expect("select runs");

    assert!(deleted_at.is_none(), "the system role was soft-deleted");
}

#[tokio::test]
async fn a_non_system_role_can_be_deleted() {
    // The control: the guard must refuse system roles only.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let created = app
        .post(
            "/api/v1/identity/roles",
            Some(&token),
            json!({ "roleCode": "ROLE-TEMPORARY", "name": "Temporary" }),
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let id = created.data()["id"].as_str().expect("id is a string");

    let response = app
        .delete(&format!("/api/v1/identity/roles/{id}"), Some(&token))
        .await;

    assert_eq!(response.status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn creating_a_role_grants_exactly_the_permissions_requested() {
    // Asserted against `role_permissions`, not against the response echo. A
    // handler that returned the ids it was handed while granting none would
    // satisfy any assertion made on its own output.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let wanted = ["identity:user:read", "identity:role:read"];
    let mut ids = Vec::new();
    for code in wanted {
        ids.push(permission_id(&app, code).await.to_string());
    }

    let created = app
        .post(
            "/api/v1/identity/roles",
            Some(&token),
            json!({
                "roleCode": "ROLE-READONLY",
                "name": "Read Only",
                "permissionIds": ids,
            }),
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    let id: Uuid = created.data()["id"]
        .as_str()
        .expect("id is a string")
        .parse()
        .expect("id is a uuid");

    let mut granted = granted_codes(&app, id).await;
    granted.sort();

    let mut expected: Vec<String> = wanted.iter().map(|code| (*code).to_owned()).collect();
    expected.sort();

    assert_eq!(
        granted, expected,
        "the stored grant does not match what was requested"
    );
}

#[tokio::test]
async fn updating_a_roles_permissions_replaces_them_rather_than_adding() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let read = permission_id(&app, "identity:user:read").await;
    let create = permission_id(&app, "identity:user:create").await;

    let created = app
        .post(
            "/api/v1/identity/roles",
            Some(&token),
            json!({
                "roleCode": "ROLE-NARROWING",
                "name": "Narrowing",
                "permissionIds": [read.to_string(), create.to_string()],
            }),
        )
        .await;

    let id: Uuid = created.data()["id"]
        .as_str()
        .expect("id is a string")
        .parse()
        .expect("id is a uuid");

    let updated = app
        .put(
            &format!("/api/v1/identity/roles/{id}"),
            Some(&token),
            json!({ "permissionIds": [read.to_string()] }),
        )
        .await;

    assert_eq!(updated.status, StatusCode::OK, "{}", updated.body);

    assert_eq!(
        granted_codes(&app, id).await,
        vec!["identity:user:read".to_owned()],
        "a narrowed grant must remove what it left out; otherwise a permission \
         can never be taken away"
    );
}

#[tokio::test]
async fn a_prefix_permission_does_not_grant_the_routes_beneath_it() {
    // The Sprint 3 claim was "exact permission matching, prefix never grants",
    // and the only evidence was a unit test on hand-built claims. This grants a
    // prefix through the database, signs in, and drives the real route: the
    // permission is in the token, and the route still refuses.
    let app = TestApp::spawn().await;

    // `identity:user` is not a permission the code checks — no route requires
    // it — so it has to be inserted rather than found.
    sqlx::query(
        "INSERT INTO permissions (id, tenant_id, permission_code, module, description)
         VALUES ($1, $2, 'identity:user', 'identity', 'A prefix, not a permission')",
    )
    .bind(Uuid::now_v7())
    .bind(fixtures::SYSTEM_TENANT_ID)
    .execute(&app.pool)
    .await
    .expect("insert runs");

    let role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "ROLE-PREFIX",
        &["identity:user"],
    )
    .await;

    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "prefix.holder",
        "prefix.holder@kelir.test",
        PASSWORD,
        &[role],
    )
    .await;

    let token = app.sign_in("prefix.holder", PASSWORD).await;

    for route in [
        "/api/v1/identity/users",
        "/api/v1/identity/roles",
        "/api/v1/identity/permissions",
    ] {
        let response = app.get(route, Some(&token)).await;

        assert_eq!(
            response.status,
            StatusCode::FORBIDDEN,
            "`identity:user` opened {route}, so matching is by prefix somewhere"
        );
    }
}

#[tokio::test]
async fn a_permission_change_appends_a_link_to_the_tenant_chain() {
    // FR-AUD-003: each row's hash covers the previous row's, so altering or
    // removing any row breaks every hash after it. The property that makes that
    // true is that a new row's `previous_hash` is the last row's `current_hash`
    // — checked here across a real permission change rather than asserted in a
    // document.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let read = permission_id(&app, "identity:user:read").await;

    let created = app
        .post(
            "/api/v1/identity/roles",
            Some(&token),
            json!({ "roleCode": "ROLE-AUDITED", "name": "Audited" }),
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    let id = created.data()["id"].as_str().expect("id is a string");

    let (tip_hash, before) = chain_tip(&app).await;

    let updated = app
        .put(
            &format!("/api/v1/identity/roles/{id}"),
            Some(&token),
            json!({ "permissionIds": [read.to_string()] }),
        )
        .await;

    assert_eq!(updated.status, StatusCode::OK, "{}", updated.body);

    let (_, after) = chain_tip(&app).await;
    assert_eq!(
        after,
        before + 1,
        "the permission change wrote no audit row at all"
    );

    let (event_type, action, previous_hash, current_hash): (String, String, String, String) =
        sqlx::query_as(
            "SELECT event_type, action, previous_hash, current_hash
               FROM audit_events
              WHERE tenant_id = $1
              ORDER BY created_at DESC, id DESC
              LIMIT 1",
        )
        .bind(fixtures::SYSTEM_TENANT_ID)
        .fetch_one(&app.pool)
        .await
        .expect("query runs");

    assert_eq!(event_type, "Role.Updated");
    assert_eq!(action, "PERMISSION_CHANGE");
    assert_eq!(
        previous_hash, tip_hash,
        "the new row does not link to the row that preceded it, so the chain is broken"
    );
    assert_ne!(
        current_hash, previous_hash,
        "a row whose hash equals its predecessor's covers none of its own content"
    );
}

/// The newest audit row's hash for the system tenant, and how many rows there
/// are.
async fn chain_tip(app: &TestApp) -> (String, i64) {
    let hash: String = sqlx::query_scalar(
        "SELECT current_hash
           FROM audit_events
          WHERE tenant_id = $1
          ORDER BY created_at DESC, id DESC
          LIMIT 1",
    )
    .bind(fixtures::SYSTEM_TENANT_ID)
    .fetch_one(&app.pool)
    .await
    .expect("the chain is not empty: the bootstrap and sign-in already wrote to it");

    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE tenant_id = $1")
        .bind(fixtures::SYSTEM_TENANT_ID)
        .fetch_one(&app.pool)
        .await
        .expect("count runs");

    (hash, count)
}
