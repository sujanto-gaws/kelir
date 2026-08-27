//! Submitting a draft, and taking its number in the same transaction (#168).
//!
//! **This file is about the submit, not about the allocator.**
//! `document_numbering.rs` holds `no_two_concurrent_allocations_take_the_same_number`,
//! which drives `allocate_in` directly at twenty-four concurrent callers. What
//! is here is that allocation happening **inside a real submit** — beside a
//! status change, a history row and #164's server-side re-evaluation, in one
//! transaction that commits whole or not at all. #168's own text names the two
//! failures that only exist at this level: a number burned by a failed submit,
//! and a document that is numbered but not submitted.
//!
//! Every test that names a control here has been seen to fail against a build
//! with that control removed (coding standard §2.9), and the doc comment on each
//! says what the mutation was and what it produced.

mod common;

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use axum::http::{Method, StatusCode};
use common::{fixtures, TestApp};
use kelir_backend::modules::document::repository as document_repo;
use serde_json::{json, Value};
use uuid::Uuid;

/// More than the test pool holds (`TEST_POOL_MAX_CONNECTIONS` is 5), which is
/// the concurrency #118 taught this project a harness has to actually reach: its
/// own tests could not, so a fix that closed a race and opened a
/// pool-exhaustion deadlock passed them all.
const CONCURRENT_SUBMITS: usize = 24;

/// A form with one `required` field and one `calculate`, which is everything a
/// submit has to answer for: the rule a draft was allowed to leave unsatisfied,
/// and the figure the client is not allowed to decide.
fn definition(form_id: &str) -> Value {
    json!({
        "formId": form_id,
        "version": "2.0.1",
        "title": "Purchase requisition",
        "components": [
            {
                "id": "subject-field", "role": "data", "type": "textfield",
                "key": "subject", "label": "Subject",
                "validation": {
                    "type": "string", "required": true, "maxLength": 200,
                    "messages": {"required": "Every request needs a subject."}
                }
            },
            {
                "id": "quantity-field", "role": "data", "type": "number",
                "key": "quantity", "label": "Quantity",
                "validation": {"type": "integer", "minimum": 1}
            },
            {
                "id": "unit-price-field", "role": "data", "type": "number",
                "key": "unit_price", "label": "Unit price",
                "validation": {"type": "number", "minimum": 0}
            },
            {
                "id": "total-field", "role": "data", "type": "number",
                "key": "total", "label": "Total",
                "validation": {"type": "number"},
                "calculate": {"*": [{"var": "unit_price"}, {"var": "quantity"}]}
            }
        ]
    })
}

fn filled_in() -> Value {
    json!({"subject": "Two standing desks", "quantity": 2, "unit_price": 10, "total": 999_999})
}

async fn published_form(app: &TestApp, token: &str, key: &str) -> Uuid {
    let created = app
        .send(
            Method::POST,
            "/api/v1/rad/forms",
            Some(token),
            Some(json!({
                "formKey": key,
                "title": "Purchase requisition",
                "definition": definition(key),
            })),
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let id = id_of(&created.body["data"]);

    let published = app
        .send(
            Method::POST,
            &format!("/api/v1/rad/forms/{id}/publish"),
            Some(token),
            None,
        )
        .await;

    assert_eq!(published.status, StatusCode::OK, "{}", published.body);

    id
}

/// A type with a published form and a numbering rule — everything a document
/// needs to get all the way to a number.
async fn numbered_type(app: &TestApp, token: &str, code: &str, gap_policy: &str) -> Uuid {
    let form = published_form(app, token, &code.to_lowercase().replace('_', "-")).await;

    let created = app
        .send(
            Method::POST,
            "/api/v1/document-types",
            Some(token),
            Some(json!({ "typeCode": code, "name": code, "formId": form })),
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let type_id = id_of(&created.body["data"]);

    let rule = app
        .send(
            Method::PUT,
            &format!("/api/v1/document-types/{type_id}/numbering-rule"),
            Some(token),
            Some(json!({
                "ruleTemplate": "PR-{year}-{sequence}",
                "sequenceScope": "YEAR",
                "gapPolicy": gap_policy,
            })),
        )
        .await;

    assert_eq!(rule.status, StatusCode::OK, "{}", rule.body);

    type_id
}

async fn draft(app: &TestApp, token: &str, type_id: Uuid, form_data: Value) -> Uuid {
    let created = app
        .send(
            Method::POST,
            "/api/v1/documents",
            Some(token),
            Some(json!({
                "documentTypeId": type_id,
                "title": "Two standing desks",
                "formData": form_data,
            })),
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    id_of(&created.body["data"])
}

async fn submit(app: &TestApp, token: &str, id: Uuid) -> common::TestResponse {
    app.send(
        Method::POST,
        &format!("/api/v1/documents/{id}/submission"),
        Some(token),
        None,
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
// AC1 — one transaction: the status, the number and the server's payload
// ---------------------------------------------------------------------------

/// **The three writes land together, and the payload is the server's.**
///
/// The draft was created holding a `total` of 999999, which the creation already
/// overwrote; the submit re-evaluates again and stores 20. Asserting on the row
/// rather than the response, because what #168 is about is what is durable.
#[tokio::test]
async fn a_submit_moves_the_status_takes_the_number_and_stores_the_servers_payload() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = numbered_type(&app, &token, "PR_SUBMIT", "GAPLESS").await;
    let id = draft(&app, &token, type_id, filled_in()).await;

    let submitted = submit(&app, &token, id).await;

    assert_eq!(submitted.status, StatusCode::OK, "{}", submitted.body);
    assert_eq!(submitted.body["data"]["status"], "SUBMITTED");

    let row = sqlx::query_as::<
        _,
        (
            String,
            Option<String>,
            Value,
            Option<chrono::DateTime<chrono::Utc>>,
        ),
    >(
        "SELECT status, document_number, form_data_json, submitted_at FROM documents WHERE id = $1",
    )
    .bind(id)
    .fetch_one(&app.pool)
    .await
    .expect("the document is readable");

    assert_eq!(row.0, "SUBMITTED");
    assert!(
        row.1
            .as_deref()
            .is_some_and(|number| number.starts_with("PR-")),
        "a submitted document has no number: {:?}",
        row.1
    );
    assert_eq!(
        row.2["total"],
        json!(20.0),
        "the submit stored the client's arithmetic: {}",
        row.2
    );
    assert!(row.3.is_some(), "a submitted document has no submitted_at");

    // And the history explains how it got there, written in the same
    // transaction: a document cannot end in a state its own history is silent
    // about.
    let transitions: Vec<(Option<String>, String)> = sqlx::query_as(
        "SELECT old_status, new_status FROM document_status_history
         WHERE document_id = $1 ORDER BY created_at, id",
    )
    .bind(id)
    .fetch_all(&app.pool)
    .await
    .expect("the history is readable");

    assert_eq!(
        transitions,
        vec![
            (None, "DRAFT".to_owned()),
            (Some("DRAFT".to_owned()), "SUBMITTED".to_owned())
        ]
    );
}

// ---------------------------------------------------------------------------
// AC2 — a failed submit leaves a draft, and the sequence behaves as #158 decided
// ---------------------------------------------------------------------------

/// **A failed submit leaves a draft with no number, and a `GAPLESS` rule keeps
/// its counter.**
///
/// The draft is legitimate — a document with nothing in it is what a draft is,
/// and `Strictness::Draft` saved it happily. `required` is what the *submit*
/// enforces, so this is the two strictnesses meeting: the same payload, saved
/// and then refused.
///
/// **The number is not burned**, which is the failure #168's own text names:
/// the allocation happens inside this transaction on a `GAPLESS` rule, so the
/// rollback rolls the counter back with it. Asserted by submitting a *second*
/// document afterwards and seeing it take `000001` — the three-move shape #200
/// established, because a counter that is merely "still 1" and a counter that
/// was never touched are indistinguishable from one observation.
///
/// **This test stays green against allocate-before-validate, and that is worth
/// knowing rather than hiding.** On a `GAPLESS` rule the allocation is inside
/// the transaction, so even a number taken too early rolls back with it. The
/// test that catches the wrong order is the one below, on a gap-tolerant rule,
/// where the number is committed separately — and it was seen red against
/// exactly that mutation.
///
/// **Seen red** (§2.9) against the `status = 'DRAFT'` predicate removed from
/// `mark_submitted` together with the service's `is_editable` refusal: the
/// unfinished document is submitted.
#[tokio::test]
async fn a_failed_submit_leaves_a_draft_and_burns_no_number() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = numbered_type(&app, &token, "PR_ROLLBACK", "GAPLESS").await;

    // `subject` is required and absent — legal in a draft, refused at submit.
    let unfinished = draft(
        &app,
        &token,
        type_id,
        json!({"quantity": 2, "unit_price": 10}),
    )
    .await;

    let refused = submit(&app, &token, unfinished).await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "an unfinished document was submitted: {}",
        refused.body
    );
    assert_eq!(refused.body["error"]["details"][0]["path"], "subject");
    assert_eq!(refused.body["error"]["details"][0]["rule"], "required");

    let row = sqlx::query_as::<_, (String, Option<String>)>(
        "SELECT status, document_number FROM documents WHERE id = $1",
    )
    .bind(unfinished)
    .fetch_one(&app.pool)
    .await
    .expect("the document is readable");

    assert_eq!(row.0, "DRAFT", "a failed submit moved the status");
    assert_eq!(row.1, None, "a failed submit assigned a number");

    // The third move: the next document takes the number the failed one did not.
    let finished = draft(&app, &token, type_id, filled_in()).await;
    let submitted = submit(&app, &token, finished).await;

    assert_eq!(submitted.status, StatusCode::OK, "{}", submitted.body);
    assert_eq!(
        submitted.body["data"]["documentNumber"],
        format!("PR-{}-000001", chrono::Utc::now().format("%Y")),
        "a failed submit burned a number on a gapless rule: {}",
        submitted.body
    );
}

// ---------------------------------------------------------------------------
// AC3, AC4 — the race, at a concurrency the pool cannot absorb
// ---------------------------------------------------------------------------

/// **Concurrent submits of different documents of one type never take the same
/// number.**
///
/// Twenty-four documents of one type, submitted at once, at a concurrency above
/// the pool ceiling — the level [#118](https://github.com/sujanto-gaws/kelir/issues/118)
/// showed a fix's own tests can fail to reach.
///
/// The assertion is on the **set** and on the **range**: twenty-four distinct
/// numbers that ran 1..24 is a working counter, and twenty-four distinct numbers
/// scattered wider would mean something consumed numbers nobody holds. Distinct
/// alone would pass over a counter that skipped.
///
/// **Seen red** (§2.9) against the naive allocator AC4 names: `allocate_bucket`'s
/// single insert-or-advance replaced in `allocate_in` by a read of
/// `next_sequence` followed by a write of it. What that produces is not a
/// duplicate *number* — `uq_document_type_sequence_buckets_type_key` is still
/// there and refuses the losing writer — but a 500 on more than half the
/// submits, which is the same defect wearing a worse face: a person clicking
/// Submit is told the server broke, and the fix is not theirs to make. The
/// atomic statement is what turns that into a queue.
///
/// **It was also this test that found a real defect in Sprint 7's
/// `numbering_service::allocate`**, on its first run: the dispatcher read the
/// rule's gap policy from `state.pool` while its caller held a transaction, so a
/// submit cost two connections and twenty-four of them deadlocked a
/// five-connection pool. That is [#118](https://github.com/sujanto-gaws/kelir/issues/118)
/// exactly, and it survived Sprint 7 because no test called `allocate` under
/// load — `document_numbering.rs` drives `allocate_in`. Fixed by
/// `numbering_repository::gap_policy`, which reads the policy on the caller's
/// own connection.
#[tokio::test]
async fn no_two_concurrent_submits_take_the_same_number() {
    let app = Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;
    let type_id = numbered_type(&app, &token, "PR_CONCURRENT_SUBMIT", "GAPLESS").await;

    let mut documents = Vec::with_capacity(CONCURRENT_SUBMITS);
    for _ in 0..CONCURRENT_SUBMITS {
        documents.push(draft(&app, &token, type_id, filled_in()).await);
    }

    let mut handles = Vec::with_capacity(CONCURRENT_SUBMITS);

    for id in documents {
        let app = Arc::clone(&app);
        let token = token.clone();

        handles.push(tokio::spawn(async move {
            let response = submit(&app, &token, id).await;

            assert_eq!(
                response.status,
                StatusCode::OK,
                "a concurrent submit failed: {}",
                response.body
            );

            response.body["data"]["documentNumber"]
                .as_str()
                .expect("a submitted document has a number")
                .to_owned()
        }));
    }

    let mut numbers = Vec::with_capacity(CONCURRENT_SUBMITS);
    for handle in handles {
        numbers.push(handle.await.expect("the submit task did not panic"));
    }

    let distinct: HashSet<&String> = numbers.iter().collect();

    assert_eq!(
        distinct.len(),
        CONCURRENT_SUBMITS,
        "{} of {CONCURRENT_SUBMITS} submits took a number another document already \
         holds: {numbers:?}",
        CONCURRENT_SUBMITS - distinct.len()
    );

    let mut sorted: Vec<&String> = numbers.iter().collect();
    sorted.sort();

    let year = chrono::Utc::now().format("%Y").to_string();
    let expected: Vec<String> = (1..=CONCURRENT_SUBMITS)
        .map(|n| format!("PR-{year}-{n:06}"))
        .collect();
    let expected_refs: Vec<&String> = expected.iter().collect();

    assert_eq!(
        sorted, expected_refs,
        "the sequence is distinct but not contiguous, so numbers were consumed \
         by nothing"
    );

    // And every number is on a document. A distinct set that is not *attached*
    // is the allocate-then-submit failure passing the assertion above.
    let numbered: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM documents WHERE document_type_id = $1 AND document_number IS NOT NULL",
    )
    .bind(type_id)
    .fetch_one(&app.pool)
    .await
    .expect("the documents are readable");

    assert_eq!(numbered, CONCURRENT_SUBMITS as i64);
}

// ---------------------------------------------------------------------------
// AC5, AC6 — refusing a second submit, and what the trail says
// ---------------------------------------------------------------------------

/// **An already-submitted document is refused, not silently re-numbered.**
///
/// The second number is the damage: one document holding two numbers over its
/// life is a document nobody can reconcile against a purchase order.
///
/// **Seen red** against a build with the `!locked.status.is_editable()` refusal
/// removed from `service::submit` *and* the `status = 'DRAFT'` predicate removed
/// from `mark_submitted`'s `WHERE`: the document takes `000002`.
#[tokio::test]
async fn an_already_submitted_document_is_refused() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = numbered_type(&app, &token, "PR_TWICE", "GAPLESS").await;
    let id = draft(&app, &token, type_id, filled_in()).await;

    let first = submit(&app, &token, id).await;
    assert_eq!(first.status, StatusCode::OK, "{}", first.body);

    let number = first.body["data"]["documentNumber"].clone();

    let second = submit(&app, &token, id).await;

    assert_eq!(
        second.status,
        StatusCode::CONFLICT,
        "a document was submitted twice: {}",
        second.body
    );

    let stored: Option<String> =
        sqlx::query_scalar("SELECT document_number FROM documents WHERE id = $1")
            .bind(id)
            .fetch_one(&app.pool)
            .await
            .expect("the document is readable");

    assert_eq!(
        json!(stored),
        number,
        "a refused second submit changed the document's number"
    );
}

/// **A submit is audited as a submit, carrying the number** (AC6).
///
/// Its own event type and its own action, distinct from `UPDATE`. An auditor
/// asking *who committed this requisition, and what number did it get* must not
/// have to read a payload to find out which kind of write happened.
#[tokio::test]
async fn a_submit_is_audited_as_a_submit_carrying_its_number() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = numbered_type(&app, &token, "PR_SUBMIT_AUDIT", "GAPLESS").await;
    let id = draft(&app, &token, type_id, filled_in()).await;

    let submitted = submit(&app, &token, id).await;
    assert_eq!(submitted.status, StatusCode::OK, "{}", submitted.body);

    let row = sqlx::query_as::<_, (String, String, Option<Value>)>(
        "SELECT event_type, action, new_value_json FROM audit_events
         WHERE object_id = $1 AND action = 'SUBMIT'",
    )
    .bind(id)
    .fetch_one(&app.pool)
    .await
    .expect("the submit was audited as a submit");

    assert_eq!(row.0, "Document.Submitted");
    assert_eq!(row.1, "SUBMIT");

    let new_value = row.2.expect("the record carries what changed");
    assert_eq!(new_value["status"], "SUBMITTED");
    assert_eq!(
        new_value["documentNumber"], submitted.body["data"]["documentNumber"],
        "the record does not carry the number the submit assigned: {new_value}"
    );
}

/// **A type with no numbering rule cannot have its documents submitted**, and
/// the refusal names the field an administrator has to fix.
///
/// The failure this prevents is a submitted document with no number, which
/// #168's own text calls unrecoverable — so it is a refusal rather than a
/// submit that quietly skips the number.
#[tokio::test]
async fn a_type_with_no_numbering_rule_cannot_submit() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let form = published_form(&app, &token, "pr-no-rule").await;
    let created = app
        .send(
            Method::POST,
            "/api/v1/document-types",
            Some(&token),
            Some(json!({ "typeCode": "PR_NO_RULE", "name": "PR_NO_RULE", "formId": form })),
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let type_id = id_of(&created.body["data"]);

    let id = draft(&app, &token, type_id, filled_in()).await;
    let refused = submit(&app, &token, id).await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(
        refused.body["error"]["details"][0]["code"], "NO_NUMBERING_RULE",
        "{}",
        refused.body
    );

    let status: String = sqlx::query_scalar("SELECT status FROM documents WHERE id = $1")
        .bind(id)
        .fetch_one(&app.pool)
        .await
        .expect("the document is readable");

    assert_eq!(status, "DRAFT", "a submit with no number moved the status");
}

/// **Submitting needs `document:submit` and not `document:update`.**
///
/// Someone who may correct a requisition's line items is not thereby someone who
/// may commit it.
///
/// **Seen red** against a build where `submit_document` requires
/// `DOCUMENT_UPDATE`: the editor submits.
#[tokio::test]
async fn submitting_needs_its_own_permission() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = numbered_type(&app, &token, "PR_SUBMIT_PERMISSION", "GAPLESS").await;
    let id = draft(&app, &token, type_id, filled_in()).await;

    // Everything a submitter needs *except* the permission to submit. Without
    // the others the refusal could be about the wrong thing — the gate §2.9
    // describes.
    let role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "DOC-EDITOR",
        &[
            "document:read",
            "document:update",
            "document-type:read",
            "rad:form:read",
        ],
    )
    .await;

    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "doc.editor",
        "doc.editor@example.test",
        common::ADMIN_PASSWORD,
        &[role],
    )
    .await;

    let editor = app.sign_in("doc.editor", common::ADMIN_PASSWORD).await;

    let refused = submit(&app, &editor, id).await;

    assert_eq!(
        refused.status,
        StatusCode::FORBIDDEN,
        "a caller with only document:update submitted a document: {}",
        refused.body
    );

    let number: Option<String> =
        sqlx::query_scalar("SELECT document_number FROM documents WHERE id = $1")
            .bind(id)
            .fetch_one(&app.pool)
            .await
            .expect("the document is readable");

    assert_eq!(number, None, "a refused submit assigned a number");
}

// ---------------------------------------------------------------------------
// The second lines of defence, reached by arranging the interleaving
// ---------------------------------------------------------------------------
//
// Three statements on this surface carry `AND status = 'DRAFT'` in a `WHERE`
// clause the service has *already* checked — `update_document`, `soft_delete`
// and `mark_submitted`. Coding standard §2.5 makes such a predicate carry
// either a test that removes the first line or a comment saying it is
// unexercised, because the first layer refuses before the statement runs and
// the guard is therefore invisible to every ordinary test.
//
// **The interleaving is arranged rather than raced for**, the way
// `an_edit_blocked_by_a_publish_applies_to_nothing` does it: a submit holds the
// row's lock in one transaction while the second statement blocks on it, and
// the second statement therefore reaches the database *after* the service check
// that would have refused it had already passed. That is the window the
// predicate exists for, and there is no other way to be inside it.

/// **An edit that reaches the database after a submit applies to nothing.**
///
/// The service read `DRAFT`, decided the edit was legal, and by the time its
/// statement ran the document had been submitted. Without the predicate the
/// edit lands: a submitted document's payload is rewritten after its number was
/// attached to the old one, which is the outcome #168 calls unrecoverable
/// arriving through the edit path.
///
/// **Seen red** (coding standard §2.9) against `update_document`'s
/// `AND status = 'DRAFT'` removed: the edit applies 1 row.
#[tokio::test]
async fn an_edit_blocked_by_a_submit_applies_to_nothing() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = numbered_type(&app, &token, "PR_EDIT_RACE", "GAPLESS").await;
    let id = draft(&app, &token, type_id, filled_in()).await;
    let tenant = fixtures::SYSTEM_TENANT_ID;

    // The submitter, holding the row and not yet committed.
    let mut submitting = app.pool.begin().await.expect("a transaction");
    let submitted = document_repo::mark_submitted(
        &mut submitting,
        tenant,
        id,
        &document_repo::Submission {
            document_number: "PR-RACE-000001",
            form_data: &json!({"subject": "As submitted"}),
            submitted_at: chrono::Utc::now(),
        },
        None,
    )
    .await
    .expect("the submit runs");
    assert_eq!(submitted, 1, "the submit is the one that wins the row");

    // The editor, blocking on that lock. It reached the statement before the
    // submit committed, which is the interleaving the service check cannot see.
    let pool = app.pool.clone();
    let editing = tokio::spawn(async move {
        document_repo::update_document(
            &pool,
            tenant,
            id,
            &document_repo::DocumentFields {
                title: Some("Edited after the submit"),
                form_data: None,
                priority: None,
                entity_type: None,
                entity_id: None,
                requested_for_department_id: None,
                requested_for_facility_id: None,
            },
            None,
        )
        .await
    });

    tokio::time::sleep(Duration::from_millis(200)).await;
    submitting.commit().await.expect("the submit commits");

    let edited = editing
        .await
        .expect("the edit task finishes")
        .expect("the edit runs");

    assert_eq!(
        edited, 0,
        "the edit applied to a document that had just been submitted"
    );

    let row =
        sqlx::query_as::<_, (String, String)>("SELECT status, title FROM documents WHERE id = $1")
            .bind(id)
            .fetch_one(&app.pool)
            .await
            .expect("the document is readable");

    assert_eq!(row.0, "SUBMITTED");
    assert_eq!(
        row.1, "Two standing desks",
        "the submitted document kept the title it was submitted with"
    );
}

/// **A discard that reaches the database after a submit applies to nothing.**
///
/// The same window on the delete path. Without the predicate a submitted,
/// numbered document is soft-deleted by a caller who read it as a draft — and
/// its number goes with it, held by a row no list returns.
///
/// **Seen red** against `soft_delete`'s `AND status = 'DRAFT'` removed: the
/// discard applies 1 row and the submitted document leaves the list.
#[tokio::test]
async fn a_discard_blocked_by_a_submit_applies_to_nothing() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = numbered_type(&app, &token, "PR_DISCARD_RACE", "GAPLESS").await;
    let id = draft(&app, &token, type_id, filled_in()).await;
    let tenant = fixtures::SYSTEM_TENANT_ID;

    let mut submitting = app.pool.begin().await.expect("a transaction");
    document_repo::mark_submitted(
        &mut submitting,
        tenant,
        id,
        &document_repo::Submission {
            document_number: "PR-DISCARD-000001",
            form_data: &json!({"subject": "As submitted"}),
            submitted_at: chrono::Utc::now(),
        },
        None,
    )
    .await
    .expect("the submit runs");

    let pool = app.pool.clone();
    let discarding =
        tokio::spawn(async move { document_repo::soft_delete(&pool, tenant, id, None).await });

    tokio::time::sleep(Duration::from_millis(200)).await;
    submitting.commit().await.expect("the submit commits");

    let discarded = discarding
        .await
        .expect("the discard task finishes")
        .expect("the discard runs");

    assert_eq!(
        discarded, 0,
        "a submitted document was discarded by a caller who read it as a draft"
    );

    let deleted: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT deleted_at FROM documents WHERE id = $1")
            .bind(id)
            .fetch_one(&app.pool)
            .await
            .expect("the document is readable");

    assert_eq!(
        deleted, None,
        "a refused discard soft-deleted the row anyway"
    );
}

/// **A second submit that reaches the database after the first applies to
/// nothing, and its number is never attached.**
///
/// The window `mark_submitted`'s own predicate exists for, and the one that
/// matters most: without it a document holds two numbers over its life, and the
/// second one is the one anybody reconciling against a purchase order will
/// find.
///
/// **Seen red** against `mark_submitted`'s `AND status = 'DRAFT'` removed: the
/// second submit applies 1 row and the document's number changes.
#[tokio::test]
async fn a_second_submit_blocked_by_the_first_applies_to_nothing() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = numbered_type(&app, &token, "PR_SUBMIT_RACE", "GAPLESS").await;
    let id = draft(&app, &token, type_id, filled_in()).await;
    let tenant = fixtures::SYSTEM_TENANT_ID;

    let mut first = app.pool.begin().await.expect("a transaction");
    document_repo::mark_submitted(
        &mut first,
        tenant,
        id,
        &document_repo::Submission {
            document_number: "PR-FIRST-000001",
            form_data: &json!({"subject": "The first submit"}),
            submitted_at: chrono::Utc::now(),
        },
        None,
    )
    .await
    .expect("the first submit runs");

    let pool = app.pool.clone();
    let second = tokio::spawn(async move {
        let mut transaction = pool.begin().await.expect("a transaction");
        let moved = document_repo::mark_submitted(
            &mut transaction,
            tenant,
            id,
            &document_repo::Submission {
                document_number: "PR-SECOND-000002",
                form_data: &json!({"subject": "The second submit"}),
                submitted_at: chrono::Utc::now(),
            },
            None,
        )
        .await
        .expect("the second submit runs");
        transaction.commit().await.expect("it commits");

        moved
    });

    tokio::time::sleep(Duration::from_millis(200)).await;
    first.commit().await.expect("the first submit commits");

    assert_eq!(
        second.await.expect("the second task finishes"),
        0,
        "a document was submitted twice through the window the service cannot see"
    );

    let number: Option<String> =
        sqlx::query_scalar("SELECT document_number FROM documents WHERE id = $1")
            .bind(id)
            .fetch_one(&app.pool)
            .await
            .expect("the document is readable");

    assert_eq!(
        number.as_deref(),
        Some("PR-FIRST-000001"),
        "the second submit overwrote the number the first assigned"
    );
}

/// **Concurrent submits of a gap-tolerant type do not exhaust the pool.**
///
/// The defect this holds is [#118](https://github.com/sujanto-gaws/kelir/issues/118)'s
/// and coding standard §2.5's: a request that holds two pooled connections at
/// once deadlocks at a concurrency equal to the pool ceiling, every task
/// holding one and waiting for one nobody will release.
///
/// `allocate_committed` opens a transaction of its own — that is what
/// `AllowGaps` *means* — so calling it from inside the submit's transaction was
/// two connections per submit. `no_two_concurrent_submits_take_the_same_number`
/// could not see it: that test uses a `GAPLESS` rule, where the allocation
/// stays inside the caller's transaction. **This is the same test at the same
/// concurrency with the other policy**, which is the only difference that
/// matters and the reason the defect survived.
///
/// **Seen red** (coding standard §2.9) against the allocation moved back inside
/// the transaction — `numbering_service::allocate(state, &mut transaction, …)`
/// in place of the pre-allocation: most of the twenty-four submits answer 500
/// after the acquire timeout.
#[tokio::test]
async fn concurrent_submits_of_a_gap_tolerant_type_do_not_exhaust_the_pool() {
    let app = Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;
    let type_id = numbered_type(&app, &token, "PR_GAPS_CONCURRENT", "ALLOW_GAPS").await;

    let mut documents = Vec::with_capacity(CONCURRENT_SUBMITS);
    for _ in 0..CONCURRENT_SUBMITS {
        documents.push(draft(&app, &token, type_id, filled_in()).await);
    }

    let mut handles = Vec::with_capacity(CONCURRENT_SUBMITS);

    for id in documents {
        let app = Arc::clone(&app);
        let token = token.clone();

        handles.push(tokio::spawn(async move {
            let response = submit(&app, &token, id).await;

            (response.status, response.body.to_string())
        }));
    }

    let mut numbers = Vec::with_capacity(CONCURRENT_SUBMITS);

    for handle in handles {
        let (status, body) = handle.await.expect("the submit task did not panic");

        assert_eq!(
            status,
            StatusCode::OK,
            "a concurrent submit of a gap-tolerant type failed: {body}"
        );

        let parsed: Value = serde_json::from_str(&body).expect("a JSON body");
        numbers.push(
            parsed["data"]["documentNumber"]
                .as_str()
                .expect("a submitted document has a number")
                .to_owned(),
        );
    }

    let distinct: HashSet<&String> = numbers.iter().collect();

    assert_eq!(
        distinct.len(),
        CONCURRENT_SUBMITS,
        "{} of {CONCURRENT_SUBMITS} submits took a number another document already \
         holds: {numbers:?}",
        CONCURRENT_SUBMITS - distinct.len()
    );
}

/// **A gap-tolerant rule loses the number a refused submission took.**
///
/// The trade `AllowGaps` names, and the behaviour that the pre-allocation above
/// restores: the number is committed before the re-evaluation runs, so a
/// submission the re-evaluation refuses leaves a hole.
///
/// A `GAPLESS` rule does not, which is the other half and the reason a rule
/// carries a policy at all — see
/// [`a_failed_submit_leaves_a_draft_and_burns_no_number`].
#[tokio::test]
async fn a_gap_tolerant_rule_loses_the_number_a_refused_submission_took() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = numbered_type(&app, &token, "PR_GAPS_BURN", "ALLOW_GAPS").await;

    // `subject` is required and absent — legal in a draft, refused at submit.
    let unfinished = draft(
        &app,
        &token,
        type_id,
        json!({"quantity": 2, "unit_price": 10}),
    )
    .await;
    let refused = submit(&app, &token, unfinished).await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );

    // And nothing was written to the document: the number it took is gone
    // rather than attached to a draft.
    let row = sqlx::query_as::<_, (String, Option<String>)>(
        "SELECT status, document_number FROM documents WHERE id = $1",
    )
    .bind(unfinished)
    .fetch_one(&app.pool)
    .await
    .expect("the document is readable");

    assert_eq!(row.0, "DRAFT");
    assert_eq!(row.1, None, "a refused submit attached its number anyway");

    // The hole: the next document takes 000002, because 000001 went with the
    // submission that failed.
    let finished = draft(&app, &token, type_id, filled_in()).await;
    let submitted = submit(&app, &token, finished).await;

    assert_eq!(submitted.status, StatusCode::OK, "{}", submitted.body);
    assert_eq!(
        submitted.body["data"]["documentNumber"],
        format!("PR-{}-000002", chrono::Utc::now().format("%Y")),
        "a gap-tolerant rule gave back the number a refused submission took, \
         which makes every rule gapless and every submit of a busy type \
         serialise: {}",
        submitted.body
    );
}

/// **A document that is not a draft is refused before a number is taken.**
///
/// The reason [`refuse_unless_draft`] is called twice. On a gap-tolerant rule
/// the allocation happens before the transaction, so a submit of an
/// already-submitted document would burn a number on its way to a 409 — a hole
/// in the sequence caused by a request that was never going to succeed.
#[tokio::test]
async fn a_refused_second_submit_takes_no_number_on_a_gap_tolerant_rule() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = numbered_type(&app, &token, "PR_GAPS_TWICE", "ALLOW_GAPS").await;
    let id = draft(&app, &token, type_id, filled_in()).await;

    let first = submit(&app, &token, id).await;
    assert_eq!(first.status, StatusCode::OK, "{}", first.body);

    let second = submit(&app, &token, id).await;
    assert_eq!(second.status, StatusCode::CONFLICT, "{}", second.body);

    // The next document takes 000002, not 000003: the refused second submit
    // took nothing.
    let next = draft(&app, &token, type_id, filled_in()).await;
    let after = submit(&app, &token, next).await;

    assert_eq!(after.status, StatusCode::OK, "{}", after.body);
    assert_eq!(
        after.body["data"]["documentNumber"],
        format!("PR-{}-000002", chrono::Utc::now().format("%Y")),
        "a submit refused for not being a draft burned a number: {}",
        after.body
    );
}
