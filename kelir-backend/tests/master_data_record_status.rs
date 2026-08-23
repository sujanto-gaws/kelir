//! The master-data governance lifecycle (FR-MDM-007; issue #99).
//!
//! `record_status` was storage from `0008` and nothing moved it: every record
//! sat at `DRAFT` and always would. What is under test is that it is now a
//! *controlled* transition — a legal set, a permission of its own, an audit
//! action of its own — rather than a column the API happens to write.
//!
//! The state machine itself is unit-tested in `domain::record_status`; what
//! these tests add is that the machine is the one the routes actually use, on
//! both entities, and that nothing else can move the column.
//!
//! # One service, two statements
//!
//! `transition()` is a single function over a `match` on `TransitionTarget`,
//! with one `query!` per entity behind each of its two repository calls. It
//! reads as one code path and is two, and the entity a test happens to use
//! decides which arm it exercises — so a file of thirteen passing tests left
//! every predicate on the facility arm untouched, including the
//! `record_status = $3` compare-and-swap that is the whole of this issue's
//! concurrency design (#139).
//!
//! Tests below the "facility arm" heading exist to cover the *statement*, not
//! the use case, and a new predicate on either arm needs one of its own.
//!
//! **Not reachable: the `tenant_id` predicate on both `move_record_status`
//! statements.** `transition()` reads the status through `find_record_status`
//! first, scoped by the same `(tenant_id, id)` on the same table, so no fixture
//! can make the id match and the tenant not. Defence in depth rather than a
//! control. The `deleted_at` predicate on the same statements is *not* in that
//! category — it guards a delete landing between the two statements — and is
//! covered against the repository, where that window can be opened on purpose.

mod common;

use axum::http::{Method, StatusCode};
use common::{fixtures, TestApp};
use serde_json::{json, Value};
use uuid::Uuid;

const PARTIES: &str = "/api/v1/master-data/parties";
const FACILITIES: &str = "/api/v1/master-data/facilities";
const TRANSITION: &str = "master-data:record-status:transition";
const PASSWORD: &str = "record-status-caller-password";

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

async fn given_party(app: &TestApp, token: &str, code: &str) -> Uuid {
    let created = app
        .post(
            PARTIES,
            Some(token),
            json!({
                "partyId": code,
                "partyTypeId": "PARTY_GROUP",
                "partyGroup": { "groupName": "Acme" },
            }),
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    created.data()["id"]
        .as_str()
        .and_then(|id| Uuid::parse_str(id).ok())
        .expect("id")
}

async fn given_facility(app: &TestApp, token: &str, code: &str) -> Uuid {
    let created = app
        .post(
            FACILITIES,
            Some(token),
            json!({ "facilityId": code, "name": "Head Office", "facilityTypeId": "BUILDING" }),
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    created.data()["id"]
        .as_str()
        .and_then(|id| Uuid::parse_str(id).ok())
        .expect("id")
}

async fn caller_holding(app: &TestApp, permissions: &[&str], nonce: usize) -> String {
    let role_id = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        &format!("ROLE-LIFECYCLE-{nonce}"),
        permissions,
    )
    .await;

    let username = format!("user.lifecycle{nonce}");
    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        &username,
        &format!("lifecycle{nonce}@kelir.test"),
        PASSWORD,
        &[role_id],
    )
    .await;

    app.sign_in(&username, PASSWORD).await
}

async fn transition(app: &TestApp, token: &str, path: &str, body: Value) -> (StatusCode, Value) {
    let response = app.post(path, Some(token), body).await;

    (response.status, response.body)
}

/// What the row itself says, rather than what the response claimed.
async fn stored_status(app: &TestApp, table: &str, id: Uuid) -> String {
    // The table name is interpolated from a literal in this file, never from
    // data, and the value is still bound (coding standard §2.5).
    sqlx::query_scalar(&format!("SELECT record_status FROM {table} WHERE id = $1"))
        .bind(id)
        .fetch_one(&app.pool)
        .await
        .expect("query runs")
}

// ---------------------------------------------------------------------------
// The lifecycle moves, on both entities
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_party_walks_the_documented_lifecycle_and_the_column_follows() {
    // The whole path, through the routes rather than through the enum, so that
    // the state machine under test is the one the API actually applies.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let party = given_party(&app, &token, "PARTY-ACME").await;
    let path = format!("{PARTIES}/{party}/transition");

    assert_eq!(stored_status(&app, "mdm_parties", party).await, "DRAFT");

    for (target, previous) in [
        ("ACTIVE", "DRAFT"),
        ("SUSPENDED", "ACTIVE"),
        ("ACTIVE", "SUSPENDED"),
        ("INACTIVE", "ACTIVE"),
        ("ARCHIVED", "INACTIVE"),
    ] {
        let (status, body) =
            transition(&app, &token, &path, json!({ "recordStatusId": target })).await;

        assert_eq!(status, StatusCode::OK, "{previous} -> {target}: {body}");
        assert_eq!(body["data"]["previousRecordStatusId"], previous, "{body}");
        assert_eq!(body["data"]["recordStatusId"], target, "{body}");
        assert_eq!(stored_status(&app, "mdm_parties", party).await, target);
    }
}

#[tokio::test]
async fn a_facility_moves_on_the_same_machine() {
    // One state machine, two entities. A second copy would be a second machine,
    // and this is what says there is only one.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let facility = given_facility(&app, &token, "FAC-HQ").await;
    let path = format!("{FACILITIES}/{facility}/transition");

    let (status, body) =
        transition(&app, &token, &path, json!({ "recordStatusId": "ACTIVE" })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        stored_status(&app, "mdm_facilities", facility).await,
        "ACTIVE"
    );

    // And the same illegal move is illegal here.
    let (status, body) =
        transition(&app, &token, &path, json!({ "recordStatusId": "ARCHIVED" })).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(
        stored_status(&app, "mdm_facilities", facility).await,
        "ACTIVE"
    );
}

#[tokio::test]
async fn an_illegal_transition_is_refused_and_changes_nothing() {
    // `ARCHIVED -> DRAFT` is the case #99 names: a route back would make the
    // archive a filter rather than a decision.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let party = given_party(&app, &token, "PARTY-ACME").await;
    let path = format!("{PARTIES}/{party}/transition");

    for target in ["ACTIVE", "INACTIVE", "ARCHIVED"] {
        transition(&app, &token, &path, json!({ "recordStatusId": target })).await;
    }
    assert_eq!(stored_status(&app, "mdm_parties", party).await, "ARCHIVED");

    let (status, body) =
        transition(&app, &token, &path, json!({ "recordStatusId": "DRAFT" })).await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["error"]["details"]
            .as_array()
            .expect("details")
            .iter()
            .any(|detail| detail["path"] == "recordStatusId"
                && detail["code"] == "ILLEGAL_TRANSITION"),
        "{body}"
    );
    assert_eq!(
        stored_status(&app, "mdm_parties", party).await,
        "ARCHIVED",
        "a refused transition moved the column anyway"
    );
}

#[tokio::test]
async fn a_direct_edit_cannot_put_a_record_into_pending_approval() {
    // Nothing can approve anything until FR-MDM-010, so a record put here would
    // await an approver that does not exist — the overstatement this issue set
    // out to remove, one value over.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let party = given_party(&app, &token, "PARTY-ACME").await;
    let path = format!("{PARTIES}/{party}/transition");

    let (status, body) = transition(
        &app,
        &token,
        &path,
        json!({ "recordStatusId": "PENDING_APPROVAL" }),
    )
    .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(stored_status(&app, "mdm_parties", party).await, "DRAFT");
}

// ---------------------------------------------------------------------------
// A transition is not a field edit
// ---------------------------------------------------------------------------

#[tokio::test]
async fn updating_a_party_cannot_move_its_record_status() {
    // #99 AC1. If `PUT /parties/{id}` could carry `recordStatusId`, the whole
    // control — the legal set, the permission, the audit action — would sit
    // behind `master-data:party:update`.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let party = given_party(&app, &token, "PARTY-ACME").await;

    let refused = app
        .put(
            &format!("{PARTIES}/{party}"),
            Some(&token),
            json!({ "recordStatusId": "ACTIVE" }),
        )
        .await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "an update carried a lifecycle transition: {}",
        refused.body
    );
    assert_eq!(stored_status(&app, "mdm_parties", party).await, "DRAFT");
}

#[tokio::test]
async fn updating_a_facility_cannot_move_its_record_status_either() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let facility = given_facility(&app, &token, "FAC-HQ").await;

    let refused = app
        .put(
            &format!("{FACILITIES}/{facility}"),
            Some(&token),
            json!({ "recordStatusId": "ACTIVE" }),
        )
        .await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(
        stored_status(&app, "mdm_facilities", facility).await,
        "DRAFT"
    );
}

#[tokio::test]
async fn the_record_status_is_readable_on_both_aggregates() {
    // #99 AC4. It was off the wire because nothing could change it; that reason
    // expires here, and a lifecycle a client cannot read is one it cannot show.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let party = given_party(&app, &token, "PARTY-ACME").await;
    let facility = given_facility(&app, &token, "FAC-HQ").await;

    assert_eq!(
        app.get(&format!("{PARTIES}/{party}"), Some(&token))
            .await
            .data()["recordStatusId"],
        "DRAFT"
    );
    assert_eq!(
        app.get(&format!("{FACILITIES}/{facility}"), Some(&token))
            .await
            .data()["recordStatusId"],
        "DRAFT"
    );

    app.post(
        &format!("{PARTIES}/{party}/transition"),
        Some(&token),
        json!({ "recordStatusId": "ACTIVE" }),
    )
    .await;

    assert_eq!(
        app.get(&format!("{PARTIES}/{party}"), Some(&token))
            .await
            .data()["recordStatusId"],
        "ACTIVE",
        "the aggregate did not follow the transition"
    );
}

// ---------------------------------------------------------------------------
// Permission, audit, tenancy, concurrency
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_transition_needs_its_own_permission_and_not_the_update_one() {
    // Correcting a supplier's address and taking the supplier out of service
    // are different authorities. A caller holding every other master-data
    // permission still cannot transition.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let party = given_party(&app, &token, "PARTY-ACME").await;
    let facility = given_facility(&app, &token, "FAC-HQ").await;

    let everything_else = caller_holding(
        &app,
        &[
            "master-data:party:read",
            "master-data:party:update",
            "master-data:party:create",
            "master-data:party:delete",
            "master-data:facility:read",
            "master-data:facility:update",
        ],
        1,
    )
    .await;

    for path in [
        format!("{PARTIES}/{party}/transition"),
        format!("{FACILITIES}/{facility}/transition"),
    ] {
        let (status, body) = transition(
            &app,
            &everything_else,
            &path,
            json!({ "recordStatusId": "ACTIVE" }),
        )
        .await;

        assert_eq!(status, StatusCode::FORBIDDEN, "{path}: {body}");
    }

    // And the permission on its own is enough — the route requires that one and
    // no other.
    let transitioner = caller_holding(&app, &[TRANSITION], 2).await;
    let (status, body) = transition(
        &app,
        &transitioner,
        &format!("{PARTIES}/{party}/transition"),
        json!({ "recordStatusId": "ACTIVE" }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
}

#[tokio::test]
async fn a_transition_is_audited_under_its_own_action_with_both_ends_and_the_reason() {
    // #99 AC3. STATUS_CHANGE already means `mdm_parties.status`; an auditor
    // asking "who took this supplier out of service" must not have to read the
    // payload to tell which column moved.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let party = given_party(&app, &token, "PARTY-ACME").await;

    app.post(
        &format!("{PARTIES}/{party}/transition"),
        Some(&token),
        json!({ "recordStatusId": "ACTIVE", "reason": "supplier onboarding complete" }),
    )
    .await;

    let recorded: (String, String, Option<String>, Option<Value>, Option<Value>) = sqlx::query_as(
        "SELECT event_type, action, reason, old_value_json, new_value_json
         FROM audit_events
         WHERE object_id = $1 AND action = 'RECORD_STATUS_CHANGE'",
    )
    .bind(party)
    .fetch_one(&app.pool)
    .await
    .expect("the transition is on the record");

    assert_eq!(recorded.0, "Party.RecordStatusChanged");
    assert_eq!(recorded.1, "RECORD_STATUS_CHANGE");
    assert_eq!(recorded.2.as_deref(), Some("supplier onboarding complete"));
    assert_eq!(recorded.3.expect("an old value")["recordStatusId"], "DRAFT");
    assert_eq!(recorded.4.expect("a new value")["recordStatusId"], "ACTIVE");

    // And it did not borrow the action the party's own status change uses.
    let borrowed: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE object_id = $1 AND action = 'STATUS_CHANGE'",
    )
    .bind(party)
    .fetch_one(&app.pool)
    .await
    .expect("query runs");
    assert_eq!(borrowed, 0, "the transition was audited as a status change");
}

#[tokio::test]
async fn another_tenants_record_cannot_be_transitioned() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let other_tenant = fixtures::create_tenant(&app.pool, "TNT-002", "Other").await;
    let foreign = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO mdm_parties (id, tenant_id, party_code, party_type)
         VALUES ($1, $2, 'PARTY-FOREIGN', 'PARTY_GROUP')",
    )
    .bind(foreign)
    .bind(other_tenant)
    .execute(&app.pool)
    .await
    .expect("insert the other tenant's party");

    let (status, body) = transition(
        &app,
        &token,
        &format!("{PARTIES}/{foreign}/transition"),
        json!({ "recordStatusId": "ACTIVE" }),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(
        stored_status(&app, "mdm_parties", foreign).await,
        "DRAFT",
        "another tenant's record was transitioned"
    );
}

#[tokio::test]
async fn two_concurrent_transitions_cannot_both_move_the_record_from_the_same_state() {
    // The check-then-act shape #105 was about, one column over: read the
    // status, then write it back. Both callers read ACTIVE, both are told the
    // move is legal, and without the `record_status = $3` predicate on the
    // UPDATE both would write from a state only one of them was looking at.
    //
    // The assertion is not "one of them fails". Two transitions that happen to
    // serialise — ACTIVE to SUSPENDED, then SUSPENDED to INACTIVE — are both
    // legal and both should succeed. What must never happen is two of them
    // reporting the *same* previous status, because at most one call can have
    // moved the record away from a given state.
    //
    // A loop rather than one attempt, for the reason #105 records: the
    // verifier's first single-shot probe passed and the defect surfaced on the
    // thirtieth run.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    const ROUNDS: usize = 20;

    for round in 0..ROUNDS {
        let party = given_party(&app, &token, &format!("PARTY-{round:04}")).await;
        let path = format!("{PARTIES}/{party}/transition");

        transition(&app, &token, &path, json!({ "recordStatusId": "ACTIVE" })).await;

        let (suspended, deactivated) = tokio::join!(
            app.post(
                &path,
                Some(&token),
                json!({ "recordStatusId": "SUSPENDED" })
            ),
            app.post(&path, Some(&token), json!({ "recordStatusId": "INACTIVE" })),
        );

        let claimed: Vec<String> = [&suspended, &deactivated]
            .iter()
            .filter(|response| response.status == StatusCode::OK)
            .map(|response| {
                response.data()["previousRecordStatusId"]
                    .as_str()
                    .unwrap_or_else(|| {
                        panic!("a success carries a previous status: {}", response.body)
                    })
                    .to_owned()
            })
            .collect();

        let mut distinct = claimed.clone();
        distinct.sort();
        distinct.dedup();
        assert_eq!(
            claimed.len(),
            distinct.len(),
            "round {round}: two transitions both moved the record from the same state — {} and {}",
            suspended.body,
            deactivated.body
        );

        // Whatever the interleaving, at least one moved and the column agrees
        // with the last one that did.
        assert!(!claimed.is_empty(), "round {round}: neither transition ran");

        let stored = stored_status(&app, "mdm_parties", party).await;
        assert!(
            stored == "SUSPENDED" || stored == "INACTIVE",
            "round {round}: the column ended at {stored}"
        );

        // A refusal, if there was one, must be about *this* record rather than
        // about the caller or the request shape.
        //
        // Two shapes are legitimate, and which appears depends on where the two
        // calls interleaved — which is what makes it worth spelling out:
        //
        //   409 — the loser read ACTIVE, was told the move was legal, and its
        //         conditional write matched nothing because the winner had
        //         already moved the record.
        //   422 — the loser read the state the winner had *already written*,
        //         and the move it was asked for is not legal from there:
        //         INACTIVE cannot become SUSPENDED, so a deactivation that
        //         lands first turns the suspension into an illegal transition
        //         rather than a lost race.
        //
        // Both are the record refusing to move twice from one state, which is
        // the invariant. CI found the second on its first round; this test had
        // asserted only the first and passed locally for twenty rounds, because
        // the interleaving never went that way on a machine this fast.
        for response in [&suspended, &deactivated] {
            assert!(
                response.status == StatusCode::OK
                    || response.status == StatusCode::CONFLICT
                    || response.status == StatusCode::UNPROCESSABLE_ENTITY,
                "round {round}: a transition answered {} — {}",
                response.status,
                response.body
            );
        }
    }
}

#[tokio::test]
async fn both_transition_routes_refuse_a_request_with_no_token() {
    let app = TestApp::spawn().await;

    for path in [
        format!("{PARTIES}/{}/transition", Uuid::now_v7()),
        format!("{FACILITIES}/{}/transition", Uuid::now_v7()),
    ] {
        let response = app
            .send_from(
                common::TEST_PEER,
                Method::POST,
                &path,
                None,
                Some(json!({ "recordStatusId": "ACTIVE" })),
            )
            .await;

        assert_eq!(
            response.status,
            StatusCode::UNAUTHORIZED,
            "{path}: {}",
            response.body
        );
    }
}

#[tokio::test]
async fn a_transition_on_a_record_that_does_not_exist_is_a_404() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let (status, body) = transition(
        &app,
        &token,
        &format!("{PARTIES}/{}/transition", Uuid::now_v7()),
        json!({ "recordStatusId": "ACTIVE" }),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

// ---------------------------------------------------------------------------
// The facility arm (#139)
// ---------------------------------------------------------------------------
//
// `transition()` is one service function over a `match` with one statement per
// entity, so it reads as a single code path and is two. Every test above this
// point that exercises a *statement* rather than the machine used a party, and
// the verification pass found the facility arm untouched on all of its own
// predicates. What follows covers the arm rather than the use case.

/// #139. The compare-and-swap, on the facility statement.
///
/// The predicate `record_status = $3` is the whole of #99's concurrency design.
/// Removing it from the party statement turns
/// `two_concurrent_transitions_cannot_both_move_the_record_from_the_same_state`
/// red; removing it from the facility statement changed nothing at all.
///
/// Called against the repository rather than through the route, because the
/// property is "this write is conditional on the state it was checked against"
/// and a race reproduces it only sometimes. The concurrency test below asserts
/// the behaviour; this one asserts the predicate.
#[tokio::test]
async fn a_facility_transition_writes_only_from_the_state_it_was_checked_against() {
    use kelir_backend::modules::master_data::domain::{RecordStatus, TransitionTarget};
    use kelir_backend::modules::master_data::repository::move_record_status;

    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let facility = given_facility(&app, &token, "FAC-CAS").await;

    // The row is DRAFT. A write that believes it is ACTIVE must match nothing.
    let moved = move_record_status(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        TransitionTarget::Facility,
        facility,
        RecordStatus::Active,
        RecordStatus::Suspended,
        None,
    )
    .await
    .expect("the query runs");

    assert_eq!(
        moved, 0,
        "a facility moved from a state it was not in — the compare-and-swap is gone"
    );
    assert_eq!(
        stored_status(&app, "mdm_facilities", facility).await,
        "DRAFT"
    );

    // And the same write from the state the row is actually in does move it, so
    // the assertion above is about the predicate and not about the call being
    // broken.
    let moved = move_record_status(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        TransitionTarget::Facility,
        facility,
        RecordStatus::Draft,
        RecordStatus::Active,
        None,
    )
    .await
    .expect("the query runs");

    assert_eq!(moved, 1);
    assert_eq!(
        stored_status(&app, "mdm_facilities", facility).await,
        "ACTIVE"
    );
}

/// #139. The behaviour the predicate above buys, on facilities.
///
/// The party equivalent is
/// `two_concurrent_transitions_cannot_both_move_the_record_from_the_same_state`,
/// and its comment explains the two legitimate refusal shapes at length. This is
/// that test with the other entity, because the statement behind it is a
/// different statement.
#[tokio::test]
async fn two_concurrent_facility_transitions_cannot_both_move_it_from_one_state() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    const ROUNDS: usize = 20;

    for round in 0..ROUNDS {
        let facility = given_facility(&app, &token, &format!("FAC-RACE-{round:04}")).await;
        let path = format!("{FACILITIES}/{facility}/transition");

        transition(&app, &token, &path, json!({ "recordStatusId": "ACTIVE" })).await;

        let (suspended, deactivated) = tokio::join!(
            app.post(
                &path,
                Some(&token),
                json!({ "recordStatusId": "SUSPENDED" })
            ),
            app.post(&path, Some(&token), json!({ "recordStatusId": "INACTIVE" })),
        );

        let claimed: Vec<String> = [&suspended, &deactivated]
            .iter()
            .filter(|response| response.status == StatusCode::OK)
            .map(|response| {
                response.data()["previousRecordStatusId"]
                    .as_str()
                    .unwrap_or_else(|| {
                        panic!("a success carries a previous status: {}", response.body)
                    })
                    .to_owned()
            })
            .collect();

        let mut distinct = claimed.clone();
        distinct.sort();
        distinct.dedup();
        assert_eq!(
            claimed.len(),
            distinct.len(),
            "round {round}: two transitions both moved the facility from one state — {} and {}",
            suspended.body,
            deactivated.body
        );
        assert!(!claimed.is_empty(), "round {round}: neither transition ran");

        let stored = stored_status(&app, "mdm_facilities", facility).await;
        assert!(
            stored == "SUSPENDED" || stored == "INACTIVE",
            "round {round}: the column ended at {stored}"
        );
    }
}

/// #139. Tenant scoping, on the facility statement.
///
/// `another_tenants_record_cannot_be_transitioned` inserts a foreign **party**,
/// so the facility arm's `tenant_id` predicate had no test: removing it changed
/// nothing.
#[tokio::test]
async fn another_tenants_facility_cannot_be_transitioned() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let other_tenant = fixtures::create_tenant(&app.pool, "TNT-003", "Other").await;
    let foreign = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO mdm_facilities (id, tenant_id, facility_code, name)
         VALUES ($1, $2, 'FAC-FOREIGN', 'Theirs')",
    )
    .bind(foreign)
    .bind(other_tenant)
    .execute(&app.pool)
    .await
    .expect("insert the other tenant's facility");

    let (status, body) = transition(
        &app,
        &token,
        &format!("{FACILITIES}/{foreign}/transition"),
        json!({ "recordStatusId": "ACTIVE" }),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(
        stored_status(&app, "mdm_facilities", foreign).await,
        "DRAFT",
        "another tenant's facility was transitioned"
    );
}

/// #139. A retired record has no lifecycle left.
///
/// `find_record_status` filters `deleted_at IS NULL` and nothing asked it to:
/// removing the predicate let a soft-deleted party be moved through the machine,
/// which would put a record nothing can reach into ACTIVE.
#[tokio::test]
async fn a_retired_record_cannot_be_transitioned() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let party = given_party(&app, &token, "PARTY-RETIRED").await;
    assert_eq!(
        app.delete(&format!("{PARTIES}/{party}"), Some(&token))
            .await
            .status,
        StatusCode::NO_CONTENT
    );

    let (status, body) = transition(
        &app,
        &token,
        &format!("{PARTIES}/{party}/transition"),
        json!({ "recordStatusId": "ACTIVE" }),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(
        stored_status(&app, "mdm_parties", party).await,
        "DRAFT",
        "a retired party was moved through the lifecycle"
    );
}

/// #139. The write refuses a retired record too, not only the read before it.
///
/// `move_record_status` carries its own `deleted_at IS NULL`, and the service
/// never reaches it with a deleted row because `find_record_status` answers
/// first — so no route-level test can isolate it. It is not redundancy: it is
/// what stops a delete landing *between* the two statements from being
/// overwritten, and that window is exactly what #105 was about. Asserted against
/// the repository, which is the only place the window can be opened on purpose.
#[tokio::test]
async fn the_lifecycle_write_refuses_a_record_retired_since_it_was_read() {
    use kelir_backend::modules::master_data::domain::{RecordStatus, TransitionTarget};
    use kelir_backend::modules::master_data::repository::move_record_status;

    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let party = given_party(&app, &token, "PARTY-WINDOW").await;
    // Read the status, as the service does...
    assert_eq!(stored_status(&app, "mdm_parties", party).await, "DRAFT");
    // ...then the delete lands.
    assert_eq!(
        app.delete(&format!("{PARTIES}/{party}"), Some(&token))
            .await
            .status,
        StatusCode::NO_CONTENT
    );

    let moved = move_record_status(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        TransitionTarget::Party,
        party,
        RecordStatus::Draft,
        RecordStatus::Active,
        None,
    )
    .await
    .expect("the query runs");

    assert_eq!(
        moved, 0,
        "the lifecycle write moved a record retired since it was read"
    );
    assert_eq!(stored_status(&app, "mdm_parties", party).await, "DRAFT");
}
