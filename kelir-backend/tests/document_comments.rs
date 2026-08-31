//! A conversation about a document (FR-CMT-001; [#249]).
//!
//! [#249]: https://github.com/sujanto-gaws/kelir/issues/249

mod common;

use axum::http::{Method, StatusCode};
use serde_json::{json, Value};
use uuid::Uuid;

use common::{fixtures, TestApp};

fn id_of(value: &Value) -> Uuid {
    value["id"]
        .as_str()
        .expect("an id")
        .parse()
        .expect("a uuid")
}

async fn document_type(app: &TestApp, token: &str, code: &str) -> Uuid {
    let created = app
        .send(
            Method::POST,
            "/api/v1/document-types",
            Some(token),
            Some(json!({ "typeCode": code, "name": code })),
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    id_of(&created.body["data"])
}

async fn draft(app: &TestApp, token: &str, type_id: Uuid) -> Uuid {
    let created = app
        .send(
            Method::POST,
            "/api/v1/documents",
            Some(token),
            Some(json!({ "documentTypeId": type_id, "title": "Two standing desks" })),
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    id_of(&created.body["data"])
}

// ---------------------------------------------------------------------------
// AC2 — a comment is added, and read back
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_comment_is_added_to_a_document_and_read_back_with_its_author() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_CMT_ADD").await;
    let document = draft(&app, &token, type_id).await;

    let added = app
        .post(
            &format!("/api/v1/documents/{document}/comments"),
            Some(&token),
            json!({ "body": "  is this the right supplier?  " }),
        )
        .await;

    assert_eq!(added.status, StatusCode::OK, "{}", added.body);
    // Trimmed on the way in (#249 AC4).
    assert_eq!(added.body["data"]["body"], "is this the right supplier?");
    assert_eq!(added.body["data"]["authorUsername"], common::ADMIN_USERNAME);

    let listed = app
        .get(
            &format!("/api/v1/documents/{document}/comments"),
            Some(&token),
        )
        .await;

    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);
    assert_eq!(listed.body["data"].as_array().expect("a page").len(), 1);
    assert_eq!(listed.body["meta"]["total"], 1);
}

#[tokio::test]
async fn a_conversation_is_read_in_the_order_it_was_said() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_CMT_ORDER").await;
    let document = draft(&app, &token, type_id).await;

    for body in ["first", "second", "third"] {
        let added = app
            .post(
                &format!("/api/v1/documents/{document}/comments"),
                Some(&token),
                json!({ "body": body }),
            )
            .await;

        assert_eq!(added.status, StatusCode::OK, "{}", added.body);
    }

    let listed = app
        .get(
            &format!("/api/v1/documents/{document}/comments"),
            Some(&token),
        )
        .await;

    let bodies: Vec<&str> = listed.body["data"]
        .as_array()
        .expect("a page")
        .iter()
        .map(|comment| comment["body"].as_str().expect("a body"))
        .collect();

    // **Oldest first**, which is the opposite of every other list in this
    // product: a conversation is read in the order it was said.
    assert_eq!(bodies, vec!["first", "second", "third"]);
}

/// **The count and the page apply the same rule**, which is
/// [#279](https://github.com/sujanto-gaws/kelir/issues/279)'s lesson applied
/// where the same duplication exists: a second document's comments must not
/// reach either.
///
/// **Seen red, 2026-08-31**, with `document_id = $2` dropped from
/// `count_for_document`: the page stayed at 2 and the total became 5.
#[tokio::test]
async fn the_total_counts_this_documents_comments_and_no_others() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_CMT_SCOPE").await;
    let mine = draft(&app, &token, type_id).await;
    let theirs = draft(&app, &token, type_id).await;

    for _ in 0..2 {
        app.post(
            &format!("/api/v1/documents/{mine}/comments"),
            Some(&token),
            json!({ "body": "on mine" }),
        )
        .await;
    }

    for _ in 0..3 {
        app.post(
            &format!("/api/v1/documents/{theirs}/comments"),
            Some(&token),
            json!({ "body": "on the other one" }),
        )
        .await;
    }

    let listed = app
        .get(&format!("/api/v1/documents/{mine}/comments"), Some(&token))
        .await;

    let page = listed.body["data"].as_array().expect("a page").len();

    assert_eq!(page, 2, "the page is scoped to this document");
    assert_eq!(
        listed.body["meta"]["total"], 2,
        "and so is the count: a total the page cannot account for is unreadable"
    );
}

// ---------------------------------------------------------------------------
// AC4 — the body's own refusals
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_comment_of_whitespace_is_refused_and_names_the_field() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_CMT_EMPTY").await;
    let document = draft(&app, &token, type_id).await;

    let refused = app
        .post(
            &format!("/api/v1/documents/{document}/comments"),
            Some(&token),
            json!({ "body": "   \n  " }),
        )
        .await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(refused.body["error"]["details"][0]["path"], "body");
    assert_eq!(refused.body["error"]["details"][0]["code"], "COMMENT_EMPTY");

    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM comments")
        .fetch_one(&app.pool)
        .await
        .expect("a count");

    assert_eq!(rows, 0);
}

#[tokio::test]
async fn a_comment_over_the_bound_is_refused_before_the_document_is_read() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    // A document that does not exist: if the bound were checked *after* the
    // read, this would answer 404 instead — which is the ordering the service
    // documents and this is what holds it to it.
    let absent = Uuid::now_v7();
    let refused = app
        .post(
            &format!("/api/v1/documents/{absent}/comments"),
            Some(&token),
            json!({ "body": "x".repeat(4001) }),
        )
        .await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(refused.body["error"]["details"][0]["code"], "TOO_LONG");
}

// ---------------------------------------------------------------------------
// Who may comment, and on what
// ---------------------------------------------------------------------------

#[tokio::test]
async fn commenting_on_a_document_that_does_not_exist_answers_404_and_stores_nothing() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let absent = Uuid::now_v7();

    let refused = app
        .post(
            &format!("/api/v1/documents/{absent}/comments"),
            Some(&token),
            json!({ "body": "about a document that is not there" }),
        )
        .await;

    assert_eq!(refused.status, StatusCode::NOT_FOUND, "{}", refused.body);

    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM comments")
        .fetch_one(&app.pool)
        .await
        .expect("a count");

    assert_eq!(rows, 0);
}

/// **The two permissions are isolated from each other** (coding standard §2.9).
///
/// A caller holding neither would be refused by whichever check ran first, so
/// removing either would leave such a test green — the gate that let five
/// predicates through in Sprint 8, and the one
/// [#244](https://github.com/sujanto-gaws/kelir/issues/244) hit again one module
/// over. This caller holds `document:read` and **not** `comment:create`.
#[tokio::test]
async fn reading_a_document_is_not_permission_to_comment_on_it() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_CMT_ISOLATE").await;
    let document = draft(&app, &token, type_id).await;

    let role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "ROLE-CMT-READER",
        &["document:read"],
    )
    .await;

    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "cmt-reader",
        "cmt-reader@example.test",
        common::ADMIN_PASSWORD,
        &[role],
    )
    .await;

    let reader = app.sign_in("cmt-reader", common::ADMIN_PASSWORD).await;

    let readable = app
        .get(&format!("/api/v1/documents/{document}"), Some(&reader))
        .await;
    assert_eq!(readable.status, StatusCode::OK, "{}", readable.body);

    let refused = app
        .post(
            &format!("/api/v1/documents/{document}/comments"),
            Some(&reader),
            json!({ "body": "may I?" }),
        )
        .await;

    assert_eq!(refused.status, StatusCode::FORBIDDEN, "{}", refused.body);
}

/// And the read side of the same isolation: `comment:create` is not
/// `comment:read`.
#[tokio::test]
async fn commenting_is_not_permission_to_read_the_conversation() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_CMT_WRITEONLY").await;
    let document = draft(&app, &token, type_id).await;

    let role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "ROLE-CMT-WRITER",
        &["document:read", "comment:create"],
    )
    .await;

    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "cmt-writer",
        "cmt-writer@example.test",
        common::ADMIN_PASSWORD,
        &[role],
    )
    .await;

    let writer = app.sign_in("cmt-writer", common::ADMIN_PASSWORD).await;

    let added = app
        .post(
            &format!("/api/v1/documents/{document}/comments"),
            Some(&writer),
            json!({ "body": "left for somebody else to read" }),
        )
        .await;
    assert_eq!(added.status, StatusCode::OK, "{}", added.body);

    let refused = app
        .get(
            &format!("/api/v1/documents/{document}/comments"),
            Some(&writer),
        )
        .await;

    assert_eq!(refused.status, StatusCode::FORBIDDEN, "{}", refused.body);
}

// ---------------------------------------------------------------------------
// The distinction the module documentation exists to state (#249 AC3)
// ---------------------------------------------------------------------------

/// **A comment is not a decision comment**, asserted over the rows rather than
/// over the prose.
///
/// #249 AC3 asks for the distinction in the module documentation; this is what
/// makes it checkable. Commenting on a document writes `comments` and touches
/// none of the three columns FR-TASK-006 writes — so a reader who assumes one is
/// the other has a failing test to read rather than a paragraph to believe.
#[tokio::test]
async fn commenting_writes_no_decision_record() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_CMT_DISTINCT").await;
    let document = draft(&app, &token, type_id).await;

    let added = app
        .post(
            &format!("/api/v1/documents/{document}/comments"),
            Some(&token),
            json!({ "body": "a conversation, not a decision" }),
        )
        .await;
    assert_eq!(added.status, StatusCode::OK, "{}", added.body);

    for table in ["approval_decisions", "workflow_history", "workflow_tasks"] {
        let rows: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(&app.pool)
            .await
            .expect("a count");

        assert_eq!(rows, 0, "commenting wrote a row in {table}");
    }

    let comments: i64 = sqlx::query_scalar("SELECT count(*) FROM comments")
        .fetch_one(&app.pool)
        .await
        .expect("a count");

    assert_eq!(comments, 1);
}
