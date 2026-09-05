//! System tasks, and the hook chain that gives them something to do
//! (FR-WF-005; [#339]).
//!
//! **The defect this file is about is silent and late.** `SERVICE_TASK` was in
//! JWSS's `taskType` enum and nowhere else in the product: a definition could
//! declare one, it validated, it published, and it then generated a human task
//! that sat in somebody's inbox waiting for a person who was not coming. Worse
//! than refusing it, because nothing said so until whoever was waiting asked.
//!
//! So the assertion that matters most here is a **negative** one — *no `tasks`
//! row* — and it is made against a definition that would have produced one the
//! day before.
//!
//! [#339]: https://github.com/sujanto-gaws/kelir/issues/339

mod common;

use axum::http::{Method, StatusCode};
use common::{fixtures, TestApp};
use serde_json::{json, Value};
use uuid::Uuid;

const APPROVER_ROLE: &str = "SYSTASK-APPROVER";

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

/// A workflow whose middle state is a service task.
///
/// `SUBMITTED` is an ordinary approval; approving it lands in `STAMPING`, which
/// declares a `SERVICE_TASK` and an `AUTO` edge to `COMPLETED`. Nobody decides
/// the middle state — that is the whole claim.
fn workflow_with_a_service_state(key: &str, guards: Value, auto_condition: Option<Value>) -> Value {
    let mut automatic = json!({
        "from": "STAMPING", "to": "COMPLETED", "action": "AUTO",
        "guards": guards
    });

    if let Some(condition) = auto_condition {
        automatic["condition"] = condition;
    }

    json!({
        "workflowKey": key,
        "version": "1.0.0",
        "name": "Stamped approval",
        "initialState": "SUBMITTED",
        "states": [
            { "code": "SUBMITTED", "name": "Awaiting approval",
              "mapsToDocumentStatus": "PENDING_APPROVAL",
              "task": { "taskDefinitionKey": "approve", "taskName": "Approve the request",
                        "assignment": { "assigneeType": "ROLE", "roleCode": APPROVER_ROLE } } },
            { "code": "STAMPING", "name": "Stamping",
              "mapsToDocumentStatus": "IN_REVIEW",
              "task": { "taskDefinitionKey": "stamp", "taskName": "Stamp it",
                        "taskType": "SERVICE_TASK",
                        "assignment": { "assigneeType": "ROLE", "roleCode": APPROVER_ROLE } } },
            { "code": "COMPLETED", "name": "Completed", "mapsToDocumentStatus": "COMPLETED",
              "isFinal": true }
        ],
        "transitions": [
            { "from": "SUBMITTED", "to": "STAMPING", "action": "APPROVE",
              "allowedBy": format!("ROLE:{APPROVER_ROLE}") },
            automatic
        ],
        "variables": [
            { "key": "amount", "dataType": "NUMBER", "source": { "var": "formData.amount" } }
        ]
    })
}

async fn create_definition(app: &TestApp, token: &str, definition: Value) -> common::TestResponse {
    let key = definition["workflowKey"]
        .as_str()
        .expect("a key")
        .to_owned();

    app.post(
        "/api/v1/workflow/definitions",
        Some(token),
        json!({ "workflowKey": key, "name": "Stamped approval", "definition": definition }),
    )
    .await
}

async fn publish_workflow(app: &TestApp, token: &str, definition: Value) -> Uuid {
    let created = create_definition(app, token, definition).await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    let id = id_of(&created.body["data"]);
    let publication = app
        .post(
            &format!("/api/v1/workflow/definitions/{id}/publication"),
            Some(token),
            json!({}),
        )
        .await;

    assert_eq!(publication.status, StatusCode::OK, "{}", publication.body);

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

    app.post(
        &format!("/api/v1/rad/forms/{id}/publish"),
        Some(token),
        json!({}),
    )
    .await;

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

    app.put(
        &format!("/api/v1/document-types/{type_id}/numbering-rule"),
        Some(token),
        json!({
            "ruleTemplate": format!("{code}-{{year}}-{{sequence}}"),
            "sequenceScope": "YEAR",
            "gapPolicy": "GAPLESS",
        }),
    )
    .await;

    type_id
}

async fn draft(app: &TestApp, token: &str, type_id: Uuid, amount: i64) -> Uuid {
    let created = app
        .post(
            "/api/v1/documents",
            Some(token),
            json!({
                "documentTypeId": type_id,
                "title": "Two standing desks",
                "formData": { "amount": amount },
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

async fn approver_role(app: &TestApp) -> Uuid {
    let existing: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM roles WHERE tenant_id = $1 AND role_code = $2 AND deleted_at IS NULL",
    )
    .bind(fixtures::SYSTEM_TENANT_ID)
    .bind(APPROVER_ROLE)
    .fetch_optional(&app.pool)
    .await
    .expect("look the role up");

    match existing {
        Some(id) => id,
        None => {
            fixtures::create_role_with_permissions(
                &app.pool,
                fixtures::SYSTEM_TENANT_ID,
                APPROVER_ROLE,
                &[
                    "workflow:task:read",
                    "workflow:task:execute",
                    "workflow:instance:read",
                    "document:read",
                    "document:create",
                    "document:submit",
                ],
            )
            .await
        }
    }
}

async fn approver(app: &TestApp, username: &str) -> String {
    let role = approver_role(app).await;

    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        username,
        &format!("{username}@kelir.test"),
        common::ADMIN_PASSWORD,
        &[role],
    )
    .await;

    app.sign_in(username, common::ADMIN_PASSWORD).await
}

/// The tasks a state has produced, by its definition key.
async fn tasks_for(app: &TestApp, document_id: Uuid, key: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM workflow_tasks
         WHERE document_id = $1 AND task_definition_key = $2 AND deleted_at IS NULL",
    )
    .bind(document_id)
    .bind(key)
    .fetch_one(&app.pool)
    .await
    .expect("count the tasks")
}

async fn state_of(app: &TestApp, document_id: Uuid) -> String {
    sqlx::query_scalar(
        "SELECT current_state FROM workflow_instances
         WHERE document_id = $1 AND deleted_at IS NULL",
    )
    .bind(document_id)
    .fetch_one(&app.pool)
    .await
    .expect("read the instance state")
}

async fn status_of(app: &TestApp, token: &str, document_id: Uuid) -> String {
    let read = app
        .send(
            Method::GET,
            &format!("/api/v1/documents/{document_id}"),
            Some(token),
            None,
        )
        .await;

    read.body["data"]["status"]
        .as_str()
        .expect("a status")
        .to_owned()
}

/// Approves the one open task on a document.
async fn approve(app: &TestApp, token: &str, document_id: Uuid) -> common::TestResponse {
    let task: Uuid = sqlx::query_scalar(
        "SELECT id FROM workflow_tasks
         WHERE document_id = $1 AND status IN ('CREATED', 'ASSIGNED', 'IN_PROGRESS')
           AND deleted_at IS NULL
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(document_id)
    .fetch_one(&app.pool)
    .await
    .expect("an open task");

    app.post(
        &format!("/api/v1/workflow/tasks/{task}/decision"),
        Some(token),
        json!({ "action": "APPROVE" }),
    )
    .await
}

async fn hook_runs(app: &TestApp, document_id: Uuid) -> Vec<(String, String)> {
    sqlx::query_as(
        "SELECT handler_reference, result FROM document_hook_executions
         WHERE document_id = $1 ORDER BY executed_at, id",
    )
    .bind(document_id)
    .fetch_all(&app.pool)
    .await
    .expect("read the hook log")
}

// ---------------------------------------------------------------------------
// AC1, AC2 — it executes rather than waiting, and reaches no inbox
// ---------------------------------------------------------------------------

/// **AC2, and the defect the issue is really about.**
///
/// The mutation that must make this red is removing the
/// `.filter(|spec| spec.task_type.is_human())` in `engine::enter_once` — the
/// build before [#339] wrote a `tasks` row for this state.
#[tokio::test]
async fn a_service_task_produces_no_row_in_anybodys_inbox() {
    let app = TestApp::spawn().await;
    let admin = app.administrator_token().await;
    let token = approver(&app, "systask.one").await;

    let workflow = publish_workflow(
        &app,
        &admin,
        workflow_with_a_service_state("stamped_one", json!([]), None),
    )
    .await;
    let type_id = document_type(&app, &admin, "STAMPED_ONE", workflow).await;
    let document = draft(&app, &token, type_id, 100).await;

    assert_eq!(submit(&app, &token, document).await.status, StatusCode::OK);

    // The human state produced its task.
    assert_eq!(tasks_for(&app, document, "approve").await, 1);

    let decided = approve(&app, &token, document).await;

    assert_eq!(decided.status, StatusCode::OK, "{}", decided.body);

    // **And the service state produced none.** A second subject in the same
    // assertion (coding standard §2.9): one count cannot tell *no task for this
    // state* from *no tasks at all*.
    assert_eq!(
        tasks_for(&app, document, "stamp").await,
        0,
        "a service task must not reach an inbox"
    );
    assert_eq!(tasks_for(&app, document, "approve").await, 1);
}

/// **AC1**: the engine performs the step and advances in the same transaction.
///
/// One decision moves the document two states — through `STAMPING` and out the
/// other side — so the approver never sees the middle one.
#[tokio::test]
async fn a_service_task_advances_the_instance_past_itself() {
    let app = TestApp::spawn().await;
    let admin = app.administrator_token().await;
    let token = approver(&app, "systask.two").await;

    let workflow = publish_workflow(
        &app,
        &admin,
        workflow_with_a_service_state("stamped_two", json!([]), None),
    )
    .await;
    let type_id = document_type(&app, &admin, "STAMPED_TWO", workflow).await;
    let document = draft(&app, &token, type_id, 100).await;

    submit(&app, &token, document).await;
    approve(&app, &token, document).await;

    assert_eq!(state_of(&app, document).await, "COMPLETED");
    assert_eq!(status_of(&app, &token, document).await, "COMPLETED");
}

/// The automatic step is in the trail, attributed to the decision that caused
/// it. A process that moved twice and recorded once would be a history that
/// cannot explain the state it ended in.
#[tokio::test]
async fn the_automatic_step_is_recorded_in_the_history() {
    let app = TestApp::spawn().await;
    let admin = app.administrator_token().await;
    let token = approver(&app, "systask.three").await;

    let workflow = publish_workflow(
        &app,
        &admin,
        workflow_with_a_service_state("stamped_three", json!([]), None),
    )
    .await;
    let type_id = document_type(&app, &admin, "STAMPED_THREE", workflow).await;
    let document = draft(&app, &token, type_id, 100).await;

    submit(&app, &token, document).await;
    approve(&app, &token, document).await;

    let steps: Vec<(Option<String>, String, Option<String>)> = sqlx::query_as(
        "SELECT from_state, to_state, action FROM workflow_history
         WHERE document_id = $1 ORDER BY created_at, id",
    )
    .bind(document)
    .fetch_all(&app.pool)
    .await
    .expect("read the history");

    let automatic = steps
        .iter()
        .find(|(_, to, action)| to == "COMPLETED" && action.as_deref() == Some("AUTO"))
        .unwrap_or_else(|| panic!("no automatic step recorded: {steps:?}"));

    assert_eq!(automatic.0.as_deref(), Some("STAMPING"));
}

// ---------------------------------------------------------------------------
// AC3 — a failing system task does not strand the instance
// ---------------------------------------------------------------------------

/// **AC3, the guard half.** A `REJECT` rolls the whole advance back, so the
/// document stays where the approver's decision found it — a state with an open
/// task, which is somewhere a person can act.
///
/// The failure mode this is written against is *an approval nobody can decide*
/// ([record 13](../../projects/verifications/13.%20Sprint%2013%20Independent%20Pass.md)).
#[tokio::test]
async fn a_guard_that_refuses_leaves_the_document_where_a_person_can_act() {
    let app = TestApp::spawn().await;
    let admin = app.administrator_token().await;
    let token = approver(&app, "systask.four").await;

    let workflow = publish_workflow(
        &app,
        &admin,
        workflow_with_a_service_state(
            "stamped_four",
            json!([{
                "handler": "core:reject_when",
                "config": {
                    "condition": { ">": [{ "var": "formData.amount" }, 50] },
                    "code": "BUDGET_EXCEEDED",
                    "message": "Above the automatic limit"
                }
            }]),
            None,
        ),
    )
    .await;
    let type_id = document_type(&app, &admin, "STAMPED_FOUR", workflow).await;
    let document = draft(&app, &token, type_id, 100).await;

    submit(&app, &token, document).await;

    let decided = approve(&app, &token, document).await;

    assert_eq!(
        decided.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        decided.body
    );
    assert_eq!(decided.body["error"]["code"], "VALIDATION_ERROR");
    assert!(
        decided.body["error"]["details"]
            .as_array()
            .expect("details")
            .iter()
            .any(|detail| detail["message"]
                .as_str()
                .is_some_and(|message| message.contains("BUDGET_EXCEEDED"))),
        "the refusal names the handler's own code: {}",
        decided.body
    );

    // **Not stranded.** The whole transaction rolled back, so the approval is
    // undone and the task is open again — the document is exactly where it was.
    assert_eq!(state_of(&app, document).await, "SUBMITTED");
    assert_eq!(tasks_for(&app, document, "approve").await, 1);
    assert_eq!(tasks_for(&app, document, "stamp").await, 0);
}

/// The other half of AC3: a service state with no automatic exit.
///
/// **The instance stays, and that is the right answer** — refusing would roll
/// back somebody's approval because a *later* state was misconfigured. The
/// document keeps a status a person can act on through its own surfaces.
#[tokio::test]
async fn a_service_state_with_no_automatic_exit_stays_put_rather_than_failing() {
    let app = TestApp::spawn().await;
    let admin = app.administrator_token().await;
    let token = approver(&app, "systask.five").await;

    let mut definition = workflow_with_a_service_state("stamped_five", json!([]), None);

    // Remove the AUTO edge, leaving STAMPING with a service task and no exit.
    // Its `mapsToDocumentStatus` is what the document then shows.
    definition["transitions"] = json!([
        { "from": "SUBMITTED", "to": "STAMPING", "action": "APPROVE",
          "allowedBy": format!("ROLE:{APPROVER_ROLE}") },
        { "from": "STAMPING", "to": "COMPLETED", "action": "APPROVE",
          "allowedBy": format!("ROLE:{APPROVER_ROLE}") }
    ]);

    let workflow = publish_workflow(&app, &admin, definition).await;
    let type_id = document_type(&app, &admin, "STAMPED_FIVE", workflow).await;
    let document = draft(&app, &token, type_id, 100).await;

    submit(&app, &token, document).await;

    let decided = approve(&app, &token, document).await;

    assert_eq!(decided.status, StatusCode::OK, "{}", decided.body);
    assert_eq!(state_of(&app, document).await, "STAMPING");
    // And still no phantom task, which is the point that must not regress.
    assert_eq!(tasks_for(&app, document, "stamp").await, 0);
}

/// A loop of service states is refused rather than run forever.
///
/// S6 catches a dead end and does not catch this: two service states pointing
/// at each other are both live and both exited.
#[tokio::test]
async fn service_states_that_route_into_one_another_are_refused_rather_than_looping() {
    let app = TestApp::spawn().await;
    let admin = app.administrator_token().await;
    let token = approver(&app, "systask.six").await;

    let definition = json!({
        "workflowKey": "stamped_six",
        "version": "1.0.0",
        "name": "A loop",
        "initialState": "SUBMITTED",
        "states": [
            { "code": "SUBMITTED", "name": "Awaiting approval",
              "mapsToDocumentStatus": "PENDING_APPROVAL",
              "task": { "taskDefinitionKey": "approve", "taskName": "Approve",
                        "assignment": { "assigneeType": "ROLE", "roleCode": APPROVER_ROLE } } },
            { "code": "PING", "name": "Ping", "mapsToDocumentStatus": "IN_REVIEW",
              "task": { "taskDefinitionKey": "ping", "taskName": "Ping",
                        "taskType": "SERVICE_TASK",
                        "assignment": { "assigneeType": "ROLE", "roleCode": APPROVER_ROLE } } },
            { "code": "PONG", "name": "Pong", "mapsToDocumentStatus": "IN_REVIEW",
              "task": { "taskDefinitionKey": "pong", "taskName": "Pong",
                        "taskType": "SERVICE_TASK",
                        "assignment": { "assigneeType": "ROLE", "roleCode": APPROVER_ROLE } } },
            { "code": "COMPLETED", "name": "Completed",
              "mapsToDocumentStatus": "COMPLETED", "isFinal": true }
        ],
        "transitions": [
            { "from": "SUBMITTED", "to": "PING", "action": "APPROVE",
              "allowedBy": format!("ROLE:{APPROVER_ROLE}") },
            { "from": "PING", "to": "PONG", "action": "AUTO" },
            { "from": "PONG", "to": "PING", "action": "AUTO" },
            { "from": "PONG", "to": "COMPLETED", "action": "APPROVE",
              "allowedBy": format!("ROLE:{APPROVER_ROLE}") }
        ],
        "variables": []
    });

    let workflow = publish_workflow(&app, &admin, definition).await;
    let type_id = document_type(&app, &admin, "STAMPED_SIX", workflow).await;
    let document = draft(&app, &token, type_id, 100).await;

    submit(&app, &token, document).await;

    let decided = approve(&app, &token, document).await;

    assert_eq!(decided.status, StatusCode::CONFLICT, "{}", decided.body);
    assert!(
        decided.body["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("loop")),
        "{}",
        decided.body
    );

    // Rolled back: the document has not moved, and the approver still has
    // their task.
    assert_eq!(state_of(&app, document).await, "SUBMITTED");
    assert_eq!(tasks_for(&app, document, "approve").await, 1);
}

// ---------------------------------------------------------------------------
// The hook chain
// ---------------------------------------------------------------------------

/// A guard runs, and its execution is recorded (LHCS §7).
#[tokio::test]
async fn a_guard_runs_and_the_execution_is_logged() {
    let app = TestApp::spawn().await;
    let admin = app.administrator_token().await;
    let token = approver(&app, "systask.seven").await;

    let workflow = publish_workflow(
        &app,
        &admin,
        workflow_with_a_service_state(
            "stamped_seven",
            json!([{ "handler": "core:continue_always" }]),
            None,
        ),
    )
    .await;
    let type_id = document_type(&app, &admin, "STAMPED_SEVEN", workflow).await;
    let document = draft(&app, &token, type_id, 100).await;

    submit(&app, &token, document).await;
    approve(&app, &token, document).await;

    let runs = hook_runs(&app, document).await;

    assert_eq!(
        runs,
        vec![("core:continue_always".to_owned(), "CONTINUE".to_owned())],
        "the chain ran and said so"
    );
    assert_eq!(state_of(&app, document).await, "COMPLETED");
}

/// A refused chain leaves **no** log rows, because the transaction it ran in
/// rolled back — the log describes what happened to a document, and nothing did.
#[tokio::test]
async fn a_rejected_chain_leaves_no_execution_log_behind_it() {
    let app = TestApp::spawn().await;
    let admin = app.administrator_token().await;
    let token = approver(&app, "systask.eight").await;

    let workflow = publish_workflow(
        &app,
        &admin,
        workflow_with_a_service_state(
            "stamped_eight",
            json!([{
                "handler": "core:reject_when",
                "config": { "condition": { "==": [1, 1] }, "code": "ALWAYS" }
            }]),
            None,
        ),
    )
    .await;
    let type_id = document_type(&app, &admin, "STAMPED_EIGHT", workflow).await;
    let document = draft(&app, &token, type_id, 100).await;

    submit(&app, &token, document).await;

    assert_eq!(
        approve(&app, &token, document).await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert!(hook_runs(&app, document).await.is_empty());
}

/// The `AUTO` edge is chosen by its `condition`, the same way a decided edge is
/// — and a service state whose conditions are all false stays put.
#[tokio::test]
async fn an_automatic_edge_is_chosen_by_its_condition() {
    let app = TestApp::spawn().await;
    let admin = app.administrator_token().await;
    let token = approver(&app, "systask.nine").await;

    let workflow = publish_workflow(
        &app,
        &admin,
        workflow_with_a_service_state(
            "stamped_nine",
            json!([]),
            // Only advance for a small amount; this document is 100.
            Some(json!({ "<": [{ "var": "variables.amount" }, 50] })),
        ),
    )
    .await;
    let type_id = document_type(&app, &admin, "STAMPED_NINE", workflow).await;
    let big = draft(&app, &token, type_id, 100).await;

    submit(&app, &token, big).await;
    approve(&app, &token, big).await;

    assert_eq!(
        state_of(&app, big).await,
        "STAMPING",
        "the condition did not hold, so the automatic edge was not taken"
    );

    // The second subject: the same definition, an amount that does hold.
    let small = draft(&app, &token, type_id, 10).await;

    submit(&app, &token, small).await;
    approve(&app, &token, small).await;

    assert_eq!(state_of(&app, small).await, "COMPLETED");
}

// ---------------------------------------------------------------------------
// AC4, AC5 — the vocabulary is enforced at publish
// ---------------------------------------------------------------------------

/// **AC5.** Each of the four names itself, so an author learns which of the six
/// they cannot have.
#[tokio::test]
async fn a_task_type_this_engine_does_not_perform_is_refused_at_save() {
    let app = TestApp::spawn().await;
    let admin = app.administrator_token().await;

    for (index, task_type) in [
        "USER_TASK",
        "REVIEW_TASK",
        "DATA_ENTRY_TASK",
        "SIGNATURE_TASK",
    ]
    .into_iter()
    .enumerate()
    {
        let mut definition =
            workflow_with_a_service_state(&format!("refused_{index}"), json!([]), None);

        definition["states"][1]["task"]["taskType"] = json!(task_type);

        let created = create_definition(&app, &admin, definition).await;

        assert_eq!(
            created.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{task_type} was accepted: {}",
            created.body
        );

        let detail = created.body["error"]["details"]
            .as_array()
            .expect("details")
            .iter()
            .find(|detail| detail["code"] == "TASK_TYPE_NOT_PERFORMED")
            .cloned()
            .unwrap_or_else(|| panic!("{task_type}: {}", created.body));

        assert!(
            detail["message"]
                .as_str()
                .is_some_and(|message| message.contains(task_type)),
            "the refusal must name the type: {detail}"
        );
    }
}

/// **AC4.** A value outside JWSS's enum is refused by the meta-schema, and this
/// is what says the two checks are not the same one: the vocabulary is the
/// specification's, and what this engine *performs* is narrower.
#[tokio::test]
async fn a_task_type_outside_the_vocabulary_is_refused_too() {
    let app = TestApp::spawn().await;
    let admin = app.administrator_token().await;

    let mut definition = workflow_with_a_service_state("not_a_type", json!([]), None);

    definition["states"][1]["task"]["taskType"] = json!("REVUE_TASK");

    let created = create_definition(&app, &admin, definition).await;

    assert_eq!(created.status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn the_two_types_this_engine_performs_are_accepted() {
    let app = TestApp::spawn().await;
    let admin = app.administrator_token().await;

    for (index, task_type) in ["APPROVAL_TASK", "SERVICE_TASK"].into_iter().enumerate() {
        let mut definition =
            workflow_with_a_service_state(&format!("accepted_{index}"), json!([]), None);

        definition["states"][1]["task"]["taskType"] = json!(task_type);

        let created = create_definition(&app, &admin, definition).await;

        assert_eq!(
            created.status,
            StatusCode::CREATED,
            "{task_type} was refused: {}",
            created.body
        );
    }
}

/// **LHCS §2**: a reference must resolve at registration time. A handler nobody
/// has heard of is refused where the author is, not discovered by the first
/// document to reach the transition.
#[tokio::test]
async fn a_guard_naming_a_handler_this_build_does_not_have_is_refused_at_save() {
    let app = TestApp::spawn().await;
    let admin = app.administrator_token().await;

    let created = create_definition(
        &app,
        &admin,
        workflow_with_a_service_state(
            "bad_handler",
            json!([{ "handler": "core:reserve_bugdet" }]),
            None,
        ),
    )
    .await;

    assert_eq!(created.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        created.body["error"]["details"]
            .as_array()
            .expect("details")
            .iter()
            .any(|detail| detail["code"] == "HANDLER_NOT_FOUND"),
        "{}",
        created.body
    );
}

#[tokio::test]
async fn a_guard_naming_a_plugin_is_refused_while_this_build_runs_none() {
    let app = TestApp::spawn().await;
    let admin = app.administrator_token().await;

    let created = create_definition(
        &app,
        &admin,
        workflow_with_a_service_state(
            "plugin_handler",
            json!([{ "handler": "plugin:erp-connector:reserve_budget" }]),
            None,
        ),
    )
    .await;

    assert_eq!(created.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        created.body["error"]["details"]
            .as_array()
            .expect("details")
            .iter()
            .any(|detail| detail["code"] == "HANDLER_PLUGIN_UNKNOWN"),
        "{}",
        created.body
    );
}

// ---------------------------------------------------------------------------
// The registry source (§6.11, §12.4)
// ---------------------------------------------------------------------------

/// Registers a `document_lifecycle_hooks` row. There is no write API for the
/// registry — [#339] gave the table its first *reader* — so a test that needs a
/// row inserts one.
async fn register_hook(
    app: &TestApp,
    document_type_id: Option<Uuid>,
    handler: &str,
    priority: i32,
    config: Value,
) {
    sqlx::query(
        "INSERT INTO document_lifecycle_hooks
             (id, tenant_id, document_type_id, hook_name, handler_reference,
              priority, config_json, is_enabled)
         VALUES ($1, $2, $3, 'before_workflow_transition', $4, $5, $6, true)",
    )
    .bind(Uuid::now_v7())
    .bind(fixtures::SYSTEM_TENANT_ID)
    .bind(document_type_id)
    .bind(handler)
    .bind(priority)
    .bind(config)
    .execute(&app.pool)
    .await
    .expect("the registration is seeded");
}

/// **A tenant-wide registration runs on a transition that declares no guard of
/// its own.**
///
/// Found by the mutation campaign: narrowing `registry_chain`'s predicate from
/// *this type or no type* to *this type* came back green, because nothing read
/// the registry at all. A tenant-wide policy that applied only where a
/// definition already had a guard would be a policy for the definitions that
/// needed it least.
///
/// The mutation that must make this red is `AND document_type_id = $3` in
/// `hook::repository::registry_chain`.
#[tokio::test]
async fn a_tenant_wide_registration_runs_on_a_transition_with_no_guards() {
    let app = TestApp::spawn().await;
    let admin = app.administrator_token().await;
    let token = approver(&app, "systask.ten").await;

    // No `documentTypeId`: the row applies to every type in the tenant.
    register_hook(
        &app,
        None,
        "core:reject_when",
        150,
        json!({ "condition": { ">": [{ "var": "formData.amount" }, 50] }, "code": "TENANT_POLICY" }),
    )
    .await;

    let workflow = publish_workflow(
        &app,
        &admin,
        // The definition declares an empty `guards` array — so anything that
        // runs here came from the registry.
        workflow_with_a_service_state("stamped_ten", json!([]), None),
    )
    .await;
    let type_id = document_type(&app, &admin, "STAMPED_TEN", workflow).await;
    let document = draft(&app, &token, type_id, 100).await;

    submit(&app, &token, document).await;

    let decided = approve(&app, &token, document).await;

    assert_eq!(
        decided.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a tenant-wide registration did not run: {}",
        decided.body
    );
    assert!(
        decided.body["error"]["details"]
            .as_array()
            .expect("details")
            .iter()
            .any(|detail| detail["message"]
                .as_str()
                .is_some_and(|message| message.contains("TENANT_POLICY"))),
        "{}",
        decided.body
    );
}

/// **A registration for another document type does not run here.**
///
/// The second subject (coding standard §2.9): one registration cannot tell
/// *scoped to this type* from *not scoped at all*, and the test above would
/// pass either way.
#[tokio::test]
async fn a_registration_for_another_document_type_does_not_run() {
    let app = TestApp::spawn().await;
    let admin = app.administrator_token().await;
    let token = approver(&app, "systask.eleven").await;

    let workflow = publish_workflow(
        &app,
        &admin,
        workflow_with_a_service_state("stamped_eleven", json!([]), None),
    )
    .await;
    let type_id = document_type(&app, &admin, "STAMPED_ELEVEN", workflow).await;

    // A second type, and a registration that belongs to it alone.
    let other_workflow = publish_workflow(
        &app,
        &admin,
        workflow_with_a_service_state("stamped_eleven_b", json!([]), None),
    )
    .await;
    let other_type = document_type(&app, &admin, "STAMPED_ELEVEN_B", other_workflow).await;

    register_hook(
        &app,
        Some(other_type),
        "core:reject_when",
        150,
        json!({ "condition": { "==": [1, 1] }, "code": "OTHER_TYPE_ONLY" }),
    )
    .await;

    let document = draft(&app, &token, type_id, 100).await;

    submit(&app, &token, document).await;

    let decided = approve(&app, &token, document).await;

    assert_eq!(
        decided.status,
        StatusCode::OK,
        "another type's registration ran here: {}",
        decided.body
    );
    assert_eq!(state_of(&app, document).await, "COMPLETED");

    // And it *does* run on its own type, so the assertion above is about the
    // scoping rather than about a registration nothing reads.
    let theirs = draft(&app, &token, other_type, 100).await;

    submit(&app, &token, theirs).await;

    assert_eq!(
        approve(&app, &token, theirs).await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
}

/// A registry entry and a definition's own guard run in priority order, across
/// sources (LHCS §3.1).
#[tokio::test]
async fn a_registry_entry_and_a_definition_guard_run_in_priority_order() {
    let app = TestApp::spawn().await;
    let admin = app.administrator_token().await;
    let token = approver(&app, "systask.twelve").await;

    // The registry's band is 100-299 and a definition guard defaults to 300, so
    // the registry entry runs first without either naming a number.
    register_hook(&app, None, "core:continue_always", 150, json!({})).await;

    let workflow = publish_workflow(
        &app,
        &admin,
        workflow_with_a_service_state(
            "stamped_twelve",
            json!([{ "handler": "core:set_form_field", "config": {} }]),
            None,
        ),
    )
    .await;
    let type_id = document_type(&app, &admin, "STAMPED_TWELVE", workflow).await;
    let document = draft(&app, &token, type_id, 100).await;

    submit(&app, &token, document).await;
    approve(&app, &token, document).await;

    assert_eq!(
        hook_runs(&app, document).await,
        vec![
            ("core:continue_always".to_owned(), "CONTINUE".to_owned()),
            ("core:set_form_field".to_owned(), "CONTINUE".to_owned()),
        ],
        "the registry's entry runs before the definition's own"
    );
}

/// A disabled registration stays registered and is skipped (LHCS §3).
#[tokio::test]
async fn a_disabled_registration_does_not_run() {
    let app = TestApp::spawn().await;
    let admin = app.administrator_token().await;
    let token = approver(&app, "systask.thirteen").await;

    sqlx::query(
        "INSERT INTO document_lifecycle_hooks
             (id, tenant_id, document_type_id, hook_name, handler_reference,
              priority, config_json, is_enabled)
         VALUES ($1, $2, NULL, 'before_workflow_transition', 'core:reject_when',
                 150, $3, false)",
    )
    .bind(Uuid::now_v7())
    .bind(fixtures::SYSTEM_TENANT_ID)
    .bind(json!({ "condition": { "==": [1, 1] }, "code": "SWITCHED_OFF" }))
    .execute(&app.pool)
    .await
    .expect("the registration is seeded");

    let workflow = publish_workflow(
        &app,
        &admin,
        workflow_with_a_service_state("stamped_thirteen", json!([]), None),
    )
    .await;
    let type_id = document_type(&app, &admin, "STAMPED_THIRTEEN", workflow).await;
    let document = draft(&app, &token, type_id, 100).await;

    submit(&app, &token, document).await;

    assert_eq!(approve(&app, &token, document).await.status, StatusCode::OK);
    assert!(hook_runs(&app, document).await.is_empty());
}
