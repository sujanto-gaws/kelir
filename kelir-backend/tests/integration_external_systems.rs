//! The external system registry, through the API and the schema (#520,
//! FR-INT-001).
//!
//! One file for every acceptance criterion the backend can hold, in AC order:
//!
//! * **AC-2** — the eight §12 tables, the deferred key on
//!   `master_data_source_references`, and the composite keys that make a
//!   cross-tenant child row unwritable;
//! * **AC-4** — each of the eight permissions gates its routes and no others,
//!   and a caller who can read a system cannot read where its secrets live;
//! * **AC-5** — a credential response is the reference and nothing else, a raw
//!   secret is refused at the boundary, and no reference is resolved;
//! * **AC-6** — register, edit, deactivate, and the endpoints beside them;
//! * **AC-7** — every route is in the OpenAPI document and no schema there has
//!   a field that could carry a secret;
//! * **tenant isolation** and **validation**, which every AC above assumes.
//!
//! # Seen to fail (coding standard §2.9)
//!
//! Each mutation below was made on the line named, the suite run, the test
//! observed red, and the mutation reverted. **Seen red, 2026-09-24**, all
//! eight.
//!
//! | Mutation | Reddened |
//! |---|---|
//! | `service::credential::list_credentials` requires `EXTERNAL_SYSTEM_READ` instead of `CREDENTIAL_READ` | `each_route_requires_its_own_permission`, `a_caller_who_can_read_a_system_cannot_read_where_its_secrets_live` |
//! | `service::external_system::deactivate_external_system` requires `EXTERNAL_SYSTEM_UPDATE` instead of `EXTERNAL_SYSTEM_DEACTIVATE` | `each_route_requires_its_own_permission` |
//! | `domain::external_system::validate_update`'s `INACTIVE` refusal removed | `an_edit_cannot_do_what_deactivate_is_gated_for` |
//! | `tenant_id = $1 AND` dropped from `repository::external_system::find_external_system` | `another_tenants_system_is_not_found_by_any_route` |
//! | `AND external_system_id = $2` dropped from `repository::credential::find_credential` | `a_child_is_found_only_under_its_own_system` |
//! | `domain::credential::is_secret_reference` returns `true` | `a_raw_secret_is_refused_and_nothing_is_stored` |
//! | `fk_integration_endpoints_external_system_id_tenant_id` written as `REFERENCES external_systems (id)` in `0046` | `a_cross_tenant_child_row_is_unwritable` |
//! | `pub note: Option<String>` added to `IntegrationCredential` | `a_credential_response_is_the_reference_and_nothing_else` |
//!
//! Activation (#520, the product owner's decision of 2026-09-25) added the
//! rows below, **run 2026-09-25**. Two guards are layered — a service check
//! over a repository predicate — and each layer alone came back **green**,
//! because the other held; that is recorded as run rather than dropped, and
//! the pair removed together is the red run.
//!
//! | Mutation | Result |
//! |---|---|
//! | `service::external_system::activate_external_system` requires `EXTERNAL_SYSTEM_UPDATE` instead of `EXTERNAL_SYSTEM_DEACTIVATE` | red: `each_route_requires_its_own_permission` |
//! | `integration::handlers::activate_external_system` removed from the router's `paths(...)` | red: `every_route_is_in_the_document_and_no_schema_can_carry_a_secret` |
//! | The `before.status == Inactive` refusal removed from `update_external_system` | green — `update_external_system`'s `status <> 'INACTIVE'` predicate refused it |
//! | That predicate removed from `repository::external_system::update_external_system` | green — the service's refusal held |
//! | Both of the above | red: `an_updater_cannot_reactivate_through_an_edit` |
//! | `activate_external_system`'s early return for an `ACTIVE` system removed | green — `activate_external_system`'s `status <> 'ACTIVE'` predicate made the repeat a no-op |
//! | That predicate removed from `repository::external_system::activate_external_system` | green — the early return held |
//! | Both of the above | red: `a_system_is_deactivated_and_activated_again` (the repeat records a second `Activated`) |
//!
//! Review (2026-09-25) added the rows below: finding F1's query refusal, the
//! credential index, and the four mutations `test-engineer` ran against the
//! tests adopted at the foot of this file, re-run here after the move. **Seen
//! red, 2026-09-25**, all six.
//!
//! | Mutation | Reddened |
//! |---|---|
//! | `domain::external_system::base_url`'s query refusal disabled | `a_base_url_with_a_query_string_is_refused_on_create_and_edit` |
//! | `0046`'s credential index put back to §12.3's `(external_system_id) WHERE is_active` | `the_credential_routes_read_through_an_index` |
//! | M1: `ObjectType::IntegrationCredential.readable_by` answers `integration:external-system:read` | `the_audit_trail_does_not_show_a_reference_to_a_caller_who_may_not_read_it` |
//! | M2: `validate_update` no longer calls `base_url()` | `an_edit_cannot_put_a_credential_into_the_base_url`, `a_base_url_with_a_query_string_is_refused_on_create_and_edit` |
//! | M4: `fk_master_data_source_references_external_system_id` single-column in `0046` | `a_source_reference_cannot_name_another_tenants_system` |
//! | M5: `activate_external_system` takes the tenant from the row rather than the caller | `a_caller_in_another_tenant_reaches_nothing_of_this_tenants_registry`, `another_tenants_system_is_not_found_by_any_route` |

mod common;

use std::collections::BTreeSet;

use axum::http::{Method, StatusCode};
use common::{fixtures, TestApp, TestResponse};
use serde_json::{json, Value};
use uuid::Uuid;

const BASE: &str = "/api/v1/integration/external-systems";
const PASSWORD: &str = "integration-test-user-password";

const PERMISSIONS: [&str; 8] = [
    "integration:external-system:create",
    "integration:external-system:read",
    "integration:external-system:update",
    "integration:external-system:deactivate",
    "integration:credential:create",
    "integration:credential:read",
    "integration:credential:update",
    "integration:credential:delete",
];

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn register(app: &TestApp, token: &str, body: Value) -> TestResponse {
    app.send(Method::POST, BASE, Some(token), Some(body)).await
}

/// A system registered through the API, returning its id.
async fn system(app: &TestApp, token: &str, code: &str) -> Uuid {
    let response = register(
        app,
        token,
        json!({ "systemCode": code, "systemName": format!("{code} system") }),
    )
    .await;

    assert_eq!(
        response.status,
        StatusCode::CREATED,
        "registering {code}: {}",
        response.body
    );

    id_of(&response)
}

fn id_of(response: &TestResponse) -> Uuid {
    response.body["data"]["id"]
        .as_str()
        .and_then(|id| id.parse().ok())
        .unwrap_or_else(|| panic!("no id in {}", response.body))
}

/// A system inserted directly, in any tenant, with nothing above it checked.
async fn system_row(app: &TestApp, tenant_id: Uuid, code: &str) -> Uuid {
    let id = Uuid::now_v7();

    sqlx::query(
        "INSERT INTO external_systems (id, tenant_id, system_code, system_name)
         VALUES ($1, $2, $3, $3)",
    )
    .bind(id)
    .bind(tenant_id)
    .bind(code)
    .execute(&app.pool)
    .await
    .expect("insert an external system row");

    id
}

async fn endpoint_row(app: &TestApp, tenant_id: Uuid, system_id: Uuid, code: &str) -> Uuid {
    let id = Uuid::now_v7();

    sqlx::query(
        "INSERT INTO integration_endpoints
             (id, tenant_id, external_system_id, endpoint_code, name, method, path)
         VALUES ($1, $2, $3, $4, $4, 'GET', '/x')",
    )
    .bind(id)
    .bind(tenant_id)
    .bind(system_id)
    .bind(code)
    .execute(&app.pool)
    .await
    .expect("insert an endpoint row");

    id
}

async fn credential_row(app: &TestApp, tenant_id: Uuid, system_id: Uuid, reference: &str) -> Uuid {
    let id = Uuid::now_v7();

    sqlx::query(
        "INSERT INTO integration_credentials
             (id, tenant_id, external_system_id, credential_type, secret_reference)
         VALUES ($1, $2, $3, 'API_KEY', $4)",
    )
    .bind(id)
    .bind(tenant_id)
    .bind(system_id)
    .bind(reference)
    .execute(&app.pool)
    .await
    .expect("insert a credential row");

    id
}

/// A signed-in caller in the system tenant holding exactly `permissions`.
async fn caller_holding(app: &TestApp, label: &str, permissions: &[&str]) -> String {
    let role_id = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        &format!("ROLE-INT-{label}"),
        permissions,
    )
    .await;

    let username = format!("user.int.{}", label.to_lowercase());
    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        &username,
        &format!("int.{}@kelir.test", label.to_lowercase()),
        PASSWORD,
        &[role_id],
    )
    .await;

    app.sign_in(&username, PASSWORD).await
}

fn keys(value: &Value) -> BTreeSet<String> {
    value
        .as_object()
        .map(|object| object.keys().cloned().collect())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// AC-2 — the schema
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_migration_creates_every_section_12_table_and_the_deferred_key() {
    let app = TestApp::spawn().await;

    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT table_name::text FROM information_schema.tables
         WHERE table_schema = 'public' AND table_name = ANY($1)
         ORDER BY table_name",
    )
    .bind(vec![
        "external_systems",
        "integration_endpoints",
        "integration_credentials",
        "integration_mappings",
        "integration_logs",
        "webhook_subscriptions",
        "webhook_events",
        "inbox_events",
        "outbox_events",
    ])
    .fetch_all(&app.pool)
    .await
    .expect("read the catalogue");

    assert_eq!(
        tables,
        vec![
            "external_systems",
            "inbox_events",
            "integration_credentials",
            "integration_endpoints",
            "integration_logs",
            "integration_mappings",
            "outbox_events",
            "webhook_events",
            "webhook_subscriptions",
        ],
        "all nine §12 tables exist once 0046 has run, outbox_events from 0045"
    );

    let key: Option<String> = sqlx::query_scalar(
        "SELECT pg_get_constraintdef(oid) FROM pg_constraint
         WHERE conname = 'fk_master_data_source_references_external_system_id'",
    )
    .fetch_optional(&app.pool)
    .await
    .expect("read the constraint");

    assert_eq!(
        key.as_deref(),
        Some("FOREIGN KEY (external_system_id, tenant_id) REFERENCES external_systems(id, tenant_id)"),
        "the key 0008 deferred, carrying the tenant"
    );

    // And it holds: a source reference naming no system is refused.
    let orphan = sqlx::query(
        "INSERT INTO master_data_source_references
             (id, tenant_id, entity_type, kelir_entity_id, external_system_id, external_entity_id)
         VALUES ($1, $2, 'PARTY', $3, $4, 'EXT-1')",
    )
    .bind(Uuid::now_v7())
    .bind(fixtures::SYSTEM_TENANT_ID)
    .bind(Uuid::now_v7())
    .bind(Uuid::now_v7())
    .execute(&app.pool)
    .await;

    assert!(
        orphan.is_err(),
        "a source reference to a system that does not exist was stored"
    );
}

#[tokio::test]
async fn a_cross_tenant_child_row_is_unwritable() {
    let app = TestApp::spawn().await;
    let other = fixtures::create_tenant(&app.pool, "TNT-INT-X", "Other tenant").await;
    let theirs = system_row(&app, other, "THEIR_ERP").await;

    // An endpoint filed under the system tenant but naming the other tenant's
    // system. Nothing would read it back; the schema refuses to hold it at all.
    let endpoint = sqlx::query(
        "INSERT INTO integration_endpoints
             (id, tenant_id, external_system_id, endpoint_code, name, method, path)
         VALUES ($1, $2, $3, 'X', 'X', 'GET', '/x')",
    )
    .bind(Uuid::now_v7())
    .bind(fixtures::SYSTEM_TENANT_ID)
    .bind(theirs)
    .execute(&app.pool)
    .await;

    let credential = sqlx::query(
        "INSERT INTO integration_credentials
             (id, tenant_id, external_system_id, credential_type, secret_reference)
         VALUES ($1, $2, $3, 'API_KEY', 'vault://x')",
    )
    .bind(Uuid::now_v7())
    .bind(fixtures::SYSTEM_TENANT_ID)
    .bind(theirs)
    .execute(&app.pool)
    .await;

    assert!(endpoint.is_err(), "a cross-tenant endpoint was written");
    assert!(credential.is_err(), "a cross-tenant credential was written");

    // The same rows in the right tenant are accepted, so the refusal above is
    // the tenant and not something else about the row.
    endpoint_row(&app, other, theirs, "X").await;
    credential_row(&app, other, theirs, "vault://x").await;
}

// ---------------------------------------------------------------------------
// AC-4 — permissions
// ---------------------------------------------------------------------------

struct Route {
    method: Method,
    path: String,
    permission: &'static str,
    body: Option<Value>,
}

/// Every route, with the one permission that opens it. Ordered so the
/// destructive ones come last and the rest still have a target.
fn routes(system: Uuid, endpoint: Uuid, credential: Uuid, nonce: usize) -> Vec<Route> {
    let system_path = format!("{BASE}/{system}");

    vec![
        Route {
            method: Method::GET,
            path: BASE.into(),
            permission: "integration:external-system:read",
            body: None,
        },
        Route {
            method: Method::GET,
            path: system_path.clone(),
            permission: "integration:external-system:read",
            body: None,
        },
        Route {
            method: Method::POST,
            path: BASE.into(),
            permission: "integration:external-system:create",
            body: Some(json!({ "systemCode": format!("MADE_{nonce}"), "systemName": "Made" })),
        },
        Route {
            method: Method::PUT,
            path: system_path.clone(),
            permission: "integration:external-system:update",
            body: Some(json!({ "systemName": "Renamed" })),
        },
        Route {
            method: Method::GET,
            path: format!("{system_path}/endpoints"),
            permission: "integration:external-system:read",
            body: None,
        },
        Route {
            method: Method::GET,
            path: format!("{system_path}/endpoints/{endpoint}"),
            permission: "integration:external-system:read",
            body: None,
        },
        Route {
            method: Method::POST,
            path: format!("{system_path}/endpoints"),
            permission: "integration:external-system:update",
            body: Some(json!({
                "endpointCode": format!("EP_{nonce}"),
                "name": "Made",
                "method": "POST",
                "path": "/made"
            })),
        },
        Route {
            method: Method::PUT,
            path: format!("{system_path}/endpoints/{endpoint}"),
            permission: "integration:external-system:update",
            body: Some(json!({ "name": "Renamed" })),
        },
        Route {
            method: Method::GET,
            path: format!("{system_path}/credentials"),
            permission: "integration:credential:read",
            body: None,
        },
        Route {
            method: Method::GET,
            path: format!("{system_path}/credentials/{credential}"),
            permission: "integration:credential:read",
            body: None,
        },
        Route {
            method: Method::POST,
            path: format!("{system_path}/credentials"),
            permission: "integration:credential:create",
            body: Some(json!({
                "credentialType": "API_KEY",
                "secretReference": format!("vault://kelir/made/{nonce}")
            })),
        },
        Route {
            method: Method::PUT,
            path: format!("{system_path}/credentials/{credential}"),
            permission: "integration:credential:update",
            body: Some(json!({ "isActive": false })),
        },
        Route {
            method: Method::DELETE,
            path: format!("{system_path}/credentials/{credential}"),
            permission: "integration:credential:delete",
            body: None,
        },
        Route {
            method: Method::POST,
            path: format!("{system_path}/deactivate"),
            permission: "integration:external-system:deactivate",
            body: None,
        },
        // After the deactivation above, so for the one caller it opens it does
        // real work rather than returning an active system unchanged. Turning a
        // system on is the same permission as turning it off (#520, 2026-09-25).
        Route {
            method: Method::POST,
            path: format!("{system_path}/activate"),
            permission: "integration:external-system:deactivate",
            body: None,
        },
    ]
}

/// Each of the eight permissions opens exactly the routes bound to it.
///
/// A caller holding one permission is sent to every route: the routes bound to
/// that permission must succeed and every other must be 403. An administrator
/// holds all eight, so no test signed in as one can tell a swapped permission
/// from the right one — which is this file's reason to exist.
#[tokio::test]
async fn each_route_requires_its_own_permission() {
    let app = TestApp::spawn().await;

    for (nonce, permission) in PERMISSIONS.iter().enumerate() {
        let token = caller_holding(&app, &format!("ONLY{nonce}"), &[permission]).await;

        let system = system_row(&app, fixtures::SYSTEM_TENANT_ID, &format!("TARGET_{nonce}")).await;
        let endpoint = endpoint_row(&app, fixtures::SYSTEM_TENANT_ID, system, "TARGET_EP").await;
        let credential = credential_row(
            &app,
            fixtures::SYSTEM_TENANT_ID,
            system,
            "vault://kelir/target",
        )
        .await;

        for route in routes(system, endpoint, credential, nonce) {
            let response = app
                .send(
                    route.method.clone(),
                    &route.path,
                    Some(&token),
                    route.body.clone(),
                )
                .await;
            let label = format!("{} {}", route.method, route.path);

            if route.permission == *permission {
                assert!(
                    response.status.is_success(),
                    "{label} should be open to a caller holding {permission}: {} {}",
                    response.status,
                    response.body
                );
            } else {
                assert_eq!(
                    response.status,
                    StatusCode::FORBIDDEN,
                    "{label} was reachable by a caller holding only {permission}: {}",
                    response.body
                );
            }
        }
    }
}

#[tokio::test]
async fn every_route_refuses_a_request_without_a_token() {
    let app = TestApp::spawn().await;
    let system = system_row(&app, fixtures::SYSTEM_TENANT_ID, "ANON").await;
    let endpoint = endpoint_row(&app, fixtures::SYSTEM_TENANT_ID, system, "EP").await;
    let credential = credential_row(&app, fixtures::SYSTEM_TENANT_ID, system, "vault://a").await;

    for route in routes(system, endpoint, credential, 900) {
        let response = app
            .send(route.method.clone(), &route.path, None, route.body.clone())
            .await;

        assert_eq!(
            response.status,
            StatusCode::UNAUTHORIZED,
            "{} {} answered without a token",
            route.method,
            route.path
        );
    }
}

/// #520, the product owner's answer 3: seeing a system is not seeing where its
/// secrets live.
#[tokio::test]
async fn a_caller_who_can_read_a_system_cannot_read_where_its_secrets_live() {
    let app = TestApp::spawn().await;
    let reference = "vault://kelir/erp/api-key";

    let system = system_row(&app, fixtures::SYSTEM_TENANT_ID, "SAP_ERP").await;
    let credential = credential_row(&app, fixtures::SYSTEM_TENANT_ID, system, reference).await;

    let reader = caller_holding(&app, "READER", &["integration:external-system:read"]).await;

    // The system and its endpoints are open to them, and say nothing of it.
    for path in [
        BASE.to_owned(),
        format!("{BASE}/{system}"),
        format!("{BASE}/{system}/endpoints"),
    ] {
        let response = app.send(Method::GET, &path, Some(&reader), None).await;

        assert_eq!(response.status, StatusCode::OK, "{path}: {}", response.body);
        assert!(
            !response.body.to_string().contains(reference),
            "{path} carried the reference to a caller without integration:credential:read"
        );
    }

    // The credentials are not.
    for path in [
        format!("{BASE}/{system}/credentials"),
        format!("{BASE}/{system}/credentials/{credential}"),
    ] {
        let response = app.send(Method::GET, &path, Some(&reader), None).await;

        assert_eq!(response.status, StatusCode::FORBIDDEN, "{path}");
        assert!(!response.body.to_string().contains(reference));
    }

    // And a caller holding the credential permission does see it — so the
    // refusal above is the permission, not an empty table.
    let keeper = caller_holding(&app, "KEEPER", &["integration:credential:read"]).await;
    let response = app
        .send(
            Method::GET,
            &format!("{BASE}/{system}/credentials/{credential}"),
            Some(&keeper),
            None,
        )
        .await;

    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.data()["secretReference"], reference);
}

// ---------------------------------------------------------------------------
// AC-5 — references, never secrets
// ---------------------------------------------------------------------------

/// The key set of a credential, on every route that returns one.
///
/// **Exactly these nine**: the reference, its type, its window and its flag,
/// plus the ids and timestamps. A field added to `IntegrationCredential` turns
/// this red, which is the point — whoever adds one has to decide here that it
/// is not a secret.
#[tokio::test]
async fn a_credential_response_is_the_reference_and_nothing_else() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let system = system(&app, &token, "CRM").await;

    let expected: BTreeSet<String> = [
        "id",
        "externalSystemId",
        "credentialType",
        "secretReference",
        "validFrom",
        "validTo",
        "isActive",
        "createdAt",
        "updatedAt",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();

    let created = app
        .send(
            Method::POST,
            &format!("{BASE}/{system}/credentials"),
            Some(&token),
            Some(json!({
                "credentialType": "OAUTH2_CLIENT_CREDENTIALS",
                "secretReference": "vault://kelir/crm/oauth#client_secret",
                "validFrom": "2026-09-01",
                "validTo": "2027-08-31"
            })),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    assert_eq!(keys(created.data()), expected, "create");

    let id = id_of(&created);
    let read = app
        .send(
            Method::GET,
            &format!("{BASE}/{system}/credentials/{id}"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(keys(read.data()), expected, "read");
    assert_eq!(
        read.data()["secretReference"],
        "vault://kelir/crm/oauth#client_secret"
    );
    assert_eq!(read.data()["credentialType"], "OAUTH2_CLIENT_CREDENTIALS");
    assert_eq!(read.data()["validFrom"], "2026-09-01");
    assert_eq!(read.data()["isActive"], true);

    let listed = app
        .send(
            Method::GET,
            &format!("{BASE}/{system}/credentials"),
            Some(&token),
            None,
        )
        .await;
    let rows = listed.data().as_array().expect("a page");
    assert_eq!(rows.len(), 1);
    assert_eq!(keys(&rows[0]), expected, "list");
    assert_eq!(listed.body["meta"]["total"], 1);
}

/// An `env://` reference is stored and returned as the reference. The value
/// the environment holds under that name is never read, so it cannot appear.
#[tokio::test]
async fn a_reference_is_never_resolved() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let system = system(&app, &token, "ENV_BACKED").await;

    // PATH is set in every environment this suite runs in, and its value is
    // long and distinctive enough that a substring match means something.
    let resolved = std::env::var("PATH").expect("PATH is set");
    assert!(resolved.len() > 8, "PATH is too short to be a useful probe");

    let created = app
        .send(
            Method::POST,
            &format!("{BASE}/{system}/credentials"),
            Some(&token),
            Some(json!({ "credentialType": "API_KEY", "secretReference": "env://PATH" })),
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    assert_eq!(created.data()["secretReference"], "env://PATH");

    let listed = app
        .send(
            Method::GET,
            &format!("{BASE}/{system}/credentials"),
            Some(&token),
            None,
        )
        .await;

    for body in [&created.body, &listed.body] {
        assert!(
            !body.to_string().contains(&resolved),
            "a response carried the value the reference names"
        );
    }
}

#[tokio::test]
async fn a_raw_secret_is_refused_and_nothing_is_stored() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let system = system(&app, &token, "PAYMENTS").await;

    for raw in [
        concat!("sk_live", "_51HxQ2eKZ8r"),
        "dXNlcjpwYXNzd29yZA==",
        "svc:hunter2",
        "https://svc:hunter2@vault.example.com/x",
    ] {
        let response = app
            .send(
                Method::POST,
                &format!("{BASE}/{system}/credentials"),
                Some(&token),
                Some(json!({ "credentialType": "API_KEY", "secretReference": raw })),
            )
            .await;

        assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY, "{raw}");
        assert_eq!(
            response.body["error"]["details"][0]["code"], "NOT_A_SECRET_REFERENCE",
            "{raw}: {}",
            response.body
        );
        assert!(
            !response.body.to_string().contains(raw),
            "the refusal echoed the value back"
        );
    }

    // An edit is held to the same rule.
    let credential = credential_row(&app, fixtures::SYSTEM_TENANT_ID, system, "vault://ok").await;
    let edit = app
        .send(
            Method::PUT,
            &format!("{BASE}/{system}/credentials/{credential}"),
            Some(&token),
            Some(json!({ "secretReference": "hunter2" })),
        )
        .await;
    assert_eq!(edit.status, StatusCode::UNPROCESSABLE_ENTITY);

    let stored: Vec<String> = sqlx::query_scalar(
        "SELECT secret_reference FROM integration_credentials WHERE external_system_id = $1",
    )
    .bind(system)
    .fetch_all(&app.pool)
    .await
    .expect("read the credentials");

    assert_eq!(
        stored,
        vec!["vault://ok"],
        "only the valid reference is stored"
    );
}

#[tokio::test]
async fn a_base_url_with_a_password_in_it_is_refused() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let response = register(
        &app,
        &token,
        json!({
            "systemCode": "LEAKY",
            "systemName": "Leaky",
            "baseUrl": "https://svc:hunter2@erp.example.com/api"
        }),
    )
    .await;

    assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(response.body["error"]["details"][0]["path"], "baseUrl");
    assert_eq!(
        response.body["error"]["details"][0]["code"],
        "CREDENTIALS_IN_URL"
    );
}

/// A query string in a base URL is where an API key lands when a working call
/// is pasted in as the system's address — and the column is shown to every
/// `integration:external-system:read` holder and written to the audit trail
/// (#520, finding F1). Any query is refused, an empty `?` too, on
/// registration and on an edit, and nothing is stored or recorded.
#[tokio::test]
async fn a_base_url_with_a_query_string_is_refused_on_create_and_edit() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    // Stripe's published example key, split so GitHub's push protection does
    // not read this file as holding a live one. Every `sk_live` fixture here
    // and in `integration::domain` is split the same way.
    let key = concat!("sk_live", "_4eC39HqLyjWDarjtT1zdp7dc");

    let queried = [
        format!("https://erp.example.com/api?api_key={key}"),
        "https://erp.example.com/api?".to_owned(),
    ];

    for url in &queried {
        let response = register(
            &app,
            &token,
            json!({ "systemCode": "QUERIED", "systemName": "Queried", "baseUrl": url }),
        )
        .await;

        assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY, "{url}");
        assert_eq!(response.body["error"]["details"][0]["path"], "baseUrl");
        assert_eq!(
            response.body["error"]["details"][0]["code"], "QUERY_IN_BASE_URL",
            "{url}: {}",
            response.body
        );
        assert!(
            !response.body.to_string().contains(key),
            "the refusal echoed the key"
        );
    }

    let id = id_of(
        &register(
            &app,
            &token,
            json!({
                "systemCode": "PLAIN",
                "systemName": "Plain",
                "baseUrl": "https://erp.example.com:8443/api"
            }),
        )
        .await,
    );

    for url in &queried {
        let response = app
            .send(
                Method::PUT,
                &format!("{BASE}/{id}"),
                Some(&token),
                Some(json!({ "baseUrl": url })),
            )
            .await;

        assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY, "{url}");
        assert_eq!(
            response.body["error"]["details"][0]["code"], "QUERY_IN_BASE_URL",
            "{url}: {}",
            response.body
        );
    }

    let stored: Vec<Option<String>> =
        sqlx::query_scalar("SELECT base_url FROM external_systems ORDER BY system_code")
            .fetch_all(&app.pool)
            .await
            .expect("read the systems");
    assert_eq!(
        stored,
        vec![Some("https://erp.example.com:8443/api".to_owned())],
        "only the plain registration was stored, and the edits changed nothing"
    );

    let recorded: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE new_value_json::text LIKE '%' || $1 || '%'",
    )
    .bind(key)
    .fetch_one(&app.pool)
    .await
    .expect("search the trail");
    assert_eq!(recorded, 0, "the key reached the audit trail");
}

/// The credential routes read through an index (#520, `migration-author`'s
/// review). Every one of them filters `tenant_id` and `external_system_id` and
/// none filters `is_active`, so §12.3's `WHERE is_active` partial index served
/// none of them and each was a scan of every tenant's credentials.
///
/// **`enable_seqscan = off` is what makes the plan mean something** on a table
/// this small: a planner that *can* use an index then uses it, and one that
/// cannot falls back to the scan anyway. The statement is the list query's
/// predicate and order, as `repository::credential::list_credentials` runs it.
#[tokio::test]
async fn the_credential_routes_read_through_an_index() {
    let app = TestApp::spawn().await;
    let system = system_row(&app, fixtures::SYSTEM_TENANT_ID, "INDEXED").await;
    credential_row(&app, fixtures::SYSTEM_TENANT_ID, system, "vault://indexed").await;

    let mut connection = app.pool.acquire().await.expect("a connection");
    sqlx::query("SET enable_seqscan = off")
        .execute(&mut *connection)
        .await
        .expect("disable sequential scans for this session");

    let plan: Vec<String> = sqlx::query_scalar(
        "EXPLAIN SELECT id FROM integration_credentials
         WHERE tenant_id = $1 AND external_system_id = $2 AND deleted_at IS NULL
         ORDER BY created_at, id",
    )
    .bind(fixtures::SYSTEM_TENANT_ID)
    .bind(system)
    .fetch_all(&mut *connection)
    .await
    .expect("explain the list query");
    let plan = plan.join("\n");

    assert!(
        plan.contains("idx_integration_credentials_tenant_id_external_system_id"),
        "the credential list does not use its index:\n{plan}"
    );
    assert!(!plan.contains("Seq Scan"), "{plan}");
}

// ---------------------------------------------------------------------------
// AC-6 — register, edit, deactivate, and the endpoints
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_system_is_registered_edited_and_deactivated() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let registered = register(
        &app,
        &token,
        json!({
            "systemCode": "SAP_ERP",
            "systemName": "Corporate ERP",
            "systemType": "ERP",
            "baseUrl": "https://erp.example.com/api",
            "authType": "OAUTH2_CLIENT_CREDENTIALS",
            "retryPolicy": { "maxRetries": 3, "backoffMultiplier": 2.0 },
            "description": "  The ledger  "
        }),
    )
    .await;

    assert_eq!(
        registered.status,
        StatusCode::CREATED,
        "{}",
        registered.body
    );
    let data = registered.data();
    assert_eq!(data["systemCode"], "SAP_ERP");
    assert_eq!(data["status"], "ACTIVE", "registered active");
    assert_eq!(data["timeoutSeconds"], 30, "§12.1's default");
    assert_eq!(
        data["retryPolicy"],
        json!({ "maxRetries": 3, "backoffMultiplier": 2.0 })
    );
    assert_eq!(data["description"], "The ledger", "trimmed");
    let id = id_of(&registered);

    // Edit: rename, clear the base URL, into maintenance.
    let edited = app
        .send(
            Method::PUT,
            &format!("{BASE}/{id}"),
            Some(&token),
            Some(json!({
                "systemName": "Corporate ERP (EU)",
                "baseUrl": null,
                "timeoutSeconds": 60,
                "status": "MAINTENANCE"
            })),
        )
        .await;

    assert_eq!(edited.status, StatusCode::OK, "{}", edited.body);
    assert_eq!(edited.data()["systemName"], "Corporate ERP (EU)");
    assert_eq!(edited.data()["baseUrl"], Value::Null, "cleared by null");
    assert_eq!(edited.data()["timeoutSeconds"], 60);
    assert_eq!(edited.data()["status"], "MAINTENANCE");
    assert_eq!(
        edited.data()["authType"],
        "OAUTH2_CLIENT_CREDENTIALS",
        "left alone"
    );
    assert_eq!(edited.data()["systemType"], "ERP", "left alone");

    // Deactivate, twice.
    for attempt in 0..2 {
        let deactivated = app
            .send(
                Method::POST,
                &format!("{BASE}/{id}/deactivate"),
                Some(&token),
                None,
            )
            .await;

        assert_eq!(deactivated.status, StatusCode::OK, "attempt {attempt}");
        assert_eq!(
            deactivated.data()["status"],
            "INACTIVE",
            "attempt {attempt}"
        );
    }

    // It is still listed and still readable: deactivated, not deleted.
    let listed = app
        .send(
            Method::GET,
            &format!("{BASE}?status=INACTIVE"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(listed.body["meta"]["total"], 1);
    assert_eq!(listed.data()[0]["id"], id.to_string());

    // One record per change, and the repeat deactivation recorded nothing.
    let events: Vec<String> = sqlx::query_scalar(
        "SELECT event_type FROM audit_events WHERE object_id = $1 ORDER BY created_at, id",
    )
    .bind(id)
    .fetch_all(&app.pool)
    .await
    .expect("read the audit trail");

    assert_eq!(
        events,
        vec![
            "ExternalSystem.Registered",
            "ExternalSystem.Updated",
            "ExternalSystem.Deactivated"
        ]
    );
}

#[tokio::test]
async fn there_is_no_delete_route_for_a_system() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let id = system(&app, &token, "UNDELETABLE").await;

    let response = app
        .send(Method::DELETE, &format!("{BASE}/{id}"), Some(&token), None)
        .await;

    assert_eq!(response.status, StatusCode::METHOD_NOT_ALLOWED);
}

/// `INACTIVE` is `:deactivate`'s, so an edit under `:update` cannot reach it —
/// otherwise the separate permission would be a gate with a side door.
#[tokio::test]
async fn an_edit_cannot_do_what_deactivate_is_gated_for() {
    let app = TestApp::spawn().await;
    let system = system_row(&app, fixtures::SYSTEM_TENANT_ID, "SIDE_DOOR").await;
    let editor = caller_holding(&app, "EDITOR", &["integration:external-system:update"]).await;

    let response = app
        .send(
            Method::PUT,
            &format!("{BASE}/{system}"),
            Some(&editor),
            Some(json!({ "status": "INACTIVE" })),
        )
        .await;

    assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(response.body["error"]["details"][0]["code"], "NOT_ALLOWED");

    let status: String = sqlx::query_scalar("SELECT status FROM external_systems WHERE id = $1")
        .bind(system)
        .fetch_one(&app.pool)
        .await
        .expect("read the system");
    assert_eq!(status, "ACTIVE");
}

/// The other half of the side door: an updater cannot switch an inactive
/// system back on through a `PUT` — neither to `ACTIVE` nor to `MAINTENANCE`,
/// which would put it back in play just the same (#520, 2026-09-25). The rest
/// of the edit is still theirs, so an inactive system can be corrected.
#[tokio::test]
async fn an_updater_cannot_reactivate_through_an_edit() {
    let app = TestApp::spawn().await;
    let system = system_row(&app, fixtures::SYSTEM_TENANT_ID, "SWITCHED_OFF").await;
    sqlx::query("UPDATE external_systems SET status = 'INACTIVE' WHERE id = $1")
        .bind(system)
        .execute(&app.pool)
        .await
        .expect("switch the system off");

    let editor = caller_holding(&app, "REVIVER", &["integration:external-system:update"]).await;

    for status in ["ACTIVE", "MAINTENANCE"] {
        let response = app
            .send(
                Method::PUT,
                &format!("{BASE}/{system}"),
                Some(&editor),
                Some(json!({ "systemName": "Revived", "status": status })),
            )
            .await;

        assert_eq!(
            response.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{status}: {}",
            response.body
        );
        assert_eq!(response.body["error"]["details"][0]["path"], "status");
        assert_eq!(response.body["error"]["details"][0]["code"], "NOT_ALLOWED");
        assert!(
            response.body["error"]["details"][0]["message"]
                .as_str()
                .is_some_and(|message| message.contains("/activate")),
            "the refusal names the route that does it: {}",
            response.body
        );
    }

    let (name, status): (String, String) =
        sqlx::query_as("SELECT system_name, status FROM external_systems WHERE id = $1")
            .bind(system)
            .fetch_one(&app.pool)
            .await
            .expect("read the system");
    assert_eq!(
        (name.as_str(), status.as_str()),
        ("SWITCHED_OFF", "INACTIVE"),
        "a refused edit wrote nothing"
    );

    // An edit that leaves status alone is still an edit.
    let renamed = app
        .send(
            Method::PUT,
            &format!("{BASE}/{system}"),
            Some(&editor),
            Some(json!({ "systemName": "Corrected" })),
        )
        .await;
    assert_eq!(renamed.status, StatusCode::OK, "{}", renamed.body);
    assert_eq!(renamed.data()["status"], "INACTIVE");
}

/// Maintenance is not activation: `ACTIVE` ↔ `MAINTENANCE` on a system that is
/// in service stays an edit under `:update`.
#[tokio::test]
async fn maintenance_is_an_edit_not_an_activation() {
    let app = TestApp::spawn().await;
    let system = system_row(&app, fixtures::SYSTEM_TENANT_ID, "SERVICED").await;
    let editor = caller_holding(&app, "SERVICER", &["integration:external-system:update"]).await;

    for status in ["MAINTENANCE", "ACTIVE"] {
        let response = app
            .send(
                Method::PUT,
                &format!("{BASE}/{system}"),
                Some(&editor),
                Some(json!({ "status": status })),
            )
            .await;

        assert_eq!(
            response.status,
            StatusCode::OK,
            "{status}: {}",
            response.body
        );
        assert_eq!(response.data()["status"], status);
    }
}

/// Off, on, on again: activation undoes deactivation, is idempotent, and is
/// recorded once.
#[tokio::test]
async fn a_system_is_deactivated_and_activated_again() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let id = system(&app, &token, "ROUND_TRIP").await;

    let off = app
        .send(
            Method::POST,
            &format!("{BASE}/{id}/deactivate"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(off.data()["status"], "INACTIVE");

    for attempt in 0..2 {
        let on = app
            .send(
                Method::POST,
                &format!("{BASE}/{id}/activate"),
                Some(&token),
                None,
            )
            .await;

        assert_eq!(on.status, StatusCode::OK, "attempt {attempt}: {}", on.body);
        assert_eq!(on.data()["status"], "ACTIVE", "attempt {attempt}");
    }

    // From maintenance, too: activate means in service, whatever it was.
    let maintained = app
        .send(
            Method::PUT,
            &format!("{BASE}/{id}"),
            Some(&token),
            Some(json!({ "status": "MAINTENANCE" })),
        )
        .await;
    assert_eq!(maintained.data()["status"], "MAINTENANCE");
    let on = app
        .send(
            Method::POST,
            &format!("{BASE}/{id}/activate"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(on.data()["status"], "ACTIVE");

    let events: Vec<String> = sqlx::query_scalar(
        "SELECT event_type FROM audit_events WHERE object_id = $1 ORDER BY created_at, id",
    )
    .bind(id)
    .fetch_all(&app.pool)
    .await
    .expect("read the audit trail");

    assert_eq!(
        events,
        vec![
            "ExternalSystem.Registered",
            "ExternalSystem.Deactivated",
            "ExternalSystem.Activated",
            "ExternalSystem.Updated",
            "ExternalSystem.Activated"
        ],
        "the repeat activation recorded nothing"
    );

    let missing = app
        .send(
            Method::POST,
            &format!("{BASE}/{}/activate", Uuid::now_v7()),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_system_code_cannot_be_edited() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let id = system(&app, &token, "FIXED_CODE").await;

    let response = app
        .send(
            Method::PUT,
            &format!("{BASE}/{id}"),
            Some(&token),
            Some(json!({ "systemCode": "NEW_CODE" })),
        )
        .await;

    assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(response.body["error"]["details"][0]["path"], "systemCode");
}

#[tokio::test]
async fn an_endpoint_is_added_edited_and_retired() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let system = system(&app, &token, "PROCUREMENT").await;

    let created = app
        .send(
            Method::POST,
            &format!("{BASE}/{system}/endpoints"),
            Some(&token),
            Some(json!({
                "endpointCode": "CREATE_PURCHASE_ORDER",
                "name": "Create purchase order",
                "method": "POST",
                "path": "/purchase-orders"
            })),
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    assert_eq!(created.data()["status"], "ACTIVE");
    assert_eq!(created.data()["externalSystemId"], system.to_string());
    let endpoint = id_of(&created);

    let retired = app
        .send(
            Method::PUT,
            &format!("{BASE}/{system}/endpoints/{endpoint}"),
            Some(&token),
            Some(json!({ "method": "PUT", "path": "/purchase-orders/{id}", "status": "INACTIVE" })),
        )
        .await;

    assert_eq!(retired.status, StatusCode::OK, "{}", retired.body);
    assert_eq!(retired.data()["status"], "INACTIVE");
    assert_eq!(retired.data()["method"], "PUT");
    assert_eq!(
        retired.data()["name"],
        "Create purchase order",
        "left alone"
    );

    // Retired endpoints stay on the list, which is where they are brought back.
    let listed = app
        .send(
            Method::GET,
            &format!("{BASE}/{system}/endpoints"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(listed.body["meta"]["total"], 1);
    assert_eq!(listed.data()[0]["status"], "INACTIVE");

    // No delete.
    let delete = app
        .send(
            Method::DELETE,
            &format!("{BASE}/{system}/endpoints/{endpoint}"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(delete.status, StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn an_endpoint_code_is_unique_within_its_system_and_only_there() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let first = system(&app, &token, "FIRST").await;
    let second = system(&app, &token, "SECOND").await;

    let body = json!({ "endpointCode": "PING", "name": "Ping", "method": "GET", "path": "/ping" });
    let post = |system: Uuid| {
        let body = body.clone();
        let token = token.clone();
        let app = &app;
        async move {
            app.send(
                Method::POST,
                &format!("{BASE}/{system}/endpoints"),
                Some(&token),
                Some(body),
            )
            .await
        }
    };

    assert_eq!(post(first).await.status, StatusCode::CREATED);
    assert_eq!(
        post(second).await.status,
        StatusCode::CREATED,
        "another system"
    );
    assert_eq!(
        post(first).await.status,
        StatusCode::CONFLICT,
        "the same system again"
    );
}

#[tokio::test]
async fn a_credential_is_edited_and_deleted() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let system = system(&app, &token, "BANK").await;
    let path = format!("{BASE}/{system}/credentials");

    let created = app
        .send(
            Method::POST,
            &path,
            Some(&token),
            Some(json!({
                "credentialType": "CERTIFICATE",
                "secretReference": "vault://kelir/bank/cert",
                "validFrom": "2026-01-01"
            })),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let id = id_of(&created);

    // A validTo before the stored validFrom is refused, although the request
    // alone names only one bound.
    let inverted = app
        .send(
            Method::PUT,
            &format!("{path}/{id}"),
            Some(&token),
            Some(json!({ "validTo": "2025-12-31" })),
        )
        .await;
    assert_eq!(inverted.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        inverted.body["error"]["details"][0]["code"],
        "WINDOW_INVERTED"
    );

    let edited = app
        .send(
            Method::PUT,
            &format!("{path}/{id}"),
            Some(&token),
            Some(json!({
                "secretReference": "env://KELIR_BANK_CERT",
                "validFrom": null,
                "isActive": false
            })),
        )
        .await;
    assert_eq!(edited.status, StatusCode::OK, "{}", edited.body);
    assert_eq!(edited.data()["secretReference"], "env://KELIR_BANK_CERT");
    assert_eq!(edited.data()["validFrom"], Value::Null);
    assert_eq!(edited.data()["isActive"], false);
    assert_eq!(edited.data()["credentialType"], "CERTIFICATE", "left alone");

    let deleted = app
        .send(Method::DELETE, &format!("{path}/{id}"), Some(&token), None)
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT);

    let again = app
        .send(Method::GET, &format!("{path}/{id}"), Some(&token), None)
        .await;
    assert_eq!(again.status, StatusCode::NOT_FOUND);

    let listed = app.send(Method::GET, &path, Some(&token), None).await;
    assert_eq!(listed.body["meta"]["total"], 0);
}

// ---------------------------------------------------------------------------
// Tenant isolation
// ---------------------------------------------------------------------------

/// Another tenant's system, with its own endpoint and credential, is invisible
/// to every route — while a system of the caller's own tenant **under the same
/// code** is visible, so the refusals are the tenant and not the code.
#[tokio::test]
async fn another_tenants_system_is_not_found_by_any_route() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let other = fixtures::create_tenant(&app.pool, "TNT-INT-B", "Tenant B").await;
    let theirs = system_row(&app, other, "SHARED_CODE").await;
    let their_endpoint = endpoint_row(&app, other, theirs, "EP").await;
    let their_credential = credential_row(&app, other, theirs, "vault://b/secret").await;

    let ours = system(&app, &token, "SHARED_CODE").await;

    let listed = app
        .send(
            Method::GET,
            &format!("{BASE}?search=SHARED"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(listed.body["meta"]["total"], 1, "{}", listed.body);
    assert_eq!(listed.data()[0]["id"], ours.to_string());

    let probes = [
        (Method::GET, format!("{BASE}/{theirs}"), None),
        (
            Method::PUT,
            format!("{BASE}/{theirs}"),
            Some(json!({ "systemName": "Taken" })),
        ),
        (Method::POST, format!("{BASE}/{theirs}/deactivate"), None),
        (Method::POST, format!("{BASE}/{theirs}/activate"), None),
        (Method::GET, format!("{BASE}/{theirs}/endpoints"), None),
        (
            Method::GET,
            format!("{BASE}/{theirs}/endpoints/{their_endpoint}"),
            None,
        ),
        (
            Method::POST,
            format!("{BASE}/{theirs}/endpoints"),
            Some(json!({ "endpointCode": "IN", "name": "In", "method": "GET", "path": "/in" })),
        ),
        (
            Method::PUT,
            format!("{BASE}/{theirs}/endpoints/{their_endpoint}"),
            Some(json!({ "name": "Taken" })),
        ),
        (Method::GET, format!("{BASE}/{theirs}/credentials"), None),
        (
            Method::GET,
            format!("{BASE}/{theirs}/credentials/{their_credential}"),
            None,
        ),
        (
            Method::POST,
            format!("{BASE}/{theirs}/credentials"),
            Some(json!({ "credentialType": "API_KEY", "secretReference": "vault://in" })),
        ),
        (
            Method::PUT,
            format!("{BASE}/{theirs}/credentials/{their_credential}"),
            Some(json!({ "isActive": false })),
        ),
        (
            Method::DELETE,
            format!("{BASE}/{theirs}/credentials/{their_credential}"),
            None,
        ),
    ];

    for (method, path, body) in probes {
        let response = app.send(method.clone(), &path, Some(&token), body).await;

        assert_eq!(
            response.status,
            StatusCode::NOT_FOUND,
            "{method} {path}: {}",
            response.body
        );
        assert!(!response.body.to_string().contains("vault://b/secret"));
    }

    // Nothing of theirs moved.
    let (name, status): (String, String) =
        sqlx::query_as("SELECT system_name, status FROM external_systems WHERE id = $1")
            .bind(theirs)
            .fetch_one(&app.pool)
            .await
            .expect("read their system");
    assert_eq!((name.as_str(), status.as_str()), ("SHARED_CODE", "ACTIVE"));

    let children: i64 = sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM integration_endpoints WHERE external_system_id = $1)
              + (SELECT count(*) FROM integration_credentials
                 WHERE external_system_id = $1 AND deleted_at IS NULL AND is_active)",
    )
    .bind(theirs)
    .fetch_one(&app.pool)
    .await
    .expect("count their children");
    assert_eq!(
        children, 2,
        "one endpoint and one live credential, untouched"
    );
}

/// A child id is found only under the system it belongs to: system A's path
/// does not answer for system B's credential or endpoint, in the same tenant.
#[tokio::test]
async fn a_child_is_found_only_under_its_own_system() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let a = system(&app, &token, "SYS_A").await;
    let b = system(&app, &token, "SYS_B").await;

    let b_endpoint = endpoint_row(&app, fixtures::SYSTEM_TENANT_ID, b, "EP_B").await;
    let b_credential = credential_row(&app, fixtures::SYSTEM_TENANT_ID, b, "vault://b").await;

    for path in [
        format!("{BASE}/{a}/endpoints/{b_endpoint}"),
        format!("{BASE}/{a}/credentials/{b_credential}"),
    ] {
        let response = app.send(Method::GET, &path, Some(&token), None).await;
        assert_eq!(response.status, StatusCode::NOT_FOUND, "{path}");
    }

    let deleted = app
        .send(
            Method::DELETE,
            &format!("{BASE}/{a}/credentials/{b_credential}"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(deleted.status, StatusCode::NOT_FOUND);

    // Under its own system it is there.
    let own = app
        .send(
            Method::GET,
            &format!("{BASE}/{b}/credentials/{b_credential}"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(own.status, StatusCode::OK);
}

// ---------------------------------------------------------------------------
// The list, and validation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_list_searches_filters_and_pages() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    system(&app, &token, "ERP_A").await;
    system(&app, &token, "ERPXA").await;
    let crm = register(
        &app,
        &token,
        json!({ "systemCode": "CRM_MAIN", "systemName": "Sales CRM", "systemType": "CRM" }),
    )
    .await;
    assert_eq!(crm.status, StatusCode::CREATED);

    // `_` is a literal, not a wildcard: "P_A" matches ERP_A and not ERPXA.
    let searched = app
        .send(
            Method::GET,
            &format!("{BASE}?search=P_A"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(searched.body["meta"]["total"], 1, "{}", searched.body);
    assert_eq!(searched.data()[0]["systemCode"], "ERP_A");

    // The name is searched as well as the code.
    let by_name = app
        .send(
            Method::GET,
            &format!("{BASE}?search=sales"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(by_name.body["meta"]["total"], 1);

    let by_type = app
        .send(
            Method::GET,
            &format!("{BASE}?systemType=CRM"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(by_type.body["meta"]["total"], 1);
    assert_eq!(by_type.data()[0]["systemCode"], "CRM_MAIN");

    // Pages: three rows, two per page, and every row on exactly one page. The
    // order is by code under the database's collation, which is why this
    // asserts the partition and not a sequence that puts `_` beside `X`.
    let first = app
        .send(
            Method::GET,
            &format!("{BASE}?pageSize=2"),
            Some(&token),
            None,
        )
        .await;
    let second = app
        .send(
            Method::GET,
            &format!("{BASE}?pageSize=2&page=2"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(first.body["meta"]["total"], 3);
    assert_eq!(first.data().as_array().map(Vec::len), Some(2));
    assert_eq!(second.data().as_array().map(Vec::len), Some(1));
    let codes: BTreeSet<&str> = first
        .data()
        .as_array()
        .into_iter()
        .flatten()
        .chain(second.data().as_array().into_iter().flatten())
        .filter_map(|row| row["systemCode"].as_str())
        .collect();
    assert_eq!(codes, ["CRM_MAIN", "ERPXA", "ERP_A"].into_iter().collect());

    let bad = app
        .send(
            Method::GET,
            &format!("{BASE}?status=GONE"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(bad.body["error"]["details"][0]["path"], "status");
}

#[tokio::test]
async fn a_system_code_is_unique_per_tenant() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    system(&app, &token, "ONCE").await;

    let again = register(
        &app,
        &token,
        json!({ "systemCode": "ONCE", "systemName": "Twice" }),
    )
    .await;

    assert_eq!(again.status, StatusCode::CONFLICT);
    assert_eq!(again.error_code(), Some("CONFLICT"));
}

#[tokio::test]
async fn a_registration_is_refused_field_by_field() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let response = register(
        &app,
        &token,
        json!({
            "systemCode": "HAS SPACE",
            "systemName": "",
            "baseUrl": "ftp://files.example.com",
            "timeoutSeconds": 0
        }),
    )
    .await;

    assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);
    let paths: BTreeSet<&str> = response.body["error"]["details"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|detail| detail["path"].as_str())
        .collect();
    assert_eq!(
        paths,
        ["baseUrl", "systemCode", "systemName", "timeoutSeconds"]
            .into_iter()
            .collect()
    );

    // An enum outside §12's vocabulary never deserializes.
    let unknown = register(
        &app,
        &token,
        json!({ "systemCode": "X", "systemName": "X", "authType": "PASSWORD" }),
    )
    .await;
    assert_eq!(unknown.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(unknown.body["error"]["details"][0]["path"], "authType");

    let stored: i64 = sqlx::query_scalar("SELECT count(*) FROM external_systems")
        .fetch_one(&app.pool)
        .await
        .expect("count");
    assert_eq!(stored, 0, "nothing refused was stored");
}

// ---------------------------------------------------------------------------
// AC-7 — OpenAPI
// ---------------------------------------------------------------------------

#[tokio::test]
async fn every_route_is_in_the_document_and_no_schema_can_carry_a_secret() {
    let app = TestApp::spawn().await;
    let document = app
        .send(Method::GET, "/api/docs/openapi.json", None, None)
        .await;
    let paths = &document.body["paths"];

    for (path, method) in [
        (BASE.to_owned(), "get"),
        (BASE.to_owned(), "post"),
        (format!("{BASE}/{{id}}"), "get"),
        (format!("{BASE}/{{id}}"), "put"),
        (format!("{BASE}/{{id}}/deactivate"), "post"),
        (format!("{BASE}/{{id}}/activate"), "post"),
        (format!("{BASE}/{{id}}/endpoints"), "get"),
        (format!("{BASE}/{{id}}/endpoints"), "post"),
        (format!("{BASE}/{{id}}/endpoints/{{endpointId}}"), "get"),
        (format!("{BASE}/{{id}}/endpoints/{{endpointId}}"), "put"),
        (format!("{BASE}/{{id}}/credentials"), "get"),
        (format!("{BASE}/{{id}}/credentials"), "post"),
        (format!("{BASE}/{{id}}/credentials/{{credentialId}}"), "get"),
        (format!("{BASE}/{{id}}/credentials/{{credentialId}}"), "put"),
        (
            format!("{BASE}/{{id}}/credentials/{{credentialId}}"),
            "delete",
        ),
    ] {
        assert!(
            paths[&path][method].is_object(),
            "{method} {path} is missing from the published document"
        );
    }

    assert!(
        paths[format!("{BASE}/{{id}}")]["delete"].is_null(),
        "a system has no delete (#520, answer 2)"
    );

    // Fifteen operations, and no more: the list above is the whole surface.
    let operations: usize = paths
        .as_object()
        .expect("paths")
        .iter()
        .filter(|(path, _)| path.starts_with(BASE))
        .map(|(_, item)| {
            ["get", "put", "post", "delete", "patch"]
                .iter()
                .filter(|method| item[**method].is_object())
                .count()
        })
        .sum();
    assert_eq!(
        operations, 15,
        "the integration surface has {operations} operations"
    );

    // Every property of every integration schema whose name mentions a secret
    // is the reference, and nothing else.
    let schemas = document.body["components"]["schemas"]
        .as_object()
        .expect("schemas");
    let mut seen = 0;

    for name in [
        "ExternalSystem",
        "RegisterExternalSystemRequest",
        "UpdateExternalSystemRequest",
        "IntegrationEndpoint",
        "CreateIntegrationEndpointRequest",
        "UpdateIntegrationEndpointRequest",
        "IntegrationCredential",
        "CreateIntegrationCredentialRequest",
        "UpdateIntegrationCredentialRequest",
    ] {
        let properties = keys(&schemas[name]["properties"]);
        assert!(!properties.is_empty(), "{name} is not in the document");

        for property in properties {
            let lower = property.to_lowercase();
            if ["secret", "password", "token", "key"]
                .iter()
                .any(|word| lower.contains(word))
            {
                assert_eq!(property, "secretReference", "{name}.{property}");
                seen += 1;
            }
        }
    }

    assert_eq!(
        seen, 3,
        "secretReference appears on the credential and its two requests, and nowhere else"
    );

    // #552 (record 19 finding 2, D-88): a reference is checked for its shape,
    // and `vault://sk_live_…` has the shape. So the published document says
    // *reference* and never that a value cannot be a secret. Seen red,
    // 2026-09-25, with the tag's `No route carries a secret value` put back.
    let credential_texts = [
        document.body["tags"]
            .as_array()
            .expect("tags")
            .iter()
            .find(|tag| tag["name"] == "integration")
            .expect("the integration tag")["description"]
            .to_string(),
        paths[format!("{BASE}/{{id}}/credentials")]["get"]["responses"]["200"]["description"]
            .to_string(),
        paths[format!("{BASE}/{{id}}/credentials/{{credentialId}}")]["get"]["responses"]["200"]
            ["description"]
            .to_string(),
        schemas["IntegrationCredential"]["properties"]["secretReference"]["description"]
            .to_string(),
    ];
    for text in &credential_texts {
        assert_ne!(
            text, "null",
            "a description is missing: {credential_texts:?}"
        );
        let lower = text.to_lowercase();
        for claim in ["never a secret", "never the secret", "carries a secret"] {
            assert!(
                !lower.contains(claim),
                "the document claims `{claim}`: {text}"
            );
        }
    }
}

/// **#552 in the database**: `0047_credential_reference_texts.sql` rewrites the
/// two texts `0046` seeded saying *never the secret*, the column comment and
/// the `integration:credential:read` description. Seen red, 2026-09-25, with
/// `0047` moved out of the migrations directory.
#[tokio::test]
async fn the_database_texts_say_reference_and_not_never_the_secret() {
    let app = TestApp::spawn().await;

    let comment: Option<String> = sqlx::query_scalar(
        "SELECT col_description('integration_credentials'::regclass, attnum)
         FROM pg_attribute
         WHERE attrelid = 'integration_credentials'::regclass
           AND attname = 'secret_reference'",
    )
    .fetch_one(&app.pool)
    .await
    .expect("the column comment");

    let descriptions: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT permission_code, description FROM permissions
         WHERE permission_code LIKE 'integration:%' AND deleted_at IS NULL",
    )
    .fetch_all(&app.pool)
    .await
    .expect("the catalogue rows");
    assert_eq!(descriptions.len(), 8, "{descriptions:?}");

    let comment = comment.expect("secret_reference has a comment");
    assert!(comment.contains("shape"), "{comment}");

    for text in std::iter::once(&comment).chain(descriptions.iter().filter_map(|(_, d)| d.as_ref()))
    {
        let lower = text.to_lowercase();
        for claim in ["never the secret", "never a secret"] {
            assert!(
                !lower.contains(claim),
                "the database says `{claim}`: {text}"
            );
        }
    }
}

// ===========================================================================
// Independent verification, adopted
// ===========================================================================
//
// The five tests below were written by `test-engineer` as the row's closing
// gate (`integration_external_systems_verification.rs`) and moved here, so the
// registry has one test file and one seen-red table. Each covers a criterion
// the tests above left unproven:
//
// * **tenant isolation from the other side** — a caller *in* tenant B holding
//   every integration permission there, sent to every route with tenant A's ids;
// * **AC-2** — the deferred source-reference key refuses a *cross-tenant* row,
//   not only an orphan, and all eight keys to a system or subscription carry
//   the tenant;
// * **AC-5** — the raw-secret shapes the tests above do not send (a JWT, a PEM
//   block, a longer base64 blob), on create and on edit, and none reaches the
//   audit trail;
// * **AC-5 through the audit trail** — a credential's recorded values are
//   withheld from a caller who can read the system but not its credentials;
// * **baseUrl on an edit** — `CREDENTIALS_IN_URL`, which only registration was
//   tested for.

/// Tenant A's system, endpoint and credential, all written through the API.
async fn tenant_a_registry(app: &TestApp, token: &str) -> (Uuid, Uuid, Uuid) {
    let system = app
        .send(
            Method::POST,
            BASE,
            Some(token),
            Some(json!({ "systemCode": "A_ERP", "systemName": "Tenant A ERP" })),
        )
        .await;
    assert_eq!(system.status, StatusCode::CREATED, "{}", system.body);
    let system = id_of(&system);

    let endpoint = app
        .send(
            Method::POST,
            &format!("{BASE}/{system}/endpoints"),
            Some(token),
            Some(json!({ "endpointCode": "A_EP", "name": "A", "method": "GET", "path": "/a" })),
        )
        .await;
    assert_eq!(endpoint.status, StatusCode::CREATED, "{}", endpoint.body);

    let credential = app
        .send(
            Method::POST,
            &format!("{BASE}/{system}/credentials"),
            Some(token),
            Some(json!({ "credentialType": "API_KEY", "secretReference": "vault://tenant-a/erp" })),
        )
        .await;
    assert_eq!(
        credential.status,
        StatusCode::CREATED,
        "{}",
        credential.body
    );

    (system, id_of(&endpoint), id_of(&credential))
}

// ---------------------------------------------------------------------------
// Tenant isolation — a real tenant-B caller, every route
// ---------------------------------------------------------------------------

/// Tenant B's caller holds all eight integration permissions **in tenant B**,
/// so a 404 here is the tenant and not a missing permission. Sent to every
/// route — `activate` included — with tenant A's ids, as a caller guessing
/// them would.
#[tokio::test]
async fn a_caller_in_another_tenant_reaches_nothing_of_this_tenants_registry() {
    let app = TestApp::spawn_with(|config| config.multi_tenant = true).await;
    let admin = app
        .sign_in_to("SYSTEM", common::ADMIN_USERNAME, common::ADMIN_PASSWORD)
        .await;
    let (system, endpoint, credential) = tenant_a_registry(&app, &admin).await;

    // Deactivated, so that a leaking `activate` would visibly change it.
    let off = app
        .send(
            Method::POST,
            &format!("{BASE}/{system}/deactivate"),
            Some(&admin),
            None,
        )
        .await;
    assert_eq!(off.status, StatusCode::OK, "{}", off.body);

    let tenant_b = fixtures::create_tenant(&app.pool, "TNT-INT-VER", "Tenant B").await;
    let role =
        fixtures::create_role_with_permissions(&app.pool, tenant_b, "INT-ALL", &PERMISSIONS).await;
    fixtures::create_user(
        &app.pool,
        tenant_b,
        "int.outsider",
        "int.outsider@example.test",
        common::ADMIN_PASSWORD,
        &[role],
    )
    .await;
    let outsider = app
        .sign_in_to("TNT-INT-VER", "int.outsider", common::ADMIN_PASSWORD)
        .await;

    // The outsider's permissions are real: their own registry works.
    let own = app
        .send(
            Method::POST,
            BASE,
            Some(&outsider),
            Some(json!({ "systemCode": "A_ERP", "systemName": "Tenant B's own" })),
        )
        .await;
    assert_eq!(own.status, StatusCode::CREATED, "{}", own.body);
    let own = id_of(&own);

    let listed = app.send(Method::GET, BASE, Some(&outsider), None).await;
    assert_eq!(listed.status, StatusCode::OK);
    assert_eq!(listed.body["meta"]["total"], 1, "{}", listed.body);
    assert_eq!(listed.body["data"][0]["id"], own.to_string());

    let system_path = format!("{BASE}/{system}");
    let probes: Vec<(Method, String, Option<Value>)> = vec![
        (Method::GET, system_path.clone(), None),
        (
            Method::PUT,
            system_path.clone(),
            Some(json!({ "systemName": "Taken" })),
        ),
        (Method::POST, format!("{system_path}/activate"), None),
        (Method::POST, format!("{system_path}/deactivate"), None),
        (Method::GET, format!("{system_path}/endpoints"), None),
        (
            Method::GET,
            format!("{system_path}/endpoints/{endpoint}"),
            None,
        ),
        (
            Method::POST,
            format!("{system_path}/endpoints"),
            Some(json!({ "endpointCode": "IN", "name": "In", "method": "GET", "path": "/in" })),
        ),
        (
            Method::PUT,
            format!("{system_path}/endpoints/{endpoint}"),
            Some(json!({ "name": "Taken", "status": "INACTIVE" })),
        ),
        (Method::GET, format!("{system_path}/credentials"), None),
        (
            Method::GET,
            format!("{system_path}/credentials/{credential}"),
            None,
        ),
        (
            Method::POST,
            format!("{system_path}/credentials"),
            Some(json!({ "credentialType": "API_KEY", "secretReference": "vault://in" })),
        ),
        (
            Method::PUT,
            format!("{system_path}/credentials/{credential}"),
            Some(json!({ "secretReference": "vault://moved", "isActive": false })),
        ),
        (
            Method::DELETE,
            format!("{system_path}/credentials/{credential}"),
            None,
        ),
        // Tenant B's own system in the path, tenant A's child ids after it.
        (
            Method::GET,
            format!("{BASE}/{own}/endpoints/{endpoint}"),
            None,
        ),
        (
            Method::GET,
            format!("{BASE}/{own}/credentials/{credential}"),
            None,
        ),
        (
            Method::DELETE,
            format!("{BASE}/{own}/credentials/{credential}"),
            None,
        ),
    ];

    for (method, path, body) in probes {
        let response = app.send(method.clone(), &path, Some(&outsider), body).await;

        assert_eq!(
            response.status,
            StatusCode::NOT_FOUND,
            "{method} {path} answered tenant B: {}",
            response.body
        );
        let text = response.body.to_string();
        assert!(
            !text.contains("vault://tenant-a/erp"),
            "{method} {path}: {text}"
        );
        assert!(!text.contains("Tenant A ERP"), "{method} {path}: {text}");
    }

    // Nothing of tenant A's moved, and nothing was filed under it.
    let (name, status): (String, String) =
        sqlx::query_as("SELECT system_name, status FROM external_systems WHERE id = $1")
            .bind(system)
            .fetch_one(&app.pool)
            .await
            .expect("read tenant A's system");
    assert_eq!(
        (name.as_str(), status.as_str()),
        ("Tenant A ERP", "INACTIVE")
    );

    let (endpoints, endpoint_name): (i64, String) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM integration_endpoints WHERE external_system_id = $1),
                (SELECT name FROM integration_endpoints WHERE id = $2)",
    )
    .bind(system)
    .bind(endpoint)
    .fetch_one(&app.pool)
    .await
    .expect("read tenant A's endpoints");
    assert_eq!((endpoints, endpoint_name.as_str()), (1, "A"));

    let (credentials, reference, active, deleted): (i64, String, bool, bool) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM integration_credentials WHERE external_system_id = $1),
                secret_reference, is_active, deleted_at IS NOT NULL
         FROM integration_credentials WHERE id = $2",
    )
    .bind(system)
    .bind(credential)
    .fetch_one(&app.pool)
    .await
    .expect("read tenant A's credential");
    assert_eq!(
        (credentials, reference.as_str(), active, deleted),
        (1, "vault://tenant-a/erp", true, false)
    );

    // And tenant B's attempts left no record against tenant A's objects.
    let foreign_audit: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE object_id = ANY($1) AND tenant_id = $2",
    )
    .bind(vec![system, endpoint, credential])
    .bind(tenant_b)
    .fetch_one(&app.pool)
    .await
    .expect("count audit rows");
    assert_eq!(foreign_audit, 0);
}

// ---------------------------------------------------------------------------
// AC-2 — tenant-carrying keys, at the database
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_source_reference_cannot_name_another_tenants_system() {
    let app = TestApp::spawn().await;
    let other = fixtures::create_tenant(&app.pool, "TNT-INT-MDR", "Other tenant").await;

    let theirs = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO external_systems (id, tenant_id, system_code, system_name)
         VALUES ($1, $2, 'THEIRS', 'Theirs')",
    )
    .bind(theirs)
    .bind(other)
    .execute(&app.pool)
    .await
    .expect("insert their system");

    let insert = |tenant: Uuid| {
        let pool = app.pool.clone();
        async move {
            sqlx::query(
                "INSERT INTO master_data_source_references
                     (id, tenant_id, entity_type, kelir_entity_id, external_system_id,
                      external_entity_id)
                 VALUES ($1, $2, 'PARTY', $3, $4, 'EXT-1')",
            )
            .bind(Uuid::now_v7())
            .bind(tenant)
            .bind(Uuid::now_v7())
            .bind(theirs)
            .execute(&pool)
            .await
        }
    };

    let crossed = insert(fixtures::SYSTEM_TENANT_ID).await;
    let error = crossed.expect_err("a source reference named another tenant's system");
    assert!(
        error
            .to_string()
            .contains("fk_master_data_source_references_external_system_id"),
        "refused, but not by the tenant-carrying key: {error}"
    );

    insert(other)
        .await
        .expect("the same row in the system's own tenant");

    // The five DDL-only tables carry the tenant in every key to a system or a
    // subscription, too.
    let definitions: Vec<(String, String)> = sqlx::query_as(
        "SELECT conname::text, pg_get_constraintdef(oid) FROM pg_constraint
         WHERE contype = 'f'
           AND conrelid::regclass::text IN ('integration_mappings', 'integration_logs',
               'webhook_subscriptions', 'webhook_events', 'inbox_events',
               'integration_endpoints', 'integration_credentials',
               'master_data_source_references')
           AND confrelid::regclass::text IN ('external_systems', 'webhook_subscriptions')
         ORDER BY conname",
    )
    .fetch_all(&app.pool)
    .await
    .expect("read the keys");

    assert_eq!(definitions.len(), 8, "{definitions:?}");
    for (name, definition) in &definitions {
        assert!(
            definition.contains(", tenant_id)") && definition.contains("(id, tenant_id)"),
            "{name} does not carry the tenant: {definition}"
        );
    }
}

// ---------------------------------------------------------------------------
// AC-5 — raw secrets, in the shapes they arrive in
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_raw_secret_in_any_usual_shape_is_refused_on_create_and_edit() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let created = app
        .send(
            Method::POST,
            BASE,
            Some(&token),
            Some(json!({ "systemCode": "SHAPES", "systemName": "Shapes" })),
        )
        .await;
    let system = id_of(&created);
    let path = format!("{BASE}/{system}/credentials");

    let good = app
        .send(
            Method::POST,
            &path,
            Some(&token),
            Some(json!({ "credentialType": "JWT", "secretReference": "vault://kelir/shapes" })),
        )
        .await;
    assert_eq!(good.status, StatusCode::CREATED, "{}", good.body);
    let credential = id_of(&good);

    let raw = [
        // A JWT.
        "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U",
        // A live API key.
        concat!("sk_live", "_4eC39HqLyjWDarjtT1zdp7dc"),
        // A user:password pair.
        "svc-erp:Tr0ub4dor&3",
        // A base64 blob.
        "c2VydmljZS1hY2NvdW50OnN1cGVyLXNlY3JldC1wYXNzd29yZC0xMjM0NTY3ODkw",
        // A PEM block, header and body.
        "-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEAu1SU1LfVLPHCozMxH2Mo4lgOEePzNm0tRgeLezV6ffAt0gun\n-----END RSA PRIVATE KEY-----",
        // A secret in the right scheme's clothing.
        "vault://kelir/erp/api key with spaces",
        concat!("env://sk_live", "_lowercase"),
    ];

    for value in raw {
        for (method, target) in [
            (Method::POST, path.clone()),
            (Method::PUT, format!("{path}/{credential}")),
        ] {
            let body = if method == Method::POST {
                json!({ "credentialType": "API_KEY", "secretReference": value })
            } else {
                json!({ "secretReference": value })
            };
            let response = app
                .send(method.clone(), &target, Some(&token), Some(body))
                .await;

            assert_eq!(
                response.status,
                StatusCode::UNPROCESSABLE_ENTITY,
                "{method} {value:?}: {}",
                response.body
            );
            assert_eq!(
                response.body["error"]["details"][0]["code"], "NOT_A_SECRET_REFERENCE",
                "{method} {value:?}: {}",
                response.body
            );
            assert!(
                !response.body.to_string().contains(value),
                "{method}: the refusal echoed the value"
            );
        }
    }

    let stored: Vec<String> = sqlx::query_scalar(
        "SELECT secret_reference FROM integration_credentials WHERE external_system_id = $1",
    )
    .bind(system)
    .fetch_all(&app.pool)
    .await
    .expect("read the credentials");
    assert_eq!(
        stored,
        vec!["vault://kelir/shapes"],
        "nothing refused was stored"
    );

    // Nor recorded: the only credential rows in the trail are the good one's.
    let trail: Vec<String> = sqlx::query_scalar(
        "SELECT coalesce(old_value_json::text, '') || coalesce(new_value_json::text, '')
         FROM audit_events WHERE object_type = 'INTEGRATION_CREDENTIAL'",
    )
    .fetch_all(&app.pool)
    .await
    .expect("read the trail");
    assert_eq!(trail.len(), 1, "{trail:?}");
    for value in raw {
        assert!(
            trail.iter().all(|row| !row.contains(value)),
            "a refused value reached the audit trail"
        );
    }
}

// ---------------------------------------------------------------------------
// AC-5 — a credential's audit values are the credential's to show
// ---------------------------------------------------------------------------

/// The trail records a credential's reference. `integration:credential:read`
/// exists so that seeing a system is not seeing where its secrets live
/// (#520, answer 3); a caller with `audit:read` and the system's read must not
/// get the reference through the trail instead.
#[tokio::test]
async fn the_audit_trail_does_not_show_a_reference_to_a_caller_who_may_not_read_it() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let reference = "vault://kelir/audit-probe/api-key";

    let created = app
        .send(
            Method::POST,
            BASE,
            Some(&token),
            Some(json!({ "systemCode": "AUDITED", "systemName": "Audited" })),
        )
        .await;
    let system = id_of(&created);
    let credential = app
        .send(
            Method::POST,
            &format!("{BASE}/{system}/credentials"),
            Some(&token),
            Some(json!({ "credentialType": "API_KEY", "secretReference": reference })),
        )
        .await;
    assert_eq!(
        credential.status,
        StatusCode::CREATED,
        "{}",
        credential.body
    );
    let credential = id_of(&credential);

    let system_reader = caller_holding(
        &app,
        "AUDIT-SYSTEM-READER",
        &["audit:read", "integration:external-system:read"],
    )
    .await;
    let keeper = caller_holding(
        &app,
        "AUDIT-CREDENTIAL-READER",
        &["audit:read", "integration:credential:read"],
    )
    .await;

    let query = format!("/api/v1/audit?objectId={credential}");

    let withheld = app.get(&query, Some(&system_reader)).await;
    assert_eq!(withheld.status, StatusCode::OK, "{}", withheld.body);
    assert_eq!(withheld.body["meta"]["total"], 1, "{}", withheld.body);
    assert_eq!(withheld.body["data"][0]["valuesWithheld"], true);
    assert!(
        !withheld.body.to_string().contains(reference),
        "the trail showed the reference to a caller without integration:credential:read"
    );

    // The system's own row stays readable to them — so the refusal above is the
    // credential's permission and not the trail refusing everything.
    let system_rows = app
        .get(
            &format!("/api/v1/audit?objectId={system}"),
            Some(&system_reader),
        )
        .await;
    assert_eq!(system_rows.body["data"][0]["valuesWithheld"], false);

    let shown = app.get(&query, Some(&keeper)).await;
    assert_eq!(shown.body["data"][0]["valuesWithheld"], false);
    assert!(shown.body.to_string().contains(reference), "{}", shown.body);
}

// ---------------------------------------------------------------------------
// baseUrl — the edit path
// ---------------------------------------------------------------------------

#[tokio::test]
async fn an_edit_cannot_put_a_credential_into_the_base_url() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let created = app
        .send(
            Method::POST,
            BASE,
            Some(&token),
            Some(json!({
                "systemCode": "URL_EDIT",
                "systemName": "Url edit",
                "baseUrl": "https://erp.example.com/api"
            })),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let system = id_of(&created);

    for url in [
        "https://svc:hunter2@erp.example.com/api",
        "https://svc@erp.example.com/api",
    ] {
        let response = app
            .send(
                Method::PUT,
                &format!("{BASE}/{system}"),
                Some(&token),
                Some(json!({ "baseUrl": url })),
            )
            .await;

        assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY, "{url}");
        assert_eq!(response.body["error"]["details"][0]["path"], "baseUrl");
        assert_eq!(
            response.body["error"]["details"][0]["code"],
            "CREDENTIALS_IN_URL"
        );
        assert!(!response.body.to_string().contains("hunter2"));
    }

    let stored: Option<String> =
        sqlx::query_scalar("SELECT base_url FROM external_systems WHERE id = $1")
            .bind(system)
            .fetch_one(&app.pool)
            .await
            .expect("read the system");
    assert_eq!(stored.as_deref(), Some("https://erp.example.com/api"));
}
