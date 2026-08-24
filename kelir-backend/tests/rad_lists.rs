//! List definition storage, through the API (#156, FR-RAD-003).
//!
//! The behaviour worth testing here is the wholesale replacement of the two
//! child collections: a caller sends the whole array, so an edit that keeps
//! stale rows or drops the wrong ones is invisible to a test that only checks
//! the list's own columns.

mod common;

use axum::http::{Method, StatusCode};
use common::TestApp;
use serde_json::{json, Value};
use uuid::Uuid;

async fn create_list(app: &TestApp, token: &str, body: Value) -> Value {
    let response = app
        .send(Method::POST, "/api/v1/rad/lists", Some(token), Some(body))
        .await;

    assert_eq!(
        response.status,
        StatusCode::CREATED,
        "creating the list failed: {}",
        response.body
    );

    response.body["data"].clone()
}

fn id_of(list: &Value) -> Uuid {
    list["id"]
        .as_str()
        .expect("the response carries an id")
        .parse()
        .expect("the id is a uuid")
}

fn full_list(key: &str) -> Value {
    json!({
        "listKey": key,
        "title": "Purchase requisitions",
        "defaultSort": [{"key": "created_at", "dir": "desc"}],
        "pageSize": 25,
        "columns": [
            {"columnKey": "document_number", "label": "Number", "dataType": "STRING"},
            {"columnKey": "amount", "label": "Amount", "dataType": "NUMBER", "format": "currency"}
        ],
        "filters": [
            {"filterKey": "status", "label": "Status", "filterType": "ENUM", "isDefault": true}
        ]
    })
}

#[tokio::test]
async fn a_list_is_created_with_its_columns_and_filters_and_read_back_in_order() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let created = create_list(&app, &token, full_list("pr-list")).await;

    assert_eq!(created["pageSize"], 25);
    assert_eq!(created["status"], "ACTIVE", "the default when none is sent");

    let id = id_of(&created);
    let read = app
        .send(
            Method::GET,
            &format!("/api/v1/rad/lists/{id}"),
            Some(&token),
            None,
        )
        .await;

    assert_eq!(read.status, StatusCode::OK);

    let columns = read.body["data"]["columns"].as_array().expect("columns");
    assert_eq!(columns.len(), 2);
    assert_eq!(
        columns[0]["columnKey"], "document_number",
        "the caller sent an ordered array and gets one back; sort order is \
         storage's business, not the caller's"
    );
    assert_eq!(columns[1]["columnKey"], "amount");
    assert_eq!(
        columns[0]["isSortable"], true,
        "a column omitting isSortable takes the stored default, not false"
    );

    let filters = read.body["data"]["filters"].as_array().expect("filters");
    assert_eq!(filters.len(), 1);
    assert_eq!(filters[0]["filterType"], "ENUM");
}

#[tokio::test]
async fn a_page_of_lists_omits_the_children() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    create_list(&app, &token, full_list("pr-page")).await;

    let listed = app
        .send(Method::GET, "/api/v1/rad/lists", Some(&token), None)
        .await;

    assert_eq!(listed.status, StatusCode::OK);
    assert!(
        listed.body["data"][0]["columns"].is_null(),
        "a page of twenty lists would otherwise carry every column of every one"
    );
}

/// Sending a collection replaces the stored set wholesale.
#[tokio::test]
async fn sending_columns_replaces_the_stored_set() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let created = create_list(&app, &token, full_list("pr-replace")).await;
    let id = id_of(&created);

    let updated = app
        .send(
            Method::PUT,
            &format!("/api/v1/rad/lists/{id}"),
            Some(&token),
            Some(json!({
                "columns": [{"columnKey": "requester", "label": "Requester"}]
            })),
        )
        .await;

    assert_eq!(updated.status, StatusCode::OK, "body {}", updated.body);

    let columns = updated.body["data"]["columns"].as_array().expect("columns");
    assert_eq!(columns.len(), 1, "the two original columns are gone");
    assert_eq!(columns[0]["columnKey"], "requester");

    // The filters were not sent, so they are untouched — the other half of
    // "a collection that *is* sent replaces the stored set".
    assert_eq!(
        updated.body["data"]["filters"]
            .as_array()
            .expect("filters")
            .len(),
        1,
        "a collection the caller did not send must be left alone"
    );

    // And no dead rows are left behind. Soft-deleting the replaced columns
    // would accumulate one per edit per column under a unique index that is
    // partial on `deleted_at IS NULL`.
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM rad_list_columns WHERE list_id = $1")
        .bind(id)
        .fetch_one(&app.pool)
        .await
        .expect("count is queryable");

    assert_eq!(rows, 1, "replaced columns are removed, not accumulated");
}

#[tokio::test]
async fn a_duplicate_column_key_is_refused_by_name() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let response = app
        .send(
            Method::POST,
            "/api/v1/rad/lists",
            Some(&token),
            Some(json!({
                "listKey": "pr-dupe-column",
                "title": "Duplicated",
                "columns": [
                    {"columnKey": "code", "label": "One"},
                    {"columnKey": "code", "label": "Two"}
                ]
            })),
        )
        .await;

    assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);

    let paths: Vec<&str> = response.body["error"]["details"]
        .as_array()
        .expect("details")
        .iter()
        .map(|detail| detail["path"].as_str().unwrap_or_default())
        .collect();

    assert!(
        paths.contains(&"columns.1.columnKey"),
        "the refusal names which one, so a caller can fix it; got {paths:?}"
    );
}

#[tokio::test]
async fn a_page_size_outside_the_stored_bounds_is_refused_by_name() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let response = app
        .send(
            Method::POST,
            "/api/v1/rad/lists",
            Some(&token),
            Some(json!({
                "listKey": "pr-bad-page",
                "title": "Bad page size",
                "pageSize": 0
            })),
        )
        .await;

    assert_eq!(
        response.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "the constraint would refuse it as a 500; the API refuses it as a 422"
    );
    assert_eq!(response.body["error"]["details"][0]["path"], "pageSize");
}

#[tokio::test]
async fn a_duplicate_list_key_is_a_conflict() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    create_list(&app, &token, full_list("pr-dupe-key")).await;

    let again = app
        .send(
            Method::POST,
            "/api/v1/rad/lists",
            Some(&token),
            Some(full_list("pr-dupe-key")),
        )
        .await;

    assert_eq!(again.status, StatusCode::CONFLICT, "body {}", again.body);
}

#[tokio::test]
async fn an_update_records_the_collections_that_moved() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let created = create_list(&app, &token, full_list("pr-audit-list")).await;
    let id = id_of(&created);

    // The title moves; the filters are re-sent identically.
    let response = app
        .send(
            Method::PUT,
            &format!("/api/v1/rad/lists/{id}"),
            Some(&token),
            Some(json!({
                "title": "Renamed",
                "filters": [
                    {"filterKey": "status", "label": "Status", "filterType": "ENUM", "isDefault": true}
                ]
            })),
        )
        .await;

    assert_eq!(response.status, StatusCode::OK, "body {}", response.body);

    let (old_value, new_value): (Value, Value) = sqlx::query_as(
        "SELECT old_value_json, new_value_json FROM audit_events
         WHERE object_id = $1 AND action = 'UPDATE'
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(id)
    .fetch_one(&app.pool)
    .await
    .expect("the update was audited");

    assert_eq!(old_value["title"], "Purchase requisitions");
    assert_eq!(new_value["title"], "Renamed");
    assert!(
        new_value.get("filters").is_none(),
        "re-sending an identical collection changed nothing, so it is not in the \
         record: a caller that always sends the whole list would otherwise write \
         a change record on every save; got {new_value}"
    );
}

#[tokio::test]
async fn a_deleted_list_is_gone_from_reads() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let created = create_list(&app, &token, full_list("pr-delete-list")).await;
    let id = id_of(&created);

    let deleted = app
        .send(
            Method::DELETE,
            &format!("/api/v1/rad/lists/{id}"),
            Some(&token),
            None,
        )
        .await;

    assert_eq!(deleted.status, StatusCode::NO_CONTENT);

    let read = app
        .send(
            Method::GET,
            &format!("/api/v1/rad/lists/{id}"),
            Some(&token),
            None,
        )
        .await;

    assert_eq!(read.status, StatusCode::NOT_FOUND);
}
