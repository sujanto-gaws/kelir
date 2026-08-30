//! A definition is executed only for the tenant that pointed at it ([#260]).
//!
//! Finding 2 of the [Sprint 11 independent pass][pass].
//! `repository::definition::definition_of_instance` is the one read in the
//! module without a tenant predicate, deliberately — and its doc comment
//! defended that by naming **one** caller where there were five.
//!
//! **The pass found no leak, and there was none**: every caller passes an id it
//! read from a row already scoped to its tenant. What had failed was the guard.
//! A call-site count goes stale the first time somebody adds a caller, and it
//! had, four times in one sprint.
//!
//! So the property is checked rather than promised, and this is the test that
//! the check is load-bearing. It builds the state the comment reasons about and
//! cannot otherwise occur — an instance in one tenant pointing at a definition
//! in another — and asserts the definition is **not** executed.
//!
//! **The planted definition is deliberately one that would work.** Same graph,
//! same state codes, same role. Without the check the decision succeeds, on
//! another tenant's rules; with it, the caller reports a definition it cannot
//! find. The test would prove nothing against a definition that could not have
//! run anyway.
//!
//! [#260]: https://github.com/sujanto-gaws/kelir/issues/260
//! [pass]: ../../projects/verifications/09.%20Sprint%2011%20Independent%20Pass.md

mod common;

use axum::http::{Method, StatusCode};
use common::{fixtures, TestApp};
use serde_json::{json, Value};
use uuid::Uuid;

const REVIEWER_ROLE: &str = "SCOPE-REVIEWER";

fn id_of(value: &Value) -> Uuid {
    value["id"]
        .as_str()
        .expect("an id")
        .parse()
        .expect("a uuid")
}

fn workflow(key: &str) -> Value {
    json!({
        "workflowKey": key,
        "version": "1.0.0",
        "name": "Review",
        "initialState": "REVIEW",
        "states": [
            { "code": "REVIEW", "name": "Review", "mapsToDocumentStatus": "IN_REVIEW",
              "task": { "taskDefinitionKey": "review", "taskName": "Review the request",
                        "assignment": { "assigneeType": "ROLE", "roleCode": REVIEWER_ROLE } } },
            { "code": "COMPLETED", "name": "Completed", "mapsToDocumentStatus": "COMPLETED",
              "isFinal": true }
        ],
        "transitions": [
            { "from": "REVIEW", "to": "COMPLETED", "action": "APPROVE",
              "allowedBy": format!("ROLE:{REVIEWER_ROLE}") }
        ]
    })
}

async fn published_workflow(app: &TestApp, token: &str, key: &str) -> Uuid {
    let created = app
        .post(
            "/api/v1/workflow/definitions",
            Some(token),
            json!({ "workflowKey": key, "name": "Review", "definition": workflow(key) }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    let id = id_of(&created.body["data"]);

    let published = app
        .post(
            &format!("/api/v1/workflow/definitions/{id}/publication"),
            Some(token),
            json!({}),
        )
        .await;
    assert_eq!(published.status, StatusCode::OK, "{}", published.body);

    id
}

async fn published_form(app: &TestApp, token: &str, key: &str) -> Uuid {
    let created = app
        .post(
            "/api/v1/rad/forms",
            Some(token),
            json!({
                "formKey": key,
                "title": "Purchase requisition",
                "definition": {
                    "formId": key,
                    "version": "2.0.1",
                    "title": "Purchase requisition",
                    "components": [{
                        "id": "amount-field", "role": "data", "type": "number",
                        "key": "amount", "label": "Amount",
                        "validation": { "type": "number", "minimum": 0 }
                    }]
                },
            }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    let id = id_of(&created.body["data"]);

    let published = app
        .post(
            &format!("/api/v1/rad/forms/{id}/publish"),
            Some(token),
            json!({}),
        )
        .await;
    assert_eq!(published.status, StatusCode::OK, "{}", published.body);

    id
}

async fn document_type(app: &TestApp, token: &str, code: &str, workflow: Uuid) -> Uuid {
    let form = published_form(app, token, &code.to_lowercase().replace('_', "-")).await;

    let created = app
        .post(
            "/api/v1/document-types",
            Some(token),
            json!({
                "typeCode": code,
                "name": code,
                "formId": form,
                "workflows": [{ "workflowDefinitionId": workflow }],
            }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    let type_id = id_of(&created.body["data"]);

    let rule = app
        .put(
            &format!("/api/v1/document-types/{type_id}/numbering-rule"),
            Some(token),
            json!({
                "ruleTemplate": format!("{code}-{{year}}-{{sequence}}"),
                "sequenceScope": "YEAR",
                "gapPolicy": "GAPLESS",
            }),
        )
        .await;
    assert_eq!(rule.status, StatusCode::OK, "{}", rule.body);

    type_id
}

async fn reviewer(app: &TestApp, username: &str) -> String {
    let role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        REVIEWER_ROLE,
        &[
            "workflow:task:read",
            "workflow:task:execute",
            "workflow:instance:read",
            "document:read",
        ],
    )
    .await;

    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        username,
        &format!("{username}@example.test"),
        common::ADMIN_PASSWORD,
        &[role],
    )
    .await;

    app.sign_in(username, common::ADMIN_PASSWORD).await
}

/// Copies a published definition into another tenant, with its projection, and
/// returns its id.
///
/// **Direct SQL, because the API cannot produce this and should not.** The rows
/// are faithful copies but for `id` and `tenant_id`, so the planted definition
/// is executable in every respect except whose it is.
///
/// **The projection has to come too, and the database is what says so.** `0025`
/// carries `fk_workflow_instances_current_state`, a composite foreign key from
/// `(workflow_definition_id, current_state)` into `workflow_states` — #175 AC4,
/// enforced rather than assumed. Pointing an instance at a bare definition row
/// is refused by it, which is worth knowing: **the database already stops the
/// crudest version of this**, and stops it for reasons that have nothing to do
/// with tenancy. What it does not stop is a definition in another tenant whose
/// states happen to line up, which is what is planted here.
async fn planted_in_another_tenant(app: &TestApp, source: Uuid, tenant: Uuid) -> Uuid {
    let id = Uuid::now_v7();

    sqlx::query(
        "INSERT INTO workflow_definitions
             (id, tenant_id, workflow_key, name, description, version, jwss_version,
              definition_json, initial_state, status, published_at)
         SELECT $1, $2, workflow_key, name, description, version, jwss_version,
                definition_json, initial_state, status, published_at
         FROM workflow_definitions WHERE id = $3",
    )
    .bind(id)
    .bind(tenant)
    .bind(source)
    .execute(&app.pool)
    .await
    .expect("plant the other tenant's definition");

    sqlx::query(
        "INSERT INTO workflow_states
             (id, tenant_id, workflow_definition_id, state_code, name,
              maps_to_document_status, is_initial, is_final, sort_order)
         SELECT gen_random_uuid(), $1, $2, state_code, name,
                maps_to_document_status, is_initial, is_final, sort_order
         FROM workflow_states WHERE workflow_definition_id = $3",
    )
    .bind(tenant)
    .bind(id)
    .bind(source)
    .execute(&app.pool)
    .await
    .expect("plant the other tenant's states");

    sqlx::query(
        "INSERT INTO workflow_transitions
             (id, tenant_id, workflow_definition_id, from_state, to_state, action,
              allowed_by_json, condition_json, sort_order)
         SELECT gen_random_uuid(), $1, $2, from_state, to_state, action,
                allowed_by_json, condition_json, sort_order
         FROM workflow_transitions WHERE workflow_definition_id = $3",
    )
    .bind(tenant)
    .bind(id)
    .bind(source)
    .execute(&app.pool)
    .await
    .expect("plant the other tenant's transitions");

    id
}

/// A decision does not execute a definition belonging to another tenant.
///
/// **Seen red** against the `in_tenant` comparison removed: the decision returns
/// 200 and the document completes, moved by rules that belong to somebody else.
#[tokio::test]
async fn a_definition_in_another_tenant_is_not_executed() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let holder = reviewer(&app, "scope-holder").await;

    let workflow_id = published_workflow(&app, &token, "wf_scope").await;
    let type_id = document_type(&app, &token, "PR_SCOPE", workflow_id).await;

    let created = app
        .post(
            "/api/v1/documents",
            Some(&token),
            json!({
                "documentTypeId": type_id,
                "title": "Two standing desks",
                "formData": { "amount": 250 },
            }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let document = id_of(&created.body["data"]);

    let submitted = app
        .send(
            Method::POST,
            &format!("/api/v1/documents/{document}/submission"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(submitted.status, StatusCode::OK, "{}", submitted.body);

    // The state the comment reasons about, which nothing in the API can reach:
    // the instance now names a definition belonging to somebody else.
    let other = fixtures::create_tenant(&app.pool, "SCOPE-OTHER", "Another tenant").await;
    let planted = planted_in_another_tenant(&app, workflow_id, other).await;

    sqlx::query("UPDATE workflow_instances SET workflow_definition_id = $1 WHERE document_id = $2")
        .bind(planted)
        .bind(document)
        .execute(&app.pool)
        .await
        .expect("point the instance at the other tenant's definition");

    let task: Uuid = sqlx::query_scalar(
        "SELECT id FROM workflow_tasks \
         WHERE document_id = $1 AND status IN ('CREATED','ASSIGNED','IN_PROGRESS')",
    )
    .bind(document)
    .fetch_one(&app.pool)
    .await
    .expect("read the open task");

    let decided = app
        .post(
            &format!("/api/v1/workflow/tasks/{task}/decision"),
            Some(&holder),
            json!({ "action": "APPROVE" }),
        )
        .await;

    assert_ne!(
        decided.status,
        StatusCode::OK,
        "a decision executed another tenant's definition: {}",
        decided.body
    );

    // And the process did not move, which is the half that matters more than
    // the status code: the planted definition would have completed it.
    let state: String =
        sqlx::query_scalar("SELECT current_state FROM workflow_instances WHERE document_id = $1")
            .bind(document)
            .fetch_one(&app.pool)
            .await
            .expect("read the instance state");

    assert_eq!(
        state, "REVIEW",
        "the instance moved on rules belonging to another tenant"
    );
}
