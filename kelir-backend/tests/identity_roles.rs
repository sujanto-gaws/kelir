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

/// **[#469]: a role naming one permission twice answered 500.**
///
/// `replace_role_permissions` inserted the second row into
/// `uq_role_permissions_role_id_permission_id`, and the unique violation came
/// back as `INTERNAL_ERROR`. It is now a 422 naming the repeat at its own
/// index, **checked before the transaction opens**, so a refused create leaves
/// no role behind and a refused update leaves the old grant in place.
///
/// **Two second subjects**: the same create without the repeat stores, and the
/// update's refusal is judged by the grant still being what it was, not by the
/// response alone.
///
/// [#469]: https://github.com/sujanto-gaws/kelir/issues/469
#[tokio::test]
async fn a_repeated_permission_id_is_refused_and_nothing_is_written() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let read = permission_id(&app, "identity:user:read").await;
    let create = permission_id(&app, "identity:user:create").await;

    // --- Create --------------------------------------------------------------
    let refused = app
        .post(
            "/api/v1/identity/roles",
            Some(&token),
            json!({
                "roleCode": "ROLE-REPEATED",
                "name": "Repeated",
                "permissionIds": [read.to_string(), create.to_string(), read.to_string()],
            }),
        )
        .await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(refused.error_code(), Some("VALIDATION_ERROR"));
    let details = refused.body["error"]["details"]
        .as_array()
        .expect("details");
    assert_eq!(details.len(), 1, "{}", refused.body);
    assert_eq!(details[0]["path"], "permissionIds.2");
    assert_eq!(details[0]["code"], "DUPLICATE_IN_ARRAY");
    assert!(
        details[0]["message"]
            .as_str()
            .is_some_and(|message| message.contains(&read.to_string())),
        "the detail names the repeated id: {}",
        details[0]
    );

    let left_behind: i64 =
        sqlx::query_scalar("SELECT count(*) FROM roles WHERE role_code = 'ROLE-REPEATED'")
            .fetch_one(&app.pool)
            .await
            .expect("query runs");
    assert_eq!(
        left_behind, 0,
        "a refused create must not leave a role behind"
    );

    let stored = app
        .post(
            "/api/v1/identity/roles",
            Some(&token),
            json!({
                "roleCode": "ROLE-REPEATED",
                "name": "Repeated",
                "permissionIds": [read.to_string(), create.to_string()],
            }),
        )
        .await;
    assert_eq!(stored.status, StatusCode::CREATED, "{}", stored.body);

    // --- Update --------------------------------------------------------------
    let id: Uuid = stored.data()["id"]
        .as_str()
        .expect("id is a string")
        .parse()
        .expect("id is a uuid");

    let refused_update = app
        .put(
            &format!("/api/v1/identity/roles/{id}"),
            Some(&token),
            json!({ "permissionIds": [create.to_string(), create.to_string()] }),
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
        "permissionIds.1"
    );
    assert_eq!(
        granted_codes(&app, id).await,
        vec![
            "identity:user:create".to_owned(),
            "identity:user:read".to_owned()
        ],
        "a refused update must leave the grant as it was"
    );
}

/// Asserts a 422 `VALIDATION_ERROR` whose only detail is `code` at `path`.
fn assert_refused_at(response: &common::TestResponse, path: &str, code: &str) {
    assert_eq!(
        response.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "expected a 422 at `{path}`, got {}: {}",
        response.status,
        response.body
    );
    assert_eq!(response.error_code(), Some("VALIDATION_ERROR"));
    let details = response.body["error"]["details"]
        .as_array()
        .expect("details");
    assert_eq!(details.len(), 1, "{}", response.body);
    assert_eq!(details[0]["path"], path, "{}", response.body);
    assert_eq!(details[0]["code"], code, "{}", response.body);
}

async fn roles_with_code(app: &TestApp, role_code: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM roles WHERE role_code = $1")
        .bind(role_code)
        .fetch_one(&app.pool)
        .await
        .expect("query runs")
}

async fn stored_name(app: &TestApp, role: Uuid) -> String {
    sqlx::query_scalar("SELECT name FROM roles WHERE id = $1")
        .bind(role)
        .fetch_one(&app.pool)
        .await
        .expect("query runs")
}

/// **[#575]: a role's code and name were never validated by the server.** An
/// empty code was stored, and one over the column's 64 characters reached
/// `VARCHAR(64)` and answered 500. Each is now a 422 naming the field, and
/// nothing is written. The positive control is
/// `a_role_code_and_name_at_their_column_lengths_are_accepted`.
///
/// [#575]: https://github.com/sujanto-gaws/kelir/issues/575
#[tokio::test]
async fn a_blank_or_over_long_role_code_or_name_is_refused_on_create() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    // (roleCode sent, name sent, path, code, roleCode a leak would store)
    let long_code = "R".repeat(65);
    let long_name = "N".repeat(201);
    let cases = [
        ("", "Empty Code", "roleCode", "REQUIRED", ""),
        ("   ", "Blank Code", "roleCode", "REQUIRED", ""),
        (
            long_code.as_str(),
            "Long Code",
            "roleCode",
            "TOO_LONG",
            long_code.as_str(),
        ),
        ("ROLE-EMPTY-NAME", "", "name", "REQUIRED", "ROLE-EMPTY-NAME"),
        (
            "ROLE-BLANK-NAME",
            " \t ",
            "name",
            "REQUIRED",
            "ROLE-BLANK-NAME",
        ),
        (
            "ROLE-LONG-NAME",
            long_name.as_str(),
            "name",
            "TOO_LONG",
            "ROLE-LONG-NAME",
        ),
    ];

    for (role_code, name, path, code, would_store) in cases {
        let response = app
            .post(
                "/api/v1/identity/roles",
                Some(&token),
                json!({ "roleCode": role_code, "name": name }),
            )
            .await;
        assert_refused_at(&response, path, code);
        assert_eq!(
            roles_with_code(&app, would_store).await,
            0,
            "a refused create must not leave a role behind: {role_code:?}, {name:?}"
        );
    }
}

/// The positive control for both routes: a code of exactly 64 characters and
/// a name of exactly 200, each padded with whitespace, are stored trimmed, and
/// an update to a 200-character name is accepted.
#[tokio::test]
async fn a_role_code_and_name_at_their_column_lengths_are_accepted() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let code = format!("ROLE-{}", "X".repeat(59));
    // Multibyte: `VARCHAR(200)` counts characters, and so must the check.
    let name = "é".repeat(200);
    assert_eq!(code.chars().count(), 64);

    let created = app
        .post(
            "/api/v1/identity/roles",
            Some(&token),
            json!({ "roleCode": format!("  {code}  "), "name": format!(" {name} ") }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    let id = role_id(&app, &code).await;
    assert_eq!(stored_name(&app, id).await, name);

    let renamed = "M".repeat(200);
    let updated = app
        .put(
            &format!("/api/v1/identity/roles/{id}"),
            Some(&token),
            json!({ "name": renamed }),
        )
        .await;
    assert_eq!(updated.status, StatusCode::OK, "{}", updated.body);
    assert_eq!(stored_name(&app, id).await, renamed);
}

/// **[#575], the update route.** A present `name` is held to the create rules,
/// so an edit cannot blank what a create could not, and an over-long one is a
/// 422, not the database's 500. The stored name judges each refusal.
///
/// [#575]: https://github.com/sujanto-gaws/kelir/issues/575
#[tokio::test]
async fn a_blank_or_over_long_role_name_is_refused_on_update() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let created = app
        .post(
            "/api/v1/identity/roles",
            Some(&token),
            json!({ "roleCode": "ROLE-RENAMED", "name": "Before" }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let id = role_id(&app, "ROLE-RENAMED").await;

    for (name, code) in [
        (String::new(), "REQUIRED"),
        ("   ".to_owned(), "REQUIRED"),
        ("N".repeat(201), "TOO_LONG"),
    ] {
        let response = app
            .put(
                &format!("/api/v1/identity/roles/{id}"),
                Some(&token),
                json!({ "name": name }),
            )
            .await;
        assert_refused_at(&response, "name", code);
        assert_eq!(
            stored_name(&app, id).await,
            "Before",
            "a refused update must leave the name as it was"
        );
    }
}

/// The `(path, rule, code)` of every detail in a 422, in the order sent.
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
        .expect("details")
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

/// **[#575], the envelope through the API.** One refused create reports a
/// blank code, an over-long name and a repeated permission id together, each
/// with the `path`, `rule` and `code` a form binds on; an update's blank name
/// is reported beside its repeated permission id, not skipped for it.
///
/// [#575]: https://github.com/sujanto-gaws/kelir/issues/575
#[tokio::test]
async fn a_refused_role_create_reports_every_problem_with_its_rule_and_code() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let read = permission_id(&app, "identity:user:read").await;

    let created = app
        .post(
            "/api/v1/identity/roles",
            Some(&token),
            json!({
                "roleCode": " \t ",
                "name": "N".repeat(201),
                "permissionIds": [read.to_string(), read.to_string()],
            }),
        )
        .await;
    assert_eq!(
        placed_details(&created),
        [
            detail("roleCode", "required", "REQUIRED"),
            detail("name", "maxLength", "TOO_LONG"),
            detail("permissionIds.1", "uniqueItems", "DUPLICATE_IN_ARRAY"),
        ]
    );

    let updated = app
        .put(
            &format!("/api/v1/identity/roles/{}", fixtures::ADMIN_ROLE_ID),
            Some(&token),
            json!({
                "name": " ",
                "permissionIds": [read.to_string(), read.to_string()],
            }),
        )
        .await;
    assert_eq!(
        placed_details(&updated),
        [
            detail("name", "required", "REQUIRED"),
            detail("permissionIds.1", "uniqueItems", "DUPLICATE_IN_ARRAY"),
        ]
    );
}

/// **[#575]: Unicode whitespace is whitespace.** A code or name of only
/// no-break or ideographic spaces is blank, and a value padded with them is
/// stored without them, on create and on update. The check and the store trim
/// alike, or a name the check passed at 200 characters would reach
/// `VARCHAR(200)` still padded.
///
/// [#575]: https://github.com/sujanto-gaws/kelir/issues/575
#[tokio::test]
async fn unicode_whitespace_is_blank_and_trimmed_from_a_role_code_and_name() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    for (role_code, name, path) in [
        ("\u{00A0}\u{3000}", "Only Spaces", "roleCode"),
        ("ROLE-NBSP-NAME", "\u{2003}\u{00A0}", "name"),
    ] {
        let response = app
            .post(
                "/api/v1/identity/roles",
                Some(&token),
                json!({ "roleCode": role_code, "name": name }),
            )
            .await;
        assert_refused_at(&response, path, "REQUIRED");
    }
    assert_eq!(roles_with_code(&app, "ROLE-NBSP-NAME").await, 0);

    let created = app
        .post(
            "/api/v1/identity/roles",
            Some(&token),
            json!({
                "roleCode": "\u{00A0}ROLE-PADDED\u{3000}",
                "name": "\u{2003}Padded\u{00A0}",
            }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let id = role_id(&app, "ROLE-PADDED").await;
    assert_eq!(stored_name(&app, id).await, "Padded");

    // 200 characters once trimmed: accepted, and stored trimmed rather than
    // sent to `VARCHAR(200)` at 202.
    let renamed = "M".repeat(200);
    let updated = app
        .put(
            &format!("/api/v1/identity/roles/{id}"),
            Some(&token),
            json!({ "name": format!("\u{00A0}{renamed}\u{3000}") }),
        )
        .await;
    assert_eq!(updated.status, StatusCode::OK, "{}", updated.body);
    assert_eq!(stored_name(&app, id).await, renamed);

    let blanked = app
        .put(
            &format!("/api/v1/identity/roles/{id}"),
            Some(&token),
            json!({ "name": "\u{3000}" }),
        )
        .await;
    assert_refused_at(&blanked, "name", "REQUIRED");
    assert_eq!(stored_name(&app, id).await, renamed);
}

/// **[#575]: a role's code is fixed once created.** An update that carries a
/// `roleCode`, even a blank one, is refused as an unknown field rather than
/// silently ignored; and an explicit `null` name leaves the name as a missing
/// one does.
///
/// [#575]: https://github.com/sujanto-gaws/kelir/issues/575
#[tokio::test]
async fn an_update_cannot_carry_a_role_code_and_a_null_name_is_left_alone() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let created = app
        .post(
            "/api/v1/identity/roles",
            Some(&token),
            json!({ "roleCode": "ROLE-FIXED", "name": "Fixed" }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let id = role_id(&app, "ROLE-FIXED").await;

    let carried = app
        .put(
            &format!("/api/v1/identity/roles/{id}"),
            Some(&token),
            json!({ "roleCode": "   ", "name": "Renamed" }),
        )
        .await;
    assert_refused_at(&carried, "roleCode", "UNKNOWN_FIELD");
    assert_eq!(stored_name(&app, id).await, "Fixed");
    assert_eq!(roles_with_code(&app, "ROLE-FIXED").await, 1);

    let nulled = app
        .put(
            &format!("/api/v1/identity/roles/{id}"),
            Some(&token),
            json!({ "name": null, "description": "Described" }),
        )
        .await;
    assert_eq!(nulled.status, StatusCode::OK, "{}", nulled.body);
    assert_eq!(stored_name(&app, id).await, "Fixed");
}

/// **[#575]: authorization is decided before the body is read.** A caller
/// holding `identity:role:read` but neither `identity:role:create` nor
/// `identity:role:update` is answered 403, never a 422 whose details describe
/// the rules to somebody who may not use the route.
///
/// [#575]: https://github.com/sujanto-gaws/kelir/issues/575
#[tokio::test]
async fn a_caller_without_the_permission_is_refused_before_the_body_is_validated() {
    let app = TestApp::spawn().await;

    let reader = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "ROLE-ROLE-READER",
        &["identity:role:read"],
    )
    .await;
    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "role.reader",
        "role.reader@kelir.test",
        PASSWORD,
        &[reader],
    )
    .await;
    let token = app.sign_in("role.reader", PASSWORD).await;

    let created = app
        .post(
            "/api/v1/identity/roles",
            Some(&token),
            json!({ "roleCode": " ", "name": "N".repeat(201) }),
        )
        .await;
    let updated = app
        .put(
            &format!("/api/v1/identity/roles/{reader}"),
            Some(&token),
            json!({ "name": "" }),
        )
        .await;

    for (route, response) in [("POST", &created), ("PUT", &updated)] {
        assert_eq!(
            response.status,
            StatusCode::FORBIDDEN,
            "{route} /identity/roles answered {} to a caller without the permission: {}",
            response.status,
            response.body
        );
        assert_eq!(response.error_code(), Some("FORBIDDEN"));
        assert!(
            response.body["error"]["details"]
                .as_array()
                .is_none_or(Vec::is_empty),
            "a 403 must not carry validation details: {}",
            response.body
        );
    }
}
