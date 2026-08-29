//! A definition that publishes can be executed ([#259]).
//!
//! Finding 1 of the [Sprint 11 independent pass][pass], in two halves that share
//! one shape: **the meta-schema did not bound what the columns bound**, so a
//! definition passed the save check, passed the publish check, and then produced
//! a 500 — on the submit for the length cases, on the decision for the
//! self-transition. What failed was never the caller's request. It was a stored
//! definition that no gate had refused.
//!
//! What each test asserted before the fix, run against `5c56bbb`:
//!
//! | Case | Save | Publish | Run time |
//! | :--- | :--- | :--- | :--- |
//! | `taskDefinitionKey` over 64 | 201 | 200 | **500** on the submit |
//! | `taskName` over 200 | 201 | 200 | **500** on the submit |
//! | `states[].name` over 200 | 201 | **500** on the publish | — |
//! | `variables[].key` over 64 | 201 | 200 | **500** on the submit |
//! | `version` over 40 | **500** on the save | — | — |
//! | `from` equal to `to` | 201 | 200 | **500** on the decision |
//!
//! **The sprint's mutation campaign could not have reached any of them.** All
//! sixteen mutations were red, and they measure the clauses that exist; these
//! were missing clauses, and there is nothing to mutate in a rule nobody wrote.
//! It is the same class as [#225], which 34 mutations at 68% did not find.
//!
//! # Why the bounds are in the meta-schema and not in `structural_errors`
//!
//! JWSS §1.3 makes the meta-schema the normative artifact, so a constraint
//! expressible as a JSON Schema keyword belongs there — the reasoning S5 and
//! S12 already follow. A `maxLength` is expressible; the self-transition
//! question was not a length at all, and is settled the other way (below).
//!
//! # The self-transition is legal, so the constraint went rather than the edge
//!
//! `REVIEW --RETURN--> REVIEW` is *send it round again*, and JWSS §4 does not
//! require `from` and `to` to differ. `0030` drops `ck_workflow_history_moved`
//! rather than adding a rule refusing the construct, because narrowing the
//! standard to fit a `CHECK` nobody had argued for is the wrong direction —
//! recorded in JWSS **R-5**, and argued in the migration.
//!
//! Every test here has been seen red; each says against what.
//!
//! [#259]: https://github.com/sujanto-gaws/kelir/issues/259
//! [#225]: https://github.com/sujanto-gaws/kelir/issues/225
//! [pass]: ../../projects/verifications/09.%20Sprint%2011%20Independent%20Pass.md

mod common;

use axum::http::{Method, StatusCode};
use common::{fixtures, TestApp};
use serde_json::{json, Value};
use uuid::Uuid;

const REVIEWER_ROLE: &str = "BOUNDS-REVIEWER";

fn id_of(value: &Value) -> Uuid {
    value["id"]
        .as_str()
        .expect("an id")
        .parse()
        .expect("a uuid")
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// The smallest workflow that generates a task and finishes.
///
/// Every string in it that reaches a column is short, so a test that lengthens
/// exactly one of them is testing exactly one bound.
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
        ],
        "variables": [
            { "key": "amount", "dataType": "NUMBER", "source": { "var": "formData.amount" } }
        ]
    })
}

/// The same workflow with a self-transition: the reviewer may send it round
/// again, or approve it and be done.
///
/// **`RETURN` rather than a conditioned `APPROVE`**, so the loop terminates when
/// the reviewer decides it should. A self-edge chosen by a condition over the
/// instance's variables would be chosen the same way every time — the variables
/// are stamped at start — and the document would never leave the state.
fn self_looping_workflow(key: &str) -> Value {
    let mut definition = workflow(key);

    definition["transitions"]
        .as_array_mut()
        .expect("transitions")
        .push(json!({
            "from": "REVIEW", "to": "REVIEW", "action": "RETURN",
            "allowedBy": format!("ROLE:{REVIEWER_ROLE}")
        }));

    definition
}

async fn create(app: &TestApp, token: &str, key: &str, definition: Value) -> common::TestResponse {
    app.post(
        "/api/v1/workflow/definitions",
        Some(token),
        json!({ "workflowKey": key, "name": "Review", "definition": definition }),
    )
    .await
}

async fn publish(app: &TestApp, token: &str, id: Uuid) -> common::TestResponse {
    app.post(
        &format!("/api/v1/workflow/definitions/{id}/publication"),
        Some(token),
        json!({}),
    )
    .await
}

async fn published_workflow(app: &TestApp, token: &str, key: &str, definition: Value) -> Uuid {
    let created = create(app, token, key, definition).await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    let id = id_of(&created.body["data"]);
    let published = publish(app, token, id).await;
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

async fn draft(app: &TestApp, token: &str, type_id: Uuid) -> Uuid {
    let created = app
        .post(
            "/api/v1/documents",
            Some(token),
            json!({
                "documentTypeId": type_id,
                "title": "Two standing desks",
                "formData": { "amount": 250 },
            }),
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

async fn reviewer(app: &TestApp, username: &str) -> String {
    let existing: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM roles WHERE tenant_id = $1 AND role_code = $2 AND deleted_at IS NULL",
    )
    .bind(fixtures::SYSTEM_TENANT_ID)
    .bind(REVIEWER_ROLE)
    .fetch_optional(&app.pool)
    .await
    .expect("look the role up");

    let role = match existing {
        Some(id) => id,
        None => {
            fixtures::create_role_with_permissions(
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
            .await
        }
    };

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

async fn open_task_of(app: &TestApp, document_id: Uuid) -> Uuid {
    sqlx::query_scalar(
        "SELECT id FROM workflow_tasks \
         WHERE document_id = $1 AND status IN ('CREATED','ASSIGNED','IN_PROGRESS')",
    )
    .bind(document_id)
    .fetch_one(&app.pool)
    .await
    .expect("read the open task")
}

async fn decide(app: &TestApp, token: &str, task: Uuid, action: &str) -> common::TestResponse {
    app.post(
        &format!("/api/v1/workflow/tasks/{task}/decision"),
        Some(token),
        json!({ "action": action }),
    )
    .await
}

/// Asserts the definition is refused **at save**, with a detail naming the
/// offending path.
///
/// The path matters as much as the refusal: a 422 that says only *invalid
/// definition* leaves the author of a two-hundred-line workflow to find which
/// string was too long by bisection.
async fn refused_at_save(app: &TestApp, token: &str, key: &str, definition: Value, path: &str) {
    let refused = create(app, token, key, definition).await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );

    let details = refused.body["error"]["details"]
        .as_array()
        .expect("details")
        .clone();

    assert!(
        details
            .iter()
            .any(|detail| detail["path"] == path && detail["code"] == "INVALID_DEFINITION"),
        "expected a detail naming {path}, got {}",
        refused.body
    );
}

// ---------------------------------------------------------------------------
// AC1, AC3, AC4 — every string that reaches a column is bounded where it is
// declared
// ---------------------------------------------------------------------------

/// `taskDefinitionKey` — `workflow_tasks.task_definition_key VARCHAR(64)`.
///
/// **Seen red** against the meta-schema with this `maxLength` removed: the save
/// returns 201, the publish 200, and the submit 500.
#[tokio::test]
async fn a_task_definition_key_longer_than_its_column_is_refused_at_save() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let mut definition = workflow("wf_bounds_task_key");
    definition["states"][0]["task"]["taskDefinitionKey"] = json!("k".repeat(65));

    refused_at_save(
        &app,
        &token,
        "wf_bounds_task_key",
        definition,
        "definition.states.0.task.taskDefinitionKey",
    )
    .await;
}

/// `taskName` — `workflow_tasks.task_name VARCHAR(200)`.
///
/// **Seen red** against the meta-schema with this `maxLength` removed: 201, 200,
/// then 500 on the submit.
#[tokio::test]
async fn a_task_name_longer_than_its_column_is_refused_at_save() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let mut definition = workflow("wf_bounds_task_name");
    definition["states"][0]["task"]["taskName"] = json!("n".repeat(201));

    refused_at_save(
        &app,
        &token,
        "wf_bounds_task_name",
        definition,
        "definition.states.0.task.taskName",
    )
    .await;
}

/// `states[].name` — `workflow_states.name VARCHAR(200)`.
///
/// **Not one of the three instances #259 was raised on**, and it fails one step
/// earlier than they do: the projection is written at publish, so this one 500s
/// on the publish itself rather than on a submit. It is here because the root
/// cause was a class and not three cases, and the sweep that found it went
/// through every column the definition feeds.
///
/// **Seen red** against the meta-schema with this `maxLength` removed.
#[tokio::test]
async fn a_state_name_longer_than_its_column_is_refused_at_save() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let mut definition = workflow("wf_bounds_state_name");
    definition["states"][0]["name"] = json!("n".repeat(201));

    refused_at_save(
        &app,
        &token,
        "wf_bounds_state_name",
        definition,
        "definition.states.0.name",
    )
    .await;
}

/// `variables[].key` — `workflow_variables.variable_key VARCHAR(64)`.
///
/// Also found by the sweep. It fails at the **submit**, because variables are
/// stamped when the instance starts.
///
/// **Seen red** against the meta-schema with this `maxLength` removed.
#[tokio::test]
async fn a_variable_key_longer_than_its_column_is_refused_at_save() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let mut definition = workflow("wf_bounds_variable_key");
    definition["variables"][0]["key"] = json!("v".repeat(65));

    refused_at_save(
        &app,
        &token,
        "wf_bounds_variable_key",
        definition,
        "definition.variables.0.key",
    )
    .await;
}

/// `version` — `workflow_definitions.jwss_version VARCHAR(40)`.
///
/// The pattern `^1\.[0-9]+\.[0-9]+$` bounds the shape and not the length, so a
/// version of forty-three characters is well-formed. It is the least likely of
/// the six to be typed by a person and the cheapest of the six to bound, and a
/// bound that costs one keyword does not need to earn its place by likelihood.
///
/// It fails earliest of all: `jwss_version` is written by the **save**, so this
/// is the one case where the caller's own request produced the 500.
///
/// **Seen red** against the meta-schema with this `maxLength` removed.
#[tokio::test]
async fn a_jwss_version_longer_than_its_column_is_refused_at_save() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let mut definition = workflow("wf_bounds_version");
    definition["version"] = json!(format!("1.{}.0", "0".repeat(39)));

    refused_at_save(
        &app,
        &token,
        "wf_bounds_version",
        definition,
        "definition.version",
    )
    .await;
}

/// A definition sitting **exactly** on every bound publishes and runs.
///
/// The other half of a length check, and the half that catches the mistake the
/// fix itself could introduce: a `maxLength` written one short refuses a
/// definition the column would have held, and nothing in the other five tests
/// would notice. `VARCHAR(64)` holds sixty-four characters.
///
/// **Seen red** with `taskDefinitionKey`'s `maxLength` set to 63.
#[tokio::test]
async fn a_definition_sitting_on_every_bound_publishes_and_runs() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let holder = reviewer(&app, "bounds-exact-holder").await;

    let mut definition = workflow("wf_bounds_exact");
    definition["states"][0]["task"]["taskDefinitionKey"] = json!("k".repeat(64));
    definition["states"][0]["task"]["taskName"] = json!("n".repeat(200));
    definition["states"][0]["name"] = json!("s".repeat(200));
    definition["variables"][0]["key"] = json!("v".repeat(64));

    let workflow_id = published_workflow(&app, &token, "wf_bounds_exact", definition).await;
    let type_id = document_type(&app, &token, "PR_BOUNDS_EXACT", workflow_id).await;
    let document = draft(&app, &token, type_id).await;

    let submitted = submit(&app, &token, document).await;
    assert_eq!(submitted.status, StatusCode::OK, "{}", submitted.body);

    let decided = decide(&app, &holder, open_task_of(&app, document).await, "APPROVE").await;
    assert_eq!(decided.status, StatusCode::OK, "{}", decided.body);
}

// ---------------------------------------------------------------------------
// AC2 — a self-transition is legal, and the history records it
// ---------------------------------------------------------------------------

/// The reviewer sends it round again, and the process survives.
///
/// The document stays in `REVIEW`, a fresh task is generated, and the decision
/// that produced neither a state change nor a new state is on the history all
/// the same — which is the whole argument for dropping the constraint: a
/// reviewer who acted and left the document where it was is a row a reader
/// needs, not a row that says nothing.
///
/// **Seen red** against `0030` reverted — the decision returns 500,
/// `ck_workflow_history_moved` refusing the insert inside the transition.
#[tokio::test]
async fn a_self_transition_runs_and_is_recorded() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let holder = reviewer(&app, "bounds-loop-holder").await;

    let workflow_id = published_workflow(
        &app,
        &token,
        "wf_bounds_loop",
        self_looping_workflow("wf_bounds_loop"),
    )
    .await;
    let type_id = document_type(&app, &token, "PR_BOUNDS_LOOP", workflow_id).await;
    let document = draft(&app, &token, type_id).await;

    assert_eq!(submit(&app, &token, document).await.status, StatusCode::OK);

    let first_task = open_task_of(&app, document).await;
    let returned = decide(&app, &holder, first_task, "RETURN").await;
    assert_eq!(returned.status, StatusCode::OK, "{}", returned.body);

    // The instance did not move, and it did not stall either.
    let state: String =
        sqlx::query_scalar("SELECT current_state FROM workflow_instances WHERE document_id = $1")
            .bind(document)
            .fetch_one(&app.pool)
            .await
            .expect("read the instance state");
    assert_eq!(state, "REVIEW");

    let second_task = open_task_of(&app, document).await;
    assert_ne!(
        second_task, first_task,
        "re-entering the state generates a new task rather than reopening the decided one"
    );

    // The row the constraint used to refuse.
    let recorded: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_history \
         WHERE document_id = $1 AND action = 'RETURN' \
           AND from_state = 'REVIEW' AND to_state = 'REVIEW'",
    )
    .bind(document)
    .fetch_one(&app.pool)
    .await
    .expect("count the history rows");
    assert_eq!(
        recorded, 1,
        "the decision that stayed put is on the history"
    );

    // And the workflow still terminates.
    let approved = decide(&app, &holder, second_task, "APPROVE").await;
    assert_eq!(approved.status, StatusCode::OK, "{}", approved.body);

    let state: String =
        sqlx::query_scalar("SELECT current_state FROM workflow_instances WHERE document_id = $1")
            .bind(document)
            .fetch_one(&app.pool)
            .await
            .expect("read the instance state");
    assert_eq!(state, "COMPLETED");
}
