//! Numbering rules and scoped sequences (#158, FR-DTYPE-004).
//!
//! **The concurrency test is the point of this file.** A sequence is read,
//! incremented and written, per scope, under concurrency, and the result is a
//! number a document keeps forever — the check-then-act shape this project has
//! produced three times (#105, #133, #137). `allocate_in` is the first surface
//! built after coding standard §2.5 was given the rule those three produced,
//! and `no_two_concurrent_allocations_take_the_same_number` is what holds it to
//! that rule.
//!
//! It runs more concurrent allocations than the test pool has connections
//! (`TEST_POOL_MAX_CONNECTIONS` is 5), which is the concurrency #118 taught us
//! a harness must actually reach: its own tests could not, so a fix that closed
//! a race and opened a pool-exhaustion deadlock passed them all.

mod common;

use std::collections::HashSet;
use std::sync::Arc;

use axum::http::{Method, StatusCode};
use chrono::{TimeZone, Utc};
use common::{fixtures, TestApp};
use kelir_backend::modules::document_type::numbering::AllocationContext;
use kelir_backend::modules::document_type::numbering_service;
use serde_json::{json, Value};
use uuid::Uuid;

/// More than the pool holds, so allocations queue for connections as well as
/// for the rule row. Both are contention, and only one of them is the one under
/// test — which is exactly why the test has to reach both.
const CONCURRENT_ALLOCATIONS: usize = 24;

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

    created.body["data"]["id"]
        .as_str()
        .expect("an id")
        .parse()
        .expect("a uuid")
}

async fn set_rule(app: &TestApp, token: &str, type_id: Uuid, body: Value) -> Value {
    let response = app
        .send(
            Method::PUT,
            &format!("/api/v1/document-types/{type_id}/numbering-rule"),
            Some(token),
            Some(body),
        )
        .await;

    assert_eq!(response.status, StatusCode::OK, "{}", response.body);

    response.body["data"].clone()
}

fn context_at(year: i32, month: u32) -> AllocationContext {
    AllocationContext {
        at: Utc
            .with_ymd_and_hms(year, month, 15, 12, 0, 0)
            .single()
            .expect("a timestamp"),
        department_id: None,
    }
}

#[tokio::test]
async fn a_rule_produces_numbers_matching_its_pattern() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_PATTERN").await;

    set_rule(
        &app,
        &token,
        type_id,
        json!({
            "ruleTemplate": "PR-{year}-{sequence}",
            "sequenceScope": "YEAR",
            "sequencePadding": 6
        }),
    )
    .await;

    let context = context_at(2026, 8);
    let mut numbers = Vec::new();

    for _ in 0..3 {
        let mut transaction = app.pool.begin().await.expect("a transaction");
        let number = numbering_service::allocate_in(
            &mut transaction,
            fixtures::SYSTEM_TENANT_ID,
            type_id,
            &context,
        )
        .await
        .expect("a number is allocated");
        transaction.commit().await.expect("the transaction commits");

        numbers.push(number);
    }

    assert_eq!(
        numbers,
        vec!["PR-2026-000001", "PR-2026-000002", "PR-2026-000003"]
    );
}

#[tokio::test]
async fn a_year_scoped_sequence_restarts_in_the_next_year() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_ROLLOVER").await;

    set_rule(
        &app,
        &token,
        type_id,
        json!({ "ruleTemplate": "PR-{year}-{sequence}", "sequenceScope": "YEAR" }),
    )
    .await;

    let first = allocate(&app, type_id, &context_at(2026, 12)).await;
    let second = allocate(&app, type_id, &context_at(2027, 1)).await;
    let third = allocate(&app, type_id, &context_at(2027, 1)).await;

    assert_eq!(first, "PR-2026-000001");
    assert_eq!(
        second, "PR-2027-000001",
        "a new year is a new bucket and the counter restarts"
    );
    assert_eq!(third, "PR-2027-000002");
}

/// A document back-dated into a bucket the sequence has left is refused.
///
/// §6.3 stores one bucket per rule, so there is no counter to go back to.
/// Restarting the closed one would re-issue `PR-2026-000001`, which a document
/// already holds — and the collision would surface at submit on an unrelated
/// document.
#[tokio::test]
async fn a_closed_bucket_is_refused_rather_than_restarted() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_BACKDATED").await;

    set_rule(
        &app,
        &token,
        type_id,
        json!({ "ruleTemplate": "PR-{year}-{sequence}", "sequenceScope": "YEAR" }),
    )
    .await;

    allocate(&app, type_id, &context_at(2027, 1)).await;

    let mut transaction = app.pool.begin().await.expect("a transaction");
    let outcome = numbering_service::allocate_in(
        &mut transaction,
        fixtures::SYSTEM_TENANT_ID,
        type_id,
        &context_at(2026, 12),
    )
    .await;

    assert!(
        outcome.is_err(),
        "a bucket the sequence has passed cannot be reopened; got {outcome:?}"
    );
}

/// **The test this item exists for.**
///
/// Twenty-four allocations at once, against a pool of five, all in one scope.
/// Every number must be distinct. Against a read-then-write implementation this
/// fails — and it was seen to, before it was accepted (§2.9).
#[tokio::test]
async fn no_two_concurrent_allocations_take_the_same_number() {
    let app = Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_CONCURRENT").await;

    set_rule(
        &app,
        &token,
        type_id,
        json!({ "ruleTemplate": "PR-{year}-{sequence}", "sequenceScope": "YEAR" }),
    )
    .await;

    let context = context_at(2026, 8);
    let mut handles = Vec::with_capacity(CONCURRENT_ALLOCATIONS);

    for _ in 0..CONCURRENT_ALLOCATIONS {
        let app = Arc::clone(&app);

        handles.push(tokio::spawn(async move {
            let mut transaction = app.pool.begin().await.expect("a transaction");
            let number = numbering_service::allocate_in(
                &mut transaction,
                fixtures::SYSTEM_TENANT_ID,
                type_id,
                &context,
            )
            .await
            .expect("a number is allocated");
            transaction.commit().await.expect("the transaction commits");

            number
        }));
    }

    let mut numbers = Vec::with_capacity(CONCURRENT_ALLOCATIONS);

    for handle in handles {
        numbers.push(handle.await.expect("the allocation task did not panic"));
    }

    let distinct: HashSet<&String> = numbers.iter().collect();

    assert_eq!(
        distinct.len(),
        CONCURRENT_ALLOCATIONS,
        "{} of {CONCURRENT_ALLOCATIONS} allocations were duplicates. A number a \
         document keeps forever must be issued once: {numbers:?}",
        CONCURRENT_ALLOCATIONS - distinct.len()
    );

    // And the sequence is contiguous — not merely distinct. Twenty-four
    // allocations that produced 1..24 in some order is a working counter;
    // twenty-four distinct numbers scattered over a wider range would mean
    // something consumed numbers nobody sees.
    let mut sorted: Vec<&String> = numbers.iter().collect();
    sorted.sort();

    let expected: Vec<String> = (1..=CONCURRENT_ALLOCATIONS)
        .map(|n| format!("PR-2026-{n:06}"))
        .collect();
    let expected_refs: Vec<&String> = expected.iter().collect();

    assert_eq!(sorted, expected_refs);
}

/// A gapless rule gives its number back when the transaction rolls back.
#[tokio::test]
async fn a_gapless_rule_rolls_its_number_back() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_GAPLESS").await;

    set_rule(
        &app,
        &token,
        type_id,
        json!({
            "ruleTemplate": "INV-{year}-{sequence}",
            "sequenceScope": "YEAR",
            "gapPolicy": "GAPLESS"
        }),
    )
    .await;

    let context = context_at(2026, 8);

    // Allocated, then abandoned.
    let mut doomed = app.pool.begin().await.expect("a transaction");
    let lost = numbering_service::allocate(
        &app.state,
        &mut doomed,
        fixtures::SYSTEM_TENANT_ID,
        type_id,
        &context,
    )
    .await
    .expect("a number is allocated");
    doomed.rollback().await.expect("the transaction rolls back");

    assert_eq!(lost, "INV-2026-000001");

    // The next allocation takes the same number, because the first never
    // happened. This is what a jurisdiction requiring an unbroken sequence
    // means, and it is the reason the policy is a column.
    let mut kept = app.pool.begin().await.expect("a transaction");
    let issued = numbering_service::allocate(
        &app.state,
        &mut kept,
        fixtures::SYSTEM_TENANT_ID,
        type_id,
        &context,
    )
    .await
    .expect("a number is allocated");
    kept.commit().await.expect("the transaction commits");

    assert_eq!(
        issued, "INV-2026-000001",
        "a gapless sequence loses nothing to a failed submission"
    );
}

/// A gap-tolerant rule keeps its number when the transaction rolls back.
#[tokio::test]
async fn a_gap_tolerant_rule_consumes_its_number() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_GAPPED").await;

    set_rule(
        &app,
        &token,
        type_id,
        json!({
            "ruleTemplate": "PR-{year}-{sequence}",
            "sequenceScope": "YEAR",
            "gapPolicy": "ALLOW_GAPS"
        }),
    )
    .await;

    let context = context_at(2026, 8);

    let mut doomed = app.pool.begin().await.expect("a transaction");
    let consumed = numbering_service::allocate(
        &app.state,
        &mut doomed,
        fixtures::SYSTEM_TENANT_ID,
        type_id,
        &context,
    )
    .await
    .expect("a number is allocated");
    doomed.rollback().await.expect("the transaction rolls back");

    assert_eq!(consumed, "PR-2026-000001");

    let mut kept = app.pool.begin().await.expect("a transaction");
    let issued = numbering_service::allocate(
        &app.state,
        &mut kept,
        fixtures::SYSTEM_TENANT_ID,
        type_id,
        &context,
    )
    .await
    .expect("a number is allocated");
    kept.commit().await.expect("the transaction commits");

    assert_eq!(
        issued, "PR-2026-000002",
        "the gap is the trade this policy names: the rule row is held for the \
         allocation rather than for the caller's whole transaction"
    );
}

#[tokio::test]
async fn a_type_with_no_rule_cannot_number_a_document() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_NO_RULE").await;

    let mut transaction = app.pool.begin().await.expect("a transaction");
    let outcome = numbering_service::allocate_in(
        &mut transaction,
        fixtures::SYSTEM_TENANT_ID,
        type_id,
        &context_at(2026, 8),
    )
    .await;

    assert!(outcome.is_err(), "got {outcome:?}");
}

#[tokio::test]
async fn a_rewound_counter_is_refused_and_an_advanced_one_is_not() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_REWIND").await;

    set_rule(
        &app,
        &token,
        type_id,
        json!({
            "ruleTemplate": "PR-{year}-{sequence}",
            "sequenceScope": "YEAR",
            "nextSequence": 500
        }),
    )
    .await;

    let rewound = app
        .send(
            Method::PUT,
            &format!("/api/v1/document-types/{type_id}/numbering-rule"),
            Some(&token),
            Some(json!({
                "ruleTemplate": "PR-{year}-{sequence}",
                "sequenceScope": "YEAR",
                "nextSequence": 5
            })),
        )
        .await;

    assert_eq!(
        rewound.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "rewinding re-issues numbers documents already hold; body {}",
        rewound.body
    );
    assert_eq!(
        rewound.body["error"]["details"][0]["code"],
        "ALREADY_ISSUED"
    );

    // Advancing is how a deployment continues an existing numbering series.
    let advanced = app
        .send(
            Method::PUT,
            &format!("/api/v1/document-types/{type_id}/numbering-rule"),
            Some(&token),
            Some(json!({
                "ruleTemplate": "PR-{year}-{sequence}",
                "sequenceScope": "YEAR",
                "nextSequence": 9000
            })),
        )
        .await;

    assert_eq!(advanced.status, StatusCode::OK, "{}", advanced.body);
}

#[tokio::test]
async fn replacing_a_rule_keeps_the_old_one_deactivated() {
    // The old rule is what explains a number issued last year. Deleting it
    // would leave `PR-2026-000123` unexplainable.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_REPLACED").await;

    set_rule(
        &app,
        &token,
        type_id,
        json!({ "ruleTemplate": "PR-{year}-{sequence}", "sequenceScope": "YEAR" }),
    )
    .await;
    set_rule(
        &app,
        &token,
        type_id,
        json!({ "ruleTemplate": "REQ-{year}-{sequence}", "sequenceScope": "YEAR" }),
    )
    .await;

    let rows: Vec<(String, bool)> = sqlx::query_as(
        "SELECT rule_template, is_active FROM document_type_numbering_rules
         WHERE document_type_id = $1 ORDER BY created_at",
    )
    .bind(type_id)
    .fetch_all(&app.pool)
    .await
    .expect("the rules are queryable");

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0], ("PR-{year}-{sequence}".to_owned(), false));
    assert_eq!(rows[1], ("REQ-{year}-{sequence}".to_owned(), true));
}

#[tokio::test]
async fn a_department_scoped_rule_needs_a_department() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_BY_DEPARTMENT").await;

    set_rule(
        &app,
        &token,
        type_id,
        json!({
            "ruleTemplate": "PR-{department}-{year}-{sequence}",
            "sequenceScope": "DEPARTMENT_YEAR"
        }),
    )
    .await;

    let mut transaction = app.pool.begin().await.expect("a transaction");
    let outcome = numbering_service::allocate_in(
        &mut transaction,
        fixtures::SYSTEM_TENANT_ID,
        type_id,
        &context_at(2026, 8),
    )
    .await;

    assert!(
        outcome.is_err(),
        "a department-scoped rule cannot number a document that names no \
         department; got {outcome:?}"
    );
}

#[tokio::test]
async fn two_departments_run_separate_sequences() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_TWO_DEPARTMENTS").await;

    set_rule(
        &app,
        &token,
        type_id,
        json!({
            "ruleTemplate": "PR-{department}-{year}-{sequence}",
            "sequenceScope": "DEPARTMENT_YEAR"
        }),
    )
    .await;

    let first = Uuid::now_v7();
    let second = Uuid::now_v7();

    let mut a = context_at(2026, 8);
    a.department_id = Some(first);
    let mut b = context_at(2026, 8);
    b.department_id = Some(second);

    let a1 = allocate(&app, type_id, &a).await;
    let b1 = allocate(&app, type_id, &b).await;

    assert!(a1.ends_with("000001"), "{a1}");
    assert!(
        b1.ends_with("000001"),
        "the second department starts its own sequence rather than continuing \
         the first's: {b1}"
    );
    assert_ne!(a1, b1, "and the two numbers are still distinct");
}

/// Allocates and commits, which is what most of these tests want.
async fn allocate(app: &TestApp, type_id: Uuid, context: &AllocationContext) -> String {
    let mut transaction = app.pool.begin().await.expect("a transaction");
    let number = numbering_service::allocate_in(
        &mut transaction,
        fixtures::SYSTEM_TENANT_ID,
        type_id,
        context,
    )
    .await
    .expect("a number is allocated");
    transaction.commit().await.expect("the transaction commits");

    number
}
