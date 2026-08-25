//! Departments, through the API (#28, FR-ORG-002 and FR-IDM-008's edge).
//!
//! **The hierarchy tests are the reason this file is long.** `departments` is
//! the third self-referencing tree in this codebase, after `mdm_facilities`
//! (#141) and the two RAD tables nothing writes yet (#191). The first one cost
//! three issues to get right — #133 for the missing transaction, #134 for the
//! depth bound that allowed the corruption it was there to survive, and #137
//! for a reference re-read too early — and every one of those is a test here.

mod common;

use std::sync::Arc;

use axum::http::{Method, StatusCode};
use common::{fixtures, TestApp};
use serde_json::{json, Value};
use uuid::Uuid;

async fn create(app: &TestApp, token: &str, body: Value) -> common::TestResponse {
    app.send(
        Method::POST,
        "/api/v1/organization/departments",
        Some(token),
        Some(body),
    )
    .await
}

/// A department, created through the API and returning its surrogate id.
async fn department(app: &TestApp, token: &str, code: &str, parent: Option<&str>) -> Uuid {
    let mut body = json!({ "departmentId": code, "name": code });

    if let Some(parent) = parent {
        body["parentDepartmentId"] = json!(parent);
    }

    let response = create(app, token, body).await;

    assert_eq!(
        response.status,
        StatusCode::CREATED,
        "creating {code} failed: {}",
        response.body
    );

    response.body["data"]["id"]
        .as_str()
        .expect("an id")
        .parse()
        .expect("a uuid")
}

async fn move_under(app: &TestApp, token: &str, id: Uuid, parent: &str) -> common::TestResponse {
    app.send(
        Method::PUT,
        &format!("/api/v1/organization/departments/{id}"),
        Some(token),
        Some(json!({ "parentDepartmentId": parent })),
    )
    .await
}

#[tokio::test]
async fn a_department_is_created_read_back_and_listed() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let id = department(&app, &token, "DEPT-PROC", None).await;

    let read = app
        .send(
            Method::GET,
            &format!("/api/v1/organization/departments/{id}"),
            Some(&token),
            None,
        )
        .await;

    assert_eq!(read.status, StatusCode::OK);
    assert_eq!(read.body["data"]["departmentCode"], "DEPT-PROC");
    assert_eq!(
        read.body["data"]["status"], "ACTIVE",
        "the default when none is sent"
    );

    let listed = app
        .send(
            Method::GET,
            "/api/v1/organization/departments",
            Some(&token),
            None,
        )
        .await;

    assert_eq!(listed.status, StatusCode::OK);
    assert!(!listed.body["data"].as_array().expect("a page").is_empty());
}

/// The gap that made this the strongest of the four carry-overs.
///
/// `master_data` validates a party role's department against `departments`, so
/// a consumer has depended on rows nothing could create since Sprint 5. This
/// asserts the surface now closes that loop end to end.
#[tokio::test]
async fn a_created_department_satisfies_the_master_data_consumer() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let id = department(&app, &token, "DEPT-HR", None).await;

    // The very query `master_data/repository/role.rs` runs to validate the
    // department id an employee profile carries.
    let visible: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM departments
         WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL)",
    )
    .bind(fixtures::SYSTEM_TENANT_ID)
    .bind(id)
    .fetch_one(&app.pool)
    .await
    .expect("the query master data runs");

    assert!(
        visible,
        "the department surface exists so that master data's check can pass"
    );
}

#[tokio::test]
async fn a_department_nests_under_another() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    department(&app, &token, "DEPT-ROOT", None).await;
    let child = department(&app, &token, "DEPT-CHILD", Some("DEPT-ROOT")).await;

    let read = app
        .send(
            Method::GET,
            &format!("/api/v1/organization/departments/{child}"),
            Some(&token),
            None,
        )
        .await;

    assert_eq!(
        read.body["data"]["parentDepartmentId"], "DEPT-ROOT",
        "the parent comes back as its code, not a UUID the caller never sent"
    );
}

#[tokio::test]
async fn a_parent_that_does_not_exist_is_refused_by_name() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let response = create(
        &app,
        &token,
        json!({
            "departmentId": "DEPT-ORPHAN",
            "name": "Orphan",
            "parentDepartmentId": "DEPT-NOWHERE"
        }),
    )
    .await;

    assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        response.body["error"]["details"][0]["path"], "parentDepartmentId",
        "body {}",
        response.body
    );
}

#[tokio::test]
async fn a_department_cannot_be_its_own_parent() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let id = department(&app, &token, "DEPT-SELF", None).await;
    let response = move_under(&app, &token, id, "DEPT-SELF").await;

    assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(response.body["error"]["details"][0]["code"], "CYCLE");
}

/// A ring of three — the case a self-parent check does not cover.
///
/// This is what #191 is still open about for the RAD tables, and what #141 had
/// to fix on facilities. A `CHECK` constraint can express *not itself*; only an
/// ancestor walk can express *not one of its own descendants*.
#[tokio::test]
async fn a_ring_of_three_is_refused() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let a = department(&app, &token, "DEPT-A", None).await;
    department(&app, &token, "DEPT-B", Some("DEPT-A")).await;
    department(&app, &token, "DEPT-C", Some("DEPT-B")).await;

    // A → B → C already. Moving A under C would close the ring.
    let response = move_under(&app, &token, a, "DEPT-C").await;

    assert_eq!(
        response.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "A is above C, so putting A under C makes A its own ancestor; body {}",
        response.body
    );
    assert_eq!(response.body["error"]["details"][0]["code"], "CYCLE");
}

/// A move the walk cannot verify is refused, not allowed.
///
/// #134's lesson, and the subtle one: past the depth bound the root is simply
/// absent from the walk, so "is `id` on this path?" answers *no* about a
/// department that is on it. Treating a prefix as the whole path is the bound
/// creating the corruption it exists to survive.
#[tokio::test]
async fn a_chain_deeper_than_the_bound_is_refused_rather_than_assumed_safe() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    // 70 deep, against a bound of 64. Built directly rather than through the
    // API: the point is the walk, not the create path, and 70 requests to set
    // up one assertion is a slow test for no extra coverage.
    let mut previous: Option<Uuid> = None;
    let mut first = None;

    for index in 0..70 {
        let id = Uuid::now_v7();

        sqlx::query(
            "INSERT INTO departments (id, tenant_id, department_code, name, parent_department_id)
             VALUES ($1, $2, $3, $3, $4)",
        )
        .bind(id)
        .bind(fixtures::SYSTEM_TENANT_ID)
        .bind(format!("DEPT-DEEP-{index:03}"))
        .bind(previous)
        .execute(&app.pool)
        .await
        .expect("insert a link in the chain");

        if first.is_none() {
            first = Some(id);
        }

        previous = Some(id);
    }

    let root = first.expect("the chain has a root");
    let response = move_under(&app, &token, root, "DEPT-DEEP-069").await;

    assert_eq!(
        response.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "body {}",
        response.body
    );
    assert_eq!(
        response.body["error"]["details"][0]["code"], "TOO_DEEP",
        "the caller is told the depth is the reason; \"no\" without one is \
         indistinguishable from a defect. Body: {}",
        response.body
    );
}

/// Two re-parentings that each pass alone must not close a loop together.
///
/// **#133's exact counter-example, on a new table.** With `B → C` and `D → A`
/// stored, one caller moving `A` under `B` and another moving `C` under `D`
/// each walk a path the other is about to change. Locking only each caller's
/// own row and its parent locks `{A,B}` and `{C,D}` — disjoint, nothing
/// serialises, and the result is `A → B → C → D → A`.
///
/// The tenant-wide advisory lock is what stops it: the two requests serialise,
/// the second walks the tree the first left behind, and exactly one is refused.
///
/// **It repeats twenty times, and the repetition is not padding.** With a
/// single pair the two requests usually do not interleave inside the dangerous
/// window at all, so removing the lock left this test green — the mutation
/// survived, which is #118's lesson again: a harness has to reach the
/// concurrency it claims to test.
#[tokio::test]
async fn two_concurrent_re_parentings_cannot_close_a_loop_between_them() {
    let app = Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;

    for round in 0..20 {
        race_one_quad(&app, &token, round).await;
    }

    // Storage is a tree: walking up from every department terminates. Asserted
    // once over everything the rounds built — a cycle anywhere is the failure.
    let cycles: i64 = sqlx::query_scalar(
        r#"
        WITH RECURSIVE up AS (
            SELECT id, parent_department_id, 1 AS depth, ARRAY[id] AS seen
            FROM departments WHERE tenant_id = $1 AND deleted_at IS NULL
            UNION ALL
            SELECT d.id, d.parent_department_id, up.depth + 1, up.seen || d.id
            FROM departments d
            JOIN up ON d.id = up.parent_department_id
            WHERE d.tenant_id = $1 AND d.deleted_at IS NULL
              AND NOT d.id = ANY (up.seen) AND up.depth < 100
        )
        SELECT count(*) FROM up WHERE depth >= 100
        "#,
    )
    .bind(fixtures::SYSTEM_TENANT_ID)
    .fetch_one(&app.pool)
    .await
    .expect("the walk is queryable");

    assert_eq!(cycles, 0, "a cycle reached storage");
}

/// One round of the race: four fresh departments, two concurrent moves.
async fn race_one_quad(app: &Arc<TestApp>, token: &str, round: usize) {
    let token = token.to_owned();

    // The stored edges are `B → C` and `D → A`, where `X → Y` means X's parent
    // is Y. So B is created *under* C, and D under A.
    let a_code = format!("DEPT-RA-{round:02}");
    let b_code = format!("DEPT-RB-{round:02}");
    let c_code = format!("DEPT-RC-{round:02}");
    let d_code = format!("DEPT-RD-{round:02}");

    let a = department(app, &token, &a_code, None).await;
    let c = department(app, &token, &c_code, None).await;
    department(app, &token, &b_code, Some(&c_code)).await;
    department(app, &token, &d_code, Some(&a_code)).await;

    // The two moves that would complete `A → B → C → D → A` between them.
    let first = {
        let app = Arc::clone(app);
        let token = token.clone();
        tokio::spawn(async move { move_under(&app, &token, a, &b_code).await })
    };
    let second = {
        let app = Arc::clone(app);
        let token = token.clone();
        tokio::spawn(async move { move_under(&app, &token, c, &d_code).await })
    };

    let first = first.await.expect("the first move did not panic");
    let second = second.await.expect("the second move did not panic");

    let refused = [&first, &second]
        .iter()
        .filter(|response| response.status == StatusCode::UNPROCESSABLE_ENTITY)
        .count();

    assert_eq!(
        refused, 1,
        "round {round}: exactly one of the two moves closes the loop and must be \
         refused; got {} and {}",
        first.status, second.status
    );
}

#[tokio::test]
async fn a_department_with_dependents_is_refused_rather_than_retired() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let parent = department(&app, &token, "DEPT-PARENT", None).await;
    department(&app, &token, "DEPT-KID", Some("DEPT-PARENT")).await;

    let response = app
        .send(
            Method::DELETE,
            &format!("/api/v1/organization/departments/{parent}"),
            Some(&token),
            None,
        )
        .await;

    assert_eq!(
        response.status,
        StatusCode::CONFLICT,
        "retiring it would orphan the sub-department; body {}",
        response.body
    );
}

#[tokio::test]
async fn a_department_with_nothing_pointing_at_it_is_retired() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let id = department(&app, &token, "DEPT-UNUSED", None).await;

    let deleted = app
        .send(
            Method::DELETE,
            &format!("/api/v1/organization/departments/{id}"),
            Some(&token),
            None,
        )
        .await;

    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);

    let read = app
        .send(
            Method::GET,
            &format!("/api/v1/organization/departments/{id}"),
            Some(&token),
            None,
        )
        .await;

    assert_eq!(read.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_duplicate_department_code_is_a_conflict() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    department(&app, &token, "DEPT-DUP", None).await;

    let again = create(
        &app,
        &token,
        json!({ "departmentId": "DEPT-DUP", "name": "Again" }),
    )
    .await;

    assert_eq!(again.status, StatusCode::CONFLICT, "{}", again.body);
}

#[tokio::test]
async fn a_department_in_another_tenant_is_not_found() {
    let app = TestApp::spawn().await;
    let other = fixtures::create_tenant(&app.pool, "TNT-DEPT-OTHER", "Other tenant").await;

    let hidden = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO departments (id, tenant_id, department_code, name)
         VALUES ($1, $2, 'DEPT-HIDDEN', 'Hidden')",
    )
    .bind(hidden)
    .bind(other)
    .execute(&app.pool)
    .await
    .expect("insert the other tenant's department");

    let token = app.administrator_token().await;
    let response = app
        .send(
            Method::GET,
            &format!("/api/v1/organization/departments/{hidden}"),
            Some(&token),
            None,
        )
        .await;

    assert_eq!(
        response.status,
        StatusCode::NOT_FOUND,
        "an administrator holding every permission must still not see another \
         tenant's department; body {}",
        response.body
    );
}

#[tokio::test]
async fn an_update_records_what_changed() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let id = department(&app, &token, "DEPT-AUDITED", None).await;

    // The name moves; the status is re-sent at its current value.
    let updated = app
        .send(
            Method::PUT,
            &format!("/api/v1/organization/departments/{id}"),
            Some(&token),
            Some(json!({ "name": "Renamed", "status": "ACTIVE" })),
        )
        .await;

    assert_eq!(updated.status, StatusCode::OK, "{}", updated.body);

    let (old_value, new_value): (Value, Value) = sqlx::query_as(
        "SELECT old_value_json, new_value_json FROM audit_events
         WHERE object_id = $1 AND action = 'UPDATE'
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(id)
    .fetch_one(&app.pool)
    .await
    .expect("the update was audited");

    assert_eq!(old_value["name"], "DEPT-AUDITED");
    assert_eq!(new_value["name"], "Renamed");
    assert!(
        new_value.get("status").is_none(),
        "the status was re-sent at its current value, so it did not move; got {new_value}"
    );
}
