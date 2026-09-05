//! The dynamic list renderer's server half (FR-RAD-003, FR-RAD-010; [#340]).
//!
//! **The list definition storage API has existed since Sprint 7 and nothing
//! read it.** These are the tests for the reader — and what they assert that a
//! component test cannot is everything that needs a database and a request: the
//! order the rows really come back in, which documents a list really covers,
//! and that a definition nothing can draw is refused rather than served as an
//! empty table.
//!
//! **The empty table is the failure this whole file is about.** A column key
//! nothing resolves, a filter the query has no parameter for, a list no
//! document type binds, a list still in `DRAFT` — every one of them, left
//! alone, renders as a table with no rows, which reads as *this tenant has no
//! documents*. [#326](https://github.com/sujanto-gaws/kelir/issues/326) is the
//! same silence in a different panel.
//!
//! [#340]: https://github.com/sujanto-gaws/kelir/issues/340

mod common;

use axum::http::{Method, StatusCode};
use common::{fixtures, TestApp};
use serde_json::{json, Value};
use uuid::Uuid;

const PASSWORD: &str = "Sup3rSecret!Pass";

/// A user holding exactly `permissions` and nothing else, signed in.
///
/// The shape `rad_permissions.rs` uses, for the reason it gives: a token that
/// held everything could not tell a route that checks a permission from one
/// that checks none.
async fn caller_holding(app: &TestApp, permissions: &[&str], nonce: usize) -> String {
    let role_id = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        &format!("ROLE-LIST-RENDER-{nonce}"),
        permissions,
    )
    .await;

    let username = format!("user.render{nonce}");

    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        &username,
        &format!("render{nonce}@kelir.test"),
        PASSWORD,
        &[role_id],
    )
    .await;

    app.sign_in(&username, PASSWORD).await
}

fn id_of(value: &Value) -> Uuid {
    value["id"]
        .as_str()
        .expect("an id")
        .parse()
        .expect("a uuid")
}

/// A list definition, created through the storage API.
async fn create_list(app: &TestApp, token: &str, key: &str, body: Value) -> Uuid {
    let mut request = json!({ "listKey": key, "title": "Requisitions" });

    for (field, value) in body.as_object().expect("an object") {
        request[field] = value.clone();
    }

    let created = app
        .send(
            Method::POST,
            "/api/v1/rad/lists",
            Some(token),
            Some(request),
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    id_of(&created.body["data"])
}

fn column(key: &str, label: &str) -> Value {
    json!({ "columnKey": key, "label": label })
}

/// A document type bound to `list_id`, which is what gives a list its rows.
async fn bound_type(app: &TestApp, token: &str, code: &str, list_id: Uuid) -> Uuid {
    let created = app
        .send(
            Method::POST,
            "/api/v1/document-types",
            Some(token),
            Some(json!({ "typeCode": code, "name": code, "listId": list_id })),
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    id_of(&created.body["data"])
}

async fn document(app: &TestApp, token: &str, type_id: Uuid, title: &str) -> Uuid {
    let created = app
        .send(
            Method::POST,
            "/api/v1/documents",
            Some(token),
            Some(json!({ "documentTypeId": type_id, "title": title })),
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    id_of(&created.body["data"])
}

async fn render(app: &TestApp, token: &str, list_key: &str) -> common::TestResponse {
    app.send(
        Method::GET,
        &format!("/api/v1/rad/lists/by-key/{list_key}"),
        Some(token),
        None,
    )
    .await
}

async fn rows(app: &TestApp, token: &str, list_id: Uuid, query: &str) -> common::TestResponse {
    app.send(
        Method::GET,
        &format!("/api/v1/rad/lists/{list_id}/rows{query}"),
        Some(token),
        None,
    )
    .await
}

fn cells(body: &Value, key: &str) -> Vec<String> {
    body["data"]
        .as_array()
        .expect("a list")
        .iter()
        .map(|row| row["cells"][key].as_str().unwrap_or_default().to_owned())
        .collect()
}

fn codes(body: &Value) -> Vec<String> {
    body["error"]["details"]
        .as_array()
        .map(|details| {
            details
                .iter()
                .map(|detail| detail["code"].as_str().unwrap_or_default().to_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// The ordinary case: an `ACTIVE` list, a bound type, two documents.
async fn a_working_list(app: &TestApp, token: &str, key: &str) -> (Uuid, Uuid) {
    let list_id = create_list(
        app,
        token,
        key,
        json!({
            "status": "ACTIVE",
            "columns": [column("documentNumber", "Number"), column("title", "Subject")],
        }),
    )
    .await;
    let type_id = bound_type(app, token, &key.to_uppercase(), list_id).await;

    (list_id, type_id)
}

// ---------------------------------------------------------------------------
// AC1 — a published definition renders, and every part comes from it
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_definition_renders_as_the_columns_it_declares() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let (_, type_id) = a_working_list(&app, &token, "requisitions").await;

    document(&app, &token, type_id, "Two standing desks").await;

    let response = render(&app, &token, "requisitions").await;

    assert_eq!(response.status, StatusCode::OK, "{}", response.body);

    let columns = response.body["data"]["columns"]
        .as_array()
        .expect("columns");

    assert_eq!(columns.len(), 2);
    assert_eq!(columns[0]["key"], "documentNumber");
    assert_eq!(columns[0]["label"], "Number");
    assert_eq!(columns[1]["key"], "title");
}

#[tokio::test]
async fn the_rows_are_the_documents_of_the_types_that_name_the_list() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let (list_id, type_id) = a_working_list(&app, &token, "requisitions").await;

    document(&app, &token, type_id, "In the list").await;

    // **A second subject** (coding standard §2.9): a document of a type that
    // names *no* list. One document cannot tell "scoped by binding" from "not
    // scoped at all" — the assertion is identical either way.
    let unbound = app
        .send(
            Method::POST,
            "/api/v1/document-types",
            Some(&token),
            Some(json!({ "typeCode": "UNBOUND", "name": "Unbound" })),
        )
        .await;

    document(
        &app,
        &token,
        id_of(&unbound.body["data"]),
        "Not in the list",
    )
    .await;

    let response = rows(&app, &token, list_id, "").await;

    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    assert_eq!(cells(&response.body, "title"), ["In the list"]);
    assert_eq!(response.body["meta"]["total"], 1);
}

#[tokio::test]
async fn a_form_data_column_is_read_from_the_stored_payload() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let list_id = create_list(
        &app,
        &token,
        "with-form-data",
        json!({
            "status": "ACTIVE",
            "columns": [column("title", "Subject"), column("form_data.quantity", "Quantity")],
        }),
    )
    .await;
    let form = app
        .send(
            Method::POST,
            "/api/v1/rad/forms",
            Some(&token),
            Some(json!({
                "formKey": "quantity-form",
                "title": "Quantity form",
                "definition": {
                    "formId": "quantity-form",
                    "version": "2.0.1",
                    "components": [{
                        "id": "quantity", "role": "data", "type": "number",
                        "key": "quantity", "label": "Quantity",
                        "validation": {"type": "number"}
                    }]
                },
            })),
        )
        .await;
    let form_id = id_of(&form.body["data"]);

    app.send(
        Method::POST,
        &format!("/api/v1/rad/forms/{form_id}/publish"),
        Some(&token),
        None,
    )
    .await;

    let created = app
        .send(
            Method::POST,
            "/api/v1/document-types",
            Some(&token),
            Some(json!({
                "typeCode": "WITH_FORM_DATA", "name": "With form data",
                "listId": list_id, "formId": form_id,
            })),
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    let type_id = id_of(&created.body["data"]);
    let document = app
        .send(
            Method::POST,
            "/api/v1/documents",
            Some(&token),
            Some(json!({
                "documentTypeId": type_id,
                "title": "Desks",
                "formData": {"quantity": 7},
            })),
        )
        .await;

    assert_eq!(document.status, StatusCode::CREATED, "{}", document.body);

    let response = rows(&app, &token, list_id, "").await;

    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    // **The cell, not the payload.** The wire carries the declared path's value
    // and never the whole `form_data_json` — which is the reason
    // `DocumentSummary` exists (NFR-PERF-002).
    assert_eq!(response.body["data"][0]["cells"]["form_data.quantity"], 7);
    assert!(
        response.body["data"][0]["cells"].get("formData").is_none(),
        "a row carries its declared cells and not the payload: {}",
        response.body
    );
}

// ---------------------------------------------------------------------------
// AC2 — the order is the definition's
// ---------------------------------------------------------------------------

/// **AC2 through the database.** The definition's default sort is ascending by
/// title; the documents are created in the reverse of that order, so a list
/// that ignored the definition would answer newest-first and put `Zinc` first.
///
/// The mutation that must make this red is `DocumentSort::default()` in place
/// of `plan.sort` in `RowQuery::sort`.
#[tokio::test]
async fn a_definition_that_declares_a_sort_produces_a_different_first_row() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let list_id = create_list(
        &app,
        &token,
        "sorted",
        json!({
            "status": "ACTIVE",
            "defaultSort": [{"key": "title", "dir": "asc"}],
            "columns": [column("title", "Subject")],
        }),
    )
    .await;
    let type_id = bound_type(&app, &token, "SORTED", list_id).await;

    // Newest last by title, so definition order and creation order disagree.
    document(&app, &token, type_id, "Zinc").await;
    document(&app, &token, type_id, "Aluminium").await;

    let response = rows(&app, &token, list_id, "").await;

    assert_eq!(
        cells(&response.body, "title"),
        ["Aluminium", "Zinc"],
        "the list must open on its own sort, not on newest-first: {}",
        response.body
    );
}

#[tokio::test]
async fn the_request_can_reverse_a_sort_the_definition_offers() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let (list_id, type_id) = a_working_list(&app, &token, "reversible").await;

    document(&app, &token, type_id, "Aluminium").await;
    document(&app, &token, type_id, "Zinc").await;

    let response = rows(&app, &token, list_id, "?sort=title&dir=desc").await;

    assert_eq!(cells(&response.body, "title"), ["Zinc", "Aluminium"]);
}

#[tokio::test]
async fn the_page_size_is_the_definitions_and_a_request_cannot_widen_it() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let list_id = create_list(
        &app,
        &token,
        "small-pages",
        json!({
            "status": "ACTIVE",
            "pageSize": 1,
            "defaultSort": [{"key": "title", "dir": "asc"}],
            "columns": [column("title", "Subject")],
        }),
    )
    .await;
    let type_id = bound_type(&app, &token, "SMALL_PAGES", list_id).await;

    document(&app, &token, type_id, "Aluminium").await;
    document(&app, &token, type_id, "Zinc").await;

    let first = rows(&app, &token, list_id, "").await;

    assert_eq!(cells(&first.body, "title"), ["Aluminium"]);
    assert_eq!(first.body["meta"]["total"], 2);

    let second = rows(&app, &token, list_id, "?page=2").await;

    assert_eq!(cells(&second.body, "title"), ["Zinc"]);

    // Named rather than silently ignored: a caller asking for a bigger page is
    // told the definition decides it.
    let widened = rows(&app, &token, list_id, "?pageSize=50").await;

    assert_eq!(widened.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        widened.body["error"]["details"][0]["message"]
            .as_str()
            .is_some_and(|message| message.contains("page size")),
        "{}",
        widened.body
    );
}

// ---------------------------------------------------------------------------
// AC3 — the declared filters work, and only they are offered
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_declared_filter_narrows_the_rows_through_the_query() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let list_id = create_list(
        &app,
        &token,
        "filtered",
        json!({
            "status": "ACTIVE",
            "columns": [column("title", "Subject")],
            "filters": [{"filterKey": "search", "label": "Search", "filterType": "TEXT"}],
        }),
    )
    .await;
    let type_id = bound_type(&app, &token, "FILTERED", list_id).await;

    document(&app, &token, type_id, "Two standing desks").await;
    document(&app, &token, type_id, "A box of pencils").await;

    let response = rows(&app, &token, list_id, "?search=desks").await;

    assert_eq!(cells(&response.body, "title"), ["Two standing desks"]);
    // The filtered total, not the population — a pager that offered pages of a
    // wider set is a pager that offers empty ones.
    assert_eq!(response.body["meta"]["total"], 1);
}

/// **A filter the definition does not declare is refused, not ignored.** An
/// ignored parameter reads to whoever sent it as a filter that matched
/// everything, which is the same wrong answer as no filter at all.
#[tokio::test]
async fn a_filter_the_definition_does_not_declare_is_refused() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let (list_id, type_id) = a_working_list(&app, &token, "unfiltered").await;

    document(&app, &token, type_id, "Two standing desks").await;

    let response = rows(&app, &token, list_id, "?status=DRAFT").await;

    assert_eq!(
        response.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        response.body
    );
    assert!(codes(&response.body).contains(&"FILTER_NOT_RENDERABLE".to_owned()));
}

/// The refusal names the **filter key** the client sent, not the query
/// parameter it would have set. Somebody looking at a control labelled *Stage*
/// should be told about `stage`.
#[tokio::test]
async fn a_bad_filter_value_is_named_by_the_definitions_own_key() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let list_id = create_list(
        &app,
        &token,
        "renamed-filter",
        json!({
            "status": "ACTIVE",
            "columns": [column("title", "Subject")],
            "filters": [{"filterKey": "status", "label": "Stage", "filterType": "ENUM"}],
        }),
    )
    .await;

    bound_type(&app, &token, "RENAMED_FILTER", list_id).await;

    let response = rows(&app, &token, list_id, "?status=NONSENSE").await;

    assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(response.body["error"]["details"][0]["path"], "status");
}

// ---------------------------------------------------------------------------
// AC4 — a definition that cannot be drawn fails visibly, naming what is wrong
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_draft_list_is_refused_rather_than_rendered_empty() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let list_id = create_list(
        &app,
        &token,
        "still-a-draft",
        json!({ "status": "DRAFT", "columns": [column("title", "Subject")] }),
    )
    .await;

    bound_type(&app, &token, "STILL_A_DRAFT", list_id).await;

    let response = render(&app, &token, "still-a-draft").await;

    assert_eq!(response.status, StatusCode::CONFLICT, "{}", response.body);
    assert!(
        response.body["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("Draft")),
        "the refusal must say which status: {}",
        response.body
    );
}

#[tokio::test]
async fn a_column_nothing_can_resolve_is_refused_and_named() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let list_id = create_list(
        &app,
        &token,
        "bad-column",
        json!({
            "status": "ACTIVE",
            "columns": [column("supplier_rating", "Rating")],
        }),
    )
    .await;

    bound_type(&app, &token, "BAD_COLUMN", list_id).await;

    let response = render(&app, &token, "bad-column").await;

    assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(codes(&response.body).contains(&"COLUMN_NOT_RENDERABLE".to_owned()));
    assert!(
        response.body["error"]["details"][0]["message"]
            .as_str()
            .is_some_and(|message| message.contains("supplier_rating")),
        "{}",
        response.body
    );
}

/// **The definition is stored happily and refused when it is opened**, which is
/// the opposite of [#338](https://github.com/sujanto-gaws/kelir/issues/338) and
/// deliberate: `rad_lists` is generic, and refusing a column key at the write
/// would put the document module's vocabulary inside a table written for lists
/// over anything.
#[tokio::test]
async fn a_definition_the_renderer_refuses_is_still_stored() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let list_id = create_list(
        &app,
        &token,
        "stored-anyway",
        json!({
            "status": "ACTIVE",
            "columns": [column("supplier_rating", "Rating")],
        }),
    )
    .await;

    let stored = app
        .send(
            Method::GET,
            &format!("/api/v1/rad/lists/{list_id}"),
            Some(&token),
            None,
        )
        .await;

    assert_eq!(stored.status, StatusCode::OK, "{}", stored.body);
    assert_eq!(
        stored.body["data"]["columns"][0]["columnKey"],
        "supplier_rating"
    );
}

#[tokio::test]
async fn a_list_no_document_type_binds_is_refused_rather_than_rendered_empty() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    create_list(
        &app,
        &token,
        "unbound-list",
        json!({ "status": "ACTIVE", "columns": [column("title", "Subject")] }),
    )
    .await;

    let response = render(&app, &token, "unbound-list").await;

    assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(codes(&response.body).contains(&"LIST_NOT_BOUND".to_owned()));
}

#[tokio::test]
async fn a_list_that_declares_no_columns_is_refused() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let list_id = create_list(
        &app,
        &token,
        "no-columns",
        json!({ "status": "ACTIVE", "columns": [] }),
    )
    .await;

    bound_type(&app, &token, "NO_COLUMNS", list_id).await;

    let response = render(&app, &token, "no-columns").await;

    assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(codes(&response.body).contains(&"LIST_HAS_NO_COLUMNS".to_owned()));
}

#[tokio::test]
async fn a_list_key_nobody_has_is_a_404() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    assert_eq!(
        render(&app, &token, "nothing-here").await.status,
        StatusCode::NOT_FOUND
    );
}

// ---------------------------------------------------------------------------
// The permission, and the tenant
// ---------------------------------------------------------------------------

/// **The rows' own permission, and no second one.** A rendered list is a view of
/// documents, so it opens exactly what `GET /documents` opens — the reading
/// [Database Schema](../docs/design/02.%20Database%20Schema.md) §5.13 already
/// takes for lookups. Requiring `rad:list:read` would mean only a configuration
/// administrator could open a screen built for everybody.
#[tokio::test]
async fn a_caller_without_document_read_cannot_render_a_list() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let (list_id, _) = a_working_list(&app, &token, "guarded").await;

    // A caller holding the *configuration* permission and not the documents'.
    let outsider = caller_holding(&app, &["rad:list:read"], 1).await;

    assert_eq!(
        render(&app, &outsider, "guarded").await.status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        rows(&app, &outsider, list_id, "").await.status,
        StatusCode::FORBIDDEN
    );
}

/// **The permission is asked before anything is read**, so a refusal never
/// depends on whether the thing existed.
///
/// Found by the mutation campaign: removing `caller.require(DOCUMENT_READ)`
/// from the render read came back **green**, because the binding check below it
/// asks for the same permission and the refusal still happened — for a list
/// that exists. For one that does not, the read gets as far as the 404 first,
/// and an unpermitted caller could then tell a real list key from an invented
/// one. That is small and it is an enumeration oracle, which is the thing
/// `service/mod.rs`'s first rule is about: *a 404 that only a permitted caller
/// could have received is itself a disclosure.*
///
/// The mutation that must make this red is deleting that `require` line.
#[tokio::test]
async fn a_caller_without_document_read_cannot_tell_a_real_list_key_from_an_invented_one() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    a_working_list(&app, &token, "real-key").await;

    let outsider = caller_holding(&app, &["rad:list:read"], 2).await;

    // The same answer for both, which is what makes it no answer at all.
    assert_eq!(
        render(&app, &outsider, "real-key").await.status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        render(&app, &outsider, "no-such-key").await.status,
        StatusCode::FORBIDDEN,
        "a list key that does not exist must refuse the same way as one that does"
    );
}

#[tokio::test]
async fn a_list_in_another_tenant_is_not_rendered_here() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let (list_id, type_id) = a_working_list(&app, &token, "mine").await;

    document(&app, &token, type_id, "Mine").await;

    // **The second subject** (coding standard §2.9): another tenant holding a
    // list of the *same key*. One tenant cannot tell *scoped by tenant* from
    // *unscoped* — the assertion is identical either way, and the key collision
    // is what makes the by-key read prove it rather than merely pass.
    //
    // Inserted directly rather than through a second sign-in: the harness runs
    // single-tenant, where `tenantCode` is ignored at login (FR-IDM-009), so a
    // foreign user could not sign in here. `rad_forms.rs` seeds a foreign form
    // the same way and for the same reason.
    let other = fixtures::create_tenant(&app.pool, "TNT-LISTS", "Another Customer").await;
    let other_list = Uuid::now_v7();

    sqlx::query(
        "INSERT INTO rad_lists (id, tenant_id, list_key, title, page_size, status)
         VALUES ($1, $2, 'mine', 'Theirs', 20, 'ACTIVE')",
    )
    .bind(other_list)
    .bind(other)
    .execute(&app.pool)
    .await
    .expect("the other tenant's list is seeded");

    sqlx::query(
        "INSERT INTO rad_list_columns (id, tenant_id, list_id, column_key, label, sort_order)
         VALUES ($1, $2, $3, 'title', 'Subject', 0)",
    )
    .bind(Uuid::now_v7())
    .bind(other)
    .bind(other_list)
    .execute(&app.pool)
    .await
    .expect("the other tenant's column is seeded");

    // This tenant's own list still resolves by that key, and it is this
    // tenant's — not the one seeded a moment ago.
    let mine = render(&app, &token, "mine").await;

    assert_eq!(mine.status, StatusCode::OK, "{}", mine.body);
    assert_eq!(mine.body["data"]["id"], list_id.to_string());
    assert_eq!(mine.body["data"]["title"], "Requisitions");

    // And the other tenant's list is not readable from here, by id either.
    assert_eq!(
        rows(&app, &token, other_list, "").await.status,
        StatusCode::NOT_FOUND,
        "another tenant's list must not serve rows here"
    );
}

// ---------------------------------------------------------------------------
// The action catalogue (§5.10)
// ---------------------------------------------------------------------------

/// Seeds one action. There is no write API for `rad_actions` — [#340] gave the
/// table its first *reader*, and a writer is the builder's (#341) — so a test
/// that needs a row inserts one.
#[allow(clippy::too_many_arguments)]
async fn seed_action(
    app: &TestApp,
    tenant_id: Uuid,
    action_key: &str,
    context: &str,
    required_permission: Option<&str>,
    is_enabled: bool,
    sort_order: i32,
) -> Uuid {
    let id = Uuid::now_v7();

    sqlx::query(
        "INSERT INTO rad_actions
             (id, tenant_id, action_key, label, context, action_type,
              config_json, required_permission, sort_order, is_enabled)
         VALUES ($1, $2, $3, $4, $5, 'NAVIGATE', '{}'::jsonb, $6, $7, $8)",
    )
    .bind(id)
    .bind(tenant_id)
    .bind(action_key)
    .bind(action_key)
    .bind(context)
    .bind(required_permission)
    .bind(sort_order)
    .bind(is_enabled)
    .execute(&app.pool)
    .await
    .expect("the action is seeded");

    id
}

async fn actions(app: &TestApp, token: &str, context: &str) -> common::TestResponse {
    app.send(
        Method::GET,
        &format!("/api/v1/rad/actions?context={context}"),
        Some(token),
        None,
    )
    .await
}

fn keys(body: &Value) -> Vec<String> {
    body["data"]
        .as_array()
        .expect("a list")
        .iter()
        .map(|action| action["actionKey"].as_str().unwrap_or_default().to_owned())
        .collect()
}

#[tokio::test]
async fn the_catalogue_serves_one_context_in_its_configured_order() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let tenant = fixtures::SYSTEM_TENANT_ID;

    seed_action(&app, tenant, "second", "LIST", None, true, 20).await;
    seed_action(&app, tenant, "first", "LIST", None, true, 10).await;
    // **A second subject**: an action of another context. One context cannot
    // tell *scoped by context* from *not scoped at all*.
    seed_action(&app, tenant, "on-a-document", "DOCUMENT", None, true, 0).await;

    let response = actions(&app, &token, "LIST").await;

    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    assert_eq!(keys(&response.body), ["first", "second"]);
}

#[tokio::test]
async fn a_disabled_action_is_not_offered() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let tenant = fixtures::SYSTEM_TENANT_ID;

    seed_action(&app, tenant, "live", "LIST", None, true, 0).await;
    seed_action(&app, tenant, "switched-off", "LIST", None, false, 1).await;

    assert_eq!(keys(&actions(&app, &token, "LIST").await.body), ["live"]);
}

/// **The security control this endpoint has instead of a permission of its
/// own.** An action the caller cannot invoke is not returned, rather than
/// returned and disabled — a disabled button states that the thing exists and
/// is not for you, which is what `required_permission` was set to withhold.
///
/// The mutation that must make this red is returning every row from
/// `service::action::list_actions` regardless of `caller.holds`.
#[tokio::test]
async fn an_action_the_caller_may_not_invoke_is_not_returned() {
    let app = TestApp::spawn().await;
    let tenant = fixtures::SYSTEM_TENANT_ID;

    seed_action(&app, tenant, "open-to-all", "LIST", None, true, 0).await;
    seed_action(
        &app,
        tenant,
        "needs-delete",
        "LIST",
        Some("document:delete"),
        true,
        1,
    )
    .await;

    // The administrator holds everything, and sees both. Without this half, a
    // service that returned nothing at all would pass the assertion below.
    let administrator = app.administrator_token().await;

    assert_eq!(
        keys(&actions(&app, &administrator, "LIST").await.body),
        ["open-to-all", "needs-delete"]
    );

    // A caller holding `document:read` and not `document:delete`.
    let narrower = caller_holding(&app, &["document:read"], 7).await;

    assert_eq!(
        keys(&actions(&app, &narrower, "LIST").await.body),
        ["open-to-all"],
        "an action gated on a permission the caller lacks must not be returned"
    );
}

#[tokio::test]
async fn an_action_in_another_tenant_is_not_offered_here() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    seed_action(
        &app,
        fixtures::SYSTEM_TENANT_ID,
        "mine",
        "LIST",
        None,
        true,
        0,
    )
    .await;

    let other = fixtures::create_tenant(&app.pool, "TNT-ACTIONS", "Another Customer").await;

    seed_action(&app, other, "theirs", "LIST", None, true, 0).await;

    assert_eq!(keys(&actions(&app, &token, "LIST").await.body), ["mine"]);
}

#[tokio::test]
async fn a_context_outside_the_vocabulary_is_refused_rather_than_answered_empty() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    // An empty list would read as *this deployment has configured no actions*,
    // which is a different and wrong answer to a wrong question.
    assert_eq!(
        actions(&app, &token, "SIDEBAR").await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
}
