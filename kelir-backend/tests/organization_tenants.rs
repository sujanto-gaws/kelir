//! Tenant administration (FR-ORG-001, [#27]) and the tenancy model decision
//! D-18 that made it buildable.
//!
//! **What these tests are really guarding.** D-13 refused to schedule this
//! surface because, under D-7, it would have managed rows nobody could sign in
//! to. Two of the tests below are the direct answer to that objection —
//! `creating_a_tenant_creates_an_administrator_who_can_sign_in` and
//! `a_suspended_tenant_stops_admitting_the_administrator_it_was_created_with`.
//! The rest guard the boundary that keeps tenant-scoped roles from being a
//! privilege escalation.
//!
//! Sign-in with a tenant code needs a multi-tenant deployment, so most tests
//! here use `TestApp::spawn_with` rather than `spawn`. That is not a
//! convenience: it is the mode D-7 refused to let anything run in.

mod common;

use axum::http::{Method, StatusCode};
use serde_json::json;

use common::{fixtures, TestApp, ADMIN_PASSWORD, ADMIN_USERNAME};

const TENANTS: &str = "/api/v1/organization/tenants";

/// A deployment serving more than one tenant, which is what every test that
/// signs in as somebody other than the bootstrap administrator needs.
async fn multi_tenant_app() -> TestApp {
    TestApp::spawn_with(|config| config.multi_tenant = true).await
}

/// The bootstrap administrator's token on a multi-tenant deployment.
///
/// `TestApp::administrator_token` sends no tenant code, which is right for
/// every other test and refused here — that refusal is the mode working. The
/// administrator lives in the deployment's default tenant, so `SYSTEM` is the
/// code that reaches it.
async fn administering_token(app: &TestApp) -> String {
    app.sign_in_to("SYSTEM", ADMIN_USERNAME, ADMIN_PASSWORD)
        .await
}

fn create_body(code: &str, username: &str) -> serde_json::Value {
    json!({
        "tenantCode": code,
        "name": format!("{code} Limited"),
        "administrator": {
            "username": username,
            "email": format!("{username}@example.test"),
            "displayName": "Tenant Administrator",
            "password": "a-sufficiently-long-password",
        },
    })
}

#[tokio::test]
async fn creating_a_tenant_creates_an_administrator_who_can_sign_in() {
    // **The test D-13's objection reduces to.** Its exact words were that a
    // tenant administration surface "would create rows nobody can sign in to".
    // If this passes, that is no longer true of this surface; if it ever fails,
    // D-13 was right after all and the feature should go back to §7.
    let app = multi_tenant_app().await;
    let token = administering_token(&app).await;

    let created = app
        .post(TENANTS, Some(&token), create_body("ACME", "acme.admin"))
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    assert_eq!(created.data()["tenantCode"], "ACME");
    assert_eq!(created.data()["status"], "ACTIVE");
    assert_eq!(
        created.data()["userCount"],
        1,
        "a created tenant holds exactly its first administrator: {}",
        created.body
    );
    assert_eq!(
        created.data()["isDefault"],
        false,
        "a created tenant is not the one administration is performed from"
    );

    // The claim in full: those credentials work, against that tenant, through
    // the real login endpoint.
    let tenant_token = app
        .sign_in_to("ACME", "acme.admin", "a-sufficiently-long-password")
        .await;

    let profile = app.get("/api/v1/auth/me", Some(&tenant_token)).await;
    assert_eq!(profile.status, StatusCode::OK);
    assert_eq!(profile.data()["username"], "acme.admin");
}

#[tokio::test]
async fn a_tenants_own_administrator_administers_its_tenant_and_not_tenants() {
    // The escalation this surface would otherwise create. Every tenant gets its
    // own `ROLE-ADMIN` holding the whole catalogue (D-18), so without the
    // withheld family plus the administering-tenant check, creating a tenant
    // would hand its administrator the power to create more.
    let app = multi_tenant_app().await;
    let token = administering_token(&app).await;

    app.post(TENANTS, Some(&token), create_body("ACME", "acme.admin"))
        .await;

    let tenant_token = app
        .sign_in_to("ACME", "acme.admin", "a-sufficiently-long-password")
        .await;

    let profile = app.get("/api/v1/auth/me", Some(&tenant_token)).await;
    let permissions: Vec<&str> = profile.data()["permissions"]
        .as_array()
        .expect("permissions is an array")
        .iter()
        .filter_map(|value| value.as_str())
        .collect();

    assert!(
        permissions.contains(&"identity:user:create"),
        "the tenant administrator cannot administer its own tenant: {permissions:?}"
    );
    assert!(
        !permissions
            .iter()
            .any(|code| code.starts_with("organization:tenant:")),
        "the tenant administrator was given tenant administration: {permissions:?}"
    );

    let refused = app.get(TENANTS, Some(&tenant_token)).await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn holding_the_permission_is_not_enough_outside_the_administering_tenant() {
    // The boundary that does the real work, isolated from the one above. The
    // permission catalogue is global and a tenant administrator holds
    // `identity:role:update`, so nothing stops them granting themselves
    // `organization:tenant:manage` — this is what refuses the request anyway.
    //
    // Reintroduced-defect check (coding standard §2.9): with the
    // `caller.tenant_id() != administering.id` test removed from
    // `require_tenant_administrator`, this case answers 200 and the assertion
    // below fails. Seen to fail before being accepted.
    let app = multi_tenant_app().await;
    let token = administering_token(&app).await;

    app.post(TENANTS, Some(&token), create_body("ACME", "acme.admin"))
        .await;

    let tenant_token = app
        .sign_in_to("ACME", "acme.admin", "a-sufficiently-long-password")
        .await;

    // The tenant's administrator grants the withheld family to their own role,
    // which they may: it is their tenant's role and the catalogue is global.
    let roles = app.get("/api/v1/identity/roles", Some(&tenant_token)).await;
    let role = roles.body["data"][0].clone();
    let role_id = role["id"].as_str().expect("the tenant has a role");

    let catalogue = app
        .get("/api/v1/identity/permissions", Some(&tenant_token))
        .await;
    let every_permission: Vec<serde_json::Value> = catalogue
        .data()
        .as_array()
        .expect("the catalogue is an array")
        .iter()
        .map(|permission| permission["id"].clone())
        .collect();

    let granted = app
        .put(
            &format!("/api/v1/identity/roles/{role_id}"),
            Some(&tenant_token),
            json!({ "permissionIds": every_permission }),
        )
        .await;
    assert_eq!(
        granted.status,
        StatusCode::OK,
        "the escalation this test is built on did not happen: {}",
        granted.body
    );

    // A fresh token, so the claims carry what was just granted.
    let escalated = app
        .sign_in_to("ACME", "acme.admin", "a-sufficiently-long-password")
        .await;

    for (method, uri) in [
        (Method::GET, TENANTS.to_owned()),
        (Method::POST, TENANTS.to_owned()),
    ] {
        let body = matches!(method, Method::POST).then(|| create_body("OTHER", "other.admin"));
        let response = app.send(method.clone(), &uri, Some(&escalated), body).await;

        assert_eq!(
            response.status,
            StatusCode::FORBIDDEN,
            "{method} {uri} was allowed from outside the administering tenant: {}",
            response.body
        );
    }
}

#[tokio::test]
async fn a_caller_without_the_permission_cannot_read_or_write_tenants() {
    let app = TestApp::spawn().await;

    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "no.tenants",
        "no.tenants@kelir.test",
        "a-sufficiently-long-password",
        &[],
    )
    .await;

    let token = app
        .sign_in("no.tenants", "a-sufficiently-long-password")
        .await;

    for (method, body) in [
        (Method::GET, None),
        (Method::POST, Some(create_body("ACME", "acme.admin"))),
    ] {
        let response = app.send(method.clone(), TENANTS, Some(&token), body).await;

        assert_eq!(
            response.status,
            StatusCode::FORBIDDEN,
            "{method} {TENANTS} was allowed without organization:tenant:*"
        );
    }
}

#[tokio::test]
async fn every_tenant_route_refuses_a_request_with_no_token() {
    let app = TestApp::spawn().await;
    let id = uuid::Uuid::now_v7();

    let routes = [
        (Method::GET, TENANTS.to_owned(), None),
        (
            Method::POST,
            TENANTS.to_owned(),
            Some(create_body("ACME", "acme.admin")),
        ),
        (Method::GET, format!("{TENANTS}/{id}"), None),
        (
            Method::PUT,
            format!("{TENANTS}/{id}"),
            Some(json!({ "name": "Renamed" })),
        ),
        (Method::DELETE, format!("{TENANTS}/{id}"), None),
    ];

    for (method, uri, body) in routes {
        let response = app.send(method.clone(), &uri, None, body).await;

        assert_eq!(
            response.status,
            StatusCode::UNAUTHORIZED,
            "{method} {uri} answered without a token"
        );
    }
}

#[tokio::test]
async fn a_tenant_code_is_one_tenant_however_it_is_spelled() {
    // Codes normalise on the way in, so `acme` and `ACME` must not become two
    // tenants a user could be told to sign in to interchangeably.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let first = app
        .post(TENANTS, Some(&token), create_body("ACME", "acme.admin"))
        .await;
    assert_eq!(first.status, StatusCode::CREATED);

    let second = app
        .post(
            TENANTS,
            Some(&token),
            create_body("  acme  ", "acme.other.admin"),
        )
        .await;

    assert_eq!(second.status, StatusCode::CONFLICT, "{}", second.body);
    assert_eq!(second.error_code(), Some("CONFLICT"));
}

#[tokio::test]
async fn the_administrators_fields_are_reported_under_the_paths_the_form_has() {
    // #67 one layer up: a per-field message against a field the form does not
    // have is a message nobody sees. The request nests the administrator, so
    // the details must too.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let refused = app
        .post(
            TENANTS,
            Some(&token),
            json!({
                "tenantCode": "ACME",
                "name": "Acme Limited",
                "administrator": {
                    "username": "not a username",
                    "email": "not-an-email",
                    "displayName": "",
                    "password": "short",
                },
            }),
        )
        .await;

    assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY);

    let paths: Vec<&str> = refused.body["error"]["details"]
        .as_array()
        .expect("details is an array")
        .iter()
        .filter_map(|detail| detail["path"].as_str())
        .collect();

    for expected in [
        "administrator.username",
        "administrator.email",
        "administrator.displayName",
        "administrator.password",
    ] {
        assert!(
            paths.contains(&expected),
            "{expected} missing from {paths:?}"
        );
    }
}

#[tokio::test]
async fn a_refused_tenant_leaves_nothing_behind() {
    // Creation is one transaction across two modules. A tenant row committed
    // beside a failed administrator is exactly the state this surface exists
    // not to produce.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let refused = app
        .post(
            TENANTS,
            Some(&token),
            json!({
                "tenantCode": "GHOST",
                "name": "Ghost Limited",
                "administrator": {
                    "username": "ghost.admin",
                    "email": "ghost@example.test",
                    "displayName": "Ghost",
                    // Below MIN_PASSWORD_LENGTH, so provisioning fails after
                    // the tenant row has been inserted in the transaction.
                    "password": "short",
                },
            }),
        )
        .await;

    assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY);

    let surviving: Option<(String,)> =
        sqlx::query_as("SELECT tenant_code FROM tenants WHERE tenant_code = 'GHOST'")
            .fetch_optional(&app.pool)
            .await
            .expect("reads tenants");

    assert!(
        surviving.is_none(),
        "the tenant row survived a failed provisioning"
    );
}

#[tokio::test]
async fn a_suspended_tenant_stops_admitting_the_administrator_it_was_created_with() {
    // Suspension has to *mean* something, and "no new sign-ins" is only half of
    // it: a refresh token issued a minute earlier would otherwise keep a
    // session alive indefinitely. The same rule `update_user` applies to a
    // deactivated account.
    let app = multi_tenant_app().await;
    let token = administering_token(&app).await;

    let created = app
        .post(TENANTS, Some(&token), create_body("ACME", "acme.admin"))
        .await;
    let tenant_id = created.data()["id"].as_str().expect("created").to_owned();

    let session = app
        .post(
            "/api/v1/auth/login",
            None,
            json!({
                "username": "acme.admin",
                "password": "a-sufficiently-long-password",
                "tenantCode": "ACME",
            }),
        )
        .await;
    let refresh_token = session.data()["refreshToken"]
        .as_str()
        .expect("a refresh token")
        .to_owned();

    let suspended = app
        .put(
            &format!("{TENANTS}/{tenant_id}"),
            Some(&token),
            json!({ "status": "SUSPENDED" }),
        )
        .await;
    assert_eq!(suspended.status, StatusCode::OK, "{}", suspended.body);
    assert_eq!(suspended.data()["status"], "SUSPENDED");

    // No new sign-in...
    let refused = app
        .post(
            "/api/v1/auth/login",
            None,
            json!({
                "username": "acme.admin",
                "password": "a-sufficiently-long-password",
                "tenantCode": "ACME",
            }),
        )
        .await;
    assert_eq!(refused.status, StatusCode::UNAUTHORIZED);

    // ...and no extending the one that already existed.
    let rotated = app
        .post(
            "/api/v1/auth/refresh",
            None,
            json!({ "refreshToken": refresh_token }),
        )
        .await;
    assert_eq!(
        rotated.status,
        StatusCode::UNAUTHORIZED,
        "a suspended tenant's session could still be extended: {}",
        rotated.body
    );
}

#[tokio::test]
async fn the_administering_tenant_cannot_suspend_or_delete_itself() {
    // Both would end the session making the request, and leave nobody able to
    // undo it — the refusal `deactivate_user` already gives for your own
    // account.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let listed = app.get(TENANTS, Some(&token)).await;
    let own = listed.body["data"]
        .as_array()
        .expect("a list")
        .iter()
        .find(|tenant| tenant["isDefault"] == true)
        .expect("the administering tenant is in its own list")
        .clone();
    let id = own["id"].as_str().expect("an id");

    let suspended = app
        .put(
            &format!("{TENANTS}/{id}"),
            Some(&token),
            json!({ "status": "SUSPENDED" }),
        )
        .await;
    assert_eq!(
        suspended.status,
        StatusCode::BAD_REQUEST,
        "{}",
        suspended.body
    );

    let deleted = app.delete(&format!("{TENANTS}/{id}"), Some(&token)).await;
    assert_eq!(deleted.status, StatusCode::BAD_REQUEST, "{}", deleted.body);

    // And the administering tenant is still there to be administered from.
    let still_signs_in = app.sign_in(ADMIN_USERNAME, ADMIN_PASSWORD).await;
    assert!(!still_signs_in.is_empty());
}

#[tokio::test]
async fn renaming_a_tenant_does_not_change_the_code_users_sign_in_with() {
    // `tenantCode` is absent from `UpdateTenantRequest` by design, and the DTO
    // denies unknown fields — so an attempt to change it is refused rather than
    // silently ignored, which is what would strand a tenant's users.
    let app = multi_tenant_app().await;
    let token = administering_token(&app).await;

    let created = app
        .post(TENANTS, Some(&token), create_body("ACME", "acme.admin"))
        .await;
    let id = created.data()["id"].as_str().expect("created").to_owned();

    let renamed = app
        .put(
            &format!("{TENANTS}/{id}"),
            Some(&token),
            json!({ "name": "Acme Holdings" }),
        )
        .await;
    assert_eq!(renamed.status, StatusCode::OK);
    assert_eq!(renamed.data()["name"], "Acme Holdings");
    assert_eq!(renamed.data()["tenantCode"], "ACME");

    let attempted = app
        .put(
            &format!("{TENANTS}/{id}"),
            Some(&token),
            json!({ "tenantCode": "RENAMED" }),
        )
        .await;
    assert_eq!(
        attempted.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "changing the sign-in code was accepted: {}",
        attempted.body
    );

    // The original code still resolves, which is the property the refusal
    // protects.
    let signed_in = app
        .sign_in_to("ACME", "acme.admin", "a-sufficiently-long-password")
        .await;
    assert!(!signed_in.is_empty());
}

#[tokio::test]
async fn a_deleted_tenant_leaves_the_list_and_admits_nobody() {
    let app = multi_tenant_app().await;
    let token = administering_token(&app).await;

    let created = app
        .post(TENANTS, Some(&token), create_body("ACME", "acme.admin"))
        .await;
    let id = created.data()["id"].as_str().expect("created").to_owned();

    let deleted = app.delete(&format!("{TENANTS}/{id}"), Some(&token)).await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT);

    let gone = app.get(&format!("{TENANTS}/{id}"), Some(&token)).await;
    assert_eq!(gone.status, StatusCode::NOT_FOUND);

    let listed = app.get(TENANTS, Some(&token)).await;
    let codes: Vec<&str> = listed.body["data"]
        .as_array()
        .expect("a list")
        .iter()
        .filter_map(|tenant| tenant["tenantCode"].as_str())
        .collect();
    assert!(!codes.contains(&"ACME"), "{codes:?}");

    let refused = app
        .post(
            "/api/v1/auth/login",
            None,
            json!({
                "username": "acme.admin",
                "password": "a-sufficiently-long-password",
                "tenantCode": "ACME",
            }),
        )
        .await;
    assert_eq!(refused.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn creating_a_tenant_is_recorded_against_the_tenant_that_created_it() {
    // Audit answers "who did this". A record filed under the *new* tenant would
    // be invisible to the only people who may read this surface.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let created = app
        .post(TENANTS, Some(&token), create_body("ACME", "acme.admin"))
        .await;
    let id = created.data()["id"].as_str().expect("created").to_owned();

    let recorded: Option<(uuid::Uuid, String)> =
        sqlx::query_as("SELECT tenant_id, event_type FROM audit_events WHERE object_id = $1::uuid")
            .bind(&id)
            .fetch_optional(&app.pool)
            .await
            .expect("reads audit events");

    let (recorded_tenant, event_type) = recorded.expect("the creation was recorded");

    assert_eq!(event_type, "Tenant.Created");
    assert_eq!(
        recorded_tenant,
        fixtures::SYSTEM_TENANT_ID,
        "the record was filed under the new tenant, where nobody can read it"
    );
}

#[tokio::test]
async fn a_single_tenant_deployment_still_ignores_a_supplied_tenant_code() {
    // The property that keeps the flag worth having, asserted end to end rather
    // than only in the resolver's unit tests: with the flag off, naming another
    // tenant must not reach it.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    app.post(TENANTS, Some(&token), create_body("ACME", "acme.admin"))
        .await;

    // ACME's administrator exists, and this deployment serves one tenant, so
    // asking for ACME lands in SYSTEM — where that account is not.
    let refused = app
        .post(
            "/api/v1/auth/login",
            None,
            json!({
                "username": "acme.admin",
                "password": "a-sufficiently-long-password",
                "tenantCode": "ACME",
            }),
        )
        .await;

    assert_eq!(
        refused.status,
        StatusCode::UNAUTHORIZED,
        "a single-tenant deployment let a caller choose a tenant: {}",
        refused.body
    );
}

#[tokio::test]
async fn a_role_grant_cannot_cross_a_tenant_boundary() {
    // #65 as a constraint rather than a convention. The application no longer
    // writes such a row — the bootstrap looks its role up inside the tenant —
    // and this asserts that nothing else can either.
    //
    // Reintroduced-defect check (coding standard §2.9): with
    // `fk_user_roles_role_id_tenant_id` dropped, the insert succeeds and the
    // assertion below fails. Seen to fail before being accepted.
    let app = TestApp::spawn().await;
    let other = fixtures::create_tenant(&app.pool, "TNT-OTHER", "Other Tenant").await;

    let user_id = fixtures::create_user(
        &app.pool,
        other,
        "other.user",
        "other.user@example.test",
        "a-sufficiently-long-password",
        &[],
    )
    .await;

    // The system tenant's ROLE-ADMIN, granted through a row carrying the other
    // tenant's id — the exact shape the bootstrap used to write.
    let attempted = sqlx::query(
        "INSERT INTO user_roles (id, tenant_id, user_id, role_id) VALUES ($1, $2, $3, $4)",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(other)
    .bind(user_id)
    .bind(fixtures::ADMIN_ROLE_ID)
    .execute(&app.pool)
    .await;

    let error = attempted.expect_err("a cross-tenant grant must be refused by the database");
    assert!(
        error
            .as_database_error()
            .is_some_and(|error| error.is_foreign_key_violation()),
        "refused, but not by the foreign key: {error}"
    );
}

#[tokio::test]
async fn permissions_resolve_only_within_the_tenant_they_are_asked_for() {
    // The query half of the same decision. The constraint above makes the bad
    // row unwritable; this makes sure the read is still scoped, so dropping
    // either one alone is caught.
    use kelir_backend::modules::identity::repository as identity_repo;

    let app = TestApp::spawn().await;
    let other = fixtures::create_tenant(&app.pool, "TNT-OTHER", "Other Tenant").await;

    let admin: (uuid::Uuid,) = sqlx::query_as("SELECT id FROM users WHERE username = $1")
        .bind(ADMIN_USERNAME)
        .fetch_one(&app.pool)
        .await
        .expect("the bootstrap administrator exists");

    let in_own_tenant =
        identity_repo::permissions_for_user(&app.pool, fixtures::SYSTEM_TENANT_ID, admin.0)
            .await
            .expect("reads permissions");
    assert!(
        !in_own_tenant.is_empty(),
        "the administrator holds nothing in its own tenant"
    );

    let in_another = identity_repo::permissions_for_user(&app.pool, other, admin.0)
        .await
        .expect("reads permissions");
    assert!(
        in_another.is_empty(),
        "permissions leaked across a tenant boundary: {in_another:?}"
    );
}

#[tokio::test]
async fn the_deployment_endpoint_reports_the_mode_without_a_token() {
    // The login form reads this before it has any credentials, so it must
    // answer unauthenticated — and it must answer truthfully, or the form
    // renders the wrong shape and #67 comes back.
    for multi_tenant in [false, true] {
        let app = TestApp::spawn_with(|config| config.multi_tenant = multi_tenant).await;

        let response = app.get("/deployment", None).await;

        assert_eq!(response.status, StatusCode::OK);
        assert_eq!(
            response.body["multiTenant"], multi_tenant,
            "for multi_tenant={multi_tenant}: {}",
            response.body
        );
    }
}
