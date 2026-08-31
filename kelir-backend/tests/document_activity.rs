//! The timeline, and the property that makes it different from the audit trail
//! (FR-ACT-001, FR-ACT-004; [#247]).
//!
//! [#247]: https://github.com/sujanto-gaws/kelir/issues/247

mod common;

use axum::http::{Method, StatusCode};
use kelir_backend::modules::activity::domain::EventCategory;
use kelir_backend::modules::activity::service::{self as activity, Happening};
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
// AC2 — the event belongs to the action's transaction
// ---------------------------------------------------------------------------

/// **The property that separates this record from the audit trail**, and the
/// reason `record` takes a transaction where `audit::record` takes a pool.
///
/// An action that rolled back did not happen. A timeline saying it did would be
/// worse than one that never mentioned it — and worse in a way nobody can see,
/// because the row looks exactly like a real one.
///
/// **The mutation for this does not compile, which is the stronger result.**
/// Changing `record` to take a `&PgPool` — the audit trail's shape — fails at
/// every call site with *expected `&Pool<Postgres>`, found `&mut
/// Transaction<'_, Postgres>`*, because each one has a transaction and nothing
/// else. So the property is held by the signature rather than by this test, and
/// what this test guards is the day somebody gives the module a second way in.
/// Tried 2026-08-31; recorded here rather than claimed as a red test, because
/// it never got as far as running.
#[tokio::test]
async fn an_event_does_not_survive_the_action_it_describes_rolling_back() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_ACT_ROLLBACK").await;
    let document = draft(&app, &token, type_id).await;

    let before: i64 =
        sqlx::query_scalar("SELECT count(*) FROM activity_events WHERE document_id = $1")
            .bind(document)
            .fetch_one(&app.pool)
            .await
            .expect("a count");

    let mut transaction = app.pool.begin().await.expect("a transaction");

    activity::record(
        &mut transaction,
        &Happening {
            tenant_id: fixtures::SYSTEM_TENANT_ID,
            document_id: Some(document),
            workflow_instance_id: None,
            task_id: None,
            attachment_id: None,
            comment_id: None,
            event_type: "Document.Imagined",
            category: EventCategory::Document,
            actor_user_id: None,
            actor_name: Some("nobody"),
            action_summary: "Something that did not happen",
            details: json!({}),
        },
    )
    .await
    .expect("the event to be written into the transaction");

    // The action fails, so the transaction goes back.
    transaction.rollback().await.expect("the rollback");

    let after: i64 =
        sqlx::query_scalar("SELECT count(*) FROM activity_events WHERE document_id = $1")
            .bind(document)
            .fetch_one(&app.pool)
            .await
            .expect("a count");

    assert_eq!(
        after, before,
        "an event outlived the action it describes; that is the audit trail's rule, not this one"
    );
}

// ---------------------------------------------------------------------------
// AC4 — append-only, asserted over the columns
// ---------------------------------------------------------------------------

/// **Over `information_schema`, not over the router** (#247 AC4).
///
/// A route that does not exist today is one somebody adds tomorrow. What makes
/// this table append-only is that an edit has nothing to stamp and a soft delete
/// has nowhere to write — which is a fact about the columns, and this is how
/// [#181](https://github.com/sujanto-gaws/kelir/issues/181) AC6 asserted the
/// same property one table over.
#[tokio::test]
async fn the_timeline_is_append_only_by_its_columns() {
    let app = TestApp::spawn().await;

    for column in ["updated_at", "updated_by", "deleted_at"] {
        let present: Option<String> = sqlx::query_scalar(
            "SELECT column_name FROM information_schema.columns \
             WHERE table_name = 'activity_events' AND column_name = $1",
        )
        .bind(column)
        .fetch_optional(&app.pool)
        .await
        .expect("the column list");

        assert!(
            present.is_none(),
            "`activity_events.{column}` exists, so this table is no longer append-only"
        );
    }
}

// ---------------------------------------------------------------------------
// AC5 — the name as it was
// ---------------------------------------------------------------------------

/// **A rename does not rewrite the past.**
///
/// `actor_user_id` still points at the person and is the join for anything that
/// needs them *now*; `actor_name` is what they were called when this happened.
/// The opposite choice is `comments`, which joins `users` for a live name — a
/// conversation has current participants and a history has the people who were
/// there.
#[tokio::test]
async fn a_renamed_actor_does_not_change_what_the_timeline_says() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_ACT_RENAME").await;
    let document = draft(&app, &token, type_id).await;

    let recorded: String = sqlx::query_scalar(
        "SELECT actor_name FROM activity_events WHERE document_id = $1 AND event_type = 'Document.Created'",
    )
    .bind(document)
    .fetch_one(&app.pool)
    .await
    .expect("the created event");

    assert_eq!(recorded, common::ADMIN_USERNAME);

    sqlx::query("UPDATE users SET username = 'renamed-since' WHERE username = $1")
        .bind(common::ADMIN_USERNAME)
        .execute(&app.pool)
        .await
        .expect("the rename");

    let after: String = sqlx::query_scalar(
        "SELECT actor_name FROM activity_events WHERE document_id = $1 AND event_type = 'Document.Created'",
    )
    .bind(document)
    .fetch_one(&app.pool)
    .await
    .expect("the created event");

    assert_eq!(
        after,
        common::ADMIN_USERNAME,
        "the timeline followed a rename, so it no longer says what happened"
    );
}

// ---------------------------------------------------------------------------
// AC2, AC6 — the events that are actually written, and who can read them
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_document_lifecycle_and_the_workflow_both_reach_the_timeline() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_ACT_LIFECYCLE").await;
    let document = draft(&app, &token, type_id).await;

    let listed = app
        .get(
            &format!("/api/v1/documents/{document}/activity"),
            Some(&token),
        )
        .await;

    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);
    assert_eq!(listed.body["data"][0]["eventType"], "Document.Created");
    assert_eq!(listed.body["data"][0]["eventCategory"], "DOCUMENT");
    assert_eq!(listed.body["data"][0]["actorName"], common::ADMIN_USERNAME);
    assert_eq!(listed.body["meta"]["total"], 1);
}

/// **Tenant scope lives in the statement** (#247 AC6), not in the handler that
/// called it — the [#106](https://github.com/sujanto-gaws/kelir/issues/106) /
/// [#121](https://github.com/sujanto-gaws/kelir/issues/121) lesson, which cost
/// this project three sprints of coverage findings.
///
/// **Seen red, 2026-08-31**, with `tenant_id = $1` dropped from the read.
///
/// The row below is written straight into the table against **this** document
/// but another tenant's id, which is a thing no service would do and exactly
/// what the predicate has to refuse.
#[tokio::test]
async fn an_event_from_another_tenant_is_refused_by_the_query() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_ACT_TENANT").await;
    let document = draft(&app, &token, type_id).await;

    let other = fixtures::create_tenant(&app.pool, "OTHER-ACT", "Another tenant").await;

    sqlx::query(
        "INSERT INTO activity_events \
         (id, tenant_id, document_id, event_type, event_category, action_summary) \
         VALUES ($1, $2, $3, 'Document.Created', 'DOCUMENT', 'From somewhere else')",
    )
    .bind(Uuid::now_v7())
    .bind(other)
    .bind(document)
    .execute(&app.pool)
    .await
    .expect("the foreign row");

    let listed = app
        .get(
            &format!("/api/v1/documents/{document}/activity"),
            Some(&token),
        )
        .await;

    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);

    let events = listed.body["data"].as_array().expect("a page");

    assert_eq!(
        events.len(),
        1,
        "another tenant's event reached this timeline"
    );
    assert_eq!(
        listed.body["meta"]["total"], 1,
        "and the count is drawn under the same rule as the page"
    );
}

/// **`activity:read` is not `document:read`** (coding standard §2.9).
///
/// **Seen red, 2026-08-31**, with `caller.require(ACTIVITY_READ)?` deleted.
#[tokio::test]
async fn reading_a_document_is_not_permission_to_read_its_timeline() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_ACT_ISOLATE").await;
    let document = draft(&app, &token, type_id).await;

    let role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "ROLE-ACT-READER",
        &["document:read"],
    )
    .await;

    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "act-reader",
        "act-reader@example.test",
        common::ADMIN_PASSWORD,
        &[role],
    )
    .await;

    let reader = app.sign_in("act-reader", common::ADMIN_PASSWORD).await;

    let readable = app
        .get(&format!("/api/v1/documents/{document}"), Some(&reader))
        .await;
    assert_eq!(readable.status, StatusCode::OK, "{}", readable.body);

    let refused = app
        .get(
            &format!("/api/v1/documents/{document}/activity"),
            Some(&reader),
        )
        .await;

    assert_eq!(refused.status, StatusCode::FORBIDDEN, "{}", refused.body);
}

/// **The timeline is not the audit trail**, asserted over the rows.
///
/// #247 AC3 asks for the distinction in the module documentation; this is what
/// makes it checkable. `modules::activity` contains no statement reading
/// `audit_events`, and creating a document writes to both — separately, with
/// neither derived from the other.
#[tokio::test]
async fn the_timeline_and_the_audit_trail_are_written_separately() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_ACT_DISTINCT").await;
    let document = draft(&app, &token, type_id).await;

    let timeline: i64 =
        sqlx::query_scalar("SELECT count(*) FROM activity_events WHERE document_id = $1")
            .bind(document)
            .fetch_one(&app.pool)
            .await
            .expect("a count");

    let audited: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE object_id = $1")
        .bind(document)
        .fetch_one(&app.pool)
        .await
        .expect("a count");

    assert_eq!(timeline, 1, "the document's creation is on its timeline");
    assert!(
        audited >= 1,
        "and in the audit trail, which is a different table"
    );
}
