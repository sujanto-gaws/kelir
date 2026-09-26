//! The task inbox: what is waiting for the person looking at it (#179).
//!
//! **The visibility rule is the item.** A caller sees a task when it is theirs
//! or when it is offered to a role they hold, and #179 AC3 puts that in the
//! query rather than the handler — so every test here reaches the *rows*, which
//! is the [#106]/[#121] lesson that cost three sprints of coverage findings.
//!
//! The fixture holds **a second user, a second role and a second document
//! type** wherever it asserts a scope: one subject cannot tell *scoped* from
//! *unscoped*, and the assertion is identical either way (coding standard §2.9,
//! and [#218]'s single root cause).
//!
//! [#106]: https://github.com/sujanto-gaws/kelir/issues/106
//! [#121]: https://github.com/sujanto-gaws/kelir/issues/121
//! [#218]: https://github.com/sujanto-gaws/kelir/issues/218

mod common;

use axum::http::{Method, StatusCode};
use common::{fixtures, TestApp};
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

const TASKS: &str = "/api/v1/tasks";

fn id_of(value: &Value) -> Uuid {
    value["id"]
        .as_str()
        .expect("an id")
        .parse()
        .expect("a uuid")
}

/// A workflow whose task is offered to the role named.
fn workflow_for(key: &str, role_code: &str) -> Value {
    json!({
        "workflowKey": key,
        "version": "1.0.0",
        "name": "Standard approval",
        "initialState": "MANAGER_APPROVAL",
        "states": [
            { "code": "MANAGER_APPROVAL", "name": "Manager approval",
              "mapsToDocumentStatus": "PENDING_APPROVAL",
              "task": { "taskDefinitionKey": "manager_approval",
                        "taskName": "Approve the request",
                        "assignment": { "assigneeType": "ROLE", "roleCode": role_code } } },
            { "code": "COMPLETED", "name": "Completed", "mapsToDocumentStatus": "COMPLETED",
              "isFinal": true },
            { "code": "REJECTED", "name": "Rejected", "mapsToDocumentStatus": "REJECTED",
              "isFinal": true }
        ],
        "transitions": [
            { "from": "MANAGER_APPROVAL", "to": "COMPLETED", "action": "APPROVE",
              "allowedBy": format!("ROLE:{role_code}") },
            { "from": "MANAGER_APPROVAL", "to": "REJECTED", "action": "REJECT",
              "allowedBy": format!("ROLE:{role_code}") }
        ]
    })
}

async fn publish_workflow(app: &TestApp, token: &str, key: &str, role_code: &str) -> Uuid {
    publish_workflow_definition(app, token, key, workflow_for(key, role_code)).await
}

/// The same, for a definition `workflow_for` does not write.
async fn publish_workflow_definition(
    app: &TestApp,
    token: &str,
    key: &str,
    definition: Value,
) -> Uuid {
    let created = app
        .post(
            "/api/v1/workflow/definitions",
            Some(token),
            json!({
                "workflowKey": key,
                "name": "Standard approval",
                "definition": definition,
            }),
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

/// A document type bound to `workflow`, with a numbering template of its own.
///
/// The template carries the type code because
/// `uq_documents_tenant_id_document_number` is tenant-wide while a numbering
/// bucket is per type — see `workflow_engine.rs`, which records the same
/// finding.
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

/// Creates a document of `type_id` and submits it, returning its id.
async fn submitted_document(app: &TestApp, token: &str, type_id: Uuid, title: &str) -> Uuid {
    let created = app
        .post(
            "/api/v1/documents",
            Some(token),
            json!({
                "documentTypeId": type_id,
                "title": title,
                "formData": { "amount": 1_000 },
            }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    let id = id_of(&created.body["data"]);

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

/// A role holding what the inbox needs, and a user holding that role.
async fn holder(app: &TestApp, role_code: &str, username: &str) -> (Uuid, String) {
    let role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        role_code,
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

    let token = app.sign_in(username, common::ADMIN_PASSWORD).await;

    (role, token)
}

async fn open_task_of(app: &TestApp, document_id: Uuid) -> Uuid {
    sqlx::query_scalar(
        "SELECT id FROM workflow_tasks WHERE document_id = $1 AND status IN ('CREATED','ASSIGNED','IN_PROGRESS')",
    )
    .bind(document_id)
    .fetch_one(&app.pool)
    .await
    .expect("read the open task")
}

// ---------------------------------------------------------------------------
// AC1, AC3 — the visibility rule, in the query
// ---------------------------------------------------------------------------

/// **A caller sees their own tasks and their roles' tasks, and nobody else's.**
///
/// Two roles, two holders, two document types, two documents. Each holder sees
/// exactly one task, and the `total` agrees with the page — a count over a wider
/// rule than the page's would report rows nobody can open.
///
/// **Seen red** against `repository::inbox::list_for_caller` with the
/// `t.assignee_user_id = $2 OR (...)` clause replaced by `TRUE`: each holder
/// sees both tasks, and `meta.total` says 2.
#[tokio::test]
async fn a_caller_sees_their_own_roles_tasks_and_no_others() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let (_, finance) = holder(&app, "TI-FINANCE", "ti.finance").await;
    let (_, legal) = holder(&app, "TI-LEGAL", "ti.legal").await;

    let finance_workflow = publish_workflow(&app, &token, "ti_finance", "TI-FINANCE").await;
    let legal_workflow = publish_workflow(&app, &token, "ti_legal", "TI-LEGAL").await;

    let finance_type = document_type(&app, &token, "TI_FIN", finance_workflow).await;
    let legal_type = document_type(&app, &token, "TI_LEG", legal_workflow).await;

    submitted_document(&app, &token, finance_type, "A finance request").await;
    submitted_document(&app, &token, legal_type, "A legal request").await;

    let inbox = app.get(TASKS, Some(&finance)).await;
    assert_eq!(inbox.status, StatusCode::OK, "{}", inbox.body);

    let rows = inbox.body["data"].as_array().expect("a page");
    assert_eq!(
        rows.len(),
        1,
        "the finance approver saw {} tasks",
        rows.len()
    );
    assert_eq!(rows[0]["documentTitle"], "A finance request");
    assert_eq!(
        inbox.body["meta"]["total"], 1,
        "the count must agree with the page: {}",
        inbox.body
    );

    let inbox = app.get(TASKS, Some(&legal)).await;
    let rows = inbox.body["data"].as_array().expect("a page");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["documentTitle"], "A legal request");
    assert_eq!(inbox.body["meta"]["total"], 1);
}

/// **Mine and unclaimed are distinguishable in the payload** (AC1).
///
/// An unclaimed role task and work that is already mine are different situations
/// for the person looking at them, and a client that had to derive it from a
/// null assignee would derive it differently in two places.
#[tokio::test]
async fn a_claimed_task_and_an_unclaimed_one_read_differently() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let (_, approver) = holder(&app, "TI-APPROVER", "ti.approver").await;
    // A second holder of the same role, so the queue is a queue.
    let (_, colleague) = holder(&app, "TI-APPROVER-2", "ti.colleague").await;
    let _ = colleague;

    let workflow = publish_workflow(&app, &token, "ti_claim", "TI-APPROVER").await;
    let type_id = document_type(&app, &token, "TI_CLAIM", workflow).await;

    let first = submitted_document(&app, &token, type_id, "Unclaimed").await;
    let second = submitted_document(&app, &token, type_id, "Claimed").await;

    let claimed_task = open_task_of(&app, second).await;
    let claim = app
        .post(
            &format!("/api/v1/workflow/tasks/{claimed_task}/claim"),
            Some(&approver),
            json!({}),
        )
        .await;
    assert_eq!(claim.status, StatusCode::OK, "{}", claim.body);

    let inbox = app.get(TASKS, Some(&approver)).await;
    assert_eq!(inbox.status, StatusCode::OK, "{}", inbox.body);

    let rows = inbox.body["data"].as_array().expect("a page");
    assert_eq!(rows.len(), 2);

    let unclaimed = rows
        .iter()
        .find(|row| row["documentId"] == json!(first))
        .expect("the unclaimed task");
    let mine = rows
        .iter()
        .find(|row| row["documentId"] == json!(second))
        .expect("the claimed task");

    assert_eq!(unclaimed["assignment"], "ROLE");
    assert_eq!(unclaimed["candidateRoleCode"], "TI-APPROVER");
    assert_eq!(mine["assignment"], "MINE");
}

/// **A task another user holds is not readable, and the detail says 404 rather
/// than 403.**
///
/// 404 because the visibility rule is what the read is filtered by: a 403 would
/// confirm the task exists, which is a fact the caller has no business
/// establishing.
///
/// **Seen red** against `service::inbox::get_task` with its `is_visible_to`
/// guard removed: the colleague reads a task assigned to somebody else,
/// including the document it is about.
#[tokio::test]
async fn one_users_task_is_not_readable_by_another() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let (_, approver) = holder(&app, "TI-OWNER", "ti.owner").await;
    let (_, outsider) = holder(&app, "TI-OUTSIDER", "ti.outsider").await;

    let workflow = publish_workflow(&app, &token, "ti_private", "TI-OWNER").await;
    let type_id = document_type(&app, &token, "TI_PRIVATE", workflow).await;
    let document = submitted_document(&app, &token, type_id, "Not yours").await;

    let task = open_task_of(&app, document).await;

    let refused = app.get(&format!("{TASKS}/{task}"), Some(&outsider)).await;

    assert_eq!(
        refused.status,
        StatusCode::NOT_FOUND,
        "another user's task was readable: {}",
        refused.body
    );

    // And the rightful holder reads it, so the refusal above is about the row
    // rather than about the endpoint refusing everybody — the gate §2.9 warns
    // about.
    let read = app.get(&format!("{TASKS}/{task}"), Some(&approver)).await;
    assert_eq!(read.status, StatusCode::OK, "{}", read.body);
}

// ---------------------------------------------------------------------------
// AC4 — what a task says for itself
// ---------------------------------------------------------------------------

/// **A task detail names the document, the process, and the decision being
/// asked** (AC4).
///
/// *A task that says only "approve?" is a task its holder cannot responsibly
/// action.* The decision list carries the definition's own name for each target
/// state, and marks which of them this release can perform — a screen that drew
/// a `RETURN` button would produce a 422 from a control the product offered.
#[tokio::test]
async fn a_task_detail_says_what_is_being_decided_and_about_what() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let (_, approver) = holder(&app, "TI-DETAIL", "ti.detail").await;

    let workflow = publish_workflow(&app, &token, "ti_detail", "TI-DETAIL").await;
    let type_id = document_type(&app, &token, "TI_DETAIL", workflow).await;
    let document = submitted_document(&app, &token, type_id, "Two standing desks").await;

    let task = open_task_of(&app, document).await;

    let read = app.get(&format!("{TASKS}/{task}"), Some(&approver)).await;
    assert_eq!(read.status, StatusCode::OK, "{}", read.body);

    let data = &read.body["data"];

    assert_eq!(data["documentTitle"], "Two standing desks");
    assert!(
        data["documentNumber"].is_string(),
        "the task must name the numbered document it is about: {}",
        read.body
    );
    assert_eq!(data["workflowKey"], "ti_detail");
    assert_eq!(data["currentState"], "MANAGER_APPROVAL");
    assert_eq!(
        data["currentStateName"], "Manager approval",
        "the definition's own name for the state, not its code"
    );

    let decisions = data["decisions"].as_array().expect("the decisions");
    assert_eq!(decisions.len(), 2);

    let approve = decisions
        .iter()
        .find(|decision| decision["action"] == "APPROVE")
        .expect("the approve edge");
    assert_eq!(approve["toState"], "COMPLETED");
    assert_eq!(approve["toStateName"], "Completed");
    assert_eq!(approve["supported"], true);
}

/// **A transition this release cannot perform is shown and not offered.**
///
/// `ESCALATE` is FR-WF-010, `Could` and unscheduled. A definition may declare it
/// now, and a screen that drew a button for it would produce a 422 from a
/// control the product itself put there — so the payload says
/// `supported: false` and the screen can render the edge without offering it.
///
/// **`RETURN` was this test's subject until [#183] built it**, and the
/// assertion below now pins the other side: the flag moved because the
/// capability did, which is the flag working rather than a reason to delete it.
///
/// [#183]: https://github.com/sujanto-gaws/kelir/issues/183
#[tokio::test]
async fn a_transition_this_release_cannot_perform_is_reported_as_unsupported() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let (_, approver) = holder(&app, "TI-RETURN", "ti.return").await;

    let definition = json!({
        "workflowKey": "ti_return",
        "version": "1.0.0",
        "name": "With a return",
        "initialState": "MANAGER_APPROVAL",
        "states": [
            { "code": "MANAGER_APPROVAL", "name": "Manager approval",
              "mapsToDocumentStatus": "PENDING_APPROVAL",
              "task": { "taskDefinitionKey": "manager_approval", "taskName": "Decide",
                        "assignment": { "assigneeType": "ROLE", "roleCode": "TI-RETURN" } } },
            { "code": "RETURNED", "name": "Returned to the author",
              "mapsToDocumentStatus": "RETURNED",
              "task": { "taskDefinitionKey": "correct_it", "taskName": "Correct the request",
                        "assignment": { "assigneeType": "OWNER" } } },
            { "code": "ESCALATED", "name": "Escalated", "mapsToDocumentStatus": "PENDING_APPROVAL",
              "task": { "taskDefinitionKey": "escalated_approval", "taskName": "Decide",
                        "assignment": { "assigneeType": "ROLE", "roleCode": "TI-RETURN" } } },
            { "code": "COMPLETED", "name": "Completed", "mapsToDocumentStatus": "COMPLETED",
              "isFinal": true }
        ],
        "transitions": [
            { "from": "MANAGER_APPROVAL", "to": "COMPLETED", "action": "APPROVE",
              "allowedBy": "ROLE:TI-RETURN" },
            { "from": "MANAGER_APPROVAL", "to": "RETURNED", "action": "RETURN",
              "allowedBy": "ROLE:TI-RETURN" },
            { "from": "MANAGER_APPROVAL", "to": "ESCALATED", "action": "ESCALATE",
              "allowedBy": "ROLE:TI-RETURN" },
            { "from": "ESCALATED", "to": "COMPLETED", "action": "APPROVE",
              "allowedBy": "ROLE:TI-RETURN" },
            { "from": "RETURNED", "to": "MANAGER_APPROVAL", "action": "RESUBMIT",
              "allowedBy": "OWNER" }
        ]
    });

    let created = app
        .post(
            "/api/v1/workflow/definitions",
            Some(&token),
            json!({ "workflowKey": "ti_return", "name": "With a return", "definition": definition }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let workflow = id_of(&created.body["data"]);

    let publication = app
        .post(
            &format!("/api/v1/workflow/definitions/{workflow}/publication"),
            Some(&token),
            json!({}),
        )
        .await;
    assert_eq!(publication.status, StatusCode::OK, "{}", publication.body);

    let type_id = document_type(&app, &token, "TI_RETURN", workflow).await;
    let document = submitted_document(&app, &token, type_id, "Returnable").await;
    let task = open_task_of(&app, document).await;

    let read = app.get(&format!("{TASKS}/{task}"), Some(&approver)).await;
    assert_eq!(read.status, StatusCode::OK, "{}", read.body);

    let decisions = read.body["data"]["decisions"]
        .as_array()
        .expect("the decisions");

    let escalate = decisions
        .iter()
        .find(|decision| decision["action"] == "ESCALATE")
        .expect("the escalate edge is visible");

    assert_eq!(
        escalate["supported"], false,
        "a transition this release cannot perform must not be offered"
    );

    // And `RETURN` is offered, because #183 built it. Both halves in one test:
    // an implementation that reported everything as supported would pass the
    // second assertion and fail the first, and one that reported everything as
    // unsupported would do the opposite.
    let returned = decisions
        .iter()
        .find(|decision| decision["action"] == "RETURN")
        .expect("the return edge is visible");

    assert_eq!(
        returned["supported"], true,
        "return has been performable since #183, and the screen is told so here"
    );
    assert_eq!(returned["toState"], "RETURNED");
    assert_eq!(returned["toStateName"], "Returned to the author");

    // The unsupported one really cannot be performed, which is what makes the
    // flag true rather than a claim: the request type has no such variant.
    let refused = app
        .post(
            &format!("/api/v1/workflow/tasks/{task}/decision"),
            Some(&approver),
            json!({ "action": "ESCALATE" }),
        )
        .await;
    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
}

/// An `AUTO` edge is not a decision, so it is not in the list of them ([#264]).
///
/// **The list answers *what may the person holding this task do*.**
/// `Graph::actions_from` answers a different question — *every transition out of
/// this state* — and for `AUTO` the two provably differ: JWSS §4 forbids
/// `allowedBy` on an `AUTO` transition because there is no caller. Nothing in
/// the engine fires one either, so a state with an `AUTO` out-edge parks the
/// process; the screen used to tell whoever was holding the task that it would
/// arrive in a later release.
///
/// **Both halves, in one test.** A filter written as *drop everything
/// unsupported* would pass the first assertion and fail the second — and
/// `ESCALATE` is exactly the case that must survive, because it is a real
/// transition that a person may one day be shown even though this release
/// cannot fire it.
///
/// **Seen red** against the filter removed: `AUTO` reappears with
/// `supported: false`.
///
/// [#264]: https://github.com/sujanto-gaws/kelir/issues/264
#[tokio::test]
async fn an_auto_transition_is_not_offered_as_a_decision_at_all() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let (_, approver) = holder(&app, "TI-AUTO", "ti.auto").await;

    let definition = json!({
        "workflowKey": "ti_auto",
        "version": "1.0.0",
        "name": "With an automatic edge",
        "initialState": "MANAGER_APPROVAL",
        "states": [
            { "code": "MANAGER_APPROVAL", "name": "Manager approval",
              "mapsToDocumentStatus": "PENDING_APPROVAL",
              "task": { "taskDefinitionKey": "manager_approval", "taskName": "Decide",
                        "assignment": { "assigneeType": "ROLE", "roleCode": "TI-AUTO" } } },
            { "code": "ESCALATED", "name": "Escalated", "mapsToDocumentStatus": "PENDING_APPROVAL",
              "task": { "taskDefinitionKey": "escalated_approval", "taskName": "Decide",
                        "assignment": { "assigneeType": "ROLE", "roleCode": "TI-AUTO" } } },
            { "code": "ARCHIVED", "name": "Archived", "mapsToDocumentStatus": "ARCHIVED",
              "isFinal": true },
            { "code": "COMPLETED", "name": "Completed", "mapsToDocumentStatus": "COMPLETED",
              "isFinal": true }
        ],
        "transitions": [
            { "from": "MANAGER_APPROVAL", "to": "COMPLETED", "action": "APPROVE",
              "allowedBy": "ROLE:TI-AUTO" },
            // No `allowedBy`, which S5 requires of an `AUTO` edge and which is
            // the specification saying in its own grammar that nobody fires it.
            { "from": "MANAGER_APPROVAL", "to": "ARCHIVED", "action": "AUTO" },
            { "from": "MANAGER_APPROVAL", "to": "ESCALATED", "action": "ESCALATE",
              "allowedBy": "ROLE:TI-AUTO" },
            { "from": "ESCALATED", "to": "COMPLETED", "action": "APPROVE",
              "allowedBy": "ROLE:TI-AUTO" }
        ]
    });

    let created = app
        .post(
            "/api/v1/workflow/definitions",
            Some(&token),
            json!({
                "workflowKey": "ti_auto",
                "name": "With an automatic edge",
                "definition": definition,
            }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let workflow = id_of(&created.body["data"]);

    let publication = app
        .post(
            &format!("/api/v1/workflow/definitions/{workflow}/publication"),
            Some(&token),
            json!({}),
        )
        .await;
    assert_eq!(publication.status, StatusCode::OK, "{}", publication.body);

    let type_id = document_type(&app, &token, "TI_AUTO", workflow).await;
    let document = submitted_document(&app, &token, type_id, "Automatic").await;
    let task = open_task_of(&app, document).await;

    let read = app.get(&format!("{TASKS}/{task}"), Some(&approver)).await;
    assert_eq!(read.status, StatusCode::OK, "{}", read.body);

    let decisions = read.body["data"]["decisions"]
        .as_array()
        .expect("the decisions");

    assert!(
        !decisions
            .iter()
            .any(|decision| decision["action"] == "AUTO"),
        "an AUTO edge is nobody's decision and must not be listed as one: {}",
        read.body
    );

    // The control. `ESCALATE` is unsupported and still listed, so the filter
    // narrows on *who fires it* rather than on *whether this release can*.
    let escalate = decisions
        .iter()
        .find(|decision| decision["action"] == "ESCALATE")
        .expect("the escalate edge is still visible");
    assert_eq!(escalate["supported"], false);
}

// ---------------------------------------------------------------------------
// AC2, AC5 — paging, and a bad page inside the envelope
// ---------------------------------------------------------------------------

/// **Paged, with `meta` reporting page, pageSize and total** (AC2).
#[tokio::test]
async fn the_inbox_pages_and_reports_its_total() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let (_, approver) = holder(&app, "TI-PAGED", "ti.paged").await;

    let workflow = publish_workflow(&app, &token, "ti_paged", "TI-PAGED").await;
    let type_id = document_type(&app, &token, "TI_PAGED", workflow).await;

    for index in 0..3 {
        submitted_document(&app, &token, type_id, &format!("Request {index}")).await;
    }

    let page = app
        .get(&format!("{TASKS}?page=1&pageSize=2"), Some(&approver))
        .await;

    assert_eq!(page.status, StatusCode::OK, "{}", page.body);
    assert_eq!(page.body["data"].as_array().expect("a page").len(), 2);
    assert_eq!(page.body["meta"]["page"], 1);
    assert_eq!(page.body["meta"]["pageSize"], 2);
    assert_eq!(page.body["meta"]["total"], 3);

    let second = app
        .get(&format!("{TASKS}?page=2&pageSize=2"), Some(&approver))
        .await;
    assert_eq!(second.body["data"].as_array().expect("a page").len(), 1);
}

/// **A bad `page` is refused inside the error envelope** (AC5).
///
/// This does not close [#122](https://github.com/sujanto-gaws/kelir/issues/122)
/// — the API-wide instances are unchanged — and the status report says so rather
/// than implying otherwise. What it says is that the inbox did not become
/// another one.
#[tokio::test]
async fn a_bad_page_is_refused_inside_the_envelope() {
    let app = TestApp::spawn().await;
    let (_, approver) = holder(&app, "TI-BADPAGE", "ti.badpage").await;

    let refused = app
        .get(&format!("{TASKS}?page=first"), Some(&approver))
        .await;

    assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(refused.body["success"], false, "{}", refused.body);
    assert!(
        refused.body["error"]["code"].is_string(),
        "a bare 400 with no envelope: {}",
        refused.body
    );
    assert!(
        refused.body.to_string().contains("page"),
        "the refusal must name the parameter as the caller spelled it: {}",
        refused.body
    );
}

/// **An unknown `scope` names the ones that exist.**
#[tokio::test]
async fn an_unknown_scope_is_refused_with_the_values_that_work() {
    let app = TestApp::spawn().await;
    let (_, approver) = holder(&app, "TI-SCOPE", "ti.scope").await;

    let refused = app
        .get(&format!("{TASKS}?scope=archived"), Some(&approver))
        .await;

    assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(refused.body["error"]["details"][0]["path"], "scope");
    // The list grew when #185 added `overdue`, and again when #256 added
    // `completed`. This assertion is what made both visible rather than leaving
    // a refusal message describing some of the values it accepts — which is the
    // failure it was written for, twice now.
    assert!(
        refused
            .body
            .to_string()
            .contains("open, overdue, completed, all"),
        "{}",
        refused.body
    );
}

/// **The default inbox is what is waiting, and `scope=all` widens it.**
///
/// An inbox that opened on every task anybody had ever held would be a log
/// rather than a queue.
#[tokio::test]
async fn a_decided_task_leaves_the_default_inbox_and_stays_findable() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let (_, approver) = holder(&app, "TI-DONE", "ti.done").await;

    let workflow = publish_workflow(&app, &token, "ti_done", "TI-DONE").await;
    let type_id = document_type(&app, &token, "TI_DONE", workflow).await;
    let document = submitted_document(&app, &token, type_id, "Decided").await;

    let task = open_task_of(&app, document).await;

    let decided = app
        .post(
            &format!("/api/v1/workflow/tasks/{task}/decision"),
            Some(&approver),
            json!({ "action": "APPROVE" }),
        )
        .await;
    assert_eq!(decided.status, StatusCode::OK, "{}", decided.body);

    let waiting = app.get(TASKS, Some(&approver)).await;
    assert_eq!(waiting.status, StatusCode::OK, "{}", waiting.body);
    assert!(
        waiting.body["data"].as_array().expect("a page").is_empty(),
        "a decided task was still waiting: {}",
        waiting.body
    );
    assert_eq!(waiting.body["meta"]["total"], 0);

    let everything = app
        .get(&format!("{TASKS}?scope=all"), Some(&approver))
        .await;
    assert_eq!(
        everything.body["data"].as_array().expect("a page").len(),
        1,
        "a decided task must stay findable: {}",
        everything.body
    );
    assert_eq!(everything.body["data"][0]["status"], "COMPLETED");
    assert_eq!(everything.body["data"][0]["assignment"], "MINE");
}

// ---------------------------------------------------------------------------
// The permission, which the inbox borrows rather than inventing
// ---------------------------------------------------------------------------

/// **The inbox requires `workflow:task:read` and no permission of its own.**
///
/// A `task:read` beside it would let a deployment grant the inbox without
/// granting the task — the gap Database Schema §5.13 refused to create for
/// `rad:lookup:read`.
///
/// **Seen red** against `service::inbox::list_inbox` with its
/// `caller.require(TASK_READ)` removed: a caller holding nothing but
/// `document:read` reads a page of somebody's tasks.
#[tokio::test]
async fn the_inbox_requires_the_workflow_task_read_permission() {
    let app = TestApp::spawn().await;

    let role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "TI-NO-TASKS",
        &["document:read"],
    )
    .await;
    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "ti.notasks",
        "ti.notasks@example.test",
        common::ADMIN_PASSWORD,
        &[role],
    )
    .await;
    let caller = app.sign_in("ti.notasks", common::ADMIN_PASSWORD).await;

    let refused = app.get(TASKS, Some(&caller)).await;

    assert_eq!(refused.status, StatusCode::FORBIDDEN, "{}", refused.body);

    // And a caller who holds it gets a page, so the refusal is about the
    // permission rather than about the route being broken.
    let (_, approver) = holder(&app, "TI-YES-TASKS", "ti.yestasks").await;
    let allowed = app.get(TASKS, Some(&approver)).await;
    assert_eq!(allowed.status, StatusCode::OK, "{}", allowed.body);
}

/// **Every route this sprint added reaches the OpenAPI document**
/// ([#187](https://github.com/sujanto-gaws/kelir/issues/187) AC6).
///
/// The both-directions test [#142](https://github.com/sujanto-gaws/kelir/issues/142)
/// introduced lives in `router.rs` and reads the source; this asserts the other
/// half from outside — that the paths are actually served in the published
/// contract, which is what a generated client reads.
#[tokio::test]
async fn the_new_routes_are_in_the_published_contract() {
    let app = TestApp::spawn().await;

    let document = app.get("/api/docs/openapi.json", None).await;
    assert_eq!(document.status, StatusCode::OK);

    for path in [
        "/api/v1/workflow/definitions",
        "/api/v1/workflow/definitions/{id}",
        "/api/v1/workflow/definitions/{id}/publication",
        "/api/v1/workflow/definitions/{id}/revisions",
        "/api/v1/workflow/instances/{id}",
        "/api/v1/workflow/tasks/{id}/claim",
        "/api/v1/workflow/tasks/{id}/decision",
        "/api/v1/documents/{id}/workflow",
        "/api/v1/tasks",
        "/api/v1/tasks/{id}",
    ] {
        assert!(
            !document.body["paths"][path].is_null(),
            "{path} is served and is not in the OpenAPI document"
        );
    }
}

// ---------------------------------------------------------------------------
// #256 — the completed view, the search, and #279's count that has to agree
// ---------------------------------------------------------------------------

// # Seen to fail (coding standard §2.9)
//
// Four mutations, run 2026-09-02:
//
// | Mutation | Reddened |
// |---|---|
// | `count_for_caller`'s join widened to `ON d.id = t.document_id`, dropping `deleted_at IS NULL` — #279's shape | *the count, the page and the gate agree…* — total 1 against an empty page |
// | `completed` parsed as `InboxScope::All` | *a completed task leaves the queue…* |
// | The search predicate neutered with `OR true` | *the search narrows…*, *a percent in a search…* |
// | The `%` escaping dropped from `normalize_search` | *a percent in a search is a percent…* |
//
// **And one thing a mutation could not do.** Removing the `documents` join from
// the count outright no longer compiles: the search reads `d.title` and
// `d.document_number` in that statement as well as in the page, so #279's exact
// defect — the join present in one and absent in the other — is now a build
// failure rather than a wrong number. The test guards the semantics; the
// compiler guards the join.

/// One page of the inbox, asked for with a query string.
async fn inbox_of(app: &TestApp, token: &str, query: &str) -> common::TestResponse {
    app.get(&format!("{TASKS}?{query}"), Some(token)).await
}

/// **What has been through my hands, distinct from what is waiting** ([#256]
/// AC1, AC5).
///
/// The completed row carries what was decided **and the reason given with it** —
/// FR-TASK-006's record, written in Sprint 11 and readable until now only on the
/// document's own history.
///
/// **Seen red** with `completed` mapped to `InboxScope::All`: the finished task
/// appears under both scopes, and *waiting for me* stops meaning anything.
///
/// [#256]: https://github.com/sujanto-gaws/kelir/issues/256
#[tokio::test]
async fn a_completed_task_leaves_the_queue_and_says_what_was_decided() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let (_, approver) = holder(&app, "TI-DONE", "ti.done").await;

    let workflow = publish_workflow(&app, &token, "ti_done", "TI-DONE").await;
    let type_id = document_type(&app, &token, "TI_DONE", workflow).await;
    let document = submitted_document(&app, &token, type_id, "Two standing desks").await;
    let task = open_task_of(&app, document).await;

    let waiting = inbox_of(&app, &approver, "scope=open").await;
    assert_eq!(waiting.body["data"].as_array().expect("a page").len(), 1);
    assert_eq!(waiting.body["meta"]["total"], 1);

    let before = inbox_of(&app, &approver, "scope=completed").await;
    assert_eq!(before.body["data"].as_array().expect("a page").len(), 0);
    assert_eq!(before.body["meta"]["total"], 0);

    let claimed = app
        .post(
            &format!("/api/v1/workflow/tasks/{task}/claim"),
            Some(&approver),
            json!({}),
        )
        .await;
    assert_eq!(claimed.status, StatusCode::OK, "{}", claimed.body);

    let decided = app
        .post(
            &format!("/api/v1/workflow/tasks/{task}/decision"),
            Some(&approver),
            json!({ "action": "APPROVE", "comment": "budget is available" }),
        )
        .await;
    assert_eq!(decided.status, StatusCode::OK, "{}", decided.body);

    let waiting_after = inbox_of(&app, &approver, "scope=open").await;
    assert_eq!(
        waiting_after.body["data"].as_array().expect("a page").len(),
        0,
        "a decided task is not waiting for anybody"
    );
    assert_eq!(waiting_after.body["meta"]["total"], 0);

    let done = inbox_of(&app, &approver, "scope=completed").await;
    let row = &done.body["data"][0];

    assert_eq!(done.body["data"].as_array().expect("a page").len(), 1);
    assert_eq!(done.body["meta"]["total"], 1);
    assert_eq!(row["id"], task.to_string());
    assert_eq!(row["action"], "APPROVE");
    assert_eq!(row["decisionComment"], "budget is available");
    assert!(!row["completedAt"].is_null());
    assert_eq!(
        row["isOverdue"], false,
        "a finished task is not late, it is done"
    );
}

/// **The search narrows the same list** ([#256] AC3) — through the statement the
/// visibility rule lives in, rather than by filtering a page afterwards.
#[tokio::test]
async fn the_search_narrows_the_inbox_by_document_and_by_task_name() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let (_, approver) = holder(&app, "TI-SEARCH", "ti.search").await;

    let workflow = publish_workflow(&app, &token, "ti_search", "TI-SEARCH").await;
    let type_id = document_type(&app, &token, "TI_SEARCH", workflow).await;

    submitted_document(&app, &token, type_id, "Two standing desks").await;
    submitted_document(&app, &token, type_id, "A replacement laptop").await;

    let everything = inbox_of(&app, &approver, "scope=open").await;
    assert_eq!(everything.body["data"].as_array().expect("a page").len(), 2);

    let desks = inbox_of(&app, &approver, "scope=open&q=desks").await;

    assert_eq!(
        desks.body["data"].as_array().expect("a page").len(),
        1,
        "the search narrows the list: {}",
        desks.body
    );
    assert_eq!(
        desks.body["meta"]["total"], 1,
        "and the count narrows with it — a total the page cannot account for is unreadable"
    );
    assert_eq!(desks.body["data"][0]["documentTitle"], "Two standing desks");

    // The other half of AC3: the task's own name.
    let by_task = inbox_of(&app, &approver, "scope=open&q=Approve%20the").await;
    assert_eq!(by_task.body["data"].as_array().expect("a page").len(), 2);
    assert_eq!(by_task.body["meta"]["total"], 2);
}

/// **A wildcard typed by a person is a character.** `%` searching for everything
/// is the version of this somebody notices; `a_b` matching `axb` is the version
/// nobody does.
#[tokio::test]
async fn a_percent_in_a_search_is_a_percent_rather_than_everything() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let (_, approver) = holder(&app, "TI-WILD", "ti.wild").await;

    let workflow = publish_workflow(&app, &token, "ti_wild", "TI-WILD").await;
    let type_id = document_type(&app, &token, "TI_WILD", workflow).await;
    submitted_document(&app, &token, type_id, "Two standing desks").await;

    let everything = inbox_of(&app, &approver, "scope=open&q=%25").await;

    assert_eq!(
        everything.body["data"].as_array().expect("a page").len(),
        0,
        "a search for a percent sign found a task that has none: {}",
        everything.body
    );
    assert_eq!(everything.body["meta"]["total"], 0);
}

/// **The count, the page and the visibility gate agree about which rows exist**
/// ([#279](https://github.com/sujanto-gaws/kelir/issues/279) AC1, AC2).
///
/// A task whose document has been soft-deleted was **counted and not listed**:
/// the page joined `documents`, the count did not, and the detail gate did not
/// either — three statements written to agree, disagreeing in two directions.
///
/// **Seen red** against the count as it stood: `meta.total` said 1 and the page
/// was empty, which is the inbox saying 23 and ending at 19 that
/// `count_for_caller`'s own comment forbids.
#[tokio::test]
async fn the_count_the_page_and_the_gate_agree_when_a_document_is_gone() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let (_, approver) = holder(&app, "TI-GONE", "ti.gone").await;

    let workflow = publish_workflow(&app, &token, "ti_gone", "TI-GONE").await;
    let type_id = document_type(&app, &token, "TI_GONE", workflow).await;
    let document = submitted_document(&app, &token, type_id, "A vanished request").await;
    let task = open_task_of(&app, document).await;

    // Soft-deleted directly: the discard path refuses a submitted document
    // (#278), so this is the state #279 says is reachable only through it.
    sqlx::query("UPDATE documents SET deleted_at = now() WHERE id = $1")
        .bind(document)
        .execute(&app.pool)
        .await
        .expect("the document");

    let after = inbox_of(&app, &approver, "scope=open").await;

    assert_eq!(
        after.body["data"].as_array().expect("a page").len(),
        0,
        "the page does not list a task whose document is gone"
    );
    assert_eq!(
        after.body["meta"]["total"], 0,
        "#279: and the count no longer says it is there"
    );

    let detail = app.get(&format!("{TASKS}/{task}"), Some(&approver)).await;

    assert_eq!(
        detail.status,
        StatusCode::NOT_FOUND,
        "the gate and the read agree: {}",
        detail.body
    );
}

// ---------------------------------------------------------------------------
// D-89 — a role with open work is not deleted out from under it (#487)
// ---------------------------------------------------------------------------

/// **A role with an open task is not deleted, and is once the task is decided**
/// (**D-89**, [#487]) **and its workflow retired** (**D-91** (3), [#510]).
///
/// Deleting a role used to answer 204 whatever was waiting on it, leaving its
/// open tasks offered to nobody and their documents in `PENDING_APPROVAL`. The
/// refusal is a 409 that says how many, and **nothing changes**: the role is
/// still live and its holder is still offered the task.
///
/// **Both reasons hold the role here, and the refusals come one at a time.**
/// The open task is asked about first, and its refusal is D-89's `CONFLICT`.
/// Once the task is decided, the published definition that names the role is
/// all that holds it, and the refusal is **D-91** (3)'s own code,
/// `ROLE_NAMED_BY_PUBLISHED_DEFINITION`. Deleting that revision, which nothing
/// is running on any more, lets the role go.
///
/// **Seen red** against `identity::service::delete_role` with the count's
/// refusal removed: the first refusal is the definition's.
///
/// [#487]: https://github.com/sujanto-gaws/kelir/issues/487
/// [#510]: https://github.com/sujanto-gaws/kelir/issues/510
#[tokio::test]
async fn a_role_with_an_open_task_is_not_deleted_until_the_task_is_decided() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let (role, approver) = holder(&app, "TI-D89-OPEN", "ti.d89.open").await;

    let workflow = publish_workflow(&app, &token, "ti_d89_open", "TI-D89-OPEN").await;
    let type_id = document_type(&app, &token, "TI_D89_OPEN", workflow).await;
    let document = submitted_document(&app, &token, type_id, "Waiting on the role").await;
    let task = open_task_of(&app, document).await;

    let refused = delete_role(&app, &token, role).await;

    assert_eq!(
        refused.status,
        StatusCode::CONFLICT,
        "a role with an open task was deleted: {}",
        refused.body
    );
    assert_eq!(
        refused.body["error"]["code"], "CONFLICT",
        "{}",
        refused.body
    );
    assert_eq!(
        refused.body["error"]["message"], ONE_OPEN_TASK,
        "the refusal says how many tasks: {}",
        refused.body
    );

    let still = app
        .get(&format!("/api/v1/identity/roles/{role}"), Some(&token))
        .await;
    assert_eq!(
        still.status,
        StatusCode::OK,
        "the role is still live: {}",
        still.body
    );

    let inbox = app.get(TASKS, Some(&approver)).await;
    assert_eq!(
        inbox.body["data"].as_array().expect("a page").len(),
        1,
        "the holder is still offered the task: {}",
        inbox.body
    );

    decide(&app, &approver, task).await;

    let named = delete_role(&app, &token, role).await;
    assert_named_by(
        &named,
        "ti_d89_open (\"Standard approval\", revision 1)",
        "with the task decided, only the definition holds the role",
    );

    retire_definition(&app, &token, workflow).await;

    let deleted = delete_role(&app, &token, role).await;
    assert_eq!(
        deleted.status,
        StatusCode::NO_CONTENT,
        "with nothing open and nothing published naming it, the role deletes: {}",
        deleted.body
    );
}

/// **A claimed task does not hold the role it was offered to, and a transition
/// that names a role holds it, claimed or not** (**D-89**, [#529]).
///
/// Record 18's probe P2. The task is offered to one role, `queue`, and its
/// edges are `allowedBy` a second, `edge` (JWSS §5 lets the two differ). The
/// approver holds both. A decision checks the caller against the assignee
/// first and reads the candidate role only while there is none, so once the
/// task is claimed `queue` is not needed, and deleting it leaves the task its
/// assignee's to decide. `edge` is needed whoever holds the task: a decision
/// resolves `allowedBy` again, and a deleted role refuses it as
/// `ASSIGNMENT_UNRESOLVED`.
///
/// Both roles carry the inbox's permissions, so the approver keeps
/// `workflow:task:execute` through `edge` once `queue` is gone, as P2's did.
///
/// The fixture raises **two** tasks and claims one. While the other is
/// unclaimed, `queue` is refused for it, because an unclaimed task is offered
/// through its role and nothing else. Once that one is decided, no task needs
/// `queue`, and the claimed task is still decided with 200. **An unrelated role
/// deletes** throughout: a refusal of every delete while anything is open would
/// pass the 409s.
///
/// The published definition names both roles, so since **D-91** (3) ([#510])
/// a role no open task needs is still refused, for the definition, with
/// `ROLE_NAMED_BY_PUBLISHED_DEFINITION`, and neither deletes until the
/// definition is retired. Each refusal is asserted whole, code and message.
///
/// **Seen red** three times against `count_open_tasks_needing_role`: with
/// #513's `candidate_role_id` clause (claimed or not) the first `queue` refusal
/// counts both tasks; without the `candidate_role_id` clause the first `queue`
/// refusal is the definition's; without the `workflow_transitions` clause the
/// first `edge` refusal counts one task.
///
/// [#510]: https://github.com/sujanto-gaws/kelir/issues/510
/// [#529]: https://github.com/sujanto-gaws/kelir/issues/529
#[tokio::test]
async fn a_claimed_task_needs_only_the_role_its_transitions_name() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let permissions = &[
        "workflow:task:read",
        "workflow:task:execute",
        "workflow:instance:read",
        "document:read",
    ];
    let queue = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "TI-D89-QUEUE",
        permissions,
    )
    .await;
    let edge = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "TI-D89-EDGE",
        permissions,
    )
    .await;
    let unrelated = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "TI-D89-UNRELATED",
        &[],
    )
    .await;
    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "ti.d89.both",
        "ti.d89.both@example.test",
        common::ADMIN_PASSWORD,
        &[queue, edge],
    )
    .await;
    let approver = app.sign_in("ti.d89.both", common::ADMIN_PASSWORD).await;

    let mut definition = workflow_for("ti_d89_edge", "TI-D89-QUEUE");
    for transition in definition["transitions"]
        .as_array_mut()
        .expect("transitions")
    {
        transition["allowedBy"] = json!("ROLE:TI-D89-EDGE");
    }
    let workflow = publish_workflow_definition(&app, &token, "ti_d89_edge", definition).await;
    let type_id = document_type(&app, &token, "TI_D89_EDGE", workflow).await;

    let claimed_document =
        submitted_document(&app, &token, type_id, "Claimed, decided by another role").await;
    let claimed = open_task_of(&app, claimed_document).await;
    let unclaimed_document =
        submitted_document(&app, &token, type_id, "Unclaimed, offered to the queue").await;
    let unclaimed = open_task_of(&app, unclaimed_document).await;

    let claim = app
        .post(
            &format!("/api/v1/workflow/tasks/{claimed}/claim"),
            Some(&approver),
            json!({}),
        )
        .await;
    assert_eq!(claim.status, StatusCode::OK, "{}", claim.body);

    let queue_refused = delete_role(&app, &token, queue).await;
    assert_eq!(
        queue_refused.status,
        StatusCode::CONFLICT,
        "an unclaimed task did not hold the role it is offered to: {}",
        queue_refused.body
    );
    // The whole sentence, in both numbers: it is what an administrator reads,
    // and each branch has its own pronouns.
    assert_eq!(
        queue_refused.body["error"]["message"], ONE_OPEN_TASK,
        "only the unclaimed task needs the queue: {}",
        queue_refused.body
    );

    let edge_refused = delete_role(&app, &token, edge).await;
    assert_eq!(
        edge_refused.status,
        StatusCode::CONFLICT,
        "a transition did not hold the role its allowedBy names: {}",
        edge_refused.body
    );
    assert_eq!(
        edge_refused.body["error"]["message"],
        "2 open tasks need this role to be decided. Deleting the role would leave them \
         offered to nobody, or with a decision nobody could make. They need to be decided \
         first",
        "both tasks, claimed or not, need the edge's role: {}",
        edge_refused.body
    );

    let unrelated_deleted = delete_role(&app, &token, unrelated).await;
    assert_eq!(
        unrelated_deleted.status,
        StatusCode::NO_CONTENT,
        "a role no open task needs was refused: {}",
        unrelated_deleted.body
    );

    decide(&app, &approver, unclaimed).await;

    let queue_named = delete_role(&app, &token, queue).await;
    assert_named_by(
        &queue_named,
        EDGE_DEFINITION,
        "a claimed task held the role it was offered to, which its assignee does not need",
    );

    let edge_still_refused = delete_role(&app, &token, edge).await;
    assert_eq!(
        edge_still_refused.status,
        StatusCode::CONFLICT,
        "{}",
        edge_still_refused.body
    );
    assert_eq!(
        edge_still_refused.body["error"]["message"], ONE_OPEN_TASK,
        "the claimed task stopped holding the role its allowedBy names: {}",
        edge_still_refused.body
    );

    // Record 18's P2: the assignee decides, with the role the task was offered
    // to gone, and holding the permissions through `edge`. The definition keeps
    // `queue` from the route, so it goes the way a release before D-91 (3)
    // would have let it.
    sqlx::query("UPDATE roles SET deleted_at = now() WHERE id = $1")
        .bind(queue)
        .execute(&app.pool)
        .await
        .expect("delete the queue as a release before D-91 (3) did");
    decide(&app, &approver, claimed).await;

    let edge_named = delete_role(&app, &token, edge).await;
    assert_named_by(
        &edge_named,
        EDGE_DEFINITION,
        "with the tasks decided, only the definition holds the role",
    );

    retire_definition(&app, &token, workflow).await;

    let edge_deleted = delete_role(&app, &token, edge).await;
    assert_eq!(
        edge_deleted.status,
        StatusCode::NO_CONTENT,
        "with the tasks decided and the definition retired, the role deletes: {}",
        edge_deleted.body
    );
}

/// A workflow of two approvals: the manager's, offered to `manager`, then
/// finance's, offered to `finance`. Each state's edges are `allowedBy` its own
/// role, so a role is named by the edges of one state only.
fn two_stage(key: &str, manager: &str, finance: &str) -> Value {
    json!({
        "workflowKey": key,
        "version": "1.0.0",
        "name": "Two approvals",
        "initialState": "MANAGER_APPROVAL",
        "states": [
            { "code": "MANAGER_APPROVAL", "name": "Manager approval",
              "mapsToDocumentStatus": "PENDING_APPROVAL",
              "task": { "taskDefinitionKey": "manager_approval",
                        "taskName": "Approve the request",
                        "assignment": { "assigneeType": "ROLE", "roleCode": manager } } },
            { "code": "FINANCE_APPROVAL", "name": "Finance approval",
              "mapsToDocumentStatus": "PENDING_APPROVAL",
              "task": { "taskDefinitionKey": "finance_approval",
                        "taskName": "Approve the spend",
                        "assignment": { "assigneeType": "ROLE", "roleCode": finance } } },
            { "code": "COMPLETED", "name": "Completed", "mapsToDocumentStatus": "COMPLETED",
              "isFinal": true },
            { "code": "REJECTED", "name": "Rejected", "mapsToDocumentStatus": "REJECTED",
              "isFinal": true }
        ],
        "transitions": [
            { "from": "MANAGER_APPROVAL", "to": "FINANCE_APPROVAL", "action": "APPROVE",
              "allowedBy": format!("ROLE:{manager}") },
            { "from": "MANAGER_APPROVAL", "to": "REJECTED", "action": "REJECT",
              "allowedBy": format!("ROLE:{manager}") },
            { "from": "FINANCE_APPROVAL", "to": "COMPLETED", "action": "APPROVE",
              "allowedBy": format!("ROLE:{finance}") },
            { "from": "FINANCE_APPROVAL", "to": "REJECTED", "action": "REJECT",
              "allowedBy": format!("ROLE:{finance}") }
        ]
    })
}

/// **Another tenant's task does not hold a role of the same code** (**D-89**).
///
/// Tenant B has an open task offered to its own `TI-XT` and `allowedBy` it. The
/// system tenant's `TI-XT` is a different role that nothing of its own needs,
/// and it deletes. The edge clause matches by **code**, and codes repeat across
/// tenants, so only the tenant filters keep B's task out of A's count.
///
/// Nor does another tenant's **published definition** naming the same code
/// (**D-91** (3), [#510]): B's definition names `TI-XT` in its task and its
/// edges, and the system tenant has none.
///
/// **Seen red** against `count_open_tasks_needing_role` with both tenant
/// predicates dropped (`t.tenant_id = $1` and `r.tenant_id = t.tenant_id`): the
/// delete answers 409, counting B's task. And against
/// `repository::definition::definitions_naming_role` with `d.tenant_id = $1`
/// dropped: the delete answers 409, naming B's definition.
///
/// [#510]: https://github.com/sujanto-gaws/kelir/issues/510
#[tokio::test]
async fn a_role_is_not_held_by_another_tenants_task() {
    let app = TestApp::spawn_with(|config| config.multi_tenant = true).await;
    let token = app
        .sign_in_to("SYSTEM", common::ADMIN_USERNAME, common::ADMIN_PASSWORD)
        .await;

    let (other, other_token) = tenant_administrator(&app, "TNT-XT", "ti.xt.other").await;
    fixtures::create_role_with_permissions(&app.pool, other, "TI-XT", &[]).await;
    let workflow = publish_workflow(&app, &other_token, "ti_xt", "TI-XT").await;
    let type_id = document_type(&app, &other_token, "TI_XT", workflow).await;
    submitted_document(&app, &other_token, type_id, "Open in another tenant").await;

    let ours =
        fixtures::create_role_with_permissions(&app.pool, fixtures::SYSTEM_TENANT_ID, "TI-XT", &[])
            .await;

    let deleted = delete_role(&app, &token, ours).await;
    assert_eq!(
        deleted.status,
        StatusCode::NO_CONTENT,
        "another tenant's task held a role of the same code: {}",
        deleted.body
    );
}

/// **Only the edges out of the task's current state, in its own definition,
/// hold a role**, and a completed task holds nothing (**D-89**).
///
/// Two definitions of two approvals share a manager role, `M`, and each has
/// its own finance role. Document A waits at the manager, so its finance role
/// `FA` is named only by a **later** state's edges and no open task needs it.
/// A third, unrelated definition names `Z` in edges out of a state with the
/// **same code** as A's current one, and has no task; no open task needs `Z`
/// either.
///
/// Document B is approved by the manager and waits at finance. Its finance role
/// `FB` is refused for exactly **one** task: the manager's task is completed,
/// although its instance now sits in the state whose edges name `FB`.
///
/// Each role is also named by its published definition, so since **D-91** (3)
/// ([#510]) `FA` and `Z` are refused too, with the definition's own code. That
/// is the point of D-91 (3) for `FA`: document A's next step needs it. A task
/// counted wrongly would turn either refusal into D-89's `CONFLICT`.
///
/// **Seen red** against `count_open_tasks_needing_role`: without
/// `tr.from_state = i.current_state` the `FA` refusal counts a task; without
/// `tr.workflow_definition_id = i.workflow_definition_id` the `Z` refusal does;
/// without the status filter the `FB` refusal counts 2 tasks.
///
/// [#510]: https://github.com/sujanto-gaws/kelir/issues/510
#[tokio::test]
async fn a_role_is_held_by_the_current_states_edges_and_by_open_tasks_only() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let (_, approver) = holder(&app, "TI-STAGE-M", "ti.stage.manager").await;
    let mut roles = Vec::new();
    for code in ["TI-STAGE-FA", "TI-STAGE-FB", "TI-STAGE-Z"] {
        roles.push(
            fixtures::create_role_with_permissions(
                &app.pool,
                fixtures::SYSTEM_TENANT_ID,
                code,
                &[],
            )
            .await,
        );
    }
    let (finance_a, finance_b, unrelated) = (roles[0], roles[1], roles[2]);

    let a = two_stage("ti_stage_a", "TI-STAGE-M", "TI-STAGE-FA");
    let a = publish_workflow_definition(&app, &token, "ti_stage_a", a).await;
    let a = document_type(&app, &token, "TI_STAGE_A", a).await;
    submitted_document(&app, &token, a, "Waiting at the manager").await;

    let b = two_stage("ti_stage_b", "TI-STAGE-M", "TI-STAGE-FB");
    let b = publish_workflow_definition(&app, &token, "ti_stage_b", b).await;
    let b = document_type(&app, &token, "TI_STAGE_B", b).await;
    let b = submitted_document(&app, &token, b, "Waiting at finance").await;
    let manager_task = open_task_of(&app, b).await;
    decide(&app, &approver, manager_task).await;

    publish_workflow(&app, &token, "ti_stage_z", "TI-STAGE-Z").await;

    let elsewhere = delete_role(&app, &token, unrelated).await;
    assert_named_by(
        &elsewhere,
        "ti_stage_z (\"Standard approval\", revision 1)",
        "an open task held a role named only by another definition's edges",
    );

    let downstream = delete_role(&app, &token, finance_a).await;
    assert_named_by(
        &downstream,
        "ti_stage_a (\"Standard approval\", revision 1)",
        "an open task held a role named only by a later state's edges",
    );

    let current = delete_role(&app, &token, finance_b).await;
    assert_eq!(
        current.status,
        StatusCode::CONFLICT,
        "the finance task did not hold its role: {}",
        current.body
    );
    assert_eq!(
        current.body["error"]["message"], ONE_OPEN_TASK,
        "the completed manager task was counted: {}",
        current.body
    );
}

/// **A `DEPARTMENT_ROLE` edge holds its role as a `ROLE` edge does**
/// (**D-89**).
///
/// A decision resolves the edge through `assignment::permits` by its
/// `roleCode`, so a deleted role refuses it as a deleted `ROLE:` one would. The
/// task is offered to another role, `Q`, so only the edge can hold `D`.
///
/// **Seen red** against `count_open_tasks_needing_role` with the edge clause
/// narrowed to `tr.allowed_by_json->>'assigneeType' = 'ROLE'`: the delete
/// answers 204. Since **D-91** (3) the published definition names `D` too and
/// refuses the delete anyway, so the code and message are asserted: under the
/// same mutation the refusal is the definition's (seen red again for #510).
#[tokio::test]
async fn a_department_role_edge_holds_its_role() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    sqlx::query(
        "INSERT INTO departments (id, tenant_id, department_code, name)
         VALUES ($1, $2, 'TI-DEPT', 'Procurement')",
    )
    .bind(Uuid::now_v7())
    .bind(fixtures::SYSTEM_TENANT_ID)
    .execute(&app.pool)
    .await
    .expect("insert the department");

    fixtures::create_role_with_permissions(&app.pool, fixtures::SYSTEM_TENANT_ID, "TI-DEPT-Q", &[])
        .await;
    let department_role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "TI-DEPT-D",
        &[],
    )
    .await;

    let mut definition = workflow_for("ti_dept", "TI-DEPT-Q");
    for transition in definition["transitions"]
        .as_array_mut()
        .expect("transitions")
    {
        transition["allowedBy"] = json!({
            "assigneeType": "DEPARTMENT_ROLE",
            "roleCode": "TI-DEPT-D",
            "departmentScope": "TI-DEPT",
        });
    }
    let workflow = publish_workflow_definition(&app, &token, "ti_dept", definition).await;
    let type_id = document_type(&app, &token, "TI_DEPT", workflow).await;
    submitted_document(&app, &token, type_id, "Decided by a department's role").await;

    let refused = delete_role(&app, &token, department_role).await;
    assert_eq!(
        refused.status,
        StatusCode::CONFLICT,
        "a DEPARTMENT_ROLE edge did not hold its role: {}",
        refused.body
    );
    assert_eq!(
        refused.body["error"]["code"], "CONFLICT",
        "{}",
        refused.body
    );
    assert_eq!(
        refused.body["error"]["message"], ONE_OPEN_TASK,
        "the open task did not hold the role its DEPARTMENT_ROLE edge names: {}",
        refused.body
    );
}

async fn delete_role(app: &TestApp, token: &str, role: Uuid) -> common::TestResponse {
    app.delete(&format!("/api/v1/identity/roles/{role}"), Some(token))
        .await
}

/// D-89's refusal for one open task. It is asked about first, and answers
/// `CONFLICT`.
const ONE_OPEN_TASK: &str = "1 open task needs this role to be decided. Deleting the role would \
                             leave it offered to nobody, or with a decision nobody could make. \
                             It needs to be decided first";

/// `a_claimed_task_needs_only_the_role_its_transitions_name`'s definition, as
/// a refusal names it.
const EDGE_DEFINITION: &str = "ti_d89_edge (\"Standard approval\", revision 1)";

/// The **D-91** (3) refusal for a role one published definition names, given
/// that definition as the refusal names it.
fn named_by(definition: &str) -> String {
    format!(
        "This role is named by 1 published workflow definition: {definition}. Deleting the \
         role would leave it unable to raise its tasks. Publish a revision that does not name \
         the role, bind its document types to that revision, and delete this one once its \
         running approvals are finished"
    )
}

/// Asserts `response` is **D-91** (3)'s refusal naming exactly one definition.
fn assert_named_by(response: &common::TestResponse, definition: &str, why: &str) {
    assert_eq!(
        response.status,
        StatusCode::CONFLICT,
        "{why}: {}",
        response.body
    );
    assert_eq!(
        response.body["error"]["code"], "ROLE_NAMED_BY_PUBLISHED_DEFINITION",
        "{why}: {}",
        response.body
    );
    assert_eq!(
        response.body["error"]["message"],
        named_by(definition),
        "{why}: {}",
        response.body
    );
}

/// Deletes a workflow revision through the route an administrator uses, which
/// answers 204 once nothing is running on it.
async fn retire_definition(app: &TestApp, token: &str, definition: Uuid) {
    let retired = app
        .delete(
            &format!("/api/v1/workflow/definitions/{definition}"),
            Some(token),
        )
        .await;
    assert_eq!(retired.status, StatusCode::NO_CONTENT, "{}", retired.body);
}

// ---------------------------------------------------------------------------
// D-91 (3) — a role a published definition names is not deleted (#510)
// ---------------------------------------------------------------------------

/// A definition naming a role in each place the engine resolves one, and one
/// place it does not: the first state's task is offered to `task` and
/// escalates to `escalation`; its `APPROVE` is `allowedBy` `object` in the
/// object form and its `REJECT` `short` in the `"ROLE:X"` shorthand. The second
/// state's task is offered to `department` as a `DEPARTMENT_ROLE`, and its
/// `REJECT` is `allowedBy` `department_edge` the same way. Its `APPROVE` is the
/// owner's, which names no role.
fn naming_everywhere(key: &str) -> Value {
    json!({
        "workflowKey": key,
        "version": "1.0.0",
        "name": "Named everywhere",
        "initialState": "MANAGER_APPROVAL",
        "states": [
            { "code": "MANAGER_APPROVAL", "name": "Manager approval",
              "mapsToDocumentStatus": "PENDING_APPROVAL",
              "task": { "taskDefinitionKey": "manager_approval",
                        "taskName": "Approve the request",
                        "assignment": { "assigneeType": "ROLE", "roleCode": "TI-NAMED-TASK" },
                        "escalation": {
                            "afterHours": 24,
                            "assignment": { "assigneeType": "ROLE",
                                            "roleCode": "TI-NAMED-ESCALATION" } } } },
            { "code": "DEPARTMENT_APPROVAL", "name": "Department approval",
              "mapsToDocumentStatus": "PENDING_APPROVAL",
              "task": { "taskDefinitionKey": "department_approval",
                        "taskName": "Approve for the department",
                        "assignment": { "assigneeType": "DEPARTMENT_ROLE",
                                        "roleCode": "TI-NAMED-DEPARTMENT",
                                        "departmentScope": "REQUESTED_DEPARTMENT" } } },
            { "code": "COMPLETED", "name": "Completed", "mapsToDocumentStatus": "COMPLETED",
              "isFinal": true },
            { "code": "REJECTED", "name": "Rejected", "mapsToDocumentStatus": "REJECTED",
              "isFinal": true }
        ],
        "transitions": [
            { "from": "MANAGER_APPROVAL", "to": "DEPARTMENT_APPROVAL", "action": "APPROVE",
              "allowedBy": { "assigneeType": "ROLE", "roleCode": "TI-NAMED-OBJECT" } },
            { "from": "MANAGER_APPROVAL", "to": "REJECTED", "action": "REJECT",
              "allowedBy": "ROLE:TI-NAMED-SHORT" },
            { "from": "DEPARTMENT_APPROVAL", "to": "COMPLETED", "action": "APPROVE",
              "allowedBy": "OWNER" },
            { "from": "DEPARTMENT_APPROVAL", "to": "REJECTED", "action": "REJECT",
              "allowedBy": { "assigneeType": "DEPARTMENT_ROLE",
                             "roleCode": "TI-NAMED-DEPARTMENT-EDGE",
                             "departmentScope": "REQUESTED_DEPARTMENT" } }
        ]
    })
}

/// **A published definition holds every role it names where the engine
/// resolves one, and nothing else** (**D-91** (3), [#510]).
///
/// No document is ever submitted: nothing is open, so each refusal is the
/// definition's alone. The roles a task's `assignment` names, as `ROLE` and as
/// `DEPARTMENT_ROLE`, and the roles an edge's `allowedBy` names, in the object
/// form and the `"ROLE:X"` shorthand and as `DEPARTMENT_ROLE`, are each refused
/// with a message naming the definition by key, name and revision.
///
/// Five roles delete: one only an `escalation.assignment` names, which nothing
/// executes (JWSS §3.1); one only a **draft** names; one of a code nothing
/// names; one whose code differs from a named one only in case, because the
/// engine resolves codes exactly; and, once the definition is retired, `task`.
///
/// **Seen red** against `definitions_naming_role`: without the status filter,
/// the draft's role is refused; with the `workflow_transitions` clause reading
/// `allowedBy` from `definition_json` rather than the normalized projection,
/// the shorthand's role deletes; with `DEPARTMENT_ROLE` dropped from either
/// clause, that clause's department role deletes; with an
/// `escalation.assignment` path added, the escalation's role is refused.
///
/// [#510]: https://github.com/sujanto-gaws/kelir/issues/510
#[tokio::test]
async fn a_published_definition_holds_every_role_it_names_and_nothing_else() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let mut roles = std::collections::BTreeMap::new();
    for code in [
        "TI-NAMED-TASK",
        "TI-NAMED-DEPARTMENT",
        "TI-NAMED-OBJECT",
        "TI-NAMED-SHORT",
        "TI-NAMED-DEPARTMENT-EDGE",
        "TI-NAMED-ESCALATION",
        "TI-NAMED-DRAFT",
        "TI-NAMED-NOBODY",
        "ti-named-task",
    ] {
        let role = fixtures::create_role_with_permissions(
            &app.pool,
            fixtures::SYSTEM_TENANT_ID,
            code,
            &[],
        )
        .await;
        roles.insert(code, role);
    }

    let workflow =
        publish_workflow_definition(&app, &token, "ti_named", naming_everywhere("ti_named")).await;

    let draft = app
        .post(
            "/api/v1/workflow/definitions",
            Some(&token),
            json!({
                "workflowKey": "ti_named_draft",
                "name": "Never published",
                "definition": workflow_for("ti_named_draft", "TI-NAMED-DRAFT"),
            }),
        )
        .await;
    assert_eq!(draft.status, StatusCode::CREATED, "{}", draft.body);

    for code in [
        "TI-NAMED-TASK",
        "TI-NAMED-DEPARTMENT",
        "TI-NAMED-OBJECT",
        "TI-NAMED-SHORT",
        "TI-NAMED-DEPARTMENT-EDGE",
    ] {
        let refused = delete_role(&app, &token, roles[code]).await;
        assert_named_by(
            &refused,
            "ti_named (\"Standard approval\", revision 1)",
            &format!("the published definition did not hold {code}"),
        );
    }

    for code in [
        "TI-NAMED-ESCALATION",
        "TI-NAMED-DRAFT",
        "TI-NAMED-NOBODY",
        "ti-named-task",
    ] {
        let deleted = delete_role(&app, &token, roles[code]).await;
        assert_eq!(
            deleted.status,
            StatusCode::NO_CONTENT,
            "{code} was held by something that does not need it: {}",
            deleted.body
        );
    }

    retire_definition(&app, &token, workflow).await;

    let deleted = delete_role(&app, &token, roles["TI-NAMED-TASK"]).await;
    assert_eq!(
        deleted.status,
        StatusCode::NO_CONTENT,
        "a retired definition still held its role: {}",
        deleted.body
    );
}

/// **A new revision does not release a role the old one names; retiring the
/// old one does, once nothing runs on it** (**D-91** (3), [#510]).
///
/// Revision 1 is two approvals, the manager's then finance's, and a document
/// waits at the manager. Revision 2 drops finance. Publishing it leaves
/// revision 1 `ACTIVE` (no route deprecates a revision), so finance is refused
/// for revision 1, and **revision 1 only**.
///
/// Revision 1 is then deprecated, and finance is still refused, now for a
/// deprecated revision: document A's approval runs on it and will reach
/// finance next. That is #510's second case, an open instance whose next step
/// needs the role, although no open task does. Once the approval finishes,
/// nothing runs on revision 1, and finance deletes.
///
/// **Seen red** against `definitions_naming_role` without its `DEPRECATED`
/// clause: the second delete answers 204.
///
/// [#510]: https://github.com/sujanto-gaws/kelir/issues/510
#[tokio::test]
async fn a_role_is_released_when_the_revisions_naming_it_are_retired() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let (_, manager) = holder(&app, "TI-RETIRE-M", "ti.retire.manager").await;
    let (finance, finance_holder) = holder(&app, "TI-RETIRE-F", "ti.retire.finance").await;

    let first = two_stage("ti_retire", "TI-RETIRE-M", "TI-RETIRE-F");
    let first = publish_workflow_definition(&app, &token, "ti_retire", first).await;
    let type_id = document_type(&app, &token, "TI_RETIRE", first).await;
    let document = submitted_document(&app, &token, type_id, "Approved on revision 1").await;

    let revision = app
        .post(
            &format!("/api/v1/workflow/definitions/{first}/revisions"),
            Some(&token),
            json!({ "definition": two_stage("ti_retire", "TI-RETIRE-M", "TI-RETIRE-M") }),
        )
        .await;
    assert_eq!(revision.status, StatusCode::CREATED, "{}", revision.body);
    let second = id_of(&revision.body["data"]);
    let published = app
        .post(
            &format!("/api/v1/workflow/definitions/{second}/publication"),
            Some(&token),
            json!({}),
        )
        .await;
    assert_eq!(published.status, StatusCode::OK, "{}", published.body);

    let active = delete_role(&app, &token, finance).await;
    assert_named_by(
        &active,
        "ti_retire (\"Standard approval\", revision 1)",
        "publishing revision 2 released the role revision 1 names",
    );

    sqlx::query("UPDATE workflow_definitions SET status = 'DEPRECATED' WHERE id = $1")
        .bind(first)
        .execute(&app.pool)
        .await
        .expect("deprecate revision 1");

    let deprecated = delete_role(&app, &token, finance).await;
    assert_named_by(
        &deprecated,
        "ti_retire (\"Standard approval\", revision 1, deprecated)",
        "a deprecated revision with an approval running on it did not hold its role",
    );

    decide(&app, &manager, open_task_of(&app, document).await).await;
    decide(&app, &finance_holder, open_task_of(&app, document).await).await;

    let deleted = delete_role(&app, &token, finance).await;
    assert_eq!(
        deleted.status,
        StatusCode::NO_CONTENT,
        "a deprecated revision nothing runs on held its role: {}",
        deleted.body
    );
}

/// An approval the approver can send back to its author: `RETURN` moves it to
/// `RETURNED`, a state with **no task**, and only `resubmitter` may take its
/// `RESUBMIT` (JWSS §8's shape, with a role where §8 has `OWNER`).
fn sent_back(key: &str, approver: &str, resubmitter: &str) -> Value {
    json!({
        "workflowKey": key,
        "version": "1.0.0",
        "name": "Approval that can send back",
        "initialState": "MANAGER_APPROVAL",
        "states": [
            { "code": "MANAGER_APPROVAL", "name": "Manager approval",
              "mapsToDocumentStatus": "PENDING_APPROVAL",
              "task": { "taskDefinitionKey": "manager_approval", "taskName": "Decide",
                        "assignment": { "assigneeType": "ROLE", "roleCode": approver } } },
            { "code": "RETURNED", "name": "Sent back", "mapsToDocumentStatus": "RETURNED" },
            { "code": "COMPLETED", "name": "Completed", "mapsToDocumentStatus": "COMPLETED",
              "isFinal": true },
            { "code": "REJECTED", "name": "Rejected", "mapsToDocumentStatus": "REJECTED",
              "isFinal": true }
        ],
        "transitions": [
            { "from": "MANAGER_APPROVAL", "to": "COMPLETED", "action": "APPROVE",
              "allowedBy": format!("ROLE:{approver}") },
            { "from": "MANAGER_APPROVAL", "to": "REJECTED", "action": "REJECT",
              "allowedBy": format!("ROLE:{approver}"), "requiresComment": true },
            { "from": "MANAGER_APPROVAL", "to": "RETURNED", "action": "RETURN",
              "allowedBy": format!("ROLE:{approver}"), "requiresComment": true },
            { "from": "RETURNED", "to": "MANAGER_APPROVAL", "action": "RESUBMIT",
              "allowedBy": format!("ROLE:{resubmitter}") }
        ]
    })
}

/// **A document sent back, with no open task, holds the role its `RESUBMIT`
/// names** (**D-91** (3), [#510]'s second case).
///
/// The document waits in `RETURNED`, which declares no task, so D-89's count
/// finds nothing; only the edge out of it needs `TI-SENT-RESUB`. The revision
/// holds the role while it is `ACTIVE`; deprecated, it still does while the
/// instance runs, `SUSPENDED` included, because a suspended instance can be
/// resumed onto that edge. Once the instance is `CANCELLED`, nothing runs on
/// the revision and the role deletes.
///
/// **Seen red** against `definitions_naming_role` with `'SUSPENDED'` dropped
/// from the instance statuses: the third delete answers 204.
///
/// [#510]: https://github.com/sujanto-gaws/kelir/issues/510
#[tokio::test]
async fn a_document_sent_back_holds_the_role_its_resubmit_names() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let (_, approver) = holder(&app, "TI-SENT-APPROVER", "ti.sent.approver").await;
    let resubmitter = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "TI-SENT-RESUB",
        &[],
    )
    .await;

    let workflow = publish_workflow_definition(
        &app,
        &token,
        "ti_sent",
        sent_back("ti_sent", "TI-SENT-APPROVER", "TI-SENT-RESUB"),
    )
    .await;
    let type_id = document_type(&app, &token, "TI_SENT", workflow).await;
    let document = submitted_document(&app, &token, type_id, "Sent back to its author").await;
    let task = open_task_of(&app, document).await;

    let returned = app
        .post(
            &format!("/api/v1/workflow/tasks/{task}/decision"),
            Some(&approver),
            json!({ "action": "RETURN", "comment": "Sent back." }),
        )
        .await;
    assert_eq!(returned.status, StatusCode::OK, "{}", returned.body);
    assert_eq!(
        returned.body["data"]["currentState"], "RETURNED",
        "{}",
        returned.body
    );

    let open: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM workflow_tasks
         WHERE document_id = $1 AND status IN ('CREATED','ASSIGNED','IN_PROGRESS')",
    )
    .bind(document)
    .fetch_one(&app.pool)
    .await
    .expect("count the open tasks");
    assert_eq!(open, 0, "the document waits with no open task");

    let active = delete_role(&app, &token, resubmitter).await;
    assert_named_by(
        &active,
        "ti_sent (\"Standard approval\", revision 1)",
        "an ACTIVE revision did not hold the role its waiting edge names",
    );

    sqlx::query("UPDATE workflow_definitions SET status = 'DEPRECATED' WHERE id = $1")
        .bind(workflow)
        .execute(&app.pool)
        .await
        .expect("deprecate the revision");

    let deprecated = delete_role(&app, &token, resubmitter).await;
    assert_named_by(
        &deprecated,
        "ti_sent (\"Standard approval\", revision 1, deprecated)",
        "a deprecated revision did not hold the role its running instance waits on",
    );

    set_instance_status(&app, document, "SUSPENDED").await;
    let suspended = delete_role(&app, &token, resubmitter).await;
    assert_named_by(
        &suspended,
        "ti_sent (\"Standard approval\", revision 1, deprecated)",
        "a suspended instance did not keep its deprecated revision's role",
    );

    set_instance_status(&app, document, "CANCELLED").await;
    let deleted = delete_role(&app, &token, resubmitter).await;
    assert_eq!(
        deleted.status,
        StatusCode::NO_CONTENT,
        "a deprecated revision whose only instance is cancelled held its role: {}",
        deleted.body
    );
}

/// **A deprecated revision is held only by its own running instances**
/// (**D-91** (3), [#510]).
///
/// Revision `ti_idle` names the role and is deprecated with nothing ever run
/// on it. Another definition, which names another role, has an approval
/// running. That instance is not `ti_idle`'s, so the role deletes.
///
/// **Seen red** against `definitions_naming_role` with
/// `i.workflow_definition_id = d.id` dropped from the instance clause: the
/// delete answers 409, the other definition's instance holding `ti_idle`.
///
/// [#510]: https://github.com/sujanto-gaws/kelir/issues/510
#[tokio::test]
async fn a_deprecated_revision_is_held_only_by_its_own_running_instances() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    holder(&app, "TI-IDLE-OTHER", "ti.idle.other").await;
    let role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "TI-IDLE-NAMED",
        &[],
    )
    .await;

    let idle = publish_workflow(&app, &token, "ti_idle", "TI-IDLE-NAMED").await;
    sqlx::query("UPDATE workflow_definitions SET status = 'DEPRECATED' WHERE id = $1")
        .bind(idle)
        .execute(&app.pool)
        .await
        .expect("deprecate the idle revision");

    let running = publish_workflow(&app, &token, "ti_idle_running", "TI-IDLE-OTHER").await;
    let type_id = document_type(&app, &token, "TI_IDLE_RUNNING", running).await;
    let document = submitted_document(&app, &token, type_id, "Running elsewhere").await;
    open_task_of(&app, document).await;

    let deleted = delete_role(&app, &token, role).await;
    assert_eq!(
        deleted.status,
        StatusCode::NO_CONTENT,
        "another definition's running instance kept a deprecated revision's role: {}",
        deleted.body
    );
}

/// **An edge's role code is matched exactly, case included** (**D-91** (3),
/// [#510]).
///
/// The edges are `allowedBy` `"ROLE:TI-CASE-EDGE"` and the task is offered to
/// another role, so only the `workflow_transitions` clause can hold a role.
/// `TI-CASE-EDGE` is held; `ti-case-edge`, a different role that
/// `assignment::permits` would never resolve for that edge, deletes.
///
/// **Seen red** against `definitions_naming_role` with the edge clause
/// comparing `lower(…)` of both codes: `ti-case-edge` is refused.
///
/// [#510]: https://github.com/sujanto-gaws/kelir/issues/510
#[tokio::test]
async fn an_edge_holds_the_role_of_its_exact_code_only() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "TI-CASE-TASK",
        &[],
    )
    .await;
    let exact = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "TI-CASE-EDGE",
        &[],
    )
    .await;
    let other_case = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "ti-case-edge",
        &[],
    )
    .await;

    let mut definition = workflow_for("ti_case", "TI-CASE-TASK");
    for transition in definition["transitions"]
        .as_array_mut()
        .expect("transitions")
    {
        transition["allowedBy"] = json!("ROLE:TI-CASE-EDGE");
    }
    publish_workflow_definition(&app, &token, "ti_case", definition).await;

    let deleted = delete_role(&app, &token, other_case).await;
    assert_eq!(
        deleted.status,
        StatusCode::NO_CONTENT,
        "an edge naming TI-CASE-EDGE held ti-case-edge: {}",
        deleted.body
    );

    let refused = delete_role(&app, &token, exact).await;
    assert_named_by(
        &refused,
        "ti_case (\"Standard approval\", revision 1)",
        "the edge did not hold the role it names",
    );
}

/// **Several definitions are named by key, then by revision** (**D-91** (3),
/// [#510]).
///
/// They are published in another order, `ti_order_b` revision 1 and then
/// `ti_order_a` revisions 1 and 2, so that the order written is not the order
/// expected. The refusal carries no `details`.
///
/// **Seen red** against `definitions_naming_role` ordering by
/// `d.workflow_key DESC`: the message lists `ti_order_b` first. **Not red**
/// with the `ORDER BY` dropped: the rows still came back by key and revision,
/// most likely because the plan reads them through
/// `uq_workflow_definitions_tenant_id_workflow_key_version`. A test cannot fix
/// the plan, so the `ORDER BY` is what guarantees the order, and this test
/// catches a wrong one rather than a missing one.
///
/// [#510]: https://github.com/sujanto-gaws/kelir/issues/510
#[tokio::test]
async fn several_definitions_are_named_in_key_then_revision_order() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "TI-ORDER",
        &[],
    )
    .await;

    publish_workflow(&app, &token, "ti_order_b", "TI-ORDER").await;
    let first = publish_workflow(&app, &token, "ti_order_a", "TI-ORDER").await;
    let revision = app
        .post(
            &format!("/api/v1/workflow/definitions/{first}/revisions"),
            Some(&token),
            json!({ "definition": workflow_for("ti_order_a", "TI-ORDER") }),
        )
        .await;
    assert_eq!(revision.status, StatusCode::CREATED, "{}", revision.body);
    let second = id_of(&revision.body["data"]);
    let published = app
        .post(
            &format!("/api/v1/workflow/definitions/{second}/publication"),
            Some(&token),
            json!({}),
        )
        .await;
    assert_eq!(published.status, StatusCode::OK, "{}", published.body);

    let refused = delete_role(&app, &token, role).await;
    assert_eq!(refused.status, StatusCode::CONFLICT, "{}", refused.body);
    assert_eq!(
        refused.body["error"]["code"], "ROLE_NAMED_BY_PUBLISHED_DEFINITION",
        "{}",
        refused.body
    );
    assert_eq!(
        refused.body["error"]["message"],
        "This role is named by 3 published workflow definitions: ti_order_a (\"Standard \
         approval\", revision 1), ti_order_a (\"Standard approval\", revision 2), ti_order_b \
         (\"Standard approval\", revision 1). Deleting the role would leave them unable to \
         raise their tasks. Publish revisions that do not name the role, bind their document \
         types to those revisions, and delete these once their running approvals are finished",
        "{}",
        refused.body
    );
    assert!(
        refused.body["error"]["details"]
            .as_array()
            .is_some_and(|details| details.is_empty()),
        "the refusal carries no details: {}",
        refused.body
    );
}

/// Sets the status of `document`'s instance directly, as the test's shortest
/// way to a `SUSPENDED` or `CANCELLED` instance.
async fn set_instance_status(app: &TestApp, document: Uuid, status: &str) {
    sqlx::query("UPDATE workflow_instances SET status = $2 WHERE document_id = $1")
        .bind(document)
        .bind(status)
        .execute(&app.pool)
        .await
        .expect("set the instance's status");
}

async fn decide(app: &TestApp, token: &str, task: Uuid) {
    let decided = app
        .post(
            &format!("/api/v1/workflow/tasks/{task}/decision"),
            Some(token),
            json!({ "action": "APPROVE" }),
        )
        .await;
    assert_eq!(decided.status, StatusCode::OK, "{}", decided.body);
}

/// **A task cannot be offered to a role while its delete is under way**
/// (**D-89**).
///
/// The delete counts open tasks under a `FOR UPDATE` lock on the role row, and
/// the count is only true if no task can arrive before the delete commits. A
/// task being raised is invisible to the count until its own transaction
/// commits, and the foreign key does not stop it, because a soft delete leaves
/// the key alone. So the transition takes `FOR KEY SHARE` on the role it
/// resolves, and waits.
///
/// The test holds the delete's half itself, in a transaction on the pool, so it
/// controls the moment between the lock and the commit: it locks the row,
/// starts a submission, soft-deletes the role and commits. The submission must
/// have waited, and must then find the role gone and refuse as
/// `ASSIGNMENT_UNRESOLVED`, with no task written.
///
/// **Seen red** against `workflow::service::assignment::direct` without its
/// `FOR KEY SHARE`: the submission answers 200 and a task is offered to the
/// deleted role.
#[tokio::test]
async fn a_submission_racing_a_role_delete_waits_and_then_finds_the_role_gone() {
    use std::sync::Arc;
    use std::time::Duration;

    let app = Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;

    let (role, _) = holder(&app, "TI-D89-RACE", "ti.d89.race").await;

    let workflow = publish_workflow(&app, &token, "ti_d89_race", "TI-D89-RACE").await;
    let type_id = document_type(&app, &token, "TI_D89_RACE", workflow).await;

    let created = app
        .post(
            "/api/v1/documents",
            Some(&token),
            json!({
                "documentTypeId": type_id,
                "title": "Submitted during a delete",
                "formData": { "amount": 1_000 },
            }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let document = id_of(&created.body["data"]);

    let mut delete = app.pool.begin().await.expect("a transaction");
    sqlx::query("SELECT id FROM roles WHERE id = $1 FOR UPDATE")
        .bind(role)
        .execute(&mut *delete)
        .await
        .expect("the delete's lock");

    let submission = {
        let app = Arc::clone(&app);
        let token = token.clone();
        tokio::spawn(async move {
            app.send(
                Method::POST,
                &format!("/api/v1/documents/{document}/submission"),
                Some(&token),
                None,
            )
            .await
        })
    };

    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        !submission.is_finished(),
        "the submission did not wait for the role's delete"
    );

    sqlx::query("UPDATE roles SET deleted_at = now() WHERE id = $1")
        .bind(role)
        .execute(&mut *delete)
        .await
        .expect("the delete");
    delete.commit().await.expect("the delete commits");

    let submitted = submission.await.expect("the submission did not panic");

    assert_eq!(
        submitted.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a task was offered to a role deleted under it: {}",
        submitted.body
    );
    assert!(
        submitted.body.to_string().contains("ASSIGNMENT_UNRESOLVED"),
        "{}",
        submitted.body
    );

    let tasks: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM workflow_tasks WHERE document_id = $1")
            .bind(document)
            .fetch_one(&app.pool)
            .await
            .expect("count the tasks");
    assert_eq!(tasks, 0, "no task was written");
}

/// **A real role delete waits on a transition holding the role, then refuses**
/// (**D-89**, [#528]).
///
/// The race test above holds the delete's half itself, so it proves that the
/// transition waits on a `FOR UPDATE`. It does not prove that
/// `identity::service::delete_role` takes one. This test takes the other order
/// and drives the real route for the delete: a submission holds the role
/// `FOR KEY SHARE` and has not committed its task, and
/// `DELETE /api/v1/identity/roles/{id}` arrives. The delete must wait for the
/// submission, then count its task and answer 409 with nothing deleted. How
/// the order is forced is [`delete_during_a_held_submission`]'s.
///
/// **Seen red** twice: with `repository::lock_role_for_delete` at
/// `FOR NO KEY UPDATE`, which does not conflict with `FOR KEY SHARE`, the
/// delete never waits and answers 204 beside the task; and with the open-task
/// count moved above the lock in `delete_role`, the delete waits, but counts
/// before the task commits and answers 204. Since **D-91** (3) ([#510]) the
/// definition that raised the task would refuse both deletes anyway, and both
/// mutations are still red (seen again for #510): the first delete answers 409
/// without waiting, and the second answers the definition's
/// `ROLE_NAMED_BY_PUBLISHED_DEFINITION`, not the task's refusal.
///
/// [#510]: https://github.com/sujanto-gaws/kelir/issues/510
/// [#528]: https://github.com/sujanto-gaws/kelir/issues/528
#[tokio::test]
async fn a_role_delete_arriving_during_a_transition_waits_for_it_and_refuses() {
    let app = Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;

    let (role, _) = holder(&app, "TI-D89-HELD", "ti.d89.held").await;

    let workflow = publish_workflow(&app, &token, "ti_d89_held", "TI-D89-HELD").await;
    let type_id = document_type(&app, &token, "TI_D89_HELD", workflow).await;
    let document = draft_document(&app, &token, type_id, "Submitted before a delete").await;

    let refused = delete_during_a_held_submission(&app, &token, document, role).await;

    assert_refused_for_one_open_task(&app, &refused, role, document).await;
}

/// **The same, for a role the task is not offered to but its edges name**
/// (**D-89**, [#509], [#528]).
///
/// The task is offered to a live role, and its `APPROVE` and `REJECT` are
/// `allowedBy` another. Deleting that other role would leave a task nobody can
/// decide, so the delete must wait for the submission as it does for the
/// task's own role. Here the transition's lock is
/// `assignment::hold_deciding_roles`'s `FOR KEY SHARE` ([#514]), not
/// `direct`'s, and the delete's count finds the task by its edge.
///
/// **Seen red** under the same two mutations as the test above, at the same
/// assertions.
///
/// [#509]: https://github.com/sujanto-gaws/kelir/issues/509
/// [#514]: https://github.com/sujanto-gaws/kelir/pull/514
/// [#528]: https://github.com/sujanto-gaws/kelir/issues/528
#[tokio::test]
async fn a_delete_of_an_edges_role_arriving_during_a_transition_waits_and_refuses() {
    let app = Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;

    holder(&app, "TI-D89-OFFERED", "ti.d89.offered").await;
    let edge = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "TI-D89-DECIDES",
        &[],
    )
    .await;

    let mut definition = workflow_for("ti_d89_decides", "TI-D89-OFFERED");
    for transition in definition["transitions"]
        .as_array_mut()
        .expect("transitions")
    {
        transition["allowedBy"] = json!("ROLE:TI-D89-DECIDES");
    }
    let workflow = publish_workflow_definition(&app, &token, "ti_d89_decides", definition).await;
    let type_id = document_type(&app, &token, "TI_D89_DECIDES", workflow).await;
    let document = draft_document(&app, &token, type_id, "Decided by a role being deleted").await;

    let refused = delete_during_a_held_submission(&app, &token, document, edge).await;

    assert_refused_for_one_open_task(&app, &refused, edge, document).await;
}

async fn draft_document(app: &TestApp, token: &str, type_id: Uuid, title: &str) -> Uuid {
    let created = app
        .post(
            "/api/v1/documents",
            Some(token),
            json!({
                "documentTypeId": type_id,
                "title": title,
                "formData": { "amount": 1_000 },
            }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    id_of(&created.body["data"])
}

/// Submits `document` and deletes `role` through the real routes, in that
/// order, with the delete arriving while the submission holds its locks.
/// Asserts the delete waited on the submission, and that the submission went
/// through. Answers the delete's response.
///
/// **The interleave is forced, not timed.** A trigger on `workflow_tasks`, in
/// this test's own database, makes the task `INSERT` take a shared advisory
/// lock this helper holds exclusively on a connection of its own. So the
/// submission stops inside its transaction, after `assignment` has locked the
/// roles it resolves and before its task commits, for as long as the helper
/// says. The helper sees it stopped in `pg_locks`, sends the delete, and sees
/// the delete's `FOR UPDATE` on `roles` blocked by the submission's backend in
/// `pg_blocking_pids` before it lets the submission go.
async fn delete_during_a_held_submission(
    app: &Arc<TestApp>,
    token: &str,
    document: Uuid,
    role: Uuid,
) -> common::TestResponse {
    use sqlx::{Connection, PgConnection};
    use std::time::Duration;
    use tokio::time::Instant;

    /// The advisory key the trigger waits on; the issue's number.
    const GATE: i64 = 528;
    const PATIENCE: Duration = Duration::from_secs(30);

    sqlx::query(&format!(
        "CREATE FUNCTION test_hold_task_insert() RETURNS trigger LANGUAGE plpgsql AS $$
         BEGIN
             PERFORM pg_advisory_xact_lock_shared({GATE});
             RETURN NEW;
         END
         $$"
    ))
    .execute(&app.pool)
    .await
    .expect("the gate's function");
    sqlx::query(
        "CREATE TRIGGER test_hold_task_insert BEFORE INSERT ON workflow_tasks
         FOR EACH ROW EXECUTE FUNCTION test_hold_task_insert()",
    )
    .execute(&app.pool)
    .await
    .expect("the gate's trigger");

    // Off the pool, so the gate never competes with the requests for a
    // connection.
    let mut gate = PgConnection::connect_with(&app.pool.connect_options())
        .await
        .expect("the gate's connection");
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(GATE)
        .execute(&mut gate)
        .await
        .expect("close the gate");

    let submission = {
        let app = Arc::clone(app);
        let token = token.to_owned();
        tokio::spawn(async move {
            app.send(
                Method::POST,
                &format!("/api/v1/documents/{document}/submission"),
                Some(&token),
                None,
            )
            .await
        })
    };

    // The submission's backend, stopped at the gate inside its task INSERT.
    let deadline = Instant::now() + PATIENCE;
    let transition: i32 = loop {
        let waiting: Option<i32> = sqlx::query_scalar(
            "SELECT pid FROM pg_locks
             WHERE locktype = 'advisory' AND objid::text::bigint = $1 AND objsubid = 1
               AND NOT granted
               AND database = (SELECT oid FROM pg_database WHERE datname = current_database())",
        )
        .bind(GATE)
        .fetch_optional(&app.pool)
        .await
        .expect("read pg_locks");
        if let Some(pid) = waiting {
            break pid;
        }
        assert!(
            !submission.is_finished(),
            "the submission finished without reaching its task INSERT"
        );
        assert!(
            Instant::now() < deadline,
            "the submission never reached its task INSERT"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    };

    let delete = {
        let app = Arc::clone(app);
        let token = token.to_owned();
        tokio::spawn(async move { delete_role(&app, &token, role).await })
    };

    // The delete's backend, in its lock on the role, blocked by the
    // submission's.
    let deadline = Instant::now() + PATIENCE;
    loop {
        let waiting: Option<i32> = sqlx::query_scalar(
            "SELECT pid FROM pg_stat_activity
             WHERE datname = current_database() AND $1 = ANY(pg_blocking_pids(pid))
               AND query LIKE '%FROM roles%' AND query LIKE '%FOR UPDATE%'",
        )
        .bind(transition)
        .fetch_optional(&app.pool)
        .await
        .expect("read pg_stat_activity");
        if waiting.is_some() {
            break;
        }
        if delete.is_finished() {
            let answered = delete.await.expect("the delete did not panic");
            panic!(
                "the delete did not wait for the transition holding the role: {} {}",
                answered.status, answered.body
            );
        }
        assert!(
            Instant::now() < deadline,
            "the delete was never seen waiting on the transition in its lock on the role"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(GATE)
        .execute(&mut gate)
        .await
        .expect("open the gate");

    let submitted = tokio::time::timeout(PATIENCE, submission)
        .await
        .expect("the submission did not finish once the gate opened")
        .expect("the submission did not panic");
    assert_eq!(submitted.status, StatusCode::OK, "{}", submitted.body);

    tokio::time::timeout(PATIENCE, delete)
        .await
        .expect("the delete did not finish once the submission committed")
        .expect("the delete did not panic")
}

/// The delete was refused for the one task the submission raised, the role is
/// live, and that task is open. Only the count and *open task* are asserted
/// of the message, which #529 rewords.
async fn assert_refused_for_one_open_task(
    app: &TestApp,
    refused: &common::TestResponse,
    role: Uuid,
    document: Uuid,
) {
    assert_eq!(
        refused.status,
        StatusCode::CONFLICT,
        "the delete went through beside a task it did not count: {}",
        refused.body
    );
    assert!(
        refused.body.to_string().contains("1 open task"),
        "{}",
        refused.body
    );

    let live: bool = sqlx::query_scalar("SELECT deleted_at IS NULL FROM roles WHERE id = $1")
        .bind(role)
        .fetch_one(&app.pool)
        .await
        .expect("read the role");
    assert!(live, "the refused delete deleted the role");

    let open: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM workflow_tasks
         WHERE document_id = $1 AND status IN ('CREATED','ASSIGNED','IN_PROGRESS')",
    )
    .bind(document)
    .fetch_one(&app.pool)
    .await
    .expect("count the open tasks");
    assert_eq!(open, 1, "the submission's task is open");
}

// ---------------------------------------------------------------------------
// #511, #529, #533 — the stranded-task query in Installation and Deployment §9
// ---------------------------------------------------------------------------

/// The query Installation and Deployment §9 prints for tasks stranded on a
/// role deleted before **D-89**, read from the document itself, so the test
/// runs what an operator would paste.
fn stranded_task_query() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../docs/operations/01. Installation and Deployment.md");
    let document = std::fs::read_to_string(&path).expect("Installation and Deployment reads");

    let entry = document
        .find("**A document sits in `PENDING_APPROVAL`")
        .expect("§9 still has the stranded-task entry");
    let rest = &document[entry..];
    let fence = rest.find("```sql").expect("the entry prints a query");
    let body = &rest[fence..];
    let start = body.find('\n').expect("the fence ends its line") + 1;
    let end = body[start..].find("```").expect("the query's fence closes");

    body[start..start + end].to_string()
}

/// A tenant's administrator, holding every permission in the catalogue in
/// that tenant, signed in there.
async fn tenant_administrator(app: &TestApp, tenant_code: &str, username: &str) -> (Uuid, String) {
    let tenant = fixtures::create_tenant(&app.pool, tenant_code, "Another Customer").await;

    let codes: Vec<String> =
        sqlx::query_scalar("SELECT permission_code FROM permissions WHERE deleted_at IS NULL")
            .fetch_all(&app.pool)
            .await
            .expect("read the permission catalogue");
    let codes: Vec<&str> = codes.iter().map(String::as_str).collect();
    let role = fixtures::create_role_with_permissions(&app.pool, tenant, "TI-ALL", &codes).await;

    fixtures::create_user(
        &app.pool,
        tenant,
        username,
        &format!("{username}@example.test"),
        common::ADMIN_PASSWORD,
        &[role],
    )
    .await;

    let token = app
        .sign_in_to(tenant_code, username, common::ADMIN_PASSWORD)
        .await;

    (tenant, token)
}

async fn claim(app: &TestApp, token: &str, task: Uuid) {
    let claimed = app
        .post(
            &format!("/api/v1/workflow/tasks/{task}/claim"),
            Some(token),
            json!({}),
        )
        .await;
    assert_eq!(claimed.status, StatusCode::OK, "{}", claimed.body);
}

/// **The stranded-task query lists what nobody can decide, and says whose it
/// is** ([#529], [#533]).
///
/// The query spans every tenant, so it is run on a multi-tenant deployment with
/// a second tenant. Each row names the tenant and the deleted role's **id**,
/// which the repair's first step needs. Seven documents in the system tenant,
/// each with one open task. Their roles are soft-deleted in SQL, as a role was
/// deleted before D-89, except where a row says the role is live:
///
/// * **P2** (record 18): offered to `Q`, edges `allowedBy` `E`, claimed; `Q`
///   deleted. **Not listed**, and its assignee decides it with 200.
/// * **Claimed, one role**: offered to `R` and `allowedBy` `R`, claimed; `R`
///   deleted. **Listed**, through the edge, and its decision is refused as
///   `ASSIGNMENT_UNRESOLVED`, which is the P2 control. The second tenant has a
///   **live** `R` of its own, which must not rescue it.
/// * **Unclaimed**: offered to `S` and `allowedBy` `S`; `S` deleted. **Listed**,
///   as offered to the role.
/// * **Live**: offered to `L` and `allowedBy` `L`, unclaimed; `L` live. **Not
///   listed.**
/// * **Recreated**: offered to `T` and `allowedBy` `T`, claimed; `T` deleted
///   and a live role created with its code. **Not listed**: the edge resolves
///   by code, to the new role.
/// * **Two approvals** on one definition, manager `M` (live) then finance `F`,
///   `F` deleted. One document waits at the manager: **not listed**, since only
///   a later state's edges name `F`. The other was approved by the manager and
///   waits at finance: **listed once**, and not again for the completed manager
///   task.
///
/// The second tenant has its own `S`, deleted, and an unclaimed task on a type
/// of the same code, so its document carries **the same number** as the system
/// tenant's. Both rows are listed, told apart by `tenant_code` and `role_id`.
///
/// **Seen red** against the query in the document:
/// * as #517 printed it, which has neither column;
/// * with the new columns and #517's predicate (`candidate_role_id` claimed or
///   not), which lists the P2 task;
/// * without `r.deleted_at IS NOT NULL`, which lists the tasks on live roles;
/// * without the anti-join, which lists the recreated one;
/// * without `live.tenant_id = r.tenant_id`, which drops the claimed-one-role
///   row;
/// * without `tr.from_state = i.current_state`, which lists the document at the
///   manager;
/// * without the status filter, which lists the finance document twice.
///
/// [#529]: https://github.com/sujanto-gaws/kelir/issues/529
/// [#533]: https://github.com/sujanto-gaws/kelir/issues/533
#[tokio::test]
async fn the_stranded_task_query_lists_what_nobody_can_decide_and_says_whose() {
    let app = TestApp::spawn_with(|config| config.multi_tenant = true).await;
    let token = app
        .sign_in_to("SYSTEM", common::ADMIN_USERNAME, common::ADMIN_PASSWORD)
        .await;

    let permissions = &[
        "workflow:task:read",
        "workflow:task:execute",
        "workflow:instance:read",
        "document:read",
    ];
    let mut roles = Vec::new();
    for code in ["TI-STRAND-Q", "TI-STRAND-E", "TI-STRAND-R", "TI-STRAND-S"] {
        roles.push(
            fixtures::create_role_with_permissions(
                &app.pool,
                fixtures::SYSTEM_TENANT_ID,
                code,
                permissions,
            )
            .await,
        );
    }
    let (q, e, r, s) = (roles[0], roles[1], roles[2], roles[3]);
    let mut more = Vec::new();
    for code in ["TI-STRAND-L", "TI-STRAND-T", "TI-STRAND-M", "TI-STRAND-F"] {
        more.push(
            fixtures::create_role_with_permissions(
                &app.pool,
                fixtures::SYSTEM_TENANT_ID,
                code,
                permissions,
            )
            .await,
        );
    }
    let (t, m, f) = (more[1], more[2], more[3]);
    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "ti.strand.approver",
        "ti.strand.approver@example.test",
        common::ADMIN_PASSWORD,
        &[q, e, r, t, m],
    )
    .await;
    let approver = app
        .sign_in_to("SYSTEM", "ti.strand.approver", common::ADMIN_PASSWORD)
        .await;

    let mut p2 = workflow_for("ti_strand_p2", "TI-STRAND-Q");
    for transition in p2["transitions"].as_array_mut().expect("transitions") {
        transition["allowedBy"] = json!("ROLE:TI-STRAND-E");
    }
    let p2 = publish_workflow_definition(&app, &token, "ti_strand_p2", p2).await;
    let p2 = document_type(&app, &token, "TI_STRAND_P2", p2).await;
    let p2 = submitted_document(&app, &token, p2, "P2: claimed, offered role deleted").await;
    let p2 = open_task_of(&app, p2).await;
    claim(&app, &approver, p2).await;

    let one = publish_workflow(&app, &token, "ti_strand_one", "TI-STRAND-R").await;
    let one = document_type(&app, &token, "TI_STRAND_ONE", one).await;
    let one = submitted_document(&app, &token, one, "Claimed: its edge role deleted").await;
    let one = open_task_of(&app, one).await;
    claim(&app, &approver, one).await;

    let queue = publish_workflow(&app, &token, "ti_strand_queue", "TI-STRAND-S").await;
    let queue = document_type(&app, &token, "TI_STRAND", queue).await;
    submitted_document(&app, &token, queue, "Unclaimed: its role deleted").await;

    let live = publish_workflow(&app, &token, "ti_strand_live", "TI-STRAND-L").await;
    let live = document_type(&app, &token, "TI_STRAND_LIVE", live).await;
    submitted_document(&app, &token, live, "Unclaimed: its role live").await;

    let recreated = publish_workflow(&app, &token, "ti_strand_again", "TI-STRAND-T").await;
    let recreated = document_type(&app, &token, "TI_STRAND_AGAIN", recreated).await;
    let recreated =
        submitted_document(&app, &token, recreated, "Claimed: its edge role recreated").await;
    let recreated = open_task_of(&app, recreated).await;
    claim(&app, &approver, recreated).await;

    let stages = two_stage("ti_strand_stages", "TI-STRAND-M", "TI-STRAND-F");
    let stages = publish_workflow_definition(&app, &token, "ti_strand_stages", stages).await;
    let stages = document_type(&app, &token, "TI_STRAND_STAGES", stages).await;
    submitted_document(&app, &token, stages, "At the manager: finance deleted").await;
    let at_finance = submitted_document(&app, &token, stages, "At finance: finance deleted").await;
    let manager_task = open_task_of(&app, at_finance).await;
    decide(&app, &approver, manager_task).await;

    let (other, other_token) = tenant_administrator(&app, "TNT-STRAND", "ti.strand.other").await;
    let other_s =
        fixtures::create_role_with_permissions(&app.pool, other, "TI-STRAND-S", &[]).await;
    let other_queue = publish_workflow(&app, &other_token, "ti_strand_queue", "TI-STRAND-S").await;
    let other_queue = document_type(&app, &other_token, "TI_STRAND", other_queue).await;
    submitted_document(
        &app,
        &other_token,
        other_queue,
        "Unclaimed in another tenant",
    )
    .await;
    fixtures::create_role_with_permissions(&app.pool, other, "TI-STRAND-R", &[]).await;

    sqlx::query("UPDATE roles SET deleted_at = now() WHERE id = ANY($1)")
        .bind(vec![q, r, s, t, f, other_s])
        .execute(&app.pool)
        .await
        .expect("delete the roles as a release before D-89 did");
    fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "TI-STRAND-T",
        &[],
    )
    .await;

    let rows = sqlx::query(&stranded_task_query())
        .fetch_all(&app.pool)
        .await
        .expect("the query as printed runs");

    use sqlx::Row;
    let listed: Vec<(String, String, String, Uuid, String)> = rows
        .iter()
        .map(|row| {
            (
                row.get("tenant_code"),
                row.get::<Option<String>, _>("document_number")
                    .expect("a submitted document has a number"),
                row.get("title"),
                row.get("role_id"),
                row.get("why"),
            )
        })
        .collect();

    assert!(
        !listed
            .iter()
            .any(|(_, _, title, _, _)| title.starts_with("P2")),
        "a claimed task offered to a deleted role was listed as stranded, and its assignee can \
         decide it: {listed:#?}"
    );

    let mut found: Vec<(&str, &str, Uuid, &str)> = listed
        .iter()
        .map(|(tenant, _, title, role, why)| (tenant.as_str(), title.as_str(), *role, why.as_str()))
        .collect();
    found.sort();
    assert_eq!(
        found,
        vec![
            (
                "SYSTEM",
                "At finance: finance deleted",
                f,
                "offered to the role, and unclaimed"
            ),
            (
                "SYSTEM",
                "Claimed: its edge role deleted",
                r,
                "a decision out of MANAGER_APPROVAL is allowedBy the role"
            ),
            (
                "SYSTEM",
                "Unclaimed: its role deleted",
                s,
                "offered to the role, and unclaimed"
            ),
            (
                "TNT-STRAND",
                "Unclaimed in another tenant",
                other_s,
                "offered to the role, and unclaimed"
            ),
        ],
        "{listed:#?}"
    );

    let number_of = |tenant: &str| {
        listed
            .iter()
            .find(|(t, _, title, _, _)| t == tenant && title.starts_with("Unclaimed"))
            .map(|(_, number, _, _, _)| number.clone())
            .expect("listed")
    };
    assert_eq!(
        number_of("SYSTEM"),
        number_of("TNT-STRAND"),
        "the fixture meant two tenants' rows to share a document number"
    );

    // What the list claims, checked against the engine. The P2 task's assignee
    // decides it; the claimed task whose edge role is gone is refused.
    decide(&app, &approver, p2).await;

    let refused = app
        .post(
            &format!("/api/v1/workflow/tasks/{one}/decision"),
            Some(&approver),
            json!({ "action": "APPROVE" }),
        )
        .await;
    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a listed task was decidable: {}",
        refused.body
    );
    assert!(
        refused.body.to_string().contains("ASSIGNMENT_UNRESOLVED"),
        "{}",
        refused.body
    );
}
