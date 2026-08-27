//! The document's own status and its transitions (#169).
//!
//! **The model worth copying rather than the code worth reusing.** [record 03]
//! called the `record_status` implementation the strongest thing in Sprint 6 —
//! a legality table red under mutation, an unreachable-by-direct-edit status, a
//! lost-race 409, and a transition permission that is not the update
//! permission. Every one of those four has a test here, and the fourth of them
//! is the one record 03 found *missing* in the facility arm of that very
//! service (#139).
//!
//! [record 03]: ../../projects/verifications/03.%20Sprint%206%20Surface%20Verification.md

mod common;

use std::sync::Arc;

use axum::http::{Method, StatusCode};
use common::{fixtures, TestApp};
use kelir_backend::modules::document::domain::DocumentStatus;
use kelir_backend::modules::document::repository as document_repo;
use serde_json::{json, Value};
use uuid::Uuid;

/// Enough simultaneous callers that the winner is decided by the database
/// rather than by the order tasks happened to be spawned in.
const CONCURRENT_TRANSITIONS: usize = 8;

fn definition(form_id: &str) -> Value {
    json!({
        "formId": form_id,
        "version": "2.0.1",
        "components": [{
            "id": "subject-field", "role": "data", "type": "textfield",
            "key": "subject", "label": "Subject",
            "validation": {"type": "string", "required": true, "maxLength": 200}
        }]
    })
}

async fn submitted_document(app: &TestApp, token: &str, code: &str) -> Uuid {
    let key = code.to_lowercase().replace('_', "-");

    let created = app
        .send(
            Method::POST,
            "/api/v1/rad/forms",
            Some(token),
            Some(json!({
                "formKey": key,
                "title": "Purchase requisition",
                "definition": definition(&key),
            })),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let form = id_of(&created.body["data"]);

    let published = app
        .send(
            Method::POST,
            &format!("/api/v1/rad/forms/{form}/publish"),
            Some(token),
            None,
        )
        .await;
    assert_eq!(published.status, StatusCode::OK, "{}", published.body);

    let type_created = app
        .send(
            Method::POST,
            "/api/v1/document-types",
            Some(token),
            Some(json!({ "typeCode": code, "name": code, "formId": form })),
        )
        .await;
    assert_eq!(
        type_created.status,
        StatusCode::CREATED,
        "{}",
        type_created.body
    );
    let type_id = id_of(&type_created.body["data"]);

    let rule = app
        .send(
            Method::PUT,
            &format!("/api/v1/document-types/{type_id}/numbering-rule"),
            Some(token),
            Some(json!({ "ruleTemplate": "PR-{sequence}", "sequenceScope": "GLOBAL" })),
        )
        .await;
    assert_eq!(rule.status, StatusCode::OK, "{}", rule.body);

    let document = app
        .send(
            Method::POST,
            "/api/v1/documents",
            Some(token),
            Some(json!({
                "documentTypeId": type_id,
                "title": "A requisition",
                "formData": {"subject": "Two standing desks"},
            })),
        )
        .await;
    assert_eq!(document.status, StatusCode::CREATED, "{}", document.body);
    let id = id_of(&document.body["data"]);

    // Submitted the way a person would, through #168, rather than by moving the
    // column: this file's subject is what happens *after* a document is
    // committed, and a fixture that faked the commit would be asserting over a
    // state the product cannot produce.
    let submitted = app
        .send(
            Method::POST,
            &format!("/api/v1/documents/{id}/submission"),
            Some(token),
            None,
        )
        .await;
    assert_eq!(submitted.status, StatusCode::OK, "{}", submitted.body);

    id
}

async fn transition(app: &TestApp, token: &str, id: Uuid, body: Value) -> common::TestResponse {
    app.send(
        Method::PUT,
        &format!("/api/v1/documents/{id}/status"),
        Some(token),
        Some(body),
    )
    .await
}

fn id_of(value: &Value) -> Uuid {
    value["id"]
        .as_str()
        .expect("an id")
        .parse()
        .expect("a uuid")
}

// ---------------------------------------------------------------------------
// AC1, AC2 — the legality table, and the shape of the route
// ---------------------------------------------------------------------------

/// The documented path is walkable through the API, not only in the enum's own
/// unit tests.
#[tokio::test]
async fn a_document_walks_its_lifecycle_end_to_end() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let id = submitted_document(&app, &token, "PR_WALK").await;

    for (from, to) in [
        ("SUBMITTED", "IN_REVIEW"),
        ("IN_REVIEW", "RETURNED"),
        ("RETURNED", "SUBMITTED"),
        ("SUBMITTED", "APPROVED"),
        ("APPROVED", "COMPLETED"),
    ] {
        let moved = transition(&app, &token, id, json!({ "status": to })).await;

        assert_eq!(
            moved.status,
            StatusCode::OK,
            "{from} -> {to}: {}",
            moved.body
        );
        assert_eq!(moved.body["data"]["previousStatus"], from);
        assert_eq!(moved.body["data"]["status"], to);
    }

    // COMPLETED is terminal, and a refusal from it says so rather than listing
    // an empty set.
    let refused = transition(&app, &token, id, json!({ "status": "CANCELLED" })).await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert!(
        refused.body["error"]["details"][0]["message"]
            .as_str()
            .expect("a message")
            .contains("final"),
        "{}",
        refused.body
    );
}

/// **An illegal transition is refused with the envelope naming both ends** (AC1).
#[tokio::test]
async fn an_illegal_transition_names_both_ends_and_what_was_possible() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let id = submitted_document(&app, &token, "PR_ILLEGAL").await;

    let refused = transition(&app, &token, id, json!({ "status": "COMPLETED" })).await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );

    let detail = &refused.body["error"]["details"][0];
    assert_eq!(detail["code"], "ILLEGAL_TRANSITION");
    assert_eq!(detail["path"], "status");

    let message = detail["message"].as_str().expect("a message");
    assert!(message.contains("SUBMITTED"), "{message}");
    assert!(message.contains("COMPLETED"), "{message}");
    assert!(message.contains("APPROVED"), "{message}");

    let status: String = sqlx::query_scalar("SELECT status FROM documents WHERE id = $1")
        .bind(id)
        .fetch_one(&app.pool)
        .await
        .expect("the document is readable");

    assert_eq!(
        status, "SUBMITTED",
        "a refused transition moved the document"
    );
}

/// **A transition is a verb sub-resource and not a field on the update
/// payload** (AC2).
///
/// #99's AC1, one module over. Letting an ordinary edit carry `status` would put
/// approval behind `document:update`, and `deny_unknown_fields` is what refuses
/// it — the same mechanism `UpdateDocumentTypeRequest` uses.
#[tokio::test]
async fn an_update_cannot_carry_a_status() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let id = submitted_document(&app, &token, "PR_FIELD").await;

    let refused = app
        .send(
            Method::PUT,
            &format!("/api/v1/documents/{id}"),
            Some(&token),
            Some(json!({ "status": "APPROVED" })),
        )
        .await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "an update carried a status: {}",
        refused.body
    );

    let status: String = sqlx::query_scalar("SELECT status FROM documents WHERE id = $1")
        .bind(id)
        .fetch_one(&app.pool)
        .await
        .expect("the document is readable");

    assert_eq!(status, "SUBMITTED");
}

/// **A draft is not submitted through the transition route.**
///
/// Submitting is a transaction that takes a number, and reaching its status half
/// through this door would produce a submitted document with no number — the
/// outcome #168 calls unrecoverable. The refusal names the endpoint that does
/// it, because a caller told only "DRAFT cannot become SUBMITTED" would
/// reasonably conclude the product cannot submit documents.
#[tokio::test]
async fn a_draft_is_not_submitted_through_the_transition_route() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let created = app
        .send(
            Method::POST,
            "/api/v1/document-types",
            Some(&token),
            Some(json!({ "typeCode": "PR_DRAFT_MOVE", "name": "PR_DRAFT_MOVE" })),
        )
        .await;
    let type_id = id_of(&created.body["data"]);

    let document = app
        .send(
            Method::POST,
            "/api/v1/documents",
            Some(&token),
            Some(json!({ "documentTypeId": type_id, "title": "Still a draft" })),
        )
        .await;
    let id = id_of(&document.body["data"]);

    let refused = transition(&app, &token, id, json!({ "status": "SUBMITTED" })).await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert!(
        refused.body["error"]["details"][0]["message"]
            .as_str()
            .expect("a message")
            .contains("/submission"),
        "the refusal does not name the endpoint that submits: {}",
        refused.body
    );

    let number: Option<String> =
        sqlx::query_scalar("SELECT document_number FROM documents WHERE id = $1")
            .bind(id)
            .fetch_one(&app.pool)
            .await
            .expect("the document is readable");

    assert_eq!(number, None);
}

/// **Nothing reaches a state Phase 5 owns** (AC5).
///
/// `PENDING_APPROVAL` is where a running approval puts a document, and nothing
/// can run an approval. A document put there today would await an approver that
/// does not exist — the overstatement #99 removed from `record_status`,
/// reintroduced one module over. `ARCHIVED` is the same shape for a different
/// reason: FR-DOC-010 is Sprint 9's cut tail.
#[tokio::test]
async fn no_transition_reaches_a_state_this_sprint_does_not_own() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let id = submitted_document(&app, &token, "PR_PHASE_FIVE").await;

    for unreachable in ["PENDING_APPROVAL", "ARCHIVED"] {
        let refused = transition(&app, &token, id, json!({ "status": unreachable })).await;

        assert_eq!(
            refused.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "a document reached {unreachable}: {}",
            refused.body
        );
    }
}

// ---------------------------------------------------------------------------
// AC3, AC7 — the lost race
// ---------------------------------------------------------------------------

/// **A concurrent transition loses the race, and exactly one caller decides the
/// document** (AC3).
///
/// Eight callers move one `SUBMITTED` document at once, four to `APPROVED` and
/// four to `REJECTED`. Exactly one may win. A decision recorded against a
/// document somebody else already decided is a signature on the wrong paper.
///
/// # A loser is refused two ways, and both are correct
///
/// **409** when it read `SUBMITTED`, decided the move was legal, and lost the
/// compare-and-swap. **422** when it acquired its connection *after* the winner
/// committed, read `APPROVED`, and was refused as an illegal move before it
/// reached the write at all. Which one a given caller gets depends on the
/// scheduler, so this asserts the property that is true of both — **nobody
/// succeeds who should not** — rather than a shape that holds only under a load
/// the test cannot control.
///
/// **That distinction was found by this test being flaky.** The first version
/// asserted every loser answered 409, passed alone and under the file, and went
/// red on five of eighteen runs of the Sprint 9 mutation campaign — where six
/// test binaries run concurrently against one database. Every one of those five
/// reds was this test rather than the predicate being mutated, which made the
/// campaign's ratio meaningless until it was fixed. Recorded as a finding of the
/// mid-sprint pass: *a flaky test is worse than no test, because it makes every
/// red run ambiguous.*
///
/// **The compare-and-swap itself is held by
/// [`a_stale_transition_writes_nothing`]**, which drives the statement directly
/// and is deterministic. This one holds the property a person cares about.
#[tokio::test]
async fn only_one_concurrent_transition_decides_the_document() {
    let app = Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;
    let id = submitted_document(&app, &token, "PR_RACE").await;

    let mut handles = Vec::with_capacity(CONCURRENT_TRANSITIONS);

    for index in 0..CONCURRENT_TRANSITIONS {
        let app = Arc::clone(&app);
        let token = token.clone();
        let target = if index % 2 == 0 {
            "APPROVED"
        } else {
            "REJECTED"
        };

        handles.push(tokio::spawn(async move {
            let response = transition(&app, &token, id, json!({ "status": target })).await;

            // The body travels with the status. A loser refused for a reason
            // that is neither of the two is a finding rather than noise, and a
            // bare `StatusCode` in the failure message would send whoever reads
            // it back to reproduce a race.
            (response.status, response.body.to_string())
        }));
    }

    let mut winners = 0;
    let mut losers = 0;
    let mut other = Vec::new();

    for handle in handles {
        match handle.await.expect("the transition task did not panic") {
            (StatusCode::OK, _) => winners += 1,
            (StatusCode::CONFLICT, _) | (StatusCode::UNPROCESSABLE_ENTITY, _) => losers += 1,
            outcome => other.push(outcome),
        }
    }

    assert_eq!(
        winners, 1,
        "{winners} callers each believe they decided this document"
    );
    assert!(
        other.is_empty(),
        "a loser was refused for a reason that is neither a lost race nor an          illegal move: {other:?}"
    );
    assert_eq!(losers, CONCURRENT_TRANSITIONS - 1);

    // And exactly one decision is on the record. Two contradictory rows is what
    // a silent overwrite leaves behind, and it is worse than the wrong status:
    // the status can be corrected and the history cannot.
    let decisions: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM document_status_history
         WHERE document_id = $1 AND new_status IN ('APPROVED', 'REJECTED')",
    )
    .bind(id)
    .fetch_one(&app.pool)
    .await
    .expect("the history is readable");

    assert_eq!(decisions, 1, "the history records more than one decision");
}

/// **A transition checked against a status the document has left writes
/// nothing** (AC3, deterministically).
///
/// The compare-and-swap, driven at the statement rather than through a race, so
/// that the guard has a test whose verdict does not depend on the scheduler.
/// `from` is the status the *check* read; binding it is the whole mechanism, and
/// this is the case a concurrent loser hits.
///
/// **Seen red** (coding standard §2.9) against `move_status`'s `status = $3`
/// weakened to `(status = $3 OR TRUE)`: the stale transition writes, and a
/// document that was approved a moment ago is rejected by a caller who never saw
/// the approval.
#[tokio::test]
async fn a_stale_transition_writes_nothing() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let id = submitted_document(&app, &token, "PR_STALE").await;

    // Somebody decides it.
    let approved = transition(&app, &token, id, json!({ "status": "APPROVED" })).await;
    assert_eq!(approved.status, StatusCode::OK, "{}", approved.body);

    // And a caller holding the status from *before* that writes nothing. The
    // legality check is bypassed deliberately: `SUBMITTED -> REJECTED` is a
    // legal move, so what is under test is the predicate rather than the table.
    let mut transaction = app.pool.begin().await.expect("a transaction");
    let moved = document_repo::move_status(
        &mut transaction,
        fixtures::SYSTEM_TENANT_ID,
        id,
        DocumentStatus::Submitted,
        DocumentStatus::Rejected,
        None,
    )
    .await
    .expect("the statement runs");
    transaction.commit().await.expect("the transaction commits");

    assert_eq!(
        moved, 0,
        "a transition checked against a status the document had left wrote anyway"
    );

    let status: String = sqlx::query_scalar("SELECT status FROM documents WHERE id = $1")
        .bind(id)
        .fetch_one(&app.pool)
        .await
        .expect("the document is readable");

    assert_eq!(
        status, "APPROVED",
        "a stale transition overwrote somebody else's decision"
    );
}

// ---------------------------------------------------------------------------
// AC4, AC7 — the permission
// ---------------------------------------------------------------------------

/// **A transition carries its own permission, not the update permission** (AC4).
///
/// Someone who may correct a requisition's line items is not thereby someone who
/// may approve it.
///
/// **Seen red** (§2.9) against a build where `service::status::transition`
/// requires `DOCUMENT_UPDATE`: the editor approves the document.
#[tokio::test]
async fn transitioning_needs_its_own_permission() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let id = submitted_document(&app, &token, "PR_TRANSITION_PERMISSION").await;

    // Everything the transition path *reads*, so that the refusal cannot be
    // about the wrong thing — the gate §2.9 describes.
    let role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "DOC-EDITOR-ONLY",
        &["document:read", "document:update", "document:submit"],
    )
    .await;

    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "doc.editor.only",
        "doc.editor.only@example.test",
        common::ADMIN_PASSWORD,
        &[role],
    )
    .await;

    let editor = app.sign_in("doc.editor.only", common::ADMIN_PASSWORD).await;

    let refused = transition(&app, &editor, id, json!({ "status": "APPROVED" })).await;

    assert_eq!(
        refused.status,
        StatusCode::FORBIDDEN,
        "a caller with document:update approved a document: {}",
        refused.body
    );

    let status: String = sqlx::query_scalar("SELECT status FROM documents WHERE id = $1")
        .bind(id)
        .fetch_one(&app.pool)
        .await
        .expect("the document is readable");

    assert_eq!(status, "SUBMITTED");
}

// ---------------------------------------------------------------------------
// AC6 — the trail and the history
// ---------------------------------------------------------------------------

/// **Every transition is audited as a status change, carrying both ends and the
/// reason** (AC6).
///
/// Its own action, distinct from `UPDATE` and from `SUBMIT`. An auditor asking
/// *who rejected this* must not have to read a payload to find out which kind of
/// write happened.
#[tokio::test]
async fn a_transition_is_audited_with_both_ends_and_its_reason() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let id = submitted_document(&app, &token, "PR_TRANSITION_AUDIT").await;

    let moved = transition(
        &app,
        &token,
        id,
        json!({ "status": "REJECTED", "reason": "The budget line is exhausted." }),
    )
    .await;

    assert_eq!(moved.status, StatusCode::OK, "{}", moved.body);

    let row = sqlx::query_as::<_, (String, String, Option<String>, Option<Value>, Option<Value>)>(
        "SELECT event_type, action, reason, old_value_json, new_value_json FROM audit_events
         WHERE object_id = $1 AND action = 'STATUS_CHANGE'",
    )
    .bind(id)
    .fetch_one(&app.pool)
    .await
    .expect("the transition was audited as a status change");

    assert_eq!(row.0, "Document.StatusChanged");
    assert_eq!(row.1, "STATUS_CHANGE");
    assert_eq!(row.2.as_deref(), Some("The budget line is exhausted."));
    assert_eq!(row.3.expect("both ends")["status"], "SUBMITTED");
    assert_eq!(row.4.expect("both ends")["status"], "REJECTED");
}

/// The history is readable, oldest first, and starts at the document's creation.
///
/// A history whose first row is a transition cannot answer *when was this
/// created and by whom* from inside itself, which is what a history is for.
#[tokio::test]
async fn the_status_history_starts_at_creation_and_reads_oldest_first() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let id = submitted_document(&app, &token, "PR_HISTORY").await;

    let moved = transition(
        &app,
        &token,
        id,
        json!({ "status": "IN_REVIEW", "reason": "Passed to finance." }),
    )
    .await;
    assert_eq!(moved.status, StatusCode::OK, "{}", moved.body);

    let history = app
        .send(
            Method::GET,
            &format!("/api/v1/documents/{id}/status-history"),
            Some(&token),
            None,
        )
        .await;

    assert_eq!(history.status, StatusCode::OK, "{}", history.body);

    let entries = history.body["data"]
        .as_array()
        .expect("the history is a list");

    assert_eq!(entries.len(), 3, "{}", history.body);
    assert_eq!(entries[0]["previousStatus"], Value::Null);
    assert_eq!(entries[0]["status"], "DRAFT");
    assert_eq!(entries[1]["status"], "SUBMITTED");
    assert_eq!(entries[2]["status"], "IN_REVIEW");
    assert_eq!(entries[2]["reason"], "Passed to finance.");
}

/// A history over another tenant's document answers 404 about the **document**.
///
/// An empty list would say "this document has no history", which is a false
/// statement about a document that is not theirs to know about.
#[tokio::test]
async fn a_history_over_a_document_that_is_not_yours_is_not_found() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let id = submitted_document(&app, &token, "PR_HISTORY_TENANT").await;

    // A document that does not exist at all, which is what another tenant's
    // document looks like from here — asserted in `documents.rs` against a real
    // second tenant.
    let missing = app
        .send(
            Method::GET,
            &format!("/api/v1/documents/{}/status-history", Uuid::now_v7()),
            Some(&token),
            None,
        )
        .await;

    assert_eq!(missing.status, StatusCode::NOT_FOUND, "{}", missing.body);

    // And the real one is readable, so the assertion above is not green because
    // the endpoint refuses everything.
    let found = app
        .send(
            Method::GET,
            &format!("/api/v1/documents/{id}/status-history"),
            Some(&token),
            None,
        )
        .await;

    assert_eq!(found.status, StatusCode::OK, "{}", found.body);
}
