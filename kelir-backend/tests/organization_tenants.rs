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

/// The `integration:*` codes a caller's `/auth/me` reports, sorted.
async fn integration_permissions(app: &TestApp, token: &str) -> Vec<String> {
    let profile = app.get("/api/v1/auth/me", Some(token)).await;
    assert_eq!(profile.status, StatusCode::OK, "{}", profile.body);

    let mut codes: Vec<String> = profile.data()["permissions"]
        .as_array()
        .expect("permissions is an array")
        .iter()
        .filter_map(|value| value.as_str())
        .filter(|code| code.starts_with("integration:"))
        .map(str::to_owned)
        .collect();
    codes.sort();
    codes
}

#[tokio::test]
async fn a_created_tenants_administrator_sees_systems_and_not_where_secrets_are_kept() {
    // **#551, and D-18's amendment of 2026-09-25.** Provisioning withholds
    // `integration:credential:*` as it withholds `organization:tenant:*`, so a
    // new tenant's administrator can register and read external systems but
    // cannot read the references that say where their secrets live until
    // somebody grants that deliberately. Record 19's probe found all eight
    // `integration:*` codes on a tenant created through this route.
    let app = multi_tenant_app().await;
    let token = administering_token(&app).await;

    let created = app
        .post(TENANTS, Some(&token), create_body("ACME", "acme.admin"))
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    let tenant_token = app
        .sign_in_to("ACME", "acme.admin", "a-sufficiently-long-password")
        .await;

    assert_eq!(
        integration_permissions(&app, &tenant_token).await,
        [
            "integration:endpoint:call",
            "integration:external-system:create",
            "integration:external-system:deactivate",
            "integration:external-system:read",
            "integration:external-system:update",
            "integration:log:read",
        ],
        "a created tenant's ROLE-ADMIN holds the four external-system codes, \
         integration:endpoint:call (0049, #547) and integration:log:read (0050, #548), \
         and no credential code"
    );

    // The routes agree with the grant: that administrator registers a system,
    // and is refused its credential list.
    let system = app
        .post(
            "/api/v1/integration/external-systems",
            Some(&tenant_token),
            json!({ "systemCode": "ERP", "systemName": "ERP system" }),
        )
        .await;
    assert_eq!(system.status, StatusCode::CREATED, "{}", system.body);
    let system_id = system.data()["id"].as_str().expect("an id").to_owned();

    let credentials = app
        .get(
            &format!("/api/v1/integration/external-systems/{system_id}/credentials"),
            Some(&tenant_token),
        )
        .await;
    assert_eq!(
        credentials.status,
        StatusCode::FORBIDDEN,
        "{}",
        credentials.body
    );

    // Control: the system tenant's administrator, whose grants `0046`, `0049`
    // and `0050` made and provisioning never touches, still holds all ten.
    assert_eq!(
        integration_permissions(&app, &token).await,
        [
            "integration:credential:create",
            "integration:credential:delete",
            "integration:credential:read",
            "integration:credential:update",
            "integration:endpoint:call",
            "integration:external-system:create",
            "integration:external-system:deactivate",
            "integration:external-system:read",
            "integration:external-system:update",
            "integration:log:read",
        ],
        "the system tenant's administrator lost an integration grant"
    );
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

// ---------------------------------------------------------------------------
// D-104: what an access token already issued does after its tenant leaves.
//
// **A decision, not a defect** (D-104, answered A by the product owner on
// 2026-10-01). Sign-in and refresh are refused the moment a tenant is
// suspended, deactivated or deleted. For an access token issued before
// that, `middleware::auth` checks signature and expiry, and that the tenant
// and the user are not *deleted*; it does not read `status`. So a suspended
// or inactive tenant's token keeps working until it expires: at most
// `ACCESS_TOKEN_TTL_MINUTES` from issue, 15 minutes, with no leeway. That
// window is the stated limit for a suspended or inactive tenant. A deleted
// one is D-105's case (#650): its token is refused at once, which the last
// test of this group and `auth_deleted_caller.rs` hold. The tests below send
// one read and one write, not every route.
//
// The tests below pin both halves, so neither can move unnoticed: the token
// works inside the window, and the window closes. A change that makes the
// middleware read the tenant's status turns the first two red, and that is
// them working: such a change reverses D-104 and needs the decision
// reopened, not these assertions edited.
// ---------------------------------------------------------------------------

const USERS: &str = "/api/v1/identity/users";
const TENANT_PASSWORD: &str = "a-sufficiently-long-password";

/// A created tenant and the session its administrator opened while it was
/// still `ACTIVE`: both halves of the session, which `TestApp::sign_in_to`
/// does not return.
struct TenantSession {
    tenant_id: uuid::Uuid,
    access: String,
    refresh: String,
}

async fn sign_in_attempt(app: &TestApp, code: &str, username: &str) -> common::TestResponse {
    app.post(
        "/api/v1/auth/login",
        None,
        json!({
            "username": username,
            "password": TENANT_PASSWORD,
            "tenantCode": code,
        }),
    )
    .await
}

async fn created_tenant_session(
    app: &TestApp,
    system_token: &str,
    code: &str,
    username: &str,
) -> TenantSession {
    let created = app
        .post(TENANTS, Some(system_token), create_body(code, username))
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let tenant_id = created.data()["id"]
        .as_str()
        .expect("a created tenant has an id")
        .parse()
        .expect("a tenant id is a uuid");

    let session = sign_in_attempt(app, code, username).await;
    assert_eq!(session.status, StatusCode::OK, "{}", session.body);

    let field = |name: &str| {
        session.data()[name]
            .as_str()
            .unwrap_or_else(|| panic!("no {name} in {}", session.body))
            .to_owned()
    };

    TenantSession {
        tenant_id,
        access: field("accessToken"),
        refresh: field("refreshToken"),
    }
}

async fn set_tenant_status(app: &TestApp, system_token: &str, tenant_id: uuid::Uuid, status: &str) {
    let changed = app
        .put(
            &format!("{TENANTS}/{tenant_id}"),
            Some(system_token),
            json!({ "status": status }),
        )
        .await;
    assert_eq!(changed.status, StatusCode::OK, "{}", changed.body);
    assert_eq!(changed.data()["status"], status);
}

fn user_body(username: &str) -> serde_json::Value {
    json!({
        "username": username,
        "email": format!("{username}@example.test"),
        "password": TENANT_PASSWORD,
        "displayName": "Created With An Outliving Token",
    })
}

/// How many live users of that name the tenant holds, read past the API: the
/// tenant's own administrator can no longer sign in to be asked.
async fn users_named(app: &TestApp, tenant_id: uuid::Uuid, username: &str) -> i64 {
    let (count,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM users \
         WHERE tenant_id = $1 AND username = $2 AND deleted_at IS NULL",
    )
    .bind(tenant_id)
    .bind(username)
    .fetch_one(&app.pool)
    .await
    .expect("reads users");

    count
}

fn usernames(listed: &common::TestResponse) -> Vec<&str> {
    listed.body["data"]
        .as_array()
        .unwrap_or_else(|| panic!("not a list: {}", listed.body))
        .iter()
        .filter_map(|user| user["username"].as_str())
        .collect()
}

/// The same claims under the same signature, as the token would be `seconds`
/// later: `iat` and `exp` both moved back, nothing else touched.
///
/// The harness has no clock to advance and the lifetime is a constant, so the
/// token is aged instead of the test waiting. With `seconds == 0` this is a
/// re-signed copy of a live token, which is the control for every refusal an
/// aged one gets.
fn aged(access: &str, seconds: i64) -> String {
    use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};

    let mut claims = decode::<serde_json::Value>(
        access,
        &DecodingKey::from_secret(common::JWT_SECRET.as_bytes()),
        &Validation::new(Algorithm::HS256),
    )
    .expect("the application's own token verifies under the test secret")
    .claims;

    for claim in ["iat", "exp"] {
        let issued = claims[claim]
            .as_i64()
            .unwrap_or_else(|| panic!("no {claim} in {claims}"));
        claims[claim] = json!(issued - seconds);
    }

    encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(common::JWT_SECRET.as_bytes()),
    )
    .expect("claims sign")
}

/// D-104 (A), the half inside the window, for one status.
async fn an_access_token_outlives_its_tenant_becoming(status: &str) {
    let app = multi_tenant_app().await;
    let token = administering_token(&app).await;
    let session = created_tenant_session(&app, &token, "ACME", "acme.admin").await;

    set_tenant_status(&app, &token, session.tenant_id, status).await;

    // The token issued before the change reads the tenant's own data...
    let listed = app.get(USERS, Some(&session.access)).await;
    assert_eq!(
        listed.status,
        StatusCode::OK,
        "D-104 (A): a token issued before its tenant became {status} is good until it \
         expires. If the middleware now refuses it, the decision was reversed; reopen \
         D-104 rather than editing this assertion: {}",
        listed.body
    );
    assert_eq!(usernames(&listed), ["acme.admin"], "{}", listed.body);

    // ...and writes to it. Not merely answered 201: the row is there.
    let written = app
        .post(USERS, Some(&session.access), user_body("acme.late"))
        .await;
    assert_eq!(
        written.status,
        StatusCode::CREATED,
        "D-104 (A): the limit covers writes as well as reads, for a {status} tenant: {}",
        written.body
    );
    assert_eq!(
        users_named(&app, session.tenant_id, "acme.late").await,
        1,
        "the write a {status} tenant's token was answered 201 for did not happen"
    );

    // What makes the limit a bound: the session cannot be started again...
    let refused = sign_in_attempt(&app, "ACME", "acme.admin").await;
    assert_eq!(
        refused.status,
        StatusCode::UNAUTHORIZED,
        "a {status} tenant admitted a new sign-in: {}",
        refused.body
    );

    // ...or extended, so the token above is the last one this tenant holds.
    let rotated = app
        .post(
            "/api/v1/auth/refresh",
            None,
            json!({ "refreshToken": session.refresh }),
        )
        .await;
    assert_eq!(
        rotated.status,
        StatusCode::UNAUTHORIZED,
        "a {status} tenant's session could still be extended: {}",
        rotated.body
    );
}

#[tokio::test]
async fn by_decision_d_104_an_access_token_outlives_its_tenants_suspension_until_it_expires() {
    // **Intended, and bounded** (D-104, A). Do not "fix" this test: see the
    // note above `USERS`.
    an_access_token_outlives_its_tenant_becoming("SUSPENDED").await;
}

#[tokio::test]
async fn by_decision_d_104_an_access_token_outlives_its_tenants_deactivation_until_it_expires() {
    // **Intended, and bounded** (D-104, A). Do not "fix" this test: see the
    // note above `USERS`.
    an_access_token_outlives_its_tenant_becoming("INACTIVE").await;
}

#[tokio::test]
async fn a_tenant_that_left_has_its_expired_access_token_refused_and_nothing_renews_it() {
    // **The bound D-104 (A) rests on.** The limit is acceptable because it
    // ends: the token above stops at its expiry, and a tenant that is not
    // `ACTIVE` has no way to another one.
    //
    // Aged past `ACCESS_TOKEN_TTL_MINUTES` by one second:
    // `verify_access_token` allows no leeway on `exp`.
    use kelir_backend::modules::auth::token::ACCESS_TOKEN_TTL_MINUTES;

    let past_expiry = ACCESS_TOKEN_TTL_MINUTES * 60 + 1;

    let app = multi_tenant_app().await;
    let token = administering_token(&app).await;

    for (status, code, username) in [
        ("SUSPENDED", "ACME", "acme.admin"),
        ("INACTIVE", "BETA", "beta.admin"),
    ] {
        let session = created_tenant_session(&app, &token, code, username).await;
        set_tenant_status(&app, &token, session.tenant_id, status).await;

        // Control: a re-signed copy with its expiry untouched is the token
        // D-104 describes, so what refuses the aged one is its age and not
        // that this test minted it.
        let live = app.get(USERS, Some(&aged(&session.access, 0))).await;
        assert_eq!(live.status, StatusCode::OK, "{status}: {}", live.body);

        let expired = aged(&session.access, past_expiry);

        let read = app.get(USERS, Some(&expired)).await;
        assert_eq!(
            read.status,
            StatusCode::UNAUTHORIZED,
            "a {status} tenant's token still read after its lifetime: {}",
            read.body
        );
        assert_eq!(read.error_code(), Some("UNAUTHORIZED"));

        let late = format!("{}.late", code.to_lowercase());
        let written = app.post(USERS, Some(&expired), user_body(&late)).await;
        assert_eq!(
            written.status,
            StatusCode::UNAUTHORIZED,
            "a {status} tenant's token still wrote after its lifetime: {}",
            written.body
        );
        assert_eq!(
            users_named(&app, session.tenant_id, &late).await,
            0,
            "a refused write from a {status} tenant happened anyway"
        );

        // And there is no way back in: not the refresh token issued with it,
        // and not the credentials.
        let rotated = app
            .post(
                "/api/v1/auth/refresh",
                None,
                json!({ "refreshToken": session.refresh }),
            )
            .await;
        assert_eq!(
            rotated.status,
            StatusCode::UNAUTHORIZED,
            "{status}: {}",
            rotated.body
        );
        assert!(
            rotated.data()["accessToken"].is_null(),
            "a refused refresh returned a token: {}",
            rotated.body
        );

        let refused = sign_in_attempt(&app, code, username).await;
        assert_eq!(
            refused.status,
            StatusCode::UNAUTHORIZED,
            "{status}: {}",
            refused.body
        );
        assert!(
            refused.data()["accessToken"].is_null(),
            "a refused sign-in returned a token: {}",
            refused.body
        );
    }
}

#[tokio::test]
async fn a_deleted_tenants_access_token_is_refused_at_once_and_nothing_renews_it() {
    // **Decided** (D-105, answered A by the product owner on 2026-10-01;
    // #650): a deleted tenant's access token is refused on its next request,
    // on every route. D-104 named suspension and deactivation and did not
    // decide this case; until D-105 this test pinned what the code did, a
    // 200, under the name `a_deleted_tenants_access_token_still_reads_its_
    // users_which_d_104_does_not_decide`.
    let app = multi_tenant_app().await;
    let token = administering_token(&app).await;
    let session = created_tenant_session(&app, &token, "ACME", "acme.admin").await;

    // Control: the token reads its tenant's users while the tenant is there.
    let before = app.get(USERS, Some(&session.access)).await;
    assert_eq!(before.status, StatusCode::OK, "{}", before.body);
    assert_eq!(usernames(&before), ["acme.admin"], "{}", before.body);

    let deleted = app
        .delete(&format!("{TENANTS}/{}", session.tenant_id), Some(&token))
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);

    let listed = app.get(USERS, Some(&session.access)).await;
    assert_eq!(
        listed.status,
        StatusCode::UNAUTHORIZED,
        "D-105: a deleted tenant's access token is refused at once: {}",
        listed.body
    );
    assert_eq!(listed.error_code(), Some("UNAUTHORIZED"));

    // And nothing renews it.
    let rotated = app
        .post(
            "/api/v1/auth/refresh",
            None,
            json!({ "refreshToken": session.refresh }),
        )
        .await;
    assert_eq!(rotated.status, StatusCode::UNAUTHORIZED, "{}", rotated.body);

    let refused = sign_in_attempt(&app, "ACME", "acme.admin").await;
    assert_eq!(refused.status, StatusCode::UNAUTHORIZED, "{}", refused.body);
}

#[tokio::test]
async fn the_administering_tenant_cannot_suspend_or_delete_itself() {
    // Both would stop the session making the request being renewed, and leave
    // nobody able to undo it — the refusal `deactivate_user` already gives for your own
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

// ---------------------------------------------------------------------------
// #655, decision D-102 B: a code that maps like a live tenant's is refused.
//
// An integration secret is read from `KELIR_INTEGRATION_SECRET_<CODE>__<NAME>`,
// `<CODE>` being the tenant code upper case with each `-` written `_`. Two
// live tenants whose prefixes match, or one of whose prefixes starts with the
// other's, share names that the resolver refuses for both (#618's gate 3).
// The route refuses such a code instead. A deleted tenant's code does not
// block. An identical code stays the unique index's 409, above.
//
// Case is held here by codes typed in lower case, which the route upper-cases
// before it compares, and by `integration::domain::secret`'s unit tests for a
// stored code that is not upper case, which no route writes.
//
// The four tests below were red against the unchanged route (each created
// the second tenant, 201). Each mutation was then applied by script, this
// file and the unit tests run, and the mutation reverted (2026-10-10):
//
// | Mutation | Red here | Red in unit tests |
// |---|---|---|
// | `lock_tenant_codes` not taken | `a_colliding_tenant_committed_while_the_check_waits_is_seen` | none |
// | `namespaces_overlap` one direction only | `..._covers_or_is_covered_by_...`, `..._a_deleted_tenants_is_accepted_...` | 2 in `domain::secret`, 2 in `organization::domain` |
// | `NAMESPACE_SEPARATOR` one `_` | `..._covers_or_is_covered_by_...` | 6 in `domain::secret`, 1 in `organization::domain` |
// | `-` not written `_` | all four | 5 in `domain::secret`, 1 in `organization::domain` |
// | The code not upper-cased | none: the route upper-cases first | 3 in `domain::secret` |
// | An identical live code compared too | `a_tenant_code_is_one_tenant_however_it_is_spelled` (422, not 409) | 1 in `organization::domain` |
//
// Live against deleted rests on `repository::live_codes`'s
// `deleted_at IS NULL`, which #618's mutation table in
// `integration_test_call.rs` already holds; here the third test creates each
// pair's second code only after the first is deleted, and refuses it while a
// suspended or inactive one is not.
// ---------------------------------------------------------------------------

/// The detail code the refusal names (`ValidationDetail::code`).
const SECRET_NAMESPACE_IN_USE: &str = "SECRET_NAMESPACE_IN_USE";

/// Asserts a creation was refused as #655 says: 422 `VALIDATION_ERROR` with
/// one detail, on `tenantCode`, coded `SECRET_NAMESPACE_IN_USE`, and naming
/// none of `others`.
fn assert_refused_as_a_shared_namespace(
    response: &common::TestResponse,
    code: &str,
    others: &[&str],
) {
    assert_eq!(
        response.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{code} was not refused: {}",
        response.body
    );
    assert_eq!(response.error_code(), Some("VALIDATION_ERROR"), "{code}");

    let details = response.body["error"]["details"]
        .as_array()
        .expect("details is an array");
    assert_eq!(details.len(), 1, "{code}: {details:?}");
    assert_eq!(details[0]["path"], "tenantCode", "{code}");
    assert_eq!(details[0]["rule"], "secretNamespace", "{code}");
    assert_eq!(details[0]["code"], SECRET_NAMESPACE_IN_USE, "{code}");

    let said = response.body.to_string();
    for other in others {
        assert!(
            !said.contains(other),
            "{code}'s refusal names {other}: {said}"
        );
    }
}

async fn tenant_rows(app: &TestApp, code: &str) -> i64 {
    let (count,): (i64,) = sqlx::query_as("SELECT count(*) FROM tenants WHERE tenant_code = $1")
        .bind(code)
        .fetch_one(&app.pool)
        .await
        .expect("reads tenants");
    count
}

fn administrator_of(code: &str) -> String {
    format!("admin.{}", code.trim().to_lowercase())
}

/// Creates `code` through the route and returns its id.
async fn created(app: &TestApp, token: &str, code: &str) -> uuid::Uuid {
    let response = app
        .post(
            TENANTS,
            Some(token),
            create_body(code, &administrator_of(code)),
        )
        .await;
    assert_eq!(
        response.status,
        StatusCode::CREATED,
        "{code}: {}",
        response.body
    );
    uuid::Uuid::parse_str(response.data()["id"].as_str().expect("an id")).expect("a uuid")
}

/// Asserts the route refuses `code` as a shared namespace, naming none of
/// `others`, and leaves neither a tenant nor an administrator behind.
async fn refused(app: &TestApp, token: &str, code: &str, others: &[&str]) {
    let response = app
        .post(
            TENANTS,
            Some(token),
            create_body(code, &administrator_of(code)),
        )
        .await;
    assert_refused_as_a_shared_namespace(&response, code, others);

    assert_eq!(
        tenant_rows(app, &code.trim().to_uppercase()).await,
        0,
        "the refused {code} left a tenant row"
    );
    let (users,): (i64,) = sqlx::query_as("SELECT count(*) FROM users WHERE username = $1")
        .bind(administrator_of(code))
        .fetch_one(&app.pool)
        .await
        .expect("reads users");
    assert_eq!(users, 0, "the refused {code} left its administrator");
}

#[tokio::test]
async fn a_code_that_maps_like_a_live_tenants_is_refused_and_names_no_other_code() {
    // `A-B` and `A_B` both map to `A_B`. Each way round, and typed in lower
    // case.
    let app = multi_tenant_app().await;
    let token = administering_token(&app).await;

    created(&app, &token, "A-B").await;
    refused(&app, &token, "A_B", &["A-B"]).await;
    refused(&app, &token, "a_b", &["A-B"]).await;

    created(&app, &token, "C_D").await;
    refused(&app, &token, "C-D", &["C_D"]).await;
    refused(&app, &token, "c-d", &["C_D"]).await;

    // Two hyphens are two underscores, which is the separator: `EF--G` is
    // `EF__G`, which `EF` covers.
    created(&app, &token, "EF").await;
    refused(&app, &token, "EF--G", &[]).await;

    // The system tenant is live like any other.
    refused(&app, &token, "SYSTEM__X", &[]).await;
    refused(&app, &token, "system-", &[]).await;
}

#[tokio::test]
async fn a_code_that_covers_or_is_covered_by_a_live_tenants_is_refused() {
    let app = multi_tenant_app().await;
    let token = administering_token(&app).await;

    // `ACME` covers every code spelled `ACME__…`, however its underscores
    // are typed, and `ACME_` and `ACME-`, whose prefix `…ACME___` starts
    // with `ACME`'s `…ACME__`.
    created(&app, &token, "ACME").await;
    for code in [
        "ACME__X",
        "acme__x",
        "ACME-_X",
        "ACME_-X",
        "ACME--X",
        "ACME__X__Y",
        "ACME_",
        "ACME-",
    ] {
        refused(&app, &token, code, &["ACME"]).await;
    }

    // ...and the other way round: `LONG__X` is covered by `LONG`.
    created(&app, &token, "LONG__X").await;
    for code in ["LONG", "long", "LONG__X__Y"] {
        refused(&app, &token, code, &["LONG__X"]).await;
    }

    // Neighbours that share characters and no name. One underscore is not the
    // separator, and `LONG__Y` is `LONG__X`'s sibling, not its cover.
    for code in ["ACME_X", "ACMEX", "ACME2", "XACME", "LONG_X", "LONG__Y"] {
        created(&app, &token, code).await;
    }
}

#[tokio::test]
async fn a_code_that_maps_like_a_deleted_tenants_is_accepted_and_a_suspended_one_still_blocks() {
    // D-102 B: live means not deleted, whatever the status. A deleted
    // tenant's prefix is free again, and the variables it left are the
    // operator's to clear (Installation and Deployment §7.1). #690's case is
    // `ACME__X` after `ACME`.
    let app = multi_tenant_app().await;
    let token = administering_token(&app).await;

    for (first, second) in [("A-B", "A_B"), ("ACME", "ACME__X"), ("LONG__X", "LONG")] {
        let id = created(&app, &token, first).await;
        refused(&app, &token, second, &[]).await;

        let deleted = app.delete(&format!("{TENANTS}/{id}"), Some(&token)).await;
        assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);

        created(&app, &token, second).await;
    }

    for (first, second, status) in [
        ("SUS-S", "SUS_S", "SUSPENDED"),
        ("SUS-I", "SUS_I", "INACTIVE"),
    ] {
        let id = created(&app, &token, first).await;
        set_tenant_status(&app, &token, id, status).await;

        refused(&app, &token, second, &[first]).await;
    }
}

/// `organization::repository::lock_tenant_codes`'s key, repeated so that the
/// test below can hold the lock the route waits on. A change there that is
/// not made here turns that test red.
const TENANT_CODE_LOCK_CLASS: i32 = 0x544E_4344;
const TENANT_CODE_LOCK_KEY: &str = "tenant_code";

#[tokio::test]
async fn a_colliding_tenant_committed_while_the_check_waits_is_seen() {
    // Two creations racing: `RACE-A` is inserted and not yet committed when
    // `RACE_A` is requested. Without a lock, the second reads the live codes,
    // does not see the first, and is inserted beside it; the unique index
    // does not stop it, because the codes differ. The interleaving is
    // arranged, not raced for: this transaction holds the lock the route
    // takes before its read, and commits once the route is waiting on it.
    let app = multi_tenant_app().await;
    let token = administering_token(&app).await;

    let mut holder = app.pool.begin().await.expect("begins");
    sqlx::query("SELECT pg_advisory_xact_lock($1, hashtext($2::text))")
        .bind(TENANT_CODE_LOCK_CLASS)
        .bind(TENANT_CODE_LOCK_KEY)
        .execute(&mut *holder)
        .await
        .expect("takes the tenant-code lock");
    sqlx::query(
        "INSERT INTO tenants (id, tenant_code, name, status) \
         VALUES ($1, 'RACE-A', 'Race A', 'ACTIVE')",
    )
    .bind(uuid::Uuid::now_v7())
    .execute(&mut *holder)
    .await
    .expect("inserts the first tenant");

    let request = app.post(TENANTS, Some(&token), create_body("RACE_A", "race.admin"));
    let release = async {
        // Commit once the route is queued on the lock. If it never queues,
        // commit after ten seconds anyway, and the assertions below say what
        // the route did without it.
        for _ in 0..200 {
            let (waiting,): (i64,) = sqlx::query_as(
                "SELECT count(*) FROM pg_locks \
                 WHERE locktype = 'advisory' AND NOT granted \
                 AND database = (SELECT oid FROM pg_database WHERE datname = current_database())",
            )
            .fetch_one(&app.pool)
            .await
            .expect("reads pg_locks");
            if waiting > 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        holder.commit().await.expect("commits the first tenant");
    };
    let (response, ()) = tokio::join!(request, release);

    assert_refused_as_a_shared_namespace(&response, "RACE_A", &["RACE-A"]);
    assert_eq!(tenant_rows(&app, "RACE-A").await, 1);
    assert_eq!(tenant_rows(&app, "RACE_A").await, 0);
}
