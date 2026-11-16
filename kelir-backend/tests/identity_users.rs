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

/// **[#610]: an update is refused for its body before the user is read.**
/// `update_user` runs `validate_update_user` before `find_user`, and the 422
/// description says "checked before the user is read": a body that would be
/// refused, sent to an id that names no user, is a 422 for the body and not a
/// 404 for the id. The control is the same id with a valid body: 404.
///
/// [#610]: https://github.com/sujanto-gaws/kelir/issues/610
#[tokio::test]
async fn an_invalid_update_is_refused_before_the_user_is_looked_up() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let uri = format!("/api/v1/identity/users/{}", Uuid::now_v7());

    let refused = app
        .put(
            &uri,
            Some(&token),
            json!({ "email": email_of(321), "displayName": "\u{200B}" }),
        )
        .await;
    assert_eq!(
        placed_details(&refused),
        [
            detail("email", "maxLength", "TOO_LONG"),
            detail("displayName", "required", "REQUIRED"),
        ]
    );

    let missing = app
        .put(&uri, Some(&token), json!({ "displayName": "Renamed" }))
        .await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND, "{}", missing.body);
}

// ---------------------------------------------------------------------------
// #795: a user edit audits the stored row
// ---------------------------------------------------------------------------

/// A department in the administrator's tenant, for a user to be placed in.
async fn department(app: &TestApp, code: &str) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO departments (id, tenant_id, department_code, name) VALUES ($1, $2, $3, $3)",
    )
    .bind(id)
    .bind(fixtures::SYSTEM_TENANT_ID)
    .bind(code)
    .execute(&app.pool)
    .await
    .expect("insert the department");
    id
}

/// An empty role in the administrator's tenant, to be granted.
async fn grantable_role(app: &TestApp, code: &str) -> Uuid {
    fixtures::create_role_with_permissions(&app.pool, fixtures::SYSTEM_TENANT_ID, code, &[]).await
}

/// The five fields a `User.Updated` half holds, as the stored row has them.
/// `roleIds` is compared as a set: [`normalized`] sorts both sides.
fn user_row(
    email: &str,
    display_name: &str,
    status: &str,
    department_id: Option<Uuid>,
    role_ids: &[Uuid],
) -> serde_json::Value {
    normalized(json!({
        "email": email,
        "displayName": display_name,
        "status": status,
        "departmentId": department_id,
        "roleIds": role_ids,
    }))
}

/// `value` with its `roleIds` sorted, so the order grants are read in does
/// not decide the comparison.
fn normalized(mut value: serde_json::Value) -> serde_json::Value {
    if let Some(roles) = value.get_mut("roleIds").and_then(|r| r.as_array_mut()) {
        roles.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
    }
    value
}

/// Every `User.Updated` row for `id`, oldest first, as `(old, new)`.
async fn user_updates(app: &TestApp, id: Uuid) -> Vec<(serde_json::Value, serde_json::Value)> {
    let rows: Vec<(Option<serde_json::Value>, Option<serde_json::Value>)> = sqlx::query_as(
        "SELECT old_value_json, new_value_json FROM audit_events
         WHERE event_type = 'User.Updated' AND object_id = $1
         ORDER BY created_at, id",
    )
    .bind(id)
    .fetch_all(&app.pool)
    .await
    .expect("read the audit rows");
    rows.into_iter()
        .map(|(old, new)| {
            (
                normalized(old.unwrap_or_default()),
                normalized(new.unwrap_or_default()),
            )
        })
        .collect()
}

/// PUTs `body` to the user and returns the `User.Updated` row it wrote,
/// asserting it wrote exactly one.
async fn edited(
    app: &TestApp,
    token: &str,
    id: Uuid,
    body: serde_json::Value,
) -> (serde_json::Value, serde_json::Value) {
    let before = user_updates(app, id).await.len();
    let response = app
        .put(
            &format!("/api/v1/identity/users/{id}"),
            Some(token),
            body.clone(),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{body}: {}", response.body);
    let mut rows = user_updates(app, id).await;
    assert_eq!(
        rows.len(),
        before + 1,
        "an edit writes one User.Updated row: {body}"
    );
    rows.pop().expect("the row just counted")
}

/// **[#795]: a partial edit audits the stored row, before and after.** Both
/// halves hold the five fields an edit can change, as the row has them: a
/// field the edit did not send is its stored value, not `null`, and a value
/// the edit sent padded is audited as stored, trimmed. A `new_value` built
/// from the request fails each edit here: it holds `null` for the unsent
/// fields, the untrimmed value, and no department or roles.
///
/// [#795]: https://github.com/sujanto-gaws/kelir/issues/795
#[tokio::test]
async fn a_partial_user_edit_audits_the_stored_row_before_and_after() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let place = department(&app, "AUD-795-PLACE").await;
    let role = grantable_role(&app, "ROLE-AUD-795-HELD").await;

    let mut body = payload("audited.edit");
    body["departmentId"] = json!(place);
    body["roleIds"] = json!([role]);
    let created = app.post("/api/v1/identity/users", Some(&token), body).await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let id = user_id_of(&app, "audited.edit").await;
    let email = "audited.edit@kelir.test";

    let (old, new) = edited(
        &app,
        &token,
        id,
        json!({ "displayName": "  Renamed Person  " }),
    )
    .await;
    assert_eq!(
        old,
        user_row(email, "Test Person", "ACTIVE", Some(place), &[role])
    );
    assert_eq!(
        new,
        user_row(email, "Renamed Person", "ACTIVE", Some(place), &[role]),
        "new_value is the stored row: the unsent fields as they are, the name trimmed"
    );

    let (old, new) = edited(
        &app,
        &token,
        id,
        json!({ "email": " moved@kelir.test ", "status": "LOCKED" }),
    )
    .await;
    assert_eq!(
        old,
        user_row(email, "Renamed Person", "ACTIVE", Some(place), &[role]),
        "old_value is the row the previous edit stored"
    );
    assert_eq!(
        new,
        user_row(
            "moved@kelir.test",
            "Renamed Person",
            "LOCKED",
            Some(place),
            &[role]
        )
    );
}

/// **[#795]: placing a user in a department, taking them out of it and
/// replacing their roles each audit.** None of the three was in the
/// `User.Updated` row before, so a role grant left no trace in the trail,
/// unlike a role's own permission change (FR-AUD-002).
///
/// [#795]: https://github.com/sujanto-gaws/kelir/issues/795
#[tokio::test]
async fn a_department_set_and_cleared_and_roles_replaced_each_audit() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let place = department(&app, "AUD-795-DEPT").await;
    let first = grantable_role(&app, "ROLE-AUD-795-FIRST").await;
    let second = grantable_role(&app, "ROLE-AUD-795-SECOND").await;
    let third = grantable_role(&app, "ROLE-AUD-795-THIRD").await;

    let mut body = payload("audited.placement");
    body["roleIds"] = json!([first]);
    let created = app.post("/api/v1/identity/users", Some(&token), body).await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let id = user_id_of(&app, "audited.placement").await;
    let email = "audited.placement@kelir.test";
    let row = |department: Option<Uuid>, roles: &[Uuid]| {
        user_row(email, "Test Person", "ACTIVE", department, roles)
    };

    let set = edited(&app, &token, id, json!({ "departmentId": place })).await;
    assert_eq!(
        set,
        (row(None, &[first]), row(Some(place), &[first])),
        "setting a department is audited"
    );

    let cleared = edited(&app, &token, id, json!({ "departmentId": null })).await;
    assert_eq!(
        cleared,
        (row(Some(place), &[first]), row(None, &[first])),
        "clearing a department is audited"
    );

    let replaced = edited(&app, &token, id, json!({ "roleIds": [second, third] })).await;
    assert_eq!(
        replaced,
        (row(None, &[first]), row(None, &[second, third])),
        "replacing the roles is audited"
    );

    let emptied = edited(&app, &token, id, json!({ "roleIds": [] })).await;
    assert_eq!(
        emptied,
        (row(None, &[second, third]), row(None, &[])),
        "taking every role away is audited"
    );
}

/// **[#795]: a user's creation audits the row it stored.** `User.Created`'s
/// `new_value` held the request: the address untrimmed, and no display
/// name, status or department. It is now the same five fields `User.Updated`
/// holds, as stored, with the username. A `new_value` built from the request
/// again fails here on the padded address and name and on the department.
///
/// [#795]: https://github.com/sujanto-gaws/kelir/issues/795
#[tokio::test]
async fn a_user_creation_audits_the_stored_row() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let place = department(&app, "AUD-795-CREATE").await;
    let first = grantable_role(&app, "ROLE-AUD-795-CREATE-A").await;
    let second = grantable_role(&app, "ROLE-AUD-795-CREATE-B").await;

    let mut body = payload("audited.create");
    body["email"] = json!("  audited.create@kelir.test  ");
    body["displayName"] = json!("\u{3000}Created Person\u{3000}");
    body["departmentId"] = json!(place);
    body["roleIds"] = json!([second, first]);
    let created = app.post("/api/v1/identity/users", Some(&token), body).await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let id = user_id_of(&app, "audited.create").await;

    let rows: Vec<(Option<serde_json::Value>, Option<serde_json::Value>)> = sqlx::query_as(
        "SELECT old_value_json, new_value_json FROM audit_events
         WHERE event_type = 'User.Created' AND object_id = $1",
    )
    .bind(id)
    .fetch_all(&app.pool)
    .await
    .expect("read the audit rows");
    assert_eq!(rows.len(), 1, "a creation writes one User.Created row");
    let (old, new) = rows.into_iter().next().expect("the row just counted");

    let mut expected = user_row(
        "audited.create@kelir.test",
        "Created Person",
        "ACTIVE",
        Some(place),
        &[first, second],
    );
    expected["username"] = json!("audited.create");
    assert_eq!(old, None, "a creation has no before");
    assert_eq!(
        normalized(new.unwrap_or_default()),
        expected,
        "new_value is the stored row: trimmed, with its department and roles"
    );
}

// ---------------------------------------------------------------------------
// #795, the independent pass: races, refusals and the order roles are read in
// ---------------------------------------------------------------------------

/// How many requests wait on a row lock of `users` now.
async fn waiting_on_users(app: &TestApp) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM pg_stat_activity
         WHERE datname = current_database() AND wait_event_type = 'Lock'
           AND query ~* '\\musers\\M'",
    )
    .fetch_one(&app.pool)
    .await
    .expect("read pg_stat_activity")
}

/// A user created in `place` with `roles`, by its username.
async fn placed_user(app: &TestApp, token: &str, name: &str, place: Uuid, roles: &[Uuid]) -> Uuid {
    let mut body = payload(name);
    body["departmentId"] = json!(place);
    body["roleIds"] = json!(roles);
    let created = app.post("/api/v1/identity/users", Some(token), body).await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    user_id_of(app, name).await
}

/// `token`'s PUT of `body` to the user, spawned so that it can wait on a lock.
fn spawn_edit(
    app: &std::sync::Arc<TestApp>,
    token: &str,
    id: Uuid,
    body: serde_json::Value,
) -> tokio::task::JoinHandle<common::TestResponse> {
    let (app, token) = (std::sync::Arc::clone(app), token.to_owned());
    tokio::spawn(async move {
        let uri = format!("/api/v1/identity/users/{id}");
        app.put(&uri, Some(&token), body).await
    })
}

/// **#795 (a): two edits of one user at once audit one history.** The test
/// holds the user's row `FOR SHARE`, which both edits' locks conflict with,
/// until both wait on it. The edit that goes first audits the original row as
/// its "before"; the other audits exactly the row the first committed, never
/// the original and never a mix. A "before" read ahead of the lock, as it was
/// until #795, gives both edits the original.
///
/// The chain is not asserted: `audit::record` reads the tenant's last hash
/// with no lock, so two audit writes at once can link to the same row. That
/// is not #795's, and is reported with this pass.
#[tokio::test]
async fn two_edits_of_one_user_at_once_audit_one_history() {
    let app = std::sync::Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;
    let place = department(&app, "AUD-795-RACE").await;
    let first = grantable_role(&app, "ROLE-AUD-795-RACE-A").await;
    let second = grantable_role(&app, "ROLE-AUD-795-RACE-B").await;
    let id = placed_user(&app, &token, "audited.race", place, &[first]).await;

    let mut holder = app.pool.begin().await.expect("begin the holder");
    sqlx::query("SELECT 1 FROM users WHERE id = $1 FOR SHARE")
        .bind(id)
        .execute(&mut *holder)
        .await
        .expect("hold the user's row");
    let edits = [
        spawn_edit(&app, &token, id, json!({ "displayName": "Raced Name" })),
        spawn_edit(
            &app,
            &token,
            id,
            json!({ "email": "raced@kelir.test", "roleIds": [second] }),
        ),
    ];
    let deadline = tokio::time::Instant::now() + common::gate::PATIENCE;
    while waiting_on_users(&app).await < 2 {
        assert!(
            tokio::time::Instant::now() < deadline,
            "both edits should wait on the row"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    holder.rollback().await.expect("release the user's row");
    for edit in edits {
        let answered = tokio::time::timeout(common::gate::PATIENCE, edit)
            .await
            .expect("an edit never answered")
            .expect("an edit panicked");
        assert_eq!(answered.status, StatusCode::OK, "{}", answered.body);
    }

    let original = user_row(
        "audited.race@kelir.test",
        "Test Person",
        "ACTIVE",
        Some(place),
        &[first],
    );
    let rows = user_updates(&app, id).await;
    assert_eq!(rows.len(), 2, "each edit writes one User.Updated row");
    let earlier = rows
        .iter()
        .position(|(old, _)| *old == original)
        .expect("one edit's before is the original row");
    let (later_old, later_new) = &rows[1 - earlier];
    assert_eq!(
        later_old, &rows[earlier].1,
        "the later before is the earlier after"
    );
    assert_eq!(
        later_new,
        &user_row(
            "raced@kelir.test",
            "Raced Name",
            "ACTIVE",
            Some(place),
            &[second]
        ),
        "the later after holds both edits"
    );
}

/// **#795 (g): the edit's lock does not wait on a write the user is making.**
/// A write by the user stamps them as its actor, and the stamp's foreign key
/// holds their row `FOR KEY SHARE` until it commits. `FOR NO KEY UPDATE` does
/// not conflict with that; `FOR UPDATE` would (record 23 §5.1).
#[tokio::test]
async fn editing_a_user_does_not_wait_on_a_write_the_user_is_making() {
    let app = std::sync::Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;
    let place = department(&app, "AUD-795-KEYSHARE").await;
    let id = placed_user(&app, &token, "audited.keyshare", place, &[]).await;

    let mut stamping = app.pool.begin().await.expect("begin the user's write");
    sqlx::query("SELECT 1 FROM users WHERE id = $1 FOR KEY SHARE")
        .bind(id)
        .execute(&mut *stamping)
        .await
        .expect("hold the row as a stamping key does");
    let edit = spawn_edit(&app, &token, id, json!({ "displayName": "Not Held" }));
    let deadline = tokio::time::Instant::now() + common::gate::PATIENCE;
    while !edit.is_finished() {
        assert_eq!(
            waiting_on_users(&app).await,
            0,
            "the edit waits on the user's write"
        );
        assert!(
            tokio::time::Instant::now() < deadline,
            "the edit never answered"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let answered = edit.await.expect("the edit panicked");
    assert_eq!(answered.status, StatusCode::OK, "{}", answered.body);
    stamping.rollback().await.expect("end the user's write");
}

/// **#795 (b), (c) and (f): what an edit that changes nothing, a refused edit
/// and a refused creation audit.** An edit that sends nothing, or every field
/// at its stored value, writes one row with equal halves; the catalogue does
/// not say so, and this pins today's behaviour. An unknown department, an
/// unknown role, a repeated role or another user's address is refused with
/// the address change beside it undone and no row written. A creation refused
/// for a taken username or address, or an unknown role, writes no
/// `User.Created` row and no user.
#[tokio::test]
async fn a_user_edit_that_changes_nothing_or_is_refused_audits_as_stored() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let place = department(&app, "AUD-795-REFUSED").await;
    let role = grantable_role(&app, "ROLE-AUD-795-REFUSED").await;
    let id = placed_user(&app, &token, "audited.refused", place, &[role]).await;
    placed_user(&app, &token, "audited.other", place, &[]).await;
    let email = "audited.refused@kelir.test";
    let stored = user_row(email, "Test Person", "ACTIVE", Some(place), &[role]);

    let unknown = Uuid::now_v7();
    for body in [
        json!({ "email": "moved@kelir.test", "departmentId": unknown }),
        json!({ "email": "moved@kelir.test", "roleIds": [unknown] }),
        json!({ "email": "moved@kelir.test", "roleIds": [role, role] }),
        json!({ "email": "audited.other@kelir.test", "displayName": "Moved" }),
    ] {
        let uri = format!("/api/v1/identity/users/{id}");
        let refused = app.put(&uri, Some(&token), body.clone()).await;
        assert!(!refused.status.is_success(), "{body}: {}", refused.body);
        assert!(
            user_updates(&app, id).await.is_empty(),
            "{body} was audited"
        );
    }

    // The empty edit's "before" is the stored row: none of the above changed it.
    let empty = edited(&app, &token, id, json!({})).await;
    assert_eq!(empty, (stored.clone(), stored.clone()), "an empty edit");
    let same = json!({
        "email": email, "displayName": "Test Person", "status": "ACTIVE",
        "departmentId": place, "roleIds": [role],
    });
    let resent = edited(&app, &token, id, same).await;
    assert_eq!(
        resent,
        (stored.clone(), stored),
        "every field at its stored value"
    );

    let created = "SELECT count(*) FROM audit_events WHERE event_type = 'User.Created'";
    let (audited, users) = (
        count_rows(&app, created).await,
        count_rows(&app, "SELECT count(*) FROM users").await,
    );
    let mut taken_name = payload("audited.refused");
    taken_name["email"] = json!("fresh@kelir.test");
    let mut taken_email = payload("audited.fresh");
    taken_email["email"] = json!(format!(" {email} "));
    let mut unknown_role = payload("audited.fresher");
    unknown_role["roleIds"] = json!([unknown]);
    for body in [taken_name, taken_email, unknown_role] {
        let refused = app
            .post("/api/v1/identity/users", Some(&token), body.clone())
            .await;
        assert!(!refused.status.is_success(), "{body}: {}", refused.body);
    }
    assert_eq!(
        count_rows(&app, created).await,
        audited,
        "a refused creation was audited"
    );
    assert_eq!(count_rows(&app, "SELECT count(*) FROM users").await, users);
}

/// **#795 (d): `roleIds` is the stored set in role-code order.** The roles are
/// made so that their code order, their id order and the request's order all
/// differ. A `roleIds` copied from the request, even merged over the stored
/// row and trimmed, fails here; the builder's tests sort both sides.
#[tokio::test]
async fn the_audited_roles_are_the_stored_set_in_role_code_order() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let c = grantable_role(&app, "ROLE-AUD-795-ORD-C").await;
    let b = grantable_role(&app, "ROLE-AUD-795-ORD-B").await;
    let a = grantable_role(&app, "ROLE-AUD-795-ORD-A").await;
    let place = department(&app, "AUD-795-ORDER").await;
    let id = placed_user(&app, &token, "audited.order", place, &[b, c, a]).await;
    edited(&app, &token, id, json!({ "roleIds": [c, a] })).await;

    let halves: Vec<(Option<serde_json::Value>, serde_json::Value)> = sqlx::query_as(
        "SELECT old_value_json -> 'roleIds', new_value_json -> 'roleIds' FROM audit_events
         WHERE object_id = $1 AND event_type IN ('User.Created', 'User.Updated')
         ORDER BY created_at, id",
    )
    .bind(id)
    .fetch_all(&app.pool)
    .await
    .expect("read the audit rows");
    assert_eq!(
        halves,
        [
            (None, json!([a, b, c])),
            (Some(json!([a, b, c])), json!([a, c]))
        ]
    );
}
