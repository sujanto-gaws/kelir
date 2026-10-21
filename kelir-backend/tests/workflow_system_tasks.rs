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
//! # A refusal is one line (#558): seen to fail (coding standard §2.9)
//!
//! Verification record 19 read three runs of 34 spaces in the
//! `HANDLER_KIND_MISMATCH` message the publication route answered: the source
//! line's indentation, kept in a string literal. The last two tests of this
//! file read that message at the save route and at the publication route, and
//! `hook::service`'s unit tests read every refusal `check_entry` writes.
//!
//! **Before the fix**, eight tests were red on the literals as they were: the
//! two here, three in `hook::service`, `documents_link`'s
//! `a_link_needs_both_of_its_halves`, `workflow_engine`'s
//! `a_document_under_a_workflow_cannot_have_its_status_set_by_hand` and
//! `router`'s `no_published_description_carries_a_run_of_spaces_inside_a_line`.
//!
//! Each mutation below was made alone, the named suites run, the named tests
//! observed red, and the mutation reverted. **Seen red, 2026-10-01.** `save`
//! is `an_actions_entry_naming_a_before_only_handler_is_refused_at_save`,
//! `publication` is
//! `a_stored_draft_naming_a_before_only_handler_is_refused_at_publication_in_one_line`,
//! and the unit tests are `hook::service`'s: `wrong_kind` is
//! `an_actions_entry_naming_a_before_only_handler_is_the_wrong_kind`,
//! `after_only` is
//! `the_after_only_sentence_reads_as_one_line_though_nothing_reaches_it`, and
//! `every` is `every_registration_refusal_reads_as_one_line`.
//!
//! | Mutation | Suites run | Reddened |
//! |---|---|---|
//! | M1: the before-only literal back to three runs of 34 spaces | `--lib modules::hook`, this file | `wrong_kind`, `every`; `save`, `publication` |
//! | M2: the after-only literal back to one run of 34 spaces | `--lib modules::hook`, this file | `after_only`. **This file stays green, 22 of 22: no route reaches that sentence** |
//! | M3: the before-only literal carries a typed `\n` and no run of spaces | `--lib modules::hook`, this file | `wrong_kind`, `every`; `save`, `publication` |
//! | M4: M1 and M2, and the unit helper's two-space assertion removed | `--lib modules::hook` | `wrong_kind` and `after_only`, on the sentence asserted whole. `every` goes green: that assertion is all that holds it |
//! | M5: M3, and the unit helper's line-break assertion removed | `--lib modules::hook` | `wrong_kind`, on the whole sentence. `every` goes green |
//! | M6: M1, and this file's two-space assertion removed | this file | `publication`, on the whole sentence. `save` goes green |
//! | M7: M3, and this file's line-break assertion removed | this file | `publication`, on the whole sentence. `save` goes green |
//! | M8: `continue_always` declared after-only | `--lib modules::hook` | seven tests, `after_only` among them on the handlers its sentence offers, and `every` on its list of codes. The sentence is then reachable, and both say so |
//! | M9: the `entityId` literal back to a typed `\n` and thirteen spaces | `documents_link` | `a_link_needs_both_of_its_halves` |
//! | M10: M9, and `documents_link`'s two-space assertion removed | `documents_link` | the same test, on the line break |
//! | M11: the status refusal back to a run of ten spaces | `workflow_engine`, that test alone | `a_document_under_a_workflow_cannot_have_its_status_set_by_hand` |
//! | M12: the roles list description back to a run of spaces | `--lib router::tests` | `no_published_description_carries_a_run_of_spaces_inside_a_line` |
//! | M13: the one-role description back to a run of spaces | `--lib router::tests` | the same test |
//! | M14: the description walk reads fenced code blocks too | `--lib router::tests` | the same test, on `WorkflowHistoryEntry`'s aligned example: the exclusion is needed |
//! | M15: the `tracing::error!` line in `workflow::repository::definition` back to a run of spaces | `--lib modules::workflow` | green, 68 of 68. **Nothing reads that log line**, and it was fixed without a test |
//!
//! **The independent campaign on the PR, after the table above.** `save` now
//! reads the sentence whole and the rule it cites, as `publication` does, and
//! `publication` reads each detail's path and rule. Seen red, 2026-10-01, each
//! alone: the blank before the first `\` of the before-only literal removed
//! ("a veto or achange") reddens `save` and `publication`, where `save` was
//! green; the kind mismatch citing `LHCS-2` in place of `LHCS-3.2` reddens
//! both, where this file was green, 22 of 22. M15's log line is read by
//! `tests/refusals_read_as_one_line.rs`, with the rest of that campaign's
//! record.
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
/// **Seen red, 2026-09-08**: removing the
/// `.filter(|spec| spec.task_type.is_human())` from the task-row branch of
/// `engine::enter_once` — the build before [#339] wrote a `tasks` row for this
/// state.
///
/// **There are two `is_human` sites in that file and only one of them is this
/// one.** The other guards the automatic-step loop; mutating it leaves this
/// test green, because an instance that does not advance still writes no task
/// row. The named site is the `if let Some(spec) = state.task.as_ref()` that
/// decides whether a row is written at all.
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
// A guard's `targetStatus` is a status, not a state code (#679)
// ---------------------------------------------------------------------------
//
// LHCS §4 makes `targetStatus` a platform document status, and the after-chain
// gives it as the reached state's `mapsToDocumentStatus`. The guard path gave
// the edge's target *state code* instead, so a guard comparing it to a status
// matched only where the two happened to be spelled alike. `STAMPING` ->
// `COMPLETED` above is such an edge, which is why nothing here saw it. Every
// target below has a code that is not its status.
//
// Each test also reads the hook log, so a guard that did not run at all is not
// mistaken for a guard that passed.
//
// # Seen red on the unfixed code (`e9a1c29`), 2026-10-21
//
// 22 passed and these three failed, each with the 422 the criteria predicted:
// AC1 and AC2 refused with `TARGET_STATUS_WRONG`, AC3 with `SAW_THE_CODE`.
//
// # Seen to fail after the fix, 2026-10-21
//
// Each mutation applied alone to `run_transition_guards`, this file run, and
// the mutation reverted. AC1 is *…not_its_code*, AC2 *…a_person_acts_in*, AC3
// *a_guard_cannot_match…*.
//
// | Mutation | Red |
// |---|---|
// | `target_status` back to `Some(&chosen.to)` | AC1, AC2, AC3: 422 |
// | `target_status` is the source state's status | AC1, AC2: 422 |
// | `target_status: None` | AC1, AC2: 422. AC3 stays green: `null` is not `DONE` |
// | `current_status` and `target_status` swapped | AC1, AC2: 422 |
// | the target's status only when it `is_final`, its code otherwise | AC2: 422 |
// | the call to `run_transition_guards` removed | ten: AC1, AC2 and AC3 on the hook log, and `a_guard_runs_and_the_execution_is_logged` with six more that read a guard's run or refusal |
// | the swap above, with AC2's first state at `PENDING_APPROVAL` (a test mutation) | AC1 only. **AC2 stays green**: a guard's `currentStatus` is the status the decision started from, so that fixture cannot tell the two fields apart, and AC2's first state is `IN_REVIEW` for it |
//
// # The independent campaign, 2026-10-21
//
// The same method, by `test-engineer`, with the three tests after AC3 added.
// *Several* is `of_several_automatic_edges…`, *chain* is `in_a_chain…` and
// *unchanged* is `…when_the_status_does_not_change`. Re-run first: the
// builder's `Some(&chosen.to)` reddens AC1, AC2, AC3, *several*, *chain* and
// *unchanged*; `None` reddens AC1, AC2, *several*, *chain* and *unchanged*,
// with AC3 green as the table says.
//
// | Mutation | Red |
// |---|---|
// | the call passes `state` (the source) as `target` | AC1, AC2, *several*, *chain*. *Unchanged* stays green: its source and target are both `IN_REVIEW` |
// | the status of the first `AUTO` candidate's target, not `chosen`'s | *several* only, on the fallback (`DONE_STATUS_WRONG`). **AC1 to AC3 stay green**: each has one `AUTO` edge |
// | the status of the last `AUTO` candidate's target | *several* only, on the conditioned edge (`ESCALATED_STATUS_WRONG`) |
// | the status the advance settles in, read ahead through every service state | *chain* only (`FIRST_STEP_STATUS_WRONG`). **AC1 to AC3 stay green**: each advance is one step |
// | `null` when the target's status is the source state's | *unchanged* only |
// | `null` when the target's status is `currentStatus` | *unchanged*, and *several* on the conditioned edge. **AC2 stays green**: its `currentStatus` is `IN_REVIEW` |
// | the status of the graph's first final state | AC2, *several*, *chain*, *unchanged*. AC1 and AC3 stay green: their target is that state |
// | `after_hooks`' `targetStatus` given as the state code `event.to` | **nothing: `outbox` 23 of 23, `workflow_engine` 69 of 69, this file 28 of 28.** No after-capable handler reads its payload (`core:continue_always` is the only one), so no test can see the after-chain's value. AC4's agreement is held by the code comment alone |

/// A guard that refuses with `code` when `condition` holds against the LHCS §4
/// payload.
fn guard_refusing_when(condition: Value, code: &str) -> Value {
    json!([{
        "handler": "core:reject_when",
        "config": { "condition": condition, "code": code, "message": code }
    }])
}

/// [`workflow_with_a_service_state`], whose final state is `DONE`: a code that
/// is not its status, `COMPLETED`.
fn workflow_ending_in_done(key: &str, guards: Value) -> Value {
    let mut definition = workflow_with_a_service_state(key, guards, None);

    definition["states"][2]["code"] = json!("DONE");
    definition["states"][2]["name"] = json!("Done");
    definition["transitions"][1]["to"] = json!("DONE");

    definition
}

/// [`workflow_with_a_service_state`], whose automatic edge reaches
/// `MANAGER_APPROVAL`: a state a person acts in, whose status is
/// `PENDING_APPROVAL`. Its approval leads on to `COMPLETED`.
///
/// **The first state is `IN_REVIEW` here, not `PENDING_APPROVAL`.** The guard
/// runs inside the decision that left `SUBMITTED`, so the payload's
/// `currentStatus` is the status that decision started from. Were that also
/// `PENDING_APPROVAL`, a guard given `currentStatus` in place of `targetStatus`
/// would pass, and this test could not tell the two apart.
fn workflow_reaching_a_manager(key: &str, guards: Value) -> Value {
    let mut definition = workflow_with_a_service_state(key, guards, None);

    definition["states"][0]["mapsToDocumentStatus"] = json!("IN_REVIEW");
    definition["states"].as_array_mut().expect("states").insert(
        2,
        json!({
            "code": "MANAGER_APPROVAL", "name": "Manager approval",
            "mapsToDocumentStatus": "PENDING_APPROVAL",
            "task": { "taskDefinitionKey": "manager_approve",
                      "taskName": "Approve as the manager",
                      "assignment": { "assigneeType": "ROLE", "roleCode": APPROVER_ROLE } }
        }),
    );
    definition["transitions"][1]["to"] = json!("MANAGER_APPROVAL");
    definition["transitions"]
        .as_array_mut()
        .expect("transitions")
        .push(json!({
            "from": "MANAGER_APPROVAL", "to": "COMPLETED", "action": "APPROVE",
            "allowedBy": format!("ROLE:{APPROVER_ROLE}")
        }));

    definition
}

/// **#679 AC1: a guard on an edge to a final state is given that state's
/// status, not its code.** The guard refuses unless `targetStatus` is
/// `COMPLETED`, and the edge's target is `DONE`.
#[tokio::test]
async fn a_guards_target_status_is_the_target_states_status_not_its_code() {
    let app = TestApp::spawn().await;
    let admin = app.administrator_token().await;
    let token = approver(&app, "systask.target.one").await;

    let workflow = publish_workflow(
        &app,
        &admin,
        workflow_ending_in_done(
            "stamped_target_one",
            guard_refusing_when(
                json!({ "!=": [{ "var": "targetStatus" }, "COMPLETED"] }),
                "TARGET_STATUS_WRONG",
            ),
        ),
    )
    .await;
    let type_id = document_type(&app, &admin, "STAMPED_TARGET_ONE", workflow).await;
    let document = draft(&app, &token, type_id, 100).await;

    submit(&app, &token, document).await;

    let decided = approve(&app, &token, document).await;

    assert_eq!(decided.status, StatusCode::OK, "{}", decided.body);
    assert_eq!(state_of(&app, document).await, "DONE");
    assert_eq!(status_of(&app, &token, document).await, "COMPLETED");
    assert_eq!(
        hook_runs(&app, document).await,
        vec![("core:reject_when".to_owned(), "CONTINUE".to_owned())],
        "the guard ran, and passed"
    );
}

/// **#679 AC2: the same for a target a person acts in.** `MANAGER_APPROVAL` is
/// not final, so a fix that mapped only a final state's status is red here.
#[tokio::test]
async fn a_guards_target_status_is_the_target_states_status_for_a_state_a_person_acts_in() {
    let app = TestApp::spawn().await;
    let admin = app.administrator_token().await;
    let token = approver(&app, "systask.target.two").await;

    let workflow = publish_workflow(
        &app,
        &admin,
        workflow_reaching_a_manager(
            "stamped_target_two",
            guard_refusing_when(
                json!({ "!=": [{ "var": "targetStatus" }, "PENDING_APPROVAL"] }),
                "TARGET_STATUS_WRONG",
            ),
        ),
    )
    .await;
    let type_id = document_type(&app, &admin, "STAMPED_TARGET_TWO", workflow).await;
    let document = draft(&app, &token, type_id, 100).await;

    submit(&app, &token, document).await;

    let decided = approve(&app, &token, document).await;

    assert_eq!(decided.status, StatusCode::OK, "{}", decided.body);
    assert_eq!(state_of(&app, document).await, "MANAGER_APPROVAL");
    assert_eq!(status_of(&app, &token, document).await, "PENDING_APPROVAL");
    assert_eq!(tasks_for(&app, document, "manager_approve").await, 1);
    assert_eq!(
        hook_runs(&app, document).await,
        vec![("core:reject_when".to_owned(), "CONTINUE".to_owned())],
        "the guard ran, and passed"
    );
}

/// **#679 AC3: a state code does not reach the guard.** AC1's edge, with a
/// guard that refuses if `targetStatus` is the code `DONE`.
#[tokio::test]
async fn a_guard_cannot_match_the_target_state_code_through_target_status() {
    let app = TestApp::spawn().await;
    let admin = app.administrator_token().await;
    let token = approver(&app, "systask.target.three").await;

    let workflow = publish_workflow(
        &app,
        &admin,
        workflow_ending_in_done(
            "stamped_target_three",
            guard_refusing_when(
                json!({ "==": [{ "var": "targetStatus" }, "DONE"] }),
                "SAW_THE_CODE",
            ),
        ),
    )
    .await;
    let type_id = document_type(&app, &admin, "STAMPED_TARGET_THREE", workflow).await;
    let document = draft(&app, &token, type_id, 100).await;

    submit(&app, &token, document).await;

    let decided = approve(&app, &token, document).await;

    assert_eq!(decided.status, StatusCode::OK, "{}", decided.body);
    assert_eq!(state_of(&app, document).await, "DONE");
    assert_eq!(
        hook_runs(&app, document).await,
        vec![("core:reject_when".to_owned(), "CONTINUE".to_owned())],
        "the guard ran, and passed"
    );
}

/// The two states every fixture below starts from: an approval, then a service
/// state whose `AUTO` edges the test declares.
fn approval_then_stamping(submitted_status: &str) -> Value {
    json!([
        { "code": "SUBMITTED", "name": "Awaiting approval",
          "mapsToDocumentStatus": submitted_status,
          "task": { "taskDefinitionKey": "approve", "taskName": "Approve the request",
                    "assignment": { "assigneeType": "ROLE", "roleCode": APPROVER_ROLE } } },
        { "code": "STAMPING", "name": "Stamping", "mapsToDocumentStatus": "IN_REVIEW",
          "task": { "taskDefinitionKey": "stamp", "taskName": "Stamp it",
                    "taskType": "SERVICE_TASK",
                    "assignment": { "assigneeType": "ROLE", "roleCode": APPROVER_ROLE } } }
    ])
}

/// A definition of `states` after [`approval_then_stamping`]'s two, with the
/// approval into `STAMPING` and then `transitions`.
fn stamping_definition(
    key: &str,
    submitted_status: &str,
    states: Value,
    transitions: Value,
) -> Value {
    let mut all_states = approval_then_stamping(submitted_status);
    let mut all_transitions = json!([
        { "from": "SUBMITTED", "to": "STAMPING", "action": "APPROVE",
          "allowedBy": format!("ROLE:{APPROVER_ROLE}") }
    ]);

    all_states
        .as_array_mut()
        .expect("states")
        .extend(states.as_array().expect("states").iter().cloned());
    all_transitions
        .as_array_mut()
        .expect("transitions")
        .extend(transitions.as_array().expect("transitions").iter().cloned());

    json!({
        "workflowKey": key,
        "version": "1.0.0",
        "name": "Stamped approval",
        "initialState": "SUBMITTED",
        "states": all_states,
        "transitions": all_transitions,
        "variables": [
            { "key": "amount", "dataType": "NUMBER", "source": { "var": "formData.amount" } }
        ]
    })
}

/// A state a person acts in, with its own task.
fn human_state(code: &str, status: &str, task_key: &str) -> Value {
    json!({
        "code": code, "name": code, "mapsToDocumentStatus": status,
        "task": { "taskDefinitionKey": task_key, "taskName": task_key,
                  "assignment": { "assigneeType": "ROLE", "roleCode": APPROVER_ROLE } }
    })
}

/// **#679, the independent campaign: of several `AUTO` edges, the guard is
/// given the status of the edge that was chosen.**
///
/// `STAMPING` has two: a conditioned edge to `ESCALATED` (`PENDING_APPROVAL`)
/// for an amount above 1000, and the fallback to `DONE` (`COMPLETED`). Each
/// carries a guard that refuses unless `targetStatus` is its own target's.
/// AC1 to AC3 have one `AUTO` edge each, so a payload read from the first or
/// the last candidate rather than from `chosen` was green there.
#[tokio::test]
async fn of_several_automatic_edges_the_guard_is_given_the_chosen_edges_target_status() {
    let app = TestApp::spawn().await;
    let admin = app.administrator_token().await;
    let token = approver(&app, "systask.target.four").await;

    let definition = stamping_definition(
        "stamped_target_four",
        "PENDING_APPROVAL",
        json!([
            human_state("ESCALATED", "PENDING_APPROVAL", "escalated_approve"),
            { "code": "DONE", "name": "Done", "mapsToDocumentStatus": "COMPLETED",
              "isFinal": true }
        ]),
        json!([
            { "from": "STAMPING", "to": "ESCALATED", "action": "AUTO",
              "condition": { ">": [{ "var": "variables.amount" }, 1000] },
              "guards": guard_refusing_when(
                  json!({ "!=": [{ "var": "targetStatus" }, "PENDING_APPROVAL"] }),
                  "ESCALATED_STATUS_WRONG",
              ) },
            { "from": "STAMPING", "to": "DONE", "action": "AUTO",
              "guards": guard_refusing_when(
                  json!({ "!=": [{ "var": "targetStatus" }, "COMPLETED"] }),
                  "DONE_STATUS_WRONG",
              ) },
            { "from": "ESCALATED", "to": "DONE", "action": "APPROVE",
              "allowedBy": format!("ROLE:{APPROVER_ROLE}") }
        ]),
    );

    let workflow = publish_workflow(&app, &admin, definition).await;
    let type_id = document_type(&app, &admin, "STAMPED_TARGET_FOUR", workflow).await;

    // The fallback, which is the last candidate.
    let small = draft(&app, &token, type_id, 100).await;

    submit(&app, &token, small).await;

    let decided = approve(&app, &token, small).await;

    assert_eq!(decided.status, StatusCode::OK, "{}", decided.body);
    assert_eq!(state_of(&app, small).await, "DONE");
    assert_eq!(
        hook_runs(&app, small).await,
        vec![("core:reject_when".to_owned(), "CONTINUE".to_owned())],
        "the fallback's guard ran, and passed"
    );

    // The conditioned edge, which is the first.
    let big = draft(&app, &token, type_id, 5000).await;

    submit(&app, &token, big).await;

    let decided = approve(&app, &token, big).await;

    assert_eq!(decided.status, StatusCode::OK, "{}", decided.body);
    assert_eq!(state_of(&app, big).await, "ESCALATED");
    assert_eq!(status_of(&app, &token, big).await, "PENDING_APPROVAL");
    assert_eq!(
        hook_runs(&app, big).await,
        vec![("core:reject_when".to_owned(), "CONTINUE".to_owned())],
        "the conditioned edge's guard ran, and passed"
    );
}

/// **#679, the independent campaign: in a chain of automatic steps, each guard
/// is given its own edge's target status.**
///
/// `STAMPING` (`IN_REVIEW`) goes on to `SEALING` (`SUBMITTED`), a second
/// service state, and that to `DONE` (`COMPLETED`). The first guard refuses
/// unless `targetStatus` is `SUBMITTED`, the second unless it is `COMPLETED`.
/// A guard given the status the whole advance settles in, which is the
/// decision's eventual status rather than this edge's, is red on the first.
#[tokio::test]
async fn in_a_chain_of_automatic_steps_each_guard_is_given_its_own_targets_status() {
    let app = TestApp::spawn().await;
    let admin = app.administrator_token().await;
    let token = approver(&app, "systask.target.five").await;

    let definition = stamping_definition(
        "stamped_target_five",
        "PENDING_APPROVAL",
        json!([
            { "code": "SEALING", "name": "Sealing", "mapsToDocumentStatus": "SUBMITTED",
              "task": { "taskDefinitionKey": "seal", "taskName": "Seal it",
                        "taskType": "SERVICE_TASK",
                        "assignment": { "assigneeType": "ROLE", "roleCode": APPROVER_ROLE } } },
            { "code": "DONE", "name": "Done", "mapsToDocumentStatus": "COMPLETED",
              "isFinal": true }
        ]),
        json!([
            { "from": "STAMPING", "to": "SEALING", "action": "AUTO",
              "guards": guard_refusing_when(
                  json!({ "!=": [{ "var": "targetStatus" }, "SUBMITTED"] }),
                  "FIRST_STEP_STATUS_WRONG",
              ) },
            { "from": "SEALING", "to": "DONE", "action": "AUTO",
              "guards": guard_refusing_when(
                  json!({ "!=": [{ "var": "targetStatus" }, "COMPLETED"] }),
                  "SECOND_STEP_STATUS_WRONG",
              ) }
        ]),
    );

    let workflow = publish_workflow(&app, &admin, definition).await;
    let type_id = document_type(&app, &admin, "STAMPED_TARGET_FIVE", workflow).await;
    let document = draft(&app, &token, type_id, 100).await;

    submit(&app, &token, document).await;

    let decided = approve(&app, &token, document).await;

    assert_eq!(decided.status, StatusCode::OK, "{}", decided.body);
    assert_eq!(state_of(&app, document).await, "DONE");
    assert_eq!(status_of(&app, &token, document).await, "COMPLETED");
    assert_eq!(
        hook_runs(&app, document).await,
        vec![
            ("core:reject_when".to_owned(), "CONTINUE".to_owned()),
            ("core:reject_when".to_owned(), "CONTINUE".to_owned()),
        ],
        "both guards ran, and passed"
    );
}

/// **#679, the independent campaign: `targetStatus` is given when it is the
/// status the document already has.**
///
/// LHCS §4 reads *when the action changes status*, and the criteria chose to
/// give the value regardless, as the after-chain gives it. Every state here
/// maps to `IN_REVIEW` until the last, so a guard nulling `targetStatus` when
/// it equals either the source state's status or `currentStatus` is red.
#[tokio::test]
async fn a_guards_target_status_is_given_when_the_status_does_not_change() {
    let app = TestApp::spawn().await;
    let admin = app.administrator_token().await;
    let token = approver(&app, "systask.target.six").await;

    let definition = stamping_definition(
        "stamped_target_six",
        "IN_REVIEW",
        json!([
            human_state("SECOND_LOOK", "IN_REVIEW", "second_look"),
            { "code": "DONE", "name": "Done", "mapsToDocumentStatus": "COMPLETED",
              "isFinal": true }
        ]),
        json!([
            { "from": "STAMPING", "to": "SECOND_LOOK", "action": "AUTO",
              "guards": guard_refusing_when(
                  json!({ "!=": [{ "var": "targetStatus" }, "IN_REVIEW"] }),
                  "TARGET_STATUS_DROPPED",
              ) },
            { "from": "SECOND_LOOK", "to": "DONE", "action": "APPROVE",
              "allowedBy": format!("ROLE:{APPROVER_ROLE}") }
        ]),
    );

    let workflow = publish_workflow(&app, &admin, definition).await;
    let type_id = document_type(&app, &admin, "STAMPED_TARGET_SIX", workflow).await;
    let document = draft(&app, &token, type_id, 100).await;

    submit(&app, &token, document).await;

    let decided = approve(&app, &token, document).await;

    assert_eq!(decided.status, StatusCode::OK, "{}", decided.body);
    assert_eq!(state_of(&app, document).await, "SECOND_LOOK");
    assert_eq!(status_of(&app, &token, document).await, "IN_REVIEW");
    assert_eq!(
        hook_runs(&app, document).await,
        vec![("core:reject_when".to_owned(), "CONTINUE".to_owned())],
        "the guard ran, and passed"
    );
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
/// **Seen red, 2026-09-08**: `AND document_type_id = $3` in place of
/// `AND (document_type_id IS NULL OR document_type_id = $3)` in
/// `hook::repository::registry_chain`.
///
/// **That mutation changes SQL text, so it does not compile under
/// `SQLX_OFFLINE`** — the cached query no longer matches. It was run with
/// `DATABASE_URL` pointed at a live database and `SQLX_OFFLINE` unset, which
/// is worth stating: a mutation somebody cannot reproduce from the offline
/// cache reads as unrunnable rather than as run.
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

// ---------------------------------------------------------------------------
// The outbox event every transition writes (ADR-0041, #519)
// ---------------------------------------------------------------------------

/// The `Workflow.Transitioned` events one document's instance has written, in
/// the order they were written.
async fn events_of(app: &TestApp, document_id: Uuid) -> Vec<(String, Uuid, String, Value)> {
    sqlx::query_as(
        "SELECT o.event_type, o.aggregate_id, o.status, o.payload_json
         FROM outbox_events o
         JOIN workflow_instances i ON i.id = o.aggregate_id
         WHERE i.document_id = $1
         ORDER BY o.id",
    )
    .bind(document_id)
    .fetch_all(&app.pool)
    .await
    .expect("read the outbox")
}

/// **A decided and an automatic transition each write one event**, and each
/// envelope validates against the EES meta-schema (ADR-0041 §2, §5; AC-2,
/// AC-4).
///
/// One approval here is two transitions — `SUBMITTED` to `STAMPING` decided,
/// `STAMPING` to `COMPLETED` taken by the engine — so the two writes are told
/// apart by their action and their actor, and the count of two is what rules
/// out *one event per decision*.
///
/// Seen red, 2026-09-24: the `publish_transition` call removed from
/// `engine::advance_automatically` — one event, not two.
#[tokio::test]
async fn a_decided_and_an_automatic_transition_each_write_one_valid_event() {
    let app = TestApp::spawn().await;
    let admin = app.administrator_token().await;
    let token = approver(&app, "systask.events").await;
    let approver_id: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE username = $1")
        .bind("systask.events")
        .fetch_one(&app.pool)
        .await
        .expect("the approver");

    let workflow = publish_workflow(
        &app,
        &admin,
        workflow_with_a_service_state("stamped_events", json!([]), None),
    )
    .await;
    let type_id = document_type(&app, &admin, "STAMPED_EVENTS", workflow).await;
    let document = draft(&app, &token, type_id, 100).await;

    submit(&app, &token, document).await;

    // The start is not a transition: its history row has no action and no
    // `from_state`, and it writes no event.
    assert!(events_of(&app, document).await.is_empty());

    assert_eq!(approve(&app, &token, document).await.status, StatusCode::OK);

    let events = events_of(&app, document).await;
    let instance: Uuid = sqlx::query_scalar(
        "SELECT id FROM workflow_instances WHERE document_id = $1 AND deleted_at IS NULL",
    )
    .bind(document)
    .fetch_one(&app.pool)
    .await
    .expect("the instance");

    assert_eq!(events.len(), 2, "{events:?}");

    let validator = common::ees_validator();

    for (event_type, aggregate_id, status, envelope) in &events {
        let errors: Vec<String> = validator
            .iter_errors(envelope)
            .map(|error| format!("{} at {}", error, error.instance_path()))
            .collect();

        assert!(errors.is_empty(), "EES S1: {errors:?}\n{envelope:#}");
        assert_eq!(event_type, "Workflow.Transitioned");
        assert_eq!(envelope["eventType"], "Workflow.Transitioned");
        assert_eq!(envelope["aggregateType"], "WORKFLOW_INSTANCE");
        assert_eq!(*aggregate_id, instance);
        assert_eq!(envelope["aggregateId"], instance.to_string());
        assert_eq!(status, "PENDING");

        // EES §4.1's `Workflow.*` profile, and the two this event adds.
        let payload = &envelope["payload"];

        for field in [
            "instanceId",
            "workflowKey",
            "workflowRevision",
            "documentId",
            "state",
            "taskId",
            "assigneeUserId",
            "action",
            "fromState",
            "toState",
        ] {
            assert!(
                payload.get(field).is_some(),
                "`{field}` is missing: {payload}"
            );
        }

        assert_eq!(payload["documentId"], document.to_string());
        assert_eq!(payload["workflowKey"], "stamped_events");
        // **No form data** (EES §4.2): the amount is on the document and not
        // in the event.
        assert!(!payload.to_string().contains("amount"), "{payload}");
    }

    let (decided, automatic) = (&events[0].3, &events[1].3);

    assert_eq!(decided["payload"]["action"], "APPROVE");
    assert_eq!(decided["payload"]["fromState"], "SUBMITTED");
    assert_eq!(decided["payload"]["toState"], "STAMPING");
    assert_eq!(decided["actor"]["actorType"], "USER");
    assert_eq!(decided["actor"]["actorId"], approver_id.to_string());

    assert_eq!(automatic["payload"]["action"], "AUTO");
    assert_eq!(automatic["payload"]["fromState"], "STAMPING");
    assert_eq!(automatic["payload"]["toState"], "COMPLETED");
    assert_eq!(automatic["actor"]["actorType"], "WORKFLOW_ENGINE");
    assert!(automatic["actor"]["actorId"].is_null());

    // `sequence` is the transition's place in the instance's history: the
    // start is 1, so the two transitions are 2 and 3.
    assert_eq!(decided["sequence"], 2);
    assert_eq!(automatic["sequence"], 3);
}

/// **A transition that rolls back writes no event** (AC-3): the guard refuses,
/// the whole advance is undone, and the decided transition before it takes its
/// event with it.
///
/// The second subject is the same workflow with an amount the guard allows,
/// which writes both — so the empty outbox is the rollback and not a producer
/// that never wrote.
///
/// Seen red, 2026-09-24: `outbox::repository::insert` executed on a connection
/// of its own, opened to the transaction's `current_database()`, instead of on
/// the caller's transaction — the refused approval left its event behind.
#[tokio::test]
async fn a_transition_that_rolls_back_writes_no_event() {
    let app = TestApp::spawn().await;
    let admin = app.administrator_token().await;
    let token = approver(&app, "systask.rollback").await;

    let workflow = publish_workflow(
        &app,
        &admin,
        workflow_with_a_service_state(
            "stamped_rollback",
            json!([{
                "handler": "core:reject_when",
                "config": { "condition": { ">": [{ "var": "formData.amount" }, 50] } }
            }]),
            None,
        ),
    )
    .await;
    let type_id = document_type(&app, &admin, "STAMPED_ROLLBACK", workflow).await;

    let refused = draft(&app, &token, type_id, 100).await;

    submit(&app, &token, refused).await;

    assert_eq!(
        approve(&app, &token, refused).await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert!(events_of(&app, refused).await.is_empty());
    // Nor any execution row: the guard's own log went with the transaction.
    assert!(hook_runs(&app, refused).await.is_empty());

    let allowed = draft(&app, &token, type_id, 10).await;

    submit(&app, &token, allowed).await;

    assert_eq!(approve(&app, &token, allowed).await.status, StatusCode::OK);
    assert_eq!(events_of(&app, allowed).await.len(), 2);
}

/// **ADR-0041 §2**: `actions` naming a before-only handler is refused where the
/// author is, with `HANDLER_KIND_MISMATCH`.
///
/// Seen red, 2026-09-24: `let serves = true;` in place of the kind check in
/// `hook::service::check_entry` — the definition was created.
#[tokio::test]
async fn an_actions_entry_naming_a_before_only_handler_is_refused_at_save() {
    let app = TestApp::spawn().await;
    let admin = app.administrator_token().await;

    let mut definition = workflow_with_a_service_state("kind_mismatch", json!([]), None);

    definition["transitions"][0]["actions"] = json!([
        { "handler": "core:set_form_field", "config": { "field": "stamped", "value": true } }
    ]);

    let created = create_definition(&app, &admin, definition).await;

    assert_eq!(
        created.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        created.body
    );

    let detail = created.body["error"]["details"]
        .as_array()
        .expect("details")
        .iter()
        .find(|detail| detail["code"] == "HANDLER_KIND_MISMATCH")
        .cloned()
        .unwrap_or_else(|| panic!("{}", created.body));

    assert_eq!(detail["path"], "definition.transitions.0.actions.0.handler");
    assert_reads_as_one_line(&detail);
    // The same detail the publication route answers, member for member: the
    // sentence whole, and the rule it cites (`LHCS-3.2` changed to `LHCS-2`
    // left all 22 tests of this file green).
    assert_eq!(detail["rule"], "LHCS-3.2");
    assert_eq!(detail["message"], KIND_MISMATCH_OF_SET_FORM_FIELD);

    // The handler that serves after commit is accepted in the same position.
    let mut definition = workflow_with_a_service_state("kind_match", json!([]), None);

    definition["transitions"][0]["actions"] = json!([{ "handler": "core:continue_always" }]);

    assert_eq!(
        create_definition(&app, &admin, definition).await.status,
        StatusCode::CREATED
    );
}

/// The `HANDLER_KIND_MISMATCH` sentence for `core:set_form_field` in `actions`,
/// as both routes answer it.
const KIND_MISMATCH_OF_SET_FORM_FIELD: &str =
    "`core:set_form_field` is a before-hook handler: its result is a veto or a change to the \
     form, and once the transition has committed there is nothing left to refuse or change. An \
     `actions` entry runs after commit; the handlers that can are `core:continue_always`";

/// A refusal's message is one line of prose (#558): no run of two spaces, and
/// no line break.
///
/// Two assertions, because they are two ways a string literal goes wrong. One
/// continued across source lines without `\` carries a newline and the next
/// line's indentation; the same literal on one source line carries the
/// indentation alone, which is what the route answered.
fn assert_reads_as_one_line(detail: &Value) {
    let message = detail["message"].as_str().expect("a message");

    assert!(
        !message.contains("  "),
        "a run of two or more spaces: {message:?}"
    );
    assert!(!message.contains('\n'), "a line break: {message:?}");
}

/// **The refusal as the publication route answers it** (#558; verification
/// record 19, probe P3b, finding 8).
///
/// The save route refuses this definition first, so a draft carrying it is one
/// an older release stored: `0.8.0` accepted a before-only handler in
/// `actions`, and the upgraded binary refuses the draft when somebody publishes
/// it. That is where record 19 read three runs of 34 spaces mid-sentence. The
/// draft is planted as `outbox.rs` plants its own, by writing the row.
#[tokio::test]
async fn a_stored_draft_naming_a_before_only_handler_is_refused_at_publication_in_one_line() {
    let app = TestApp::spawn().await;
    let admin = app.administrator_token().await;

    let created = create_definition(
        &app,
        &admin,
        workflow_with_a_service_state("kind_mismatch_stored", json!([]), None),
    )
    .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    let id = id_of(&created.body["data"]);

    sqlx::query(
        "UPDATE workflow_definitions
         SET definition_json = jsonb_set(definition_json, '{transitions,0,actions}', $1)
         WHERE id = $2",
    )
    .bind(json!([
        { "handler": "core:set_form_field", "config": { "field": "stamped", "value": true } },
        { "handler": "core:reject_when", "config": { "condition": { "==": [1, 1] } } }
    ]))
    .bind(id)
    .execute(&app.pool)
    .await
    .expect("the draft as an older release stored it");

    let publication = app
        .post(
            &format!("/api/v1/workflow/definitions/{id}/publication"),
            Some(&admin),
            json!({}),
        )
        .await;

    assert_eq!(
        publication.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        publication.body
    );

    let details = publication.body["error"]["details"]
        .as_array()
        .expect("details");

    assert_eq!(
        details
            .iter()
            .map(|detail| detail["code"].as_str().expect("a code"))
            .collect::<Vec<_>>(),
        ["HANDLER_KIND_MISMATCH", "HANDLER_KIND_MISMATCH"],
        "{}",
        publication.body
    );

    for detail in details {
        assert_reads_as_one_line(detail);
    }

    // The sentence whole, as the author reads it.
    assert_eq!(details[0]["message"], KIND_MISMATCH_OF_SET_FORM_FIELD);
    // Each entry at its own path, under the rule the save route cites.
    assert_eq!(
        details
            .iter()
            .map(|detail| (detail["path"].as_str(), detail["rule"].as_str()))
            .collect::<Vec<_>>(),
        [
            (
                Some("definition.transitions.0.actions.0.handler"),
                Some("LHCS-3.2")
            ),
            (
                Some("definition.transitions.0.actions.1.handler"),
                Some("LHCS-3.2")
            ),
        ],
        "{}",
        publication.body
    );
    assert_eq!(
        details[1]["message"],
        KIND_MISMATCH_OF_SET_FORM_FIELD.replace("core:set_form_field", "core:reject_when")
    );
    // The envelope's own message is not where the spaces were, and is read too.
    assert_reads_as_one_line(&publication.body["error"]);
}

// ---------------------------------------------------------------------------
// #706 — the condition context the engine builds, at each of its three callers
// ---------------------------------------------------------------------------
//
// A condition is evaluated against JWSS §6.1's context at three places: the
// submit that starts an instance, the resubmission that moves it on from
// `RETURNED`, and a decision. [`routed_everywhere`] puts one condition at each
// of the three, so a caller that builds a different context from the others
// routes its own step the other way and is named by the assertion that fails.

/// A definition that evaluates `condition` at the start, at a resubmission and
/// at a decision, and goes to a `MATCHED` state where it holds and an `OTHER`
/// one where it does not.
///
/// `ROUTING` is the initial state and a service state, so the submit itself
/// takes its `AUTO` edge. `RETURNED`'s `RESUBMIT` edges carry the condition,
/// so the resubmission evaluates it. An approval reaches `CLOSING`, another
/// service state, whose `AUTO` edges are evaluated in the decision's own
/// transaction, against the decision's context.
fn routed_everywhere(key: &str, condition: Value) -> Value {
    let person = |code: &str| {
        json!({
            "code": code, "name": code, "mapsToDocumentStatus": "PENDING_APPROVAL",
            "task": { "taskDefinitionKey": code.to_lowercase(), "taskName": code,
                      "assignment": { "assigneeType": "ROLE", "roleCode": APPROVER_ROLE } }
        })
    };
    let service = |code: &str| {
        json!({
            "code": code, "name": code, "mapsToDocumentStatus": "IN_REVIEW",
            "task": { "taskDefinitionKey": code.to_lowercase(), "taskName": code,
                      "taskType": "SERVICE_TASK",
                      "assignment": { "assigneeType": "ROLE", "roleCode": APPROVER_ROLE } }
        })
    };
    let finished = |code: &str| json!({ "code": code, "name": code, "mapsToDocumentStatus": "COMPLETED", "isFinal": true });
    let decided = |from: &str, to: &str, action: &str| {
        json!({ "from": from, "to": to, "action": action,
                "allowedBy": format!("ROLE:{APPROVER_ROLE}") })
    };

    json!({
        "workflowKey": key,
        "version": "1.0.0",
        "name": "Routed at every caller",
        "initialState": "ROUTING",
        "states": [
            service("ROUTING"),
            person("MATCHED"),
            person("OTHER"),
            { "code": "RETURNED", "name": "Sent back", "mapsToDocumentStatus": "RETURNED" },
            service("CLOSING"),
            finished("DONE_MATCHED"),
            finished("DONE_OTHER")
        ],
        "transitions": [
            { "from": "ROUTING", "to": "MATCHED", "action": "AUTO", "condition": condition },
            { "from": "ROUTING", "to": "OTHER", "action": "AUTO" },
            decided("MATCHED", "RETURNED", "RETURN"),
            decided("OTHER", "RETURNED", "RETURN"),
            { "from": "RETURNED", "to": "MATCHED", "action": "RESUBMIT", "allowedBy": "OWNER",
              "condition": condition },
            { "from": "RETURNED", "to": "OTHER", "action": "RESUBMIT", "allowedBy": "OWNER" },
            decided("MATCHED", "CLOSING", "APPROVE"),
            decided("OTHER", "CLOSING", "APPROVE"),
            { "from": "CLOSING", "to": "DONE_MATCHED", "action": "AUTO", "condition": condition },
            { "from": "CLOSING", "to": "DONE_OTHER", "action": "AUTO" }
        ]
    })
}

/// Decides the one open task on a document with `action`.
async fn decide_open_task(
    app: &TestApp,
    token: &str,
    document_id: Uuid,
    action: &str,
) -> common::TestResponse {
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
        json!({ "action": action, "comment": "Routed by the test." }),
    )
    .await
}

/// Drafts a document of `type_id` as `owner`, submits it, has it returned,
/// resubmits it, and has `decider` approve it. Gives the state the start, the
/// resubmission and the decision each routed it to.
async fn route_through_every_caller(
    app: &TestApp,
    owner: &str,
    decider: &str,
    type_id: Uuid,
) -> [String; 3] {
    let document = draft(app, owner, type_id, 100).await;

    let submitted = submit(app, owner, document).await;
    assert_eq!(submitted.status, StatusCode::OK, "{}", submitted.body);
    let started = state_of(app, document).await;

    let sent_back = decide_open_task(app, owner, document, "RETURN").await;
    assert_eq!(sent_back.status, StatusCode::OK, "{}", sent_back.body);
    assert_eq!(state_of(app, document).await, "RETURNED");

    let resubmitted = submit(app, owner, document).await;
    assert_eq!(resubmitted.status, StatusCode::OK, "{}", resubmitted.body);
    let after_resubmission = state_of(app, document).await;

    let approved = decide_open_task(app, decider, document, "APPROVE").await;
    assert_eq!(approved.status, StatusCode::OK, "{}", approved.body);
    let after_decision = state_of(app, document).await;

    [started, after_resubmission, after_decision]
}

const MATCHED_EVERYWHERE: [&str; 3] = ["MATCHED", "MATCHED", "DONE_MATCHED"];
const OTHER_EVERYWHERE: [&str; 3] = ["OTHER", "OTHER", "DONE_OTHER"];

/// **#706 AC1: `document.documentTypeKey` is the type's stored `type_code`, at
/// the start, at a resubmission and at a decision.** Two types bound to one
/// definition, whose condition names the first type's code: the first routes
/// to `MATCHED` at all three, the second to `OTHER` at all three.
///
/// The key is spelled as stored, upper case (DTDS §2), and the spelling is
/// asserted by the match itself.
#[tokio::test]
async fn the_document_type_key_routes_at_the_start_a_resubmission_and_a_decision() {
    let app = TestApp::spawn().await;
    let admin = app.administrator_token().await;
    let token = approver(&app, "systask.key.one").await;

    let workflow = publish_workflow(
        &app,
        &admin,
        routed_everywhere(
            "routed_by_key",
            json!({ "==": [{ "var": "document.documentTypeKey" }, "KEYED_ONE"] }),
        ),
    )
    .await;
    let keyed = document_type(&app, &admin, "KEYED_ONE", workflow).await;
    let other = document_type(&app, &admin, "KEYED_TWO", workflow).await;

    assert_eq!(
        route_through_every_caller(&app, &token, &token, keyed).await,
        MATCHED_EVERYWHERE,
        "[start, resubmission, decision]: the condition on the type's key did not hold"
    );
    assert_eq!(
        route_through_every_caller(&app, &token, &token, other).await,
        OTHER_EVERYWHERE,
        "[start, resubmission, decision]: another type's document matched the key"
    );
}

/// **#706 AC5, with AC2 and AC3 at the two submit callers: the actor's three
/// facts come from one builder at every caller, and the actor is the person
/// acting.** The condition holds only for a user who is `owner`, holds
/// `SYSTASK-SENIOR`, and sits in `department`. A caller that dropped any one
/// fact, or built the actor from somebody else, routes its step to `OTHER`.
///
/// The third walk has `outsider` decide `owner`'s document: the decision's
/// actor is the decider, so only that step goes the other way.
#[tokio::test]
async fn the_actors_facts_route_at_the_start_a_resubmission_and_a_decision() {
    let app = TestApp::spawn().await;
    let admin = app.administrator_token().await;

    let approving = approver_role(&app).await;
    let senior = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "SYSTASK-SENIOR",
        &[],
    )
    .await;
    let department = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO departments (id, tenant_id, department_code, name)
         VALUES ($1, $2, 'SYSTASK-DEPT', 'Systask department')",
    )
    .bind(department)
    .bind(fixtures::SYSTEM_TENANT_ID)
    .execute(&app.pool)
    .await
    .expect("insert the department");

    let owner_id = fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "systask.actor.owner",
        "systask.actor.owner@kelir.test",
        common::ADMIN_PASSWORD,
        &[approving, senior],
    )
    .await;
    sqlx::query("UPDATE users SET department_id = $1 WHERE id = $2")
        .bind(department)
        .bind(owner_id)
        .execute(&app.pool)
        .await
        .expect("place the owner in the department");
    let owner = app
        .sign_in("systask.actor.owner", common::ADMIN_PASSWORD)
        .await;
    let outsider = approver(&app, "systask.actor.outsider").await;

    let workflow = publish_workflow(
        &app,
        &admin,
        routed_everywhere(
            "routed_by_actor",
            json!({ "and": [
                { "==": [{ "var": "actor.userId" }, owner_id.to_string()] },
                { "in": ["SYSTASK-SENIOR", { "var": "actor.roles" }] },
                { "==": [{ "var": "actor.departmentId" }, department.to_string()] }
            ] }),
        ),
    )
    .await;
    let type_id = document_type(&app, &admin, "ACTOR_ROUTED", workflow).await;

    assert_eq!(
        route_through_every_caller(&app, &owner, &owner, type_id).await,
        MATCHED_EVERYWHERE,
        "[start, resubmission, decision]: the acting user's facts did not reach the condition"
    );
    assert_eq!(
        route_through_every_caller(&app, &outsider, &outsider, type_id).await,
        OTHER_EVERYWHERE,
        "[start, resubmission, decision]: another user's facts matched"
    );
    assert_eq!(
        route_through_every_caller(&app, &owner, &outsider, type_id).await,
        ["MATCHED", "MATCHED", "DONE_OTHER"],
        "[start, resubmission, decision]: the decision's actor is the decider, not the owner"
    );
}

/// **#755 (#706 AC11): a guard's `documentTypeKey` is the document type's
/// `type_code`**, the value the after-chain is given. Two guards on the edge
/// out of a `SERVICE_TASK`: the first refuses unless the key is the type's
/// code, the second refuses when it is `""`, which is what the guard was
/// given while it read a `typeCode` nothing set. Both pass, and the hook log
/// shows each `CONTINUE`.
#[tokio::test]
async fn a_guards_document_type_key_is_the_document_types_code() {
    let app = TestApp::spawn().await;
    let admin = app.administrator_token().await;
    let token = approver(&app, "systask.guard.key").await;

    let mut guards = guard_refusing_when(
        json!({ "!=": [{ "var": "documentTypeKey" }, "STAMPED_GUARD_KEY"] }),
        "TYPE_KEY_WRONG",
    );
    guards.as_array_mut().expect("guards").extend(
        guard_refusing_when(
            json!({ "==": [{ "var": "documentTypeKey" }, ""] }),
            "TYPE_KEY_EMPTY",
        )
        .as_array()
        .expect("guards")
        .iter()
        .cloned(),
    );

    let workflow = publish_workflow(
        &app,
        &admin,
        workflow_ending_in_done("stamped_guard_key", guards),
    )
    .await;
    let type_id = document_type(&app, &admin, "STAMPED_GUARD_KEY", workflow).await;
    let document = draft(&app, &token, type_id, 100).await;

    submit(&app, &token, document).await;

    let decided = approve(&app, &token, document).await;

    assert_eq!(decided.status, StatusCode::OK, "{}", decided.body);
    assert_eq!(state_of(&app, document).await, "DONE");
    assert_eq!(
        hook_runs(&app, document).await,
        vec![
            ("core:reject_when".to_owned(), "CONTINUE".to_owned()),
            ("core:reject_when".to_owned(), "CONTINUE".to_owned()),
        ],
        "both guards ran, and passed"
    );
}

/// **#706 AC1: `document.documentTypeKey` is the type's code, and renaming the
/// type does not move it.** A type is renamed after its workflow is bound; its
/// documents still route on its `type_code`, at the start, at a resubmission
/// and at a decision. The code is immutable (an update carries no
/// `typeCode`), and the name is not the key.
///
/// Found by the `test-engineer` campaign on row 14: every other fixture names
/// its type with its own code, so a key read from `name` passed every test.
#[tokio::test]
async fn a_renamed_types_documents_route_on_its_unchanged_code() {
    let app = TestApp::spawn().await;
    let admin = app.administrator_token().await;
    let token = approver(&app, "systask.key.renamed").await;

    let workflow = publish_workflow(
        &app,
        &admin,
        routed_everywhere(
            "routed_by_renamed_key",
            json!({ "==": [{ "var": "document.documentTypeKey" }, "RENAMED_KEY"] }),
        ),
    )
    .await;
    let type_id = document_type(&app, &admin, "RENAMED_KEY", workflow).await;

    let renamed = app
        .put(
            &format!("/api/v1/document-types/{type_id}"),
            Some(&admin),
            json!({ "name": "Requisition, renamed" }),
        )
        .await;
    assert_eq!(renamed.status, StatusCode::OK, "{}", renamed.body);
    assert_eq!(renamed.body["data"]["name"], "Requisition, renamed");
    assert_eq!(renamed.body["data"]["typeCode"], "RENAMED_KEY");

    assert_eq!(
        route_through_every_caller(&app, &token, &token, type_id).await,
        MATCHED_EVERYWHERE,
        "[start, resubmission, decision]: a renamed type's documents did not route on its code"
    );
}
