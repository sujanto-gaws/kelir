//! Six list endpoints take `search` and `status`, so a chooser reaches a row
//! that sorts past the first hundred
//! ([#525](https://github.com/sujanto-gaws/kelir/issues/525), Sprint 21 plan 17
//! row 18).
//!
//! The endpoints are `GET /document-types`, `/rad/forms`, `/rad/lists`,
//! `/workflow/definitions`, `/identity/users` and `/identity/roles`. Each test
//! below seeds **105 rows that sort ahead of the wanted ones**, which is the
//! wall #521's audit hit, and then holds the contract the frontend relies on:
//!
//! * `search` is a case-insensitive substring of the key and the name (for a
//!   user: the username, the email and the display name);
//! * `%`, `_` and `\` in a search match themselves — `ZZ_SEEK` does not find
//!   `ZZXSEEK`;
//! * `status` is an exact match, and a value outside the vocabulary is a 422
//!   naming `status` (a role has no status, so there it is ignored like any
//!   unknown parameter);
//! * `meta.total` counts the rows matching **both**;
//! * a blank search is no search, and the order is by key as before;
//! * a soft-deleted user stays out, whatever the search.
//!
//! Every list shares one escaping rule, `utils::search::like_contains`.
//!
//! # Seen to fail (coding standard §2.9)
//!
//! Each mutation below was made on the line named, the suite run, the test
//! observed red, and the mutation reverted. **Seen red, 2026-09-29.**
//!
//! | Mutation | Reddened |
//! |---|---|
//! | `utils::search::like_contains` stops escaping `_` | all six `*_past_the_first_hundred` |
//! | `OR name ILIKE $2` dropped from `document_type::repository::list_types` | `a_document_type_is_found_past_the_first_hundred` |
//! | `AND ($3::text IS NULL OR status = $3)` dropped from `rad::repository::form::count_forms` | `a_form_is_found_past_the_first_hundred` |
//! | `rad::repository::list::list_lists` binds `format!("%{term}%")` instead of `like_contains` | `a_list_is_found_past_the_first_hundred` |
//! | `workflow::service::definition::list_definitions` passes `query.search` untrimmed instead of through `search_term` | `a_workflow_definition_is_found_past_the_first_hundred` |
//! | `OR email ILIKE $2` dropped from `identity::repository::list_users` and `count_users` | `a_user_is_found_past_the_first_hundred` |
//! | `AND deleted_at IS NULL` dropped from `identity::repository::count_users` | `a_user_is_found_past_the_first_hundred` |
//! | `identity::service::list_roles` passes `None` to `repo::list_roles` (the count still searches) | `a_role_is_found_past_the_first_hundred` |
//! | `params(FormQuery)` put back to `params(Pagination)` on `rad::handlers::list_forms` | `each_list_documents_its_search_and_status` |
//!
//! The test-engineer gate added the last four tests and the padded search in
//! `holds_the_contract`, for mutations that survived the table above. **Seen
//! red, 2026-09-29.**
//!
//! | Mutation | Reddened |
//! |---|---|
//! | `tenant_id = $1` dropped from `workflow::repository::definition::list_definitions` | `a_search_never_reaches_another_tenants_rows` |
//! | `tenant_id = $1` dropped from `identity::repository::count_roles`, and from `rad::repository::list::count_lists` | `a_search_never_reaches_another_tenants_rows` |
//! | `AND deleted_at IS NULL` dropped from `identity::repository::list_roles` | `a_search_never_reaches_a_soft_deleted_row` |
//! | `utils::search::like_contains` stops escaping `%`, or `\` | `a_percent_or_a_backslash_in_a_search_matches_itself` |
//! | `utils::search::search_term` stops trimming but still treats blank as no search | all six `*_past_the_first_hundred` |

mod common;

use axum::http::{Method, StatusCode};
use common::{TestApp, TestResponse, ADMIN_USERNAME};
use serde_json::Value;
use uuid::Uuid;

/// Rows seeded ahead of the wanted ones: more than a chooser's one page.
const FILLERS: i32 = 105;

/// The search every test runs. Lower case, against upper-case keys, so the
/// match is case-insensitive or it is nothing; and `_`, so `ZZXSEEK` is the
/// row a wildcard would wrongly find.
const SEARCH: &str = "zz_seek";

async fn tenant_of_administrator(app: &TestApp) -> Uuid {
    sqlx::query_scalar("SELECT tenant_id FROM users WHERE username = $1")
        .bind(ADMIN_USERNAME)
        .fetch_one(&app.pool)
        .await
        .expect("the administrator's tenant")
}

async fn get(app: &TestApp, token: &str, url: &str) -> TestResponse {
    app.send(Method::GET, url, Some(token), None).await
}

/// The values of `field` on the page, in the order the server gave them.
fn column(response: &TestResponse, field: &str) -> Vec<String> {
    response
        .data()
        .as_array()
        .unwrap_or_else(|| panic!("a list response: {}", response.body))
        .iter()
        .filter_map(|row| row[field].as_str().map(str::to_owned))
        .collect()
}

fn total(response: &TestResponse) -> u64 {
    response.body["meta"]["total"]
        .as_u64()
        .unwrap_or_else(|| panic!("a meta.total: {}", response.body))
}

async fn execute(app: &TestApp, sql: &str, tenant_id: Uuid) {
    sqlx::query(sql)
        .bind(tenant_id)
        .execute(&app.pool)
        .await
        .unwrap_or_else(|error| panic!("seed: {error}\n{sql}"));
}

/// The contract every endpoint shares, given one seeded as the tests below
/// seed it: 105 fillers in `active` sorting first, then `ZZ_NAMED` (found by
/// its name) and `ZZ_SEEK_1` in `active`, `ZZ_SEEK_2` in `other`, and
/// `ZZXSEEK` (found only if `_` is a wildcard) in `active`.
///
/// `key` is the response field holding the key. Keys are compared upper-cased,
/// because a username is stored in lower case.
async fn holds_the_contract(
    app: &TestApp,
    token: &str,
    base: &str,
    key: &str,
    statuses: Option<(&str, &str)>,
) -> TestResponse {
    let upper = |values: Vec<String>| -> Vec<String> {
        values.into_iter().map(|v| v.to_uppercase()).collect()
    };

    // The wall: the first page, without a search, does not reach the rows.
    let unfiltered = get(app, token, &format!("{base}?pageSize=100")).await;
    assert_eq!(unfiltered.status, StatusCode::OK, "{}", unfiltered.body);
    assert!(total(&unfiltered) > 100, "{}", unfiltered.body);
    assert!(
        !upper(column(&unfiltered, key)).contains(&"ZZ_SEEK_1".to_owned()),
        "the seeded rows did not sort past the first page"
    );

    // The search reaches them, by key and by name, in key order, and `_` is
    // a literal: ZZXSEEK is not among them. `page` is absent and means 1.
    let searched = get(app, token, &format!("{base}?search={SEARCH}&pageSize=100")).await;
    assert_eq!(searched.status, StatusCode::OK, "{}", searched.body);
    assert_eq!(
        upper(column(&searched, key)),
        ["ZZ_NAMED", "ZZ_SEEK_1", "ZZ_SEEK_2"],
        "{}",
        searched.body
    );
    assert_eq!(total(&searched), 3, "{}", searched.body);
    assert_eq!(searched.body["meta"]["page"], 1);

    // A blank search is no search.
    let blank = get(app, token, &format!("{base}?search=%20%20&pageSize=100")).await;
    assert_eq!(blank.status, StatusCode::OK, "{}", blank.body);
    assert_eq!(total(&blank), total(&unfiltered), "{}", blank.body);

    // A padded search is the search: the trim reaches every list, not only
    // the helper's unit test (test-engineer gate).
    let padded = get(
        app,
        token,
        &format!("{base}?search=%20{SEARCH}%20&pageSize=100"),
    )
    .await;
    assert_eq!(total(&padded), 3, "padded search: {}", padded.body);

    if let Some((active, other)) = statuses {
        // meta.total counts the rows matching both, not either.
        let narrowed = get(
            app,
            token,
            &format!("{base}?search={SEARCH}&status={active}&pageSize=100"),
        )
        .await;
        assert_eq!(narrowed.status, StatusCode::OK, "{}", narrowed.body);
        assert_eq!(
            upper(column(&narrowed, key)),
            ["ZZ_NAMED", "ZZ_SEEK_1"],
            "{}",
            narrowed.body
        );
        assert_eq!(total(&narrowed), 2, "{}", narrowed.body);

        let other_status = get(
            app,
            token,
            &format!("{base}?search={SEARCH}&status={other}"),
        )
        .await;
        assert_eq!(upper(column(&other_status, key)), ["ZZ_SEEK_2"]);
        assert_eq!(total(&other_status), 1);

        // The status alone counts every filler as well: it is a filter, not
        // a search.
        let status_only = get(app, token, &format!("{base}?status={active}")).await;
        assert!(total(&status_only) >= u64::try_from(FILLERS).unwrap() + 3);

        let bad = get(app, token, &format!("{base}?status=BOGUS")).await;
        assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY, "{}", bad.body);
        assert_eq!(bad.body["error"]["details"][0]["path"], "status");
    }

    searched
}

/// The id of the row whose key is `wanted`, from a search response.
fn id_of(response: &TestResponse, key: &str, wanted: &str) -> String {
    response
        .data()
        .as_array()
        .into_iter()
        .flatten()
        .find(|row| row[key].as_str().map(str::to_uppercase).as_deref() == Some(wanted))
        .and_then(|row| row["id"].as_str())
        .unwrap_or_else(|| panic!("{wanted} in {}", response.body))
        .to_owned()
}

#[tokio::test]
async fn a_document_type_is_found_past_the_first_hundred() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let tenant_id = tenant_of_administrator(&app).await;

    execute(
        &app,
        &format!(
            "INSERT INTO document_types (id, tenant_id, type_code, name, status)
             SELECT gen_random_uuid(), $1, 'AA_FILL_' || lpad(i::text, 3, '0'),
                    'Filler type ' || i, 'ACTIVE'
             FROM generate_series(1, {FILLERS}) AS i"
        ),
        tenant_id,
    )
    .await;
    execute(
        &app,
        "INSERT INTO document_types (id, tenant_id, type_code, name, status) VALUES
            (gen_random_uuid(), $1, 'ZZ_SEEK_1', 'Sought', 'ACTIVE'),
            (gen_random_uuid(), $1, 'ZZ_SEEK_2', 'Sought, drafted', 'DRAFT'),
            (gen_random_uuid(), $1, 'ZZ_NAMED', 'Found as zz_seek by name', 'ACTIVE'),
            (gen_random_uuid(), $1, 'ZZXSEEK', 'A wildcard would find me', 'ACTIVE')",
        tenant_id,
    )
    .await;

    holds_the_contract(
        &app,
        &token,
        "/api/v1/document-types",
        "typeCode",
        Some(("ACTIVE", "DRAFT")),
    )
    .await;
}

#[tokio::test]
async fn a_form_is_found_past_the_first_hundred() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let tenant_id = tenant_of_administrator(&app).await;

    execute(
        &app,
        &format!(
            "INSERT INTO rad_forms (id, tenant_id, form_key, title, jfss_version,
                                    definition_json, status, published_at)
             SELECT gen_random_uuid(), $1, 'AA_FILL_' || lpad(i::text, 3, '0'),
                    'Filler form ' || i, '2.0.1', '{{}}', 'PUBLISHED', now()
             FROM generate_series(1, {FILLERS}) AS i"
        ),
        tenant_id,
    )
    .await;
    execute(
        &app,
        "INSERT INTO rad_forms (id, tenant_id, form_key, title, revision, jfss_version,
                                definition_json, status, published_at) VALUES
            (gen_random_uuid(), $1, 'ZZ_SEEK_1', 'Sought', 3, '2.0.1', '{}', 'PUBLISHED', now()),
            (gen_random_uuid(), $1, 'ZZ_SEEK_2', 'Sought, drafted', 1, '2.0.1', '{}', 'DRAFT', NULL),
            (gen_random_uuid(), $1, 'ZZ_NAMED', 'Found as zz_seek by name', 1, '2.0.1', '{}',
             'PUBLISHED', now()),
            (gen_random_uuid(), $1, 'ZZXSEEK', 'A wildcard would find me', 1, '2.0.1', '{}',
             'PUBLISHED', now())",
        tenant_id,
    )
    .await;

    let searched = holds_the_contract(
        &app,
        &token,
        "/api/v1/rad/forms",
        "formKey",
        Some(("PUBLISHED", "DRAFT")),
    )
    .await;

    // The frontend labels an edited binding from the single read.
    let id = id_of(&searched, "formKey", "ZZ_SEEK_1");
    let one = get(&app, &token, &format!("/api/v1/rad/forms/{id}")).await;
    assert_eq!(one.status, StatusCode::OK, "{}", one.body);
    assert_eq!(one.data()["title"], "Sought");
    assert_eq!(one.data()["revision"], 3);
}

#[tokio::test]
async fn a_list_is_found_past_the_first_hundred() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let tenant_id = tenant_of_administrator(&app).await;

    execute(
        &app,
        &format!(
            "INSERT INTO rad_lists (id, tenant_id, list_key, title, status)
             SELECT gen_random_uuid(), $1, 'AA_FILL_' || lpad(i::text, 3, '0'),
                    'Filler list ' || i, 'ACTIVE'
             FROM generate_series(1, {FILLERS}) AS i"
        ),
        tenant_id,
    )
    .await;
    execute(
        &app,
        "INSERT INTO rad_lists (id, tenant_id, list_key, title, status) VALUES
            (gen_random_uuid(), $1, 'ZZ_SEEK_1', 'Sought', 'ACTIVE'),
            (gen_random_uuid(), $1, 'ZZ_SEEK_2', 'Sought, drafted', 'DRAFT'),
            (gen_random_uuid(), $1, 'ZZ_NAMED', 'Found as zz_seek by name', 'ACTIVE'),
            (gen_random_uuid(), $1, 'ZZXSEEK', 'A wildcard would find me', 'ACTIVE')",
        tenant_id,
    )
    .await;

    let searched = holds_the_contract(
        &app,
        &token,
        "/api/v1/rad/lists",
        "listKey",
        Some(("ACTIVE", "DRAFT")),
    )
    .await;

    let id = id_of(&searched, "listKey", "ZZ_SEEK_1");
    let one = get(&app, &token, &format!("/api/v1/rad/lists/{id}")).await;
    assert_eq!(one.status, StatusCode::OK, "{}", one.body);
    assert_eq!(one.data()["title"], "Sought");
    assert_eq!(one.data()["listKey"], "ZZ_SEEK_1");
}

#[tokio::test]
async fn a_workflow_definition_is_found_past_the_first_hundred() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let tenant_id = tenant_of_administrator(&app).await;

    execute(
        &app,
        &format!(
            "INSERT INTO workflow_definitions (id, tenant_id, workflow_key, name, jwss_version,
                                               definition_json, initial_state, status,
                                               published_at)
             SELECT gen_random_uuid(), $1, 'AA_FILL_' || lpad(i::text, 3, '0'),
                    'Filler workflow ' || i, '1.0.0', '{{}}', 'draft', 'ACTIVE', now()
             FROM generate_series(1, {FILLERS}) AS i"
        ),
        tenant_id,
    )
    .await;
    execute(
        &app,
        "INSERT INTO workflow_definitions (id, tenant_id, workflow_key, name, jwss_version,
                                           definition_json, initial_state, status,
                                           published_at) VALUES
            (gen_random_uuid(), $1, 'ZZ_SEEK_1', 'Sought', '1.0.0', '{}', 'draft', 'ACTIVE', now()),
            (gen_random_uuid(), $1, 'ZZ_SEEK_2', 'Sought, drafted', '1.0.0', '{}', 'draft',
             'DRAFT', NULL),
            (gen_random_uuid(), $1, 'ZZ_NAMED', 'Found as zz_seek by name', '1.0.0', '{}',
             'draft', 'ACTIVE', now()),
            (gen_random_uuid(), $1, 'ZZXSEEK', 'A wildcard would find me', '1.0.0', '{}',
             'draft', 'ACTIVE', now())",
        tenant_id,
    )
    .await;

    holds_the_contract(
        &app,
        &token,
        "/api/v1/workflow/definitions",
        "workflowKey",
        Some(("ACTIVE", "DRAFT")),
    )
    .await;
}

#[tokio::test]
async fn a_user_is_found_past_the_first_hundred() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let tenant_id = tenant_of_administrator(&app).await;

    // Usernames are lower case; `aa_fill_*` sorts ahead of the bootstrap
    // administrator as well as of the wanted rows.
    execute(
        &app,
        &format!(
            "INSERT INTO users (id, tenant_id, username, email, password_hash, display_name, status)
             SELECT gen_random_uuid(), $1, 'aa_fill_' || lpad(i::text, 3, '0'),
                    'fill' || i || '@example.test', 'not-a-hash', 'Filler ' || i, 'ACTIVE'
             FROM generate_series(1, {FILLERS}) AS i"
        ),
        tenant_id,
    )
    .await;
    // ZZ_NAMED is found by its email here, and the display name has its own
    // row: a user is searched on three columns.
    execute(
        &app,
        "INSERT INTO users (id, tenant_id, username, email, password_hash, display_name, status,
                            deleted_at) VALUES
            (gen_random_uuid(), $1, 'zz_seek_1', 'one@example.test', 'x', 'Sought', 'ACTIVE', NULL),
            (gen_random_uuid(), $1, 'zz_seek_2', 'two@example.test', 'x', 'Sought, idle',
             'INACTIVE', NULL),
            (gen_random_uuid(), $1, 'zz_named', 'ZZ_SEEK@example.test', 'x', 'By email',
             'ACTIVE', NULL),
            (gen_random_uuid(), $1, 'zzxseek', 'wild@example.test', 'x', 'A wildcard would find me',
             'ACTIVE', NULL),
            (gen_random_uuid(), $1, 'zz_seek_gone', 'gone@example.test', 'x', 'Soft-deleted',
             'ACTIVE', now())",
        tenant_id,
    )
    .await;

    holds_the_contract(
        &app,
        &token,
        "/api/v1/identity/users",
        "username",
        Some(("ACTIVE", "INACTIVE")),
    )
    .await;

    let by_display_name = get(&app, &token, "/api/v1/identity/users?search=IDLE").await;
    assert_eq!(column(&by_display_name, "username"), ["zz_seek_2"]);
    assert_eq!(total(&by_display_name), 1);

    let deleted = get(&app, &token, "/api/v1/identity/users?search=gone").await;
    assert_eq!(total(&deleted), 0, "{}", deleted.body);
}

#[tokio::test]
async fn a_role_is_found_past_the_first_hundred() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let tenant_id = tenant_of_administrator(&app).await;

    execute(
        &app,
        &format!(
            "INSERT INTO roles (id, tenant_id, role_code, name)
             SELECT gen_random_uuid(), $1, 'AA_FILL_' || lpad(i::text, 3, '0'), 'Filler role ' || i
             FROM generate_series(1, {FILLERS}) AS i"
        ),
        tenant_id,
    )
    .await;
    execute(
        &app,
        "INSERT INTO roles (id, tenant_id, role_code, name) VALUES
            (gen_random_uuid(), $1, 'ZZ_SEEK_1', 'Sought'),
            (gen_random_uuid(), $1, 'ZZ_SEEK_2', 'Sought again'),
            (gen_random_uuid(), $1, 'ZZ_NAMED', 'Found as zz_seek by name'),
            (gen_random_uuid(), $1, 'ZZXSEEK', 'A wildcard would find me')",
        tenant_id,
    )
    .await;

    holds_the_contract(&app, &token, "/api/v1/identity/roles", "roleCode", None).await;

    // A role has no status: the parameter is ignored like any unknown one,
    // rather than silently filtering on a column that does not exist.
    let ignored = get(
        &app,
        &token,
        &format!("/api/v1/identity/roles?search={SEARCH}&status=BOGUS"),
    )
    .await;
    assert_eq!(ignored.status, StatusCode::OK, "{}", ignored.body);
    assert_eq!(total(&ignored), 3);
}

#[tokio::test]
async fn each_list_documents_its_search_and_status() {
    let app = TestApp::spawn().await;
    let document = app
        .send(Method::GET, "/api/docs/openapi.json", None, None)
        .await;

    for (path, has_status) in [
        ("/api/v1/document-types", true),
        ("/api/v1/rad/forms", true),
        ("/api/v1/rad/lists", true),
        ("/api/v1/workflow/definitions", true),
        ("/api/v1/identity/users", true),
        ("/api/v1/identity/roles", false),
    ] {
        let names: Vec<&str> = document.body["paths"][path]["get"]["parameters"]
            .as_array()
            .unwrap_or_else(|| panic!("{path} has parameters"))
            .iter()
            .filter_map(|parameter: &Value| parameter["name"].as_str())
            .collect();

        for expected in ["page", "pageSize", "search"] {
            assert!(
                names.contains(&expected),
                "{path} lacks {expected}: {names:?}"
            );
        }
        assert_eq!(names.contains(&"status"), has_status, "{path}: {names:?}");
    }
}

// ---------------------------------------------------------------------------
// Test-engineer gate (#525): the predicates the tests above do not reach.
//
// Each test below collects every endpoint's failure before asserting, so one
// run names every list a mutation breaks rather than only the first.
// ---------------------------------------------------------------------------

/// The six lists, with the response field holding each one's key.
const LISTS: [(&str, &str); 6] = [
    ("/api/v1/document-types", "typeCode"),
    ("/api/v1/rad/forms", "formKey"),
    ("/api/v1/rad/lists", "listKey"),
    ("/api/v1/workflow/definitions", "workflowKey"),
    ("/api/v1/identity/users", "username"),
    ("/api/v1/identity/roles", "roleCode"),
];

/// One `zz_seek`-matching row in each of the six tables, in `tenant_id`, with
/// `deleted_at` set or not. `suffix` keeps the keys apart between calls.
async fn seed_one_of_each(app: &TestApp, tenant_id: Uuid, suffix: &str, deleted: bool) {
    let deleted_at = if deleted { "now()" } else { "NULL" };
    let key = format!("ZZ_SEEK_{suffix}");
    let username = key.to_lowercase();

    for sql in [
        format!(
            "INSERT INTO document_types (id, tenant_id, type_code, name, status, deleted_at)
             VALUES (gen_random_uuid(), $1, '{key}', 'Hidden type', 'ACTIVE', {deleted_at})"
        ),
        format!(
            "INSERT INTO rad_forms (id, tenant_id, form_key, title, jfss_version, definition_json,
                                    status, published_at, deleted_at)
             VALUES (gen_random_uuid(), $1, '{key}', 'Hidden form', '2.0.1', '{{}}', 'PUBLISHED',
                     now(), {deleted_at})"
        ),
        format!(
            "INSERT INTO rad_lists (id, tenant_id, list_key, title, status, deleted_at)
             VALUES (gen_random_uuid(), $1, '{key}', 'Hidden list', 'ACTIVE', {deleted_at})"
        ),
        format!(
            "INSERT INTO workflow_definitions (id, tenant_id, workflow_key, name, jwss_version,
                                               definition_json, initial_state, status,
                                               published_at, deleted_at)
             VALUES (gen_random_uuid(), $1, '{key}', 'Hidden workflow', '1.0.0', '{{}}', 'draft',
                     'ACTIVE', now(), {deleted_at})"
        ),
        format!(
            "INSERT INTO users (id, tenant_id, username, email, password_hash, display_name,
                                status, deleted_at)
             VALUES (gen_random_uuid(), $1, '{username}', '{username}@example.test', 'x',
                     'Hidden user', 'ACTIVE', {deleted_at})"
        ),
        format!(
            "INSERT INTO roles (id, tenant_id, role_code, name, deleted_at)
             VALUES (gen_random_uuid(), $1, '{key}', 'Hidden role', {deleted_at})"
        ),
    ] {
        execute(app, &sql, tenant_id).await;
    }
}

/// Every list, searched for `zz_seek`, must show nothing and count nothing:
/// the only matching rows are another tenant's or soft-deleted. The page and
/// `meta.total` are checked apart, because each has its own `WHERE`.
async fn every_list_finds_nothing(app: &TestApp, token: &str, why: &str) {
    let mut failures = Vec::new();

    for (base, key) in LISTS {
        let response = get(app, token, &format!("{base}?search={SEARCH}&pageSize=100")).await;
        if response.status != StatusCode::OK {
            failures.push(format!("{base}: {} {}", response.status, response.body));
            continue;
        }
        let keys = column(&response, key);
        if !keys.is_empty() {
            failures.push(format!("{base} lists {keys:?}"));
        }
        if total(&response) != 0 {
            failures.push(format!("{base} counts {}", total(&response)));
        }
    }

    assert!(failures.is_empty(), "{why}:\n{}", failures.join("\n"));
}

#[tokio::test]
async fn a_search_never_reaches_another_tenants_rows() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let foreign = common::fixtures::create_tenant(&app.pool, "TNT-525", "Foreign tenant").await;

    seed_one_of_each(&app, foreign, "FOREIGN", false).await;

    every_list_finds_nothing(&app, &token, "another tenant's rows were searched").await;
}

#[tokio::test]
async fn a_search_never_reaches_a_soft_deleted_row() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let tenant_id = tenant_of_administrator(&app).await;

    seed_one_of_each(&app, tenant_id, "GONE", true).await;

    every_list_finds_nothing(&app, &token, "a soft-deleted row was searched").await;
}

#[tokio::test]
async fn a_percent_or_a_backslash_in_a_search_matches_itself() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let tenant_id = tenant_of_administrator(&app).await;

    // The decoys are what an unescaped pattern finds instead: `100%` as a
    // pattern is "100 then anything", and `a\b` as a pattern is "ab".
    execute(
        &app,
        r"INSERT INTO document_types (id, tenant_id, type_code, name, status) VALUES
            (gen_random_uuid(), $1, 'PCT_ONE', 'Sure 100% of it', 'ACTIVE'),
            (gen_random_uuid(), $1, 'PCT_DECOY', 'Sure 1000 of it', 'ACTIVE'),
            (gen_random_uuid(), $1, 'BSL_ONE', 'Path a\b here', 'ACTIVE'),
            (gen_random_uuid(), $1, 'BSL_DECOY', 'Path ab here', 'ACTIVE')",
        tenant_id,
    )
    .await;
    execute(
        &app,
        r"INSERT INTO roles (id, tenant_id, role_code, name) VALUES
            (gen_random_uuid(), $1, 'PCT_ONE', 'Sure 100% of it'),
            (gen_random_uuid(), $1, 'PCT_DECOY', 'Sure 1000 of it'),
            (gen_random_uuid(), $1, 'BSL_ONE', 'Path a\b here'),
            (gen_random_uuid(), $1, 'BSL_DECOY', 'Path ab here')",
        tenant_id,
    )
    .await;

    let mut failures = Vec::new();
    for (base, key) in [
        ("/api/v1/document-types", "typeCode"),
        ("/api/v1/identity/roles", "roleCode"),
    ] {
        // `%25` is `%`, `%5C` is `\`.
        for (search, wanted) in [("100%25", "PCT_ONE"), ("a%5Cb", "BSL_ONE")] {
            let response = get(&app, &token, &format!("{base}?search={search}")).await;
            let keys = column(&response, key);
            if keys != [wanted] || total(&response) != 1 {
                failures.push(format!(
                    "{base}?search={search}: {keys:?}, total {}",
                    total(&response)
                ));
            }
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Inputs a hand-written URL can carry and a chooser never sends. None of them
/// may be a 500; what each one *is* is pinned below.
///
/// **A search containing NUL (`%00`) is a 422 on `search`**, not a 500 and not
/// a search for the term with the NUL stripped out. PostgreSQL `text` cannot
/// hold 0x00, so binding one failed the statement ("invalid byte sequence for
/// encoding UTF8: 0x00"); `utils::search` now refuses it before any query runs,
/// on these six lists and on every other list that searches (#525).
#[tokio::test]
async fn an_unusual_search_or_paging_value_is_never_a_server_error() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let long = "a".repeat(1000);

    let mut failures = Vec::new();
    for (base, _) in LISTS {
        for query in [
            format!("search={long}"),
            "search=a%01%1F%7Fb".to_owned(),
            "search=%0A%09".to_owned(),
            "pageSize=0".to_owned(),
            "page=0".to_owned(),
        ] {
            let response = get(&app, &token, &format!("{base}?{query}")).await;
            if response.status != StatusCode::OK {
                failures.push(format!("{base}?{query:.40}: {}", response.status));
            }
        }

        // NUL alone and NUL inside a term: both refused, naming the field.
        for query in ["search=%00", "search=a%00b"] {
            let response = get(&app, &token, &format!("{base}?{query}")).await;
            if response.status != StatusCode::UNPROCESSABLE_ENTITY
                || response.body["error"]["details"][0]["path"] != "search"
            {
                failures.push(format!(
                    "{base}?{query}: {} {}",
                    response.status, response.body["error"]
                ));
            }
        }

        // pageSize=0 is clamped up to 1, and page=0 is page 1.
        let zero = get(&app, &token, &format!("{base}?pageSize=0&page=0")).await;
        if zero.body["meta"]["pageSize"] != 1 || zero.body["meta"]["page"] != 1 {
            failures.push(format!("{base}?pageSize=0&page=0: {}", zero.body["meta"]));
        }

        // A status is matched exactly, so lower case is outside the vocabulary.
        if base != "/api/v1/identity/roles" {
            let lower = get(&app, &token, &format!("{base}?status=active")).await;
            if lower.status != StatusCode::UNPROCESSABLE_ENTITY {
                failures.push(format!("{base}?status=active: {}", lower.status));
            }
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
