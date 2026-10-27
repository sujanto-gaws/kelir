//! The user-administration flows Sprint 3 recorded as hand-verified (#59).
//!
//! Duplicate handling, validation reporting, the self-deactivation guard and
//! the session consequences of an administrative password reset had no
//! automated coverage. Each is a rule that reads as obviously true and holds
//! only because a specific line of code makes it so.

mod common;

use axum::http::StatusCode;
use common::{fixtures, TestApp};
use serde_json::json;
use uuid::Uuid;

const PASSWORD: &str = "correct horse battery";

/// A well-formed create-user payload, for tests that vary one thing about it.
fn payload(username: &str) -> serde_json::Value {
    json!({
        "username": username,
        "email": format!("{username}@kelir.test"),
        "password": PASSWORD,
        "displayName": "Test Person",
    })
}

async fn user_id_of(app: &TestApp, username: &str) -> Uuid {
    sqlx::query_scalar("SELECT id FROM users WHERE username = $1")
        .bind(username)
        .fetch_one(&app.pool)
        .await
        .expect("the user exists")
}

#[tokio::test]
async fn creating_a_user_with_a_duplicate_username_is_a_conflict() {
    // The unique index is the thing that actually holds, and a 500 from a raw
    // constraint violation would be indistinguishable from a broken server to
    // any client trying to report "that name is taken".
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let first = app
        .post(
            "/api/v1/identity/users",
            Some(&token),
            payload("duplicate.name"),
        )
        .await;

    assert_eq!(first.status, StatusCode::CREATED, "{}", first.body);

    let second = app
        .post(
            "/api/v1/identity/users",
            Some(&token),
            json!({
                "username": "duplicate.name",
                // A different address, so the username is unambiguously what
                // collided.
                "email": "someone.else@kelir.test",
                "password": PASSWORD,
                "displayName": "Someone Else",
            }),
        )
        .await;

    assert_eq!(second.status, StatusCode::CONFLICT);
    assert_eq!(second.error_code(), Some("CONFLICT"));

    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM users WHERE username = $1")
        .bind("duplicate.name")
        .fetch_one(&app.pool)
        .await
        .expect("count runs");

    assert_eq!(count, 1, "the refused create must not have written a row");
}

#[tokio::test]
async fn creating_a_user_with_a_duplicate_email_is_a_conflict() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    app.post(
        "/api/v1/identity/users",
        Some(&token),
        payload("first.holder"),
    )
    .await;

    let response = app
        .post(
            "/api/v1/identity/users",
            Some(&token),
            json!({
                "username": "second.holder",
                "email": "first.holder@kelir.test",
                "password": PASSWORD,
                "displayName": "Second Holder",
            }),
        )
        .await;

    assert_eq!(response.status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn creating_a_user_reports_every_invalid_field_at_once() {
    // A form marks all its bad fields in one pass (JSON Form Schema S10.3).
    // Stopping at the first would make a four-mistake payload take four
    // round trips to fix, and the validator collects rather than returns early
    // precisely so it does not.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let response = app
        .post(
            "/api/v1/identity/users",
            Some(&token),
            json!({
                "username": "",
                "email": "not-an-address",
                "password": "short",
                "displayName": "   ",
            }),
        )
        .await;

    assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(response.error_code(), Some("VALIDATION_ERROR"));

    let reported: Vec<&str> = response.body["error"]["details"]
        .as_array()
        .expect("details is an array")
        .iter()
        .map(|detail| detail["path"].as_str().expect("path is a string"))
        .collect();

    for field in ["username", "email", "password", "displayName"] {
        assert!(
            reported.contains(&field),
            "`{field}` was not reported; got {reported:?}"
        );
    }
}

#[tokio::test]
async fn an_administrator_cannot_deactivate_their_own_account() {
    // The permission check passes first — an administrator holds
    // `identity:user:delete` — and the self-check is what refuses. Without it a
    // single-administrator deployment can be left with no way in, which is the
    // same failure mode the permanent lockout had (#55).
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let admin_id = user_id_of(&app, common::ADMIN_USERNAME).await;

    let response = app
        .delete(&format!("/api/v1/identity/users/{admin_id}"), Some(&token))
        .await;

    assert_eq!(
        response.status,
        StatusCode::BAD_REQUEST,
        "expected the self-deactivation guard, got {}: {}",
        response.status,
        response.body
    );

    // Still able to sign in, which is the consequence that matters.
    let deleted_at: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT deleted_at FROM users WHERE id = $1")
            .bind(admin_id)
            .fetch_one(&app.pool)
            .await
            .expect("select runs");

    assert!(
        deleted_at.is_none(),
        "the administrator's account was soft-deleted by the refused request"
    );
}

#[tokio::test]
async fn an_administrator_can_deactivate_another_account() {
    // The control for the guard above: it must refuse self-deletion only, not
    // deletion generally.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let id = fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "removable.user",
        "removable.user@kelir.test",
        PASSWORD,
        &[],
    )
    .await;

    let response = app
        .delete(&format!("/api/v1/identity/users/{id}"), Some(&token))
        .await;

    assert_eq!(response.status, StatusCode::NO_CONTENT);
}

/// What an administrative reset is *for*: the previous holder of the password
/// stops being able to extend their session.
///
/// It ends the refresh tokens, not the access token already issued, which
/// works until it expires, at most `ACCESS_TOKEN_TTL_MINUTES` (15) after issue
/// with no leeway (#650). That is the documented cost of carrying permissions
/// in the token (architecture 01 §18.1), and the check `middleware::auth`
/// makes on every request reads deletion, not a password. A change and a
/// self-service reset leave the same window, each pinned by its own test, and
/// since #697 (decision D-107 A) every OpenAPI 204 text of the three says so.
///
/// **The `200` on `/auth/me` pins a gap, not a guarantee.** If it ever fails,
/// the window has been closed and the set-password text can widen. Read the
/// failure as news, not as a regression.
///
/// # Seen to fail (coding standard 2.9)
///
/// 2026-10-16: a stand-in put in `Authenticated::from_request_parts`, after
/// the deletion check, refusing with `AppError::Unauthorized` a caller with no
/// unrevoked row in `refresh_tokens`. This test reddened on the target's
/// `200`. The stand-in was reverted, and it was green.
///
/// A second stand-in, refusing a token whose `iat` is before
/// `users.updated_at` (one-second grained), reddened this test in only some
/// runs: a login and a reset in the same second let the token through. The
/// sleep below closes that, and with it the stand-in reddened this test in
/// three runs of three (`test-engineer`, 2026-10-16).
#[tokio::test]
async fn setting_a_password_ends_every_refresh_token_but_not_an_issued_access_token() {
    let app = TestApp::spawn().await;
    let admin = app.administrator_token().await;

    let id = fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "reset.target",
        "reset.target@kelir.test",
        PASSWORD,
        &[],
    )
    .await;

    let signed_in = app
        .post(
            "/api/v1/auth/login",
            None,
            json!({ "username": "reset.target", "password": PASSWORD }),
        )
        .await;

    let refresh = signed_in.data()["refreshToken"]
        .as_str()
        .expect("refreshToken is a string")
        .to_owned();
    let access = signed_in.data()["accessToken"]
        .as_str()
        .expect("accessToken is a string")
        .to_owned();

    // The reset lands in a later second than the token's `iat`, so a check
    // with one-second granularity (`iat` before `users.updated_at`) would
    // refuse the token, and the `200` below would fail as news.
    tokio::time::sleep(std::time::Duration::from_millis(1_500)).await;

    let reset = app
        .post(
            &format!("/api/v1/identity/users/{id}/password"),
            Some(&admin),
            json!({ "password": "a different long password" }),
        )
        .await;

    assert_eq!(reset.status, StatusCode::NO_CONTENT, "{}", reset.body);

    // The target's access token, issued before the reset, still reads the
    // account.
    let me = app.get("/api/v1/auth/me", Some(&access)).await;

    assert_eq!(
        me.status,
        StatusCode::OK,
        "the target's pre-reset access token was refused — the window is closed, \
         so widen the set-password text in identity/handlers.rs and rewrite this \
         test; body {}",
        me.body
    );
    assert_eq!(
        me.body["data"]["id"],
        json!(id),
        "the pre-reset access token read somebody else's account"
    );

    let refreshed = app
        .post(
            "/api/v1/auth/refresh",
            None,
            json!({ "refreshToken": refresh }),
        )
        .await;

    assert_eq!(
        refreshed.status,
        StatusCode::UNAUTHORIZED,
        "the pre-reset refresh token still worked"
    );

    // And the new password is the one that works.
    let with_new = app
        .post(
            "/api/v1/auth/login",
            None,
            json!({ "username": "reset.target", "password": "a different long password" }),
        )
        .await;

    assert_eq!(with_new.status, StatusCode::OK, "{}", with_new.body);
}

/// **The tenant boundary, asserted from outside it** (#204).
///
/// Quarantined from Sprint 3 until 2026-08-27 on decision **D-7**, which
/// deferred per-request tenant resolution: sign-in resolved the configured
/// default tenant for every credential, so no token could carry another
/// tenant and the caller this test needs could not be arranged. **D-18**
/// superseded D-7 on 2026-08-25 — `resolve_for_sign_in` honours a requested
/// code on a multi-tenant deployment — so the condition the `#[ignore]` named
/// is met and the quarantine is gone.
///
/// **The caller holds `identity:user:read` deliberately, and taking that away
/// turns this into a different test.** Without it the route answers `403` at
/// the permission gate, before tenancy is consulted at all, and the assertion
/// below would be green while guarding nothing about tenants. The quarantined
/// body passed `&[]` and had exactly that defect: un-ignored as written it
/// would have read as tenant isolation and asserted the permission check.
///
/// Seen red (coding standard §2.9) against `find_user`'s `tenant_id = $1`
/// weakened to `(tenant_id = $1 OR TRUE)`: the foreign user's record comes
/// back `200` with its username and address in it.
#[tokio::test]
async fn reading_another_tenants_user_by_id_is_not_found() {
    // The tenant code has to reach sign-in, which needs the deployment mode
    // D-7 refused to let anything run in.
    let app = TestApp::spawn_with(|config| config.multi_tenant = true).await;

    let other_tenant = fixtures::create_tenant(&app.pool, "TNT-002", "Another Customer").await;
    let reader = fixtures::create_role_with_permissions(
        &app.pool,
        other_tenant,
        "OTHER-TENANT-READER",
        &["identity:user:read"],
    )
    .await;
    fixtures::create_user(
        &app.pool,
        other_tenant,
        "tenant.caller",
        "tenant.caller@other.test",
        PASSWORD,
        &[reader],
    )
    .await;

    let system_user = fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "system.subject",
        "system.subject@kelir.test",
        PASSWORD,
        &[],
    )
    .await;

    let token = app.sign_in_to("TNT-002", "tenant.caller", PASSWORD).await;

    let response = app
        .get(
            &format!("/api/v1/identity/users/{system_user}"),
            Some(&token),
        )
        .await;

    assert_eq!(
        response.status,
        StatusCode::NOT_FOUND,
        "another tenant's user must be invisible, not merely unreadable: {}",
        response.body
    );
}

/// **[#469], for a user's roles.** `replace_user_roles` has the same shape as
/// the role grant, and `uq_user_roles_user_id_role_id_department_id` refused a
/// repeated role the same way: 500. It is refused as a 422 at its own index,
/// on create with the other invalid fields and on update before anything is
/// written.
///
/// [#469]: https://github.com/sujanto-gaws/kelir/issues/469
#[tokio::test]
async fn a_repeated_role_id_is_refused_on_create_and_on_update() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let role: Uuid = sqlx::query_scalar("SELECT id FROM roles WHERE role_code = 'ROLE-ADMIN'")
        .fetch_one(&app.pool)
        .await
        .expect("the administrator role is seeded");

    let mut body = payload("repeated.roles");
    body["roleIds"] = json!([role.to_string(), role.to_string()]);

    let refused = app.post("/api/v1/identity/users", Some(&token), body).await;
    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(refused.body["error"]["details"][0]["path"], "roleIds.1");
    assert_eq!(
        refused.body["error"]["details"][0]["code"],
        "DUPLICATE_IN_ARRAY"
    );

    let left_behind: i64 =
        sqlx::query_scalar("SELECT count(*) FROM users WHERE username = 'repeated.roles'")
            .fetch_one(&app.pool)
            .await
            .expect("query runs");
    assert_eq!(
        left_behind, 0,
        "a refused create must not leave a user behind"
    );

    // The second subject: the same user with the role once is created.
    let mut once = payload("repeated.roles");
    once["roleIds"] = json!([role.to_string()]);
    let created = app.post("/api/v1/identity/users", Some(&token), once).await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    let id = user_id_of(&app, "repeated.roles").await;
    let refused_update = app
        .put(
            &format!("/api/v1/identity/users/{id}"),
            Some(&token),
            json!({ "roleIds": [role.to_string(), role.to_string()] }),
        )
        .await;
    assert_eq!(
        refused_update.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused_update.body
    );
    assert_eq!(
        refused_update.body["error"]["details"][0]["path"],
        "roleIds.1"
    );

    let grants: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM user_roles WHERE user_id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_one(&app.pool)
    .await
    .expect("query runs");
    assert_eq!(
        grants, 1,
        "a refused update must leave the one grant in place"
    );
}

/// The five code points #616 names. None is `char::is_whitespace`, so
/// `str::trim` kept each and a value of them alone was not blank.
const ZERO_WIDTH: [char; 5] = ['\u{200B}', '\u{200C}', '\u{200D}', '\u{2060}', '\u{FEFF}'];

/// `(path, rule, code)` of every detail in a 422 `VALIDATION_ERROR`, in order.
fn placed_details(response: &common::TestResponse) -> Vec<(String, String, String)> {
    assert_eq!(
        response.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        response.body
    );
    assert_eq!(response.error_code(), Some("VALIDATION_ERROR"));
    response.body["error"]["details"]
        .as_array()
        .expect("details is an array")
        .iter()
        .map(|detail| {
            let text = |key: &str| detail[key].as_str().unwrap_or_default().to_owned();
            (text("path"), text("rule"), text("code"))
        })
        .collect()
}

fn detail(path: &str, rule: &str, code: &str) -> (String, String, String) {
    (path.to_owned(), rule.to_owned(), code.to_owned())
}

/// An address of exactly `length` characters.
fn email_of(length: usize) -> String {
    let domain = "@kelir.test";
    format!("{}{domain}", "e".repeat(length - domain.len()))
}

async fn count_rows(app: &TestApp, sql: &str) -> i64 {
    sqlx::query_scalar(sql)
        .fetch_one(&app.pool)
        .await
        .expect("count runs")
}

/// Rows a refused create must not leave: users, their grants, and the
/// `User.*` audit events.
async fn written_rows(app: &TestApp) -> (i64, i64, i64) {
    (
        count_rows(app, "SELECT count(*) FROM users").await,
        count_rows(app, "SELECT count(*) FROM user_roles").await,
        count_rows(
            app,
            "SELECT count(*) FROM audit_events WHERE event_type LIKE 'User.%'",
        )
        .await,
    )
}

async fn stored_user(app: &TestApp, id: Uuid) -> (String, String, String) {
    sqlx::query_as("SELECT username, email, display_name FROM users WHERE id = $1")
        .bind(id)
        .fetch_one(&app.pool)
        .await
        .expect("the user exists")
}

/// **[#610]: a username over `users.username VARCHAR(64)` answered 500.** It
/// is a 422 at `username`, naming the bound, padded or not, and no user, no
/// grant and no audit event is written. The positive control is
/// `a_username_of_64_characters_is_created_and_stored_trimmed`.
///
/// [#610]: https://github.com/sujanto-gaws/kelir/issues/610
#[tokio::test]
async fn a_username_over_64_characters_is_refused_and_nothing_is_written() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let before = written_rows(&app).await;

    let long = "u".repeat(65);
    for username in [long.clone(), format!("  {long}  ")] {
        let mut body = payload("long.username");
        body["username"] = json!(username);
        body["roleIds"] = json!([fixtures::ADMIN_ROLE_ID.to_string()]);

        let refused = app.post("/api/v1/identity/users", Some(&token), body).await;
        assert_eq!(
            placed_details(&refused),
            [detail("username", "maxLength", "TOO_LONG")]
        );
        let message = refused.body["error"]["details"][0]["message"]
            .as_str()
            .expect("message");
        assert!(message.contains("64"), "{message}");
    }

    assert_eq!(
        written_rows(&app).await,
        before,
        "a refused create must write no user, no grant and no audit event"
    );
}

/// The positive control for #610's username: exactly 64 characters is
/// created, and 64 padded with whitespace is created and stored as the 64,
/// because the check counts what the store keeps.
#[tokio::test]
async fn a_username_of_64_characters_is_created_and_stored_trimmed() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    for (sent, stored) in [
        ("a".repeat(64), "a".repeat(64)),
        (format!("  {}  ", "b".repeat(64)), "b".repeat(64)),
    ] {
        let mut body = payload("placeholder");
        body["username"] = json!(sent);
        body["email"] = json!(format!("{}@kelir.test", &stored[..1]));

        let created = app.post("/api/v1/identity/users", Some(&token), body).await;
        assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

        let id = user_id_of(&app, &stored).await;
        assert_eq!(stored_user(&app, id).await.0, stored);
    }
}

/// **[#610]: `email` and `displayName` had no length check.** One over each
/// column is a 422 at the field and writes nothing; exactly at each column,
/// padded, is created and stored trimmed. A display name is counted in
/// characters: 200 of `é` is 400 bytes.
///
/// [#610]: https://github.com/sujanto-gaws/kelir/issues/610
#[tokio::test]
async fn an_email_or_display_name_over_its_column_is_refused_on_create() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let before = written_rows(&app).await;

    let mut long_email = payload("long.email");
    long_email["email"] = json!(email_of(321));
    assert_eq!(
        placed_details(
            &app.post("/api/v1/identity/users", Some(&token), long_email)
                .await
        ),
        [detail("email", "maxLength", "TOO_LONG")]
    );

    let mut long_name = payload("long.name");
    long_name["displayName"] = json!("é".repeat(201));
    assert_eq!(
        placed_details(
            &app.post("/api/v1/identity/users", Some(&token), long_name)
                .await
        ),
        [detail("displayName", "maxLength", "TOO_LONG")]
    );

    assert_eq!(written_rows(&app).await, before);

    let mut at_bounds = payload("at.bounds");
    at_bounds["email"] = json!(format!(" {} ", email_of(320)));
    at_bounds["displayName"] = json!(format!("\u{00A0}{}\u{00A0}", "é".repeat(200)));
    let created = app
        .post("/api/v1/identity/users", Some(&token), at_bounds)
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    let id = user_id_of(&app, "at.bounds").await;
    let (_, email, display_name) = stored_user(&app, id).await;
    assert_eq!(email, email_of(320));
    assert_eq!(display_name, "é".repeat(200));
}

/// **[#610]: one request, three long fields, three details** in field order,
/// as `creating_a_user_reports_every_invalid_field_at_once` holds for the
/// other refusals. A long username with a space in it is `TOO_LONG`, not
/// `INVALID_FORMAT`: blank, then length, then characters.
///
/// [#610]: https://github.com/sujanto-gaws/kelir/issues/610
#[tokio::test]
async fn every_long_user_field_is_reported_at_once_in_field_order() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let refused = app
        .post(
            "/api/v1/identity/users",
            Some(&token),
            json!({
                "username": format!("long name{}", "u".repeat(56)),
                "email": email_of(321),
                "password": PASSWORD,
                "displayName": "d".repeat(201),
            }),
        )
        .await;

    assert_eq!(
        placed_details(&refused),
        [
            detail("username", "maxLength", "TOO_LONG"),
            detail("email", "maxLength", "TOO_LONG"),
            detail("displayName", "maxLength", "TOO_LONG"),
        ]
    );
}

/// **[#610], the update route.** `PUT /identity/users/{id}` checked no field:
/// a blank display name was stored, and an over-long email or display name
/// answered 500. A present field is now held to the create rules, and a
/// refusal leaves the stored row as it was. An absent field is left alone,
/// and each bound exactly is accepted and stored trimmed.
///
/// [#610]: https://github.com/sujanto-gaws/kelir/issues/610
#[tokio::test]
async fn an_update_holds_a_present_email_and_display_name_to_the_create_rules() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let created = app
        .post("/api/v1/identity/users", Some(&token), payload("edited"))
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let id = user_id_of(&app, "edited").await;
    let original = stored_user(&app, id).await;
    let uri = format!("/api/v1/identity/users/{id}");

    let mut cases = vec![
        (
            json!({ "email": email_of(321) }),
            detail("email", "maxLength", "TOO_LONG"),
        ),
        (
            json!({ "email": "" }),
            detail("email", "format", "INVALID_FORMAT"),
        ),
        (
            json!({ "displayName": "" }),
            detail("displayName", "required", "REQUIRED"),
        ),
        (
            json!({ "displayName": " \t " }),
            detail("displayName", "required", "REQUIRED"),
        ),
        (
            json!({ "displayName": "d".repeat(201) }),
            detail("displayName", "maxLength", "TOO_LONG"),
        ),
    ];
    for c in ZERO_WIDTH {
        cases.push((
            json!({ "displayName": c.to_string() }),
            detail("displayName", "required", "REQUIRED"),
        ));
    }

    for (body, expected) in cases {
        let refused = app.put(&uri, Some(&token), body.clone()).await;
        assert_eq!(placed_details(&refused), [expected], "{body}");
        assert_eq!(
            stored_user(&app, id).await,
            original,
            "a refused update must leave the user as it was: {body}"
        );
    }

    let renamed = "é".repeat(200);
    let updated = app
        .put(
            &uri,
            Some(&token),
            json!({
                "email": format!(" {} ", email_of(320)),
                "displayName": format!("\u{3000}{renamed}\u{3000}"),
            }),
        )
        .await;
    assert_eq!(updated.status, StatusCode::OK, "{}", updated.body);
    assert_eq!(
        stored_user(&app, id).await,
        (original.0.clone(), email_of(320), renamed.clone())
    );

    // Absent fields are left as they are.
    let untouched = app
        .put(&uri, Some(&token), json!({ "status": "ACTIVE" }))
        .await;
    assert_eq!(untouched.status, StatusCode::OK, "{}", untouched.body);
    assert_eq!(
        stored_user(&app, id).await,
        (original.0, email_of(320), renamed)
    );
}

/// **[#616]: a username or display name of zero-width characters alone is
/// blank.** `str::trim` kept each of the five, so the username was refused
/// for its characters rather than as missing, and the display name was
/// stored. Each is now `REQUIRED` at its own path, alone, and nothing is
/// written.
///
/// [#616]: https://github.com/sujanto-gaws/kelir/issues/616
#[tokio::test]
async fn a_username_or_display_name_of_zero_width_characters_is_required() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let before = written_rows(&app).await;

    for c in ZERO_WIDTH {
        let mut blank_username = payload("zero.width");
        blank_username["username"] = json!(format!("{c}{c}"));
        assert_eq!(
            placed_details(
                &app.post("/api/v1/identity/users", Some(&token), blank_username)
                    .await
            ),
            [detail("username", "required", "REQUIRED")],
            "U+{:04X}",
            c as u32
        );

        let mut blank_name = payload("zero.width");
        blank_name["displayName"] = json!(format!(" {c} "));
        assert_eq!(
            placed_details(
                &app.post("/api/v1/identity/users", Some(&token), blank_name)
                    .await
            ),
            [detail("displayName", "required", "REQUIRED")],
            "U+{:04X}",
            c as u32
        );
    }

    assert_eq!(written_rows(&app).await, before);
}

/// **[#610] and [#616] on the tenant route.** A tenant's first administrator
/// is created by `provision_tenant_identity`, which runs the create-user rules
/// and reports them under `administrator.*`, the paths the tenant form has.
/// The long fields and the zero-width blanks are refused there too, and the
/// tenant is not created.
///
/// [#610]: https://github.com/sujanto-gaws/kelir/issues/610
/// [#616]: https://github.com/sujanto-gaws/kelir/issues/616
#[tokio::test]
async fn the_first_administrators_fields_are_bounded_under_its_own_paths() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let tenant = |administrator: serde_json::Value| {
        json!({
            "tenantCode": "BOUNDED",
            "name": "Bounded Limited",
            "administrator": administrator,
        })
    };

    let long = app
        .post(
            "/api/v1/organization/tenants",
            Some(&token),
            tenant(json!({
                "username": "a".repeat(65),
                "email": email_of(321),
                "displayName": "d".repeat(201),
                "password": PASSWORD,
            })),
        )
        .await;
    assert_eq!(
        placed_details(&long),
        [
            detail("administrator.username", "maxLength", "TOO_LONG"),
            detail("administrator.email", "maxLength", "TOO_LONG"),
            detail("administrator.displayName", "maxLength", "TOO_LONG"),
        ]
    );

    let blank = app
        .post(
            "/api/v1/organization/tenants",
            Some(&token),
            tenant(json!({
                "username": "\u{200B}",
                "email": "bounded@kelir.test",
                "displayName": "\u{FEFF}",
                "password": PASSWORD,
            })),
        )
        .await;
    assert_eq!(
        placed_details(&blank),
        [
            detail("administrator.username", "required", "REQUIRED"),
            detail("administrator.displayName", "required", "REQUIRED"),
        ]
    );

    let tenants: i64 =
        sqlx::query_scalar("SELECT count(*) FROM tenants WHERE tenant_code = 'BOUNDED'")
            .fetch_one(&app.pool)
            .await
            .expect("count runs");
    assert_eq!(tenants, 0, "a refused tenant must not be created");
}
