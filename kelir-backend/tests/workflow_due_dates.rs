//! Task due dates, and the indicator that makes them mean something
//! (FR-WF-011, FR-TASK-007; [#185]).
//!
//! **One item because a due date nobody is shown is a column.** So the
//! assertions below come in pairs: what the engine stamped on the row, and what
//! the inbox says about it — and where they could disagree, the test is written
//! so that they cannot.
//!
//! Every test that names a control has been seen to fail against a build with
//! that control removed (coding standard §2.9); each says what the mutation was
//! and what it produced.
//!
//! [#185]: https://github.com/sujanto-gaws/kelir/issues/185

mod common;

use axum::http::{Method, StatusCode};
use chrono::{DateTime, Utc};
use common::{fixtures, TestApp};
use serde_json::{json, Value};
use uuid::Uuid;

const APPROVER_ROLE: &str = "DUE-APPROVER";

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

/// A workflow whose one task is offered to a role, with an optional window.
///
/// `dueInHours` rather than a date, which is JWSS §3.1's own shape and AC1's:
/// a definition outlives every instance that runs it, so an absolute date in
/// one is wrong for every instance after the first.
fn workflow(key: &str, due_in_hours: Option<f64>) -> Value {
    let mut task = json!({
        "taskDefinitionKey": "manager_approval",
        "taskName": "Approve the request",
        "assignment": { "assigneeType": "ROLE", "roleCode": APPROVER_ROLE },
    });

    if let Some(hours) = due_in_hours {
        task["dueInHours"] = json!(hours);
    }

    json!({
        "workflowKey": key,
        "version": "1.0.0",
        "name": "Standard approval",
        "initialState": "MANAGER_APPROVAL",
        "states": [
            { "code": "MANAGER_APPROVAL", "name": "Manager approval",
              "mapsToDocumentStatus": "PENDING_APPROVAL", "task": task },
            { "code": "COMPLETED", "name": "Completed", "mapsToDocumentStatus": "COMPLETED",
              "isFinal": true }
        ],
        "transitions": [
            { "from": "MANAGER_APPROVAL", "to": "COMPLETED", "action": "APPROVE",
              "allowedBy": format!("ROLE:{APPROVER_ROLE}") }
        ]
    })
}

async fn publish_workflow(app: &TestApp, token: &str, definition: Value) -> Uuid {
    let key = definition["workflowKey"]
        .as_str()
        .expect("a key")
        .to_owned();

    let created = app
        .post(
            "/api/v1/workflow/definitions",
            Some(token),
            json!({ "workflowKey": key, "name": "Standard approval", "definition": definition }),
        )
        .await;
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
                "formData": { "amount": 45_000_000 },
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
        &format!("{username}@example.test"),
        common::ADMIN_PASSWORD,
        &[role],
    )
    .await;

    app.sign_in(username, common::ADMIN_PASSWORD).await
}

/// The open task of a document, with its stamped deadline.
async fn open_task_of(app: &TestApp, document_id: Uuid) -> (Uuid, Option<DateTime<Utc>>) {
    sqlx::query_as(
        "SELECT id, due_at FROM workflow_tasks \
         WHERE document_id = $1 AND status IN ('CREATED','ASSIGNED','IN_PROGRESS')",
    )
    .bind(document_id)
    .fetch_one(&app.pool)
    .await
    .expect("read the open task")
}

/// Moves a task's deadline directly, which is the only way to make one that has
/// already passed.
///
/// **The window is relative and the engine stamps it forward**, so a task that
/// is already late cannot be produced through the API — which is the design
/// rather than an obstacle. Ageing the row is what lets the indicator be tested
/// without a test that sleeps.
async fn set_due_at(app: &TestApp, task_id: Uuid, sql_interval: &str) {
    sqlx::query(&format!(
        "UPDATE workflow_tasks SET due_at = now() {sql_interval} WHERE id = $1"
    ))
    .bind(task_id)
    .execute(&app.pool)
    .await
    .expect("move the deadline");
}

fn row_for(body: &Value, document_id: Uuid) -> Option<&Value> {
    body["data"]
        .as_array()
        .expect("rows")
        .iter()
        .find(|task| task["documentId"] == document_id.to_string())
}

// ---------------------------------------------------------------------------
// AC1, AC2 — the definition declares a window and the engine stamps it
// ---------------------------------------------------------------------------

/// **Seen red** against `engine::enter` passing `due_in_seconds: None`: the task
/// is created with no deadline, and `dueInHours` goes back to being a field the
/// parser reads and nothing writes.
#[tokio::test]
async fn a_declared_window_is_stamped_on_the_task_when_it_is_generated() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    approver_role(&app).await;

    let workflow = publish_workflow(&app, &token, workflow("wf_due_stamp", Some(48.0))).await;
    let type_id = document_type(&app, &token, "PR_DUE_STAMP", workflow).await;
    let document = draft(&app, &token, type_id).await;

    let before = Utc::now();
    assert_eq!(submit(&app, &token, document).await.status, StatusCode::OK);
    let after = Utc::now();

    let (_, due_at) = open_task_of(&app, document).await;
    let due_at = due_at.expect("a deadline");

    // Forty-eight hours from when the task was generated, bracketed by the two
    // instants the submit happened between — which is as exact as a wall clock
    // gets and is what makes "relative to generation" observable.
    assert!(
        due_at >= before + chrono::TimeDelta::hours(48)
            && due_at <= after + chrono::TimeDelta::hours(48),
        "expected a deadline 48h after generation, got {due_at}"
    );
}

/// A definition that declares no window produces a task with no deadline —
/// which AC5 then requires not to read as overdue.
#[tokio::test]
async fn a_task_whose_definition_declares_no_window_has_no_deadline() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    approver_role(&app).await;

    let workflow = publish_workflow(&app, &token, workflow("wf_due_none", None)).await;
    let type_id = document_type(&app, &token, "PR_DUE_NONE", workflow).await;
    let document = draft(&app, &token, type_id).await;

    assert_eq!(submit(&app, &token, document).await.status, StatusCode::OK);

    assert_eq!(open_task_of(&app, document).await.1, None);
}

/// **The deadline does not move when the definition does** (AC2).
///
/// A task's deadline is a fact about the task, stamped once. Computing it on
/// read would let a republished revision shorten a deadline somebody is already
/// working to, which is a deadline nobody agreed to.
///
/// **This pins the absence of a derivation, so there is no control in it to
/// remove.** The mutation that reaches the same line is
/// `a_declared_window_is_stamped_on_the_task_when_it_is_generated`'s: with the
/// stamp gone both tests fail, which is what says the value read here is the one
/// written at generation rather than one computed on the way out.
#[tokio::test]
async fn a_deadline_does_not_move_when_the_definition_is_revised() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    approver_role(&app).await;

    let definition_id = publish_workflow(&app, &token, workflow("wf_due_pinned", Some(48.0))).await;
    let type_id = document_type(&app, &token, "PR_DUE_PINNED", definition_id).await;
    let document = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, document).await.status, StatusCode::OK);

    let (_, stamped) = open_task_of(&app, document).await;
    let stamped = stamped.expect("a deadline");

    // A second revision, published, saying something very different.
    let revision = app
        .post(
            &format!("/api/v1/workflow/definitions/{definition_id}/revisions"),
            Some(&token),
            json!({ "definition": workflow("wf_due_pinned", Some(1.0)) }),
        )
        .await;
    assert_eq!(revision.status, StatusCode::CREATED, "{}", revision.body);

    let next = id_of(&revision.body["data"]);
    let published = app
        .post(
            &format!("/api/v1/workflow/definitions/{next}/publication"),
            Some(&token),
            json!({}),
        )
        .await;
    assert_eq!(published.status, StatusCode::OK, "{}", published.body);

    assert_eq!(
        open_task_of(&app, document).await.1,
        Some(stamped),
        "the deadline was agreed when the task was generated; a later revision \
         does not get to shorten it"
    );
}

// ---------------------------------------------------------------------------
// AC3, AC4, AC5 — the inbox says which are late, and against which clock
// ---------------------------------------------------------------------------

/// **The indicator, and the filter that narrows to it** (AC3).
///
/// Three tasks: one late, one due later, one with no deadline at all. The
/// default inbox shows all three and marks one; `scope=overdue` shows only that
/// one.
///
/// **Seen red twice.** With `repository::inbox`'s `overdue_only` predicate
/// removed, the filtered list returns all three, so a person who asked what is
/// late is handed their whole queue. With `is_overdue` written as
/// `COALESCE(due_at, '-infinity') < now()` — AC5's trap, spelled the way it is
/// usually spelled — the undated task comes back marked, years late, having
/// never had a deadline at all.
#[tokio::test]
async fn the_inbox_marks_what_is_late_and_can_be_narrowed_to_it() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let holder = approver(&app, "due-inbox-holder").await;

    let dated_workflow = publish_workflow(&app, &token, workflow("wf_due_inbox", Some(48.0))).await;
    let dated = document_type(&app, &token, "PR_DUE_DATED", dated_workflow).await;

    let undated_workflow = publish_workflow(&app, &token, workflow("wf_due_undated", None)).await;
    let undated = document_type(&app, &token, "PR_DUE_UNDATED", undated_workflow).await;

    let late = draft(&app, &token, dated).await;
    assert_eq!(submit(&app, &token, late).await.status, StatusCode::OK);
    let (late_task, _) = open_task_of(&app, late).await;
    set_due_at(&app, late_task, "- interval '3 hours'").await;

    let soon = draft(&app, &token, dated).await;
    assert_eq!(submit(&app, &token, soon).await.status, StatusCode::OK);

    let never = draft(&app, &token, undated).await;
    assert_eq!(submit(&app, &token, never).await.status, StatusCode::OK);

    // The default inbox: everything waiting, with one of them marked.
    let inbox = app.get("/api/v1/tasks", Some(&holder)).await;
    assert_eq!(inbox.status, StatusCode::OK, "{}", inbox.body);

    assert_eq!(
        row_for(&inbox.body, late).expect("the late one")["isOverdue"],
        true
    );
    assert_eq!(
        row_for(&inbox.body, soon).expect("the coming one")["isOverdue"],
        false
    );
    assert_eq!(
        row_for(&inbox.body, never).expect("the undated one")["isOverdue"],
        false,
        "a task with no deadline is not overdue — AC5's trap is the other \
         spelling, where a null read as the epoch reports it years late"
    );

    // Narrowed: only the late one, and `meta.total` agrees with the page.
    let overdue = app.get("/api/v1/tasks?scope=overdue", Some(&holder)).await;
    assert_eq!(overdue.status, StatusCode::OK, "{}", overdue.body);

    assert!(row_for(&overdue.body, late).is_some(), "{}", overdue.body);
    assert!(row_for(&overdue.body, soon).is_none(), "{}", overdue.body);
    assert!(row_for(&overdue.body, never).is_none(), "{}", overdue.body);
    assert_eq!(
        overdue.body["meta"]["total"], 1,
        "the count and the page are one predicate written twice, and a person \
         paging a list that says 3 and ends at 1 cannot tell a bug from a race"
    );
}

/// **A task finished after its date passed is done, not late** (AC4's reader
/// half).
///
/// The indicator exists to say what needs doing now. Colouring finished rows
/// red would bury the ones that do, and `scope=all` is where somebody looks at
/// what has been through their hands.
///
/// **Seen red** against the `is_overdue` expression with its status clause
/// removed: the decided task comes back marked, in a list whose whole purpose
/// is that it is finished.
#[tokio::test]
async fn a_task_decided_after_its_date_is_not_reported_as_late() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let holder = approver(&app, "due-done-holder").await;

    let workflow = publish_workflow(&app, &token, workflow("wf_due_done", Some(48.0))).await;
    let type_id = document_type(&app, &token, "PR_DUE_DONE", workflow).await;
    let document = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, document).await.status, StatusCode::OK);

    let (task_id, _) = open_task_of(&app, document).await;
    set_due_at(&app, task_id, "- interval '3 hours'").await;

    let claimed = app
        .post(
            &format!("/api/v1/workflow/tasks/{task_id}/claim"),
            Some(&holder),
            json!({}),
        )
        .await;
    assert_eq!(claimed.status, StatusCode::OK, "{}", claimed.body);

    let decided = app
        .post(
            &format!("/api/v1/workflow/tasks/{task_id}/decision"),
            Some(&holder),
            json!({ "action": "APPROVE" }),
        )
        .await;
    assert_eq!(decided.status, StatusCode::OK, "{}", decided.body);

    let all = app.get("/api/v1/tasks?scope=all", Some(&holder)).await;
    assert_eq!(all.status, StatusCode::OK, "{}", all.body);

    let row = row_for(&all.body, document).expect("the finished task");

    assert_eq!(row["isOverdue"], false);
    assert!(
        row["dueAt"].is_string(),
        "the date is still on the row — what changed is that it is no longer \
         something to act on, {}",
        all.body
    );

    // And it is not in the overdue list either.
    let overdue = app.get("/api/v1/tasks?scope=overdue", Some(&holder)).await;
    assert!(
        row_for(&overdue.body, document).is_none(),
        "{}",
        overdue.body
    );
}

/// **`isOverdue` is the server's answer, beside the date rather than instead of
/// it** (AC4).
///
/// The detail view carries both, so a screen can say *when* without ever
/// comparing a date to the browser's clock — which is the second opinion AC4
/// exists to prevent.
#[tokio::test]
async fn the_task_detail_answers_whether_as_well_as_when() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let holder = approver(&app, "due-detail-holder").await;

    let workflow = publish_workflow(&app, &token, workflow("wf_due_detail", Some(48.0))).await;
    let type_id = document_type(&app, &token, "PR_DUE_DETAIL", workflow).await;
    let document = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, document).await.status, StatusCode::OK);

    let (task_id, _) = open_task_of(&app, document).await;
    set_due_at(&app, task_id, "- interval '90 minutes'").await;

    let detail = app
        .get(&format!("/api/v1/tasks/{task_id}"), Some(&holder))
        .await;
    assert_eq!(detail.status, StatusCode::OK, "{}", detail.body);

    assert_eq!(detail.body["data"]["isOverdue"], true);
    assert!(detail.body["data"]["dueAt"].is_string(), "{}", detail.body);
}

/// A scope the inbox does not serve is refused by name, and the message lists
/// the ones that exist.
#[tokio::test]
async fn an_unknown_scope_names_the_three_that_exist() {
    let app = TestApp::spawn().await;
    let holder = approver(&app, "due-scope-holder").await;

    let refused = app.get("/api/v1/tasks?scope=late", Some(&holder)).await;

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
            .contains("open, overdue, all"),
        "{}",
        refused.body
    );
}
