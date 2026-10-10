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
        "/api/v1/workflow/tasks/{id}/reassign",
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
/// The open task is asked about first, and its refusal is D-89's
/// `ROLE_HAS_OPEN_TASKS`.
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
        refused.body["error"]["code"], "ROLE_HAS_OPEN_TASKS",
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
         offered to nobody, or with a decision nobody could make. They need to be decided or \
         reassigned first",
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
/// counted wrongly would turn either refusal into D-89's `ROLE_HAS_OPEN_TASKS`.
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
        refused.body["error"]["code"], "ROLE_HAS_OPEN_TASKS",
        "{}",
        refused.body
    );
    assert_eq!(
        refused.body["error"]["message"], ONE_OPEN_TASK,
        "the open task did not hold the role its DEPARTMENT_ROLE edge names: {}",
        refused.body
    );
}

// ---------------------------------------------------------------------------
// #532 — the refusal names its tasks: GET /identity/roles/{id}/open-tasks
// ---------------------------------------------------------------------------

// # Seen to fail (coding standard §2.9)
//
// Mutations run 2026-09-26, each against the shared statement
// `repository::task::open_tasks_needing_role` or its service, reverted after:
//
// | Mutation | Reddened |
// |---|---|
// | Both tenant predicates dropped (`t.tenant_id = $1` and `r.tenant_id = t.tenant_id`) | *another tenant's task is not listed*: the other tenant's task is listed, and counted |
// | The clauses' `OR` turned into `AND` | *the list is exactly what the delete counts…*: `queue` lists nothing and its delete answers 204 |
// | `t.assignee_user_id IS NULL` dropped from clause (a) | *the list is exactly what the delete counts…*: `queue` lists the claimed task too |
// | `identity:role:delete` widened to `identity:role:read` | *only identity:role:delete reads the list*: 200 to a caller without it |
// | `ROLE_HAS_OPEN_TASKS` put back to `CONFLICT` | *the list is exactly…* and *a role with an open task is not deleted…* |
// | `meta.total` taken from the page's length | *the list pages and its total is every task*: page 1 says 2 |
//
// **Both predicates are dropped because either one alone holds the line**:
// `r.tenant_id = t.tenant_id` with `r.id = $2` keeps the task in the role's
// tenant, which the service has already checked is the caller's. So a
// mutation of one would stay green by design, not by a gap. D-89's
// *a role is not held by another tenant's task* was seen red the same way.
//
// **`meta.total` and the rows come from one `fetch_all`** of one statement,
// `open_tasks_needing_role`; a total from a second query in the same snapshot
// cannot be told apart by a test, so the page-length mutation stands in for it.

/// The open tasks a delete of `role` waits on, one page of them.
async fn open_tasks(app: &TestApp, token: &str, role: Uuid, query: &str) -> common::TestResponse {
    app.get(
        &format!("/api/v1/identity/roles/{role}/open-tasks?{query}"),
        Some(token),
    )
    .await
}

/// The number a `ROLE_HAS_OPEN_TASKS` refusal leads its message with.
fn refused_count(refused: &common::TestResponse) -> u64 {
    assert_eq!(
        refused.body["error"]["code"], "ROLE_HAS_OPEN_TASKS",
        "{}",
        refused.body
    );
    refused.body["error"]["message"]
        .as_str()
        .and_then(|message| message.split(' ').next())
        .and_then(|count| count.parse().ok())
        .unwrap_or_else(|| panic!("the refusal leads with its count: {}", refused.body))
}

async fn task_ref_of(app: &TestApp, task: Uuid) -> String {
    sqlx::query_scalar("SELECT task_ref FROM workflow_tasks WHERE id = $1")
        .bind(task)
        .fetch_one(&app.pool)
        .await
        .expect("read the task_ref")
}

/// **The list is exactly what the delete counts**, row for row, through both
/// clauses ([#532] AC3, AC4, AC7).
///
/// Record 18's P2 shape: tasks offered to `queue` with edges `allowedBy`
/// `edge`. One is claimed, one is not.
///
/// * `queue` lists only the **unclaimed** task, through clause (a). The claimed
///   one was offered to `queue` too and is absent (AC4).
/// * `edge` lists **both**, through clause (b), the claimed one with its
///   holder's id and name.
///
/// Each list's length and `meta.total` equal the count the delete's
/// `ROLE_HAS_OPEN_TASKS` refusal reports, and each row's `why` is the §9
/// query's wording.
///
/// [#532]: https://github.com/sujanto-gaws/kelir/issues/532
#[tokio::test]
async fn the_list_is_exactly_what_the_delete_counts_through_both_clauses() {
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
        "TI-532-QUEUE",
        permissions,
    )
    .await;
    let edge = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "TI-532-EDGE",
        permissions,
    )
    .await;
    let holder_id = fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "ti.532.both",
        "ti.532.both@example.test",
        common::ADMIN_PASSWORD,
        &[queue, edge],
    )
    .await;
    let approver = app.sign_in("ti.532.both", common::ADMIN_PASSWORD).await;

    let mut definition = workflow_for("ti_532_edge", "TI-532-QUEUE");
    for transition in definition["transitions"]
        .as_array_mut()
        .expect("transitions")
    {
        transition["allowedBy"] = json!("ROLE:TI-532-EDGE");
    }
    let workflow = publish_workflow_definition(&app, &token, "ti_532_edge", definition).await;
    let type_id = document_type(&app, &token, "TI_532_EDGE", workflow).await;

    let claimed_document = submitted_document(&app, &token, type_id, "Claimed").await;
    let claimed = open_task_of(&app, claimed_document).await;
    let unclaimed_document = submitted_document(&app, &token, type_id, "Unclaimed").await;
    let unclaimed = open_task_of(&app, unclaimed_document).await;
    claim(&app, &approver, claimed).await;

    let claimed_ref = task_ref_of(&app, claimed).await;
    let unclaimed_ref = task_ref_of(&app, unclaimed).await;

    // `queue`: clause (a) only, and not the claimed task.
    let listed = open_tasks(&app, &token, queue, "").await;
    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);
    let rows = listed.body["data"].as_array().expect("a page");
    let count = refused_count(&delete_role(&app, &token, queue).await);
    assert_eq!(
        rows.len() as u64,
        count,
        "the list and the count differ: {}",
        listed.body
    );
    assert_eq!(listed.body["meta"]["total"], count, "{}", listed.body);
    assert_eq!(count, 1, "{}", listed.body);

    let row = &rows[0];
    assert_eq!(row["taskRef"], unclaimed_ref.as_str(), "{}", listed.body);
    // The id the task routes take, such as row 10's reassign.
    assert_eq!(row["id"], unclaimed.to_string(), "{}", listed.body);
    assert_eq!(row["documentTitle"], "Unclaimed", "{}", listed.body);
    assert!(row["documentNumber"].is_string(), "{}", listed.body);
    assert_eq!(row["currentState"], "MANAGER_APPROVAL", "{}", listed.body);
    assert_eq!(
        row["why"], "offered to the role, and unclaimed",
        "{}",
        listed.body
    );
    assert!(row["assigneeUserId"].is_null(), "{}", listed.body);
    assert!(row["assigneeDisplayName"].is_null(), "{}", listed.body);
    // Nothing beyond the fields that explain the refusal.
    let mut fields: Vec<&str> = row
        .as_object()
        .expect("a row")
        .keys()
        .map(String::as_str)
        .collect();
    fields.sort_unstable();
    assert_eq!(
        fields,
        [
            "assigneeDisplayName",
            "assigneeUserId",
            "currentState",
            "documentNumber",
            "documentTitle",
            "id",
            "status",
            "taskRef",
            "why"
        ],
        "{}",
        listed.body
    );

    // `edge`: clause (b), claimed or not.
    let listed = open_tasks(&app, &token, edge, "").await;
    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);
    let rows = listed.body["data"].as_array().expect("a page");
    let count = refused_count(&delete_role(&app, &token, edge).await);
    assert_eq!(
        rows.len() as u64,
        count,
        "the list and the count differ: {}",
        listed.body
    );
    assert_eq!(listed.body["meta"]["total"], count, "{}", listed.body);
    assert_eq!(count, 2, "{}", listed.body);

    let held = rows
        .iter()
        .find(|row| row["taskRef"] == claimed_ref.as_str())
        .unwrap_or_else(|| {
            panic!(
                "the claimed task is not listed for its edge's role: {}",
                listed.body
            )
        });
    assert_eq!(
        held["assigneeUserId"],
        holder_id.to_string(),
        "{}",
        listed.body
    );
    assert_eq!(
        held["assigneeDisplayName"], "ti.532.both",
        "{}",
        listed.body
    );
    assert_eq!(held["id"], claimed.to_string(), "{}", listed.body);
    assert_eq!(held["documentTitle"], "Claimed", "{}", listed.body);
    assert_eq!(held["status"], "ASSIGNED", "{}", listed.body);
    for row in rows {
        assert_eq!(
            row["why"], "a decision out of MANAGER_APPROVAL is allowedBy the role",
            "{}",
            listed.body
        );
    }
}

/// **A task that needs the role both ways says both** ([#532] AC4).
///
/// The usual shape: the task is offered to the role, and its edges are
/// `allowedBy` the same role. Unclaimed, it needs the role through clause (a)
/// and clause (b) at once, and its `why` names the two. Claimed, clause (a)
/// lets go (#529) and only the edge is left.
///
/// **Seen red** with the `CASE` in `open_tasks_needing_role` asking `named`
/// first, so the edge's reason wins: the unclaimed task said only *a decision
/// out of MANAGER_APPROVAL is allowedBy the role*.
///
/// [#532]: https://github.com/sujanto-gaws/kelir/issues/532
#[tokio::test]
async fn a_task_needing_the_role_both_ways_says_both() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let (role, approver) = holder(&app, "TI-532-BOTH", "ti.532.both.ways").await;
    let workflow = publish_workflow(&app, &token, "ti_532_both", "TI-532-BOTH").await;
    let type_id = document_type(&app, &token, "TI_532_BOTH", workflow).await;
    let document = submitted_document(&app, &token, type_id, "Both ways").await;
    let task = open_task_of(&app, document).await;

    let listed = open_tasks(&app, &token, role, "").await;
    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);
    assert_eq!(listed.body["meta"]["total"], 1, "{}", listed.body);
    let row = &listed.body["data"][0];
    assert_eq!(row["id"], task.to_string(), "{}", listed.body);
    assert_eq!(
        row["why"],
        "offered to the role, and unclaimed, and a decision out of MANAGER_APPROVAL is \
         allowedBy the role",
        "an unclaimed task offered to the role its edges name did not say both: {}",
        listed.body
    );

    claim(&app, &approver, task).await;

    let listed = open_tasks(&app, &token, role, "").await;
    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);
    assert_eq!(listed.body["meta"]["total"], 1, "{}", listed.body);
    assert_eq!(
        listed.body["data"][0]["why"], "a decision out of MANAGER_APPROVAL is allowedBy the role",
        "a claimed task still said it was offered: {}",
        listed.body
    );
}

/// **A system role is refused as a system role, open tasks or not** ([#532]).
///
/// `ROLE-ADMIN` is offered a task, so the open-task refusal would apply too.
/// The system role's refusal comes first and answers `CONFLICT`, not
/// `ROLE_HAS_OPEN_TASKS`: a client must not offer the list of tasks for a role
/// that cannot be deleted whatever happens to them.
///
/// **Seen red** with `delete_role`'s system-role check moved after the
/// open-task count: the refusal was `ROLE_HAS_OPEN_TASKS`.
///
/// [#532]: https://github.com/sujanto-gaws/kelir/issues/532
#[tokio::test]
async fn a_system_role_open_tasks_need_is_refused_as_a_system_role() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let admin_role = fixtures::ADMIN_ROLE_ID;

    let workflow = publish_workflow(&app, &token, "ti_532_system", "ROLE-ADMIN").await;
    let type_id = document_type(&app, &token, "TI_532_SYSTEM", workflow).await;
    submitted_document(&app, &token, type_id, "Waiting on the administrators").await;

    // The open-task refusal would apply: the role is needed.
    let listed = open_tasks(&app, &token, admin_role, "").await;
    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);
    assert_eq!(listed.body["meta"]["total"], 1, "{}", listed.body);

    let refused = delete_role(&app, &token, admin_role).await;
    assert_eq!(refused.status, StatusCode::CONFLICT, "{}", refused.body);
    assert_eq!(
        refused.body["error"]["code"], "CONFLICT",
        "a system role was refused for its open tasks, not as a system role: {}",
        refused.body
    );

    let still = app
        .get(
            &format!("/api/v1/identity/roles/{admin_role}"),
            Some(&token),
        )
        .await;
    assert_eq!(still.status, StatusCode::OK, "{}", still.body);
}

/// **Only `identity:role:delete` reads the list** ([#532] AC1, AC9).
///
/// A caller holding every other permission in the catalogue, including
/// `workflow:task:read` and `identity:role:read`, is refused with 403. A caller
/// holding `identity:role:delete` alone reads it.
///
/// [#532]: https://github.com/sujanto-gaws/kelir/issues/532
#[tokio::test]
async fn only_identity_role_delete_reads_the_list() {
    let app = TestApp::spawn().await;

    let target = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "TI-532-T",
        &[],
    )
    .await;

    let codes: Vec<String> = sqlx::query_scalar(
        "SELECT permission_code FROM permissions
         WHERE deleted_at IS NULL AND permission_code <> 'identity:role:delete'",
    )
    .fetch_all(&app.pool)
    .await
    .expect("read the permission catalogue");
    assert!(codes.iter().any(|code| code == "workflow:task:read"));
    assert!(codes.iter().any(|code| code == "identity:role:read"));
    let codes: Vec<&str> = codes.iter().map(String::as_str).collect();

    let everything_else = holder_of(&app, "TI-532-ELSE", "ti.532.else", &codes).await;
    let refused = open_tasks(&app, &everything_else, target, "").await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN, "{}", refused.body);

    let deleter = holder_of(&app, "TI-532-DEL", "ti.532.del", &["identity:role:delete"]).await;
    let read = open_tasks(&app, &deleter, target, "").await;
    assert_eq!(read.status, StatusCode::OK, "{}", read.body);
    assert_eq!(read.body["meta"]["total"], 0, "{}", read.body);
}

/// **An unknown role, another tenant's, or a deleted one answers 404** ([#532]
/// AC2), in the shape `get_role` answers it.
///
/// A deleted role is 404 by decision: tasks stranded on a role deleted before
/// D-89 are Installation and Deployment §9's query to find.
///
/// [#532]: https://github.com/sujanto-gaws/kelir/issues/532
#[tokio::test]
async fn an_unknown_a_foreign_or_a_deleted_role_is_not_found() {
    let app = TestApp::spawn_with(|config| config.multi_tenant = true).await;
    let token = app
        .sign_in_to("SYSTEM", common::ADMIN_USERNAME, common::ADMIN_PASSWORD)
        .await;

    let (other, _) = tenant_administrator(&app, "TNT-532", "ti.532.other").await;
    let foreign = fixtures::create_role_with_permissions(&app.pool, other, "TI-532-F", &[]).await;

    let deleted = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "TI-532-D",
        &[],
    )
    .await;
    let gone = delete_role(&app, &token, deleted).await;
    assert_eq!(gone.status, StatusCode::NO_CONTENT, "{}", gone.body);

    for (what, role) in [
        ("an unknown role", Uuid::now_v7()),
        ("another tenant's role", foreign),
        ("a deleted role", deleted),
    ] {
        let listed = open_tasks(&app, &token, role, "").await;
        let read = app
            .get(&format!("/api/v1/identity/roles/{role}"), Some(&token))
            .await;
        assert_eq!(
            listed.status,
            StatusCode::NOT_FOUND,
            "{what}: {}",
            listed.body
        );
        assert_eq!(read.status, StatusCode::NOT_FOUND, "{what}: {}", read.body);
        assert_eq!(
            listed.body["error"], read.body["error"],
            "{what}: the 404s differ"
        );
    }
}

/// **Another tenant's task is not listed**, though it names a role of the
/// same code in its offer and its edges ([#532] AC6).
///
/// The system tenant's `TI-532-XT` is live, so the #533 anti-join has nothing
/// to do here: a live role is its own live namesake.
///
/// [#532]: https://github.com/sujanto-gaws/kelir/issues/532
#[tokio::test]
async fn another_tenants_task_is_not_listed() {
    let app = TestApp::spawn_with(|config| config.multi_tenant = true).await;
    let token = app
        .sign_in_to("SYSTEM", common::ADMIN_USERNAME, common::ADMIN_PASSWORD)
        .await;

    let (other, other_token) = tenant_administrator(&app, "TNT-532X", "ti.532.xt").await;
    fixtures::create_role_with_permissions(&app.pool, other, "TI-532-XT", &[]).await;
    let workflow = publish_workflow(&app, &other_token, "ti_532_xt", "TI-532-XT").await;
    let type_id = document_type(&app, &other_token, "TI_532_XT", workflow).await;
    submitted_document(&app, &other_token, type_id, "Open in another tenant").await;

    let ours = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "TI-532-XT",
        &[],
    )
    .await;

    let listed = open_tasks(&app, &token, ours, "").await;
    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);
    assert_eq!(
        listed.body["data"].as_array().expect("a page").len(),
        0,
        "another tenant's task was listed: {}",
        listed.body
    );
    assert_eq!(listed.body["meta"]["total"], 0, "{}", listed.body);
}

/// **The list pages, and its total is every task** ([#532] AC5): three tasks,
/// pages of two, `meta.total` 3 on each page, past the end included, and a
/// page size over the limit clamped to 100.
///
/// [#532]: https://github.com/sujanto-gaws/kelir/issues/532
#[tokio::test]
async fn the_list_pages_and_its_total_is_every_task() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let (role, _) = holder(&app, "TI-532-PAGE", "ti.532.page").await;
    let workflow = publish_workflow(&app, &token, "ti_532_page", "TI-532-PAGE").await;
    let type_id = document_type(&app, &token, "TI_532_PAGE", workflow).await;
    for title in ["First", "Second", "Third"] {
        submitted_document(&app, &token, type_id, title).await;
    }

    let mut seen = Vec::new();
    for (page, expected) in [(1, 2), (2, 1), (3, 0)] {
        let listed = open_tasks(&app, &token, role, &format!("page={page}&pageSize=2")).await;
        assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);
        let rows = listed.body["data"].as_array().expect("a page");
        assert_eq!(rows.len(), expected, "page {page}: {}", listed.body);
        assert_eq!(
            listed.body["meta"]["total"], 3,
            "page {page}: {}",
            listed.body
        );
        assert_eq!(listed.body["meta"]["pageSize"], 2, "{}", listed.body);
        seen.extend(rows.iter().map(|row| row["documentTitle"].clone()));
    }
    assert_eq!(
        seen,
        [json!("First"), json!("Second"), json!("Third")],
        "oldest first, each once"
    );

    let clamped = open_tasks(&app, &token, role, "pageSize=500").await;
    assert_eq!(clamped.body["meta"]["pageSize"], 100, "{}", clamped.body);
    assert_eq!(clamped.body["meta"]["total"], 3, "{}", clamped.body);
}

/// A user holding a role of `permissions`, signed in.
async fn holder_of(app: &TestApp, role_code: &str, username: &str, permissions: &[&str]) -> String {
    let role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        role_code,
        permissions,
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

async fn delete_role(app: &TestApp, token: &str, role: Uuid) -> common::TestResponse {
    app.delete(&format!("/api/v1/identity/roles/{role}"), Some(token))
        .await
}

/// D-89's refusal for one open task. It is asked about first, and answers
/// `ROLE_HAS_OPEN_TASKS`.
const ONE_OPEN_TASK: &str = "1 open task needs this role to be decided. Deleting the role would \
                             leave it offered to nobody, or with a decision nobody could make. \
                             It needs to be decided or reassigned first";

/// `a_claimed_task_needs_only_the_role_its_transitions_name`'s definition, as
/// a refusal names it.
const EDGE_DEFINITION: &str = "ti_d89_edge (\"Standard approval\", revision 1)";

/// The **D-91** (3) refusal for a role one published definition names, given
/// that definition as the refusal names it.
fn named_by(definition: &str) -> String {
    format!(
        "This role is named by 1 published workflow definition: {definition}. Deleting the \
         role would leave it unable to raise its tasks. Publish a revision that does not name \
         the role, bind its document types to that revision, and deprecate it if it is still \
         published. A deprecated revision stops holding the role once its running approvals \
         are finished"
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
         types to those revisions, and deprecate any still published. A deprecated revision \
         stops holding the role once its running approvals are finished",
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
/// [`second_waits_on_first`] forces the order: the submission stops at its
/// task `INSERT`, after `assignment` has locked the roles it resolves and
/// before its task commits, and the delete is seen blocked in its
/// `FOR UPDATE` on `roles`.
async fn delete_during_a_held_submission(
    app: &Arc<TestApp>,
    token: &str,
    document: Uuid,
    role: Uuid,
) -> common::TestResponse {
    let (submitted, deleted) = second_waits_on_first(
        app,
        Gate {
            table: "workflow_tasks",
            event: "INSERT",
        },
        ("the submission", submission(app, token, document)),
        ("the delete", || spawn_delete_role(app, token, role)),
        "FOR UPDATE",
    )
    .await;

    assert_eq!(submitted.status, StatusCode::OK, "{}", submitted.body);

    deleted
}

/// Where [`second_waits_on_first`] stops the first request: before any row of
/// `event` on `table` in its transaction, in this test's own database.
struct Gate {
    table: &'static str,
    event: &'static str,
}

/// Sends `first`, holds it inside its transaction at `gate`, sends `second`,
/// and answers both once `second` has been seen blocked by `first` in a
/// statement on `roles` taking `lock`. Each is named for the panics.
///
/// **The interleave is forced, not timed** (#528). A trigger at the gate makes
/// the first request's statement take a shared advisory lock this helper holds
/// exclusively on a connection of its own. So the first request stops there,
/// holding whatever it locked before the statement, for as long as the helper
/// says. The helper sees it stopped in `pg_locks`, sends the second, and sees
/// the second's backend in `pg_blocking_pids` behind the first's before it
/// opens the gate. `first` is spawned after the gate is closed, so it is a
/// closure too.
async fn second_waits_on_first<First, Second>(
    app: &Arc<TestApp>,
    gate: Gate,
    (first_name, first): (&str, First),
    (second_name, second): (&str, Second),
    lock: &str,
) -> (common::TestResponse, common::TestResponse)
where
    First: FnOnce() -> tokio::task::JoinHandle<common::TestResponse>,
    Second: FnOnce() -> tokio::task::JoinHandle<common::TestResponse>,
{
    use sqlx::{Connection, PgConnection};
    use std::time::Duration;
    use tokio::time::Instant;

    /// The advisory key the trigger waits on; the issue's number.
    const GATE: i64 = 528;
    const PATIENCE: Duration = Duration::from_secs(30);

    sqlx::query(&format!(
        "CREATE FUNCTION test_gate() RETURNS trigger LANGUAGE plpgsql AS $$
         BEGIN
             PERFORM pg_advisory_xact_lock_shared({GATE});
             RETURN NEW;
         END
         $$"
    ))
    .execute(&app.pool)
    .await
    .expect("the gate's function");
    sqlx::query(&format!(
        "CREATE TRIGGER test_gate BEFORE {} ON {}
         FOR EACH ROW EXECUTE FUNCTION test_gate()",
        gate.event, gate.table
    ))
    .execute(&app.pool)
    .await
    .expect("the gate's trigger");

    // Off the pool, so the gate never competes with the requests for a
    // connection.
    let mut gate_connection = PgConnection::connect_with(&app.pool.connect_options())
        .await
        .expect("the gate's connection");
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(GATE)
        .execute(&mut gate_connection)
        .await
        .expect("close the gate");

    let first = first();

    // The first request's backend, stopped at the gate.
    let deadline = Instant::now() + PATIENCE;
    let held: i32 = loop {
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
        if first.is_finished() {
            let answered = first.await.expect("the first request did not panic");
            panic!(
                "{first_name} finished without reaching the gate on {} {}: {} {}",
                gate.event, gate.table, answered.status, answered.body
            );
        }
        assert!(
            Instant::now() < deadline,
            "{first_name} never reached the gate on {} {}",
            gate.event,
            gate.table
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    };

    let second = second();

    // The second request's backend, in its lock on a role, blocked by the
    // first's.
    let pattern = format!("%{lock}%");
    let deadline = Instant::now() + PATIENCE;
    loop {
        let waiting: Option<i32> = sqlx::query_scalar(
            "SELECT pid FROM pg_stat_activity
             WHERE datname = current_database() AND $1 = ANY(pg_blocking_pids(pid))
               AND query LIKE '%FROM roles%' AND query LIKE $2",
        )
        .bind(held)
        .bind(&pattern)
        .fetch_optional(&app.pool)
        .await
        .expect("read pg_stat_activity");
        if waiting.is_some() {
            break;
        }
        if second.is_finished() {
            let answered = second.await.expect("the second request did not panic");
            panic!(
                "{second_name} did not wait for {first_name}: {} {}",
                answered.status, answered.body
            );
        }
        assert!(
            Instant::now() < deadline,
            "{second_name} was never seen waiting on {first_name} in its {lock} on a role"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(GATE)
        .execute(&mut gate_connection)
        .await
        .expect("open the gate");

    let first = tokio::time::timeout(PATIENCE, first)
        .await
        .unwrap_or_else(|_| panic!("{first_name} did not finish once the gate opened"))
        .expect("the first request did not panic");
    let second = tokio::time::timeout(PATIENCE, second)
        .await
        .unwrap_or_else(|_| panic!("{second_name} did not finish once {first_name} committed"))
        .expect("the second request did not panic");

    (first, second)
}

/// A submission of `document`, not yet sent.
fn submission(
    app: &Arc<TestApp>,
    token: &str,
    document: Uuid,
) -> impl FnOnce() -> tokio::task::JoinHandle<common::TestResponse> {
    let app = Arc::clone(app);
    let token = token.to_owned();
    move || {
        tokio::spawn(async move {
            app.send(
                Method::POST,
                &format!("/api/v1/documents/{document}/submission"),
                Some(&token),
                None,
            )
            .await
        })
    }
}

fn spawn_delete_role(
    app: &Arc<TestApp>,
    token: &str,
    role: Uuid,
) -> tokio::task::JoinHandle<common::TestResponse> {
    let app = Arc::clone(app);
    let token = token.to_owned();
    tokio::spawn(async move { delete_role(&app, &token, role).await })
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
    assert_eq!(
        refused.body["error"]["code"], "ROLE_HAS_OPEN_TASKS",
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

// ---------------------------------------------------------------------------
// #530 — a claimed task nobody can decide is not waiting for its holder
// ---------------------------------------------------------------------------

/// What the #530 holders need: the inbox, the dashboard, and a decision.
const WAITING_PERMISSIONS: &[&str] = &[
    "reporting:dashboard:read",
    "workflow:task:read",
    "workflow:task:execute",
    "workflow:instance:read",
    "document:read",
];

/// A role with no permissions, which a task is offered to or an edge names.
async fn bare_role(app: &TestApp, role_code: &str) -> Uuid {
    fixtures::create_role_with_permissions(&app.pool, fixtures::SYSTEM_TENANT_ID, role_code, &[])
        .await
}

/// A holder of `WAITING_PERMISSIONS` and of `roles`, with their id and token.
async fn waiting_holder(
    app: &TestApp,
    role_code: &str,
    username: &str,
    roles: &[Uuid],
) -> (Uuid, String) {
    let base = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        role_code,
        WAITING_PERMISSIONS,
    )
    .await;

    let mut granted = vec![base];
    granted.extend_from_slice(roles);

    let id = fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        username,
        &format!("{username}@example.test"),
        common::ADMIN_PASSWORD,
        &granted,
    )
    .await;

    (id, app.sign_in(username, common::ADMIN_PASSWORD).await)
}

/// A workflow whose task is offered to `queue`, with `APPROVE` and `REJECT`
/// allowedBy the two rules given.
fn two_edge_workflow(key: &str, queue: &str, approve: Value, reject: Value) -> Value {
    let mut definition = workflow_for(key, queue);
    let transitions = definition["transitions"]
        .as_array_mut()
        .expect("transitions");
    transitions[0]["allowedBy"] = approve;
    transitions[1]["allowedBy"] = reject;
    definition
}

/// Submits a document of a new type on `definition`, claims its task as
/// `holder`, and dates the task a day late. Returns the task.
async fn claimed_late_task(
    app: &TestApp,
    admin: &str,
    holder: &str,
    key: &str,
    definition: Value,
) -> Uuid {
    let workflow = publish_workflow_definition(app, admin, key, definition).await;
    let type_id = document_type(app, admin, &key.to_uppercase(), workflow).await;
    let document = submitted_document(app, admin, type_id, key).await;
    let task = open_task_of(app, document).await;

    let claim = app
        .post(
            &format!("/api/v1/workflow/tasks/{task}/claim"),
            Some(holder),
            json!({}),
        )
        .await;
    assert_eq!(claim.status, StatusCode::OK, "{}", claim.body);

    sqlx::query("UPDATE workflow_tasks SET due_at = now() - interval '1 day' WHERE id = $1")
        .bind(task)
        .execute(&app.pool)
        .await
        .expect("date the task a day late");

    task
}

/// Soft-deleted in SQL, as `reporting_dashboard.rs`'s #487 test does: since
/// **D-89** the API refuses to delete a role an open task needs, so the state
/// is reachable only as a role deleted before that refusal shipped.
async fn soft_delete_role(app: &TestApp, role: Uuid) {
    sqlx::query("UPDATE roles SET deleted_at = now() WHERE id = $1")
        .bind(role)
        .execute(&app.pool)
        .await
        .expect("soft-delete the role");
}

/// The ids of a list of task rows, sorted.
fn ids_in(rows: &Value) -> Vec<Uuid> {
    let mut ids: Vec<Uuid> = rows
        .as_array()
        .unwrap_or_else(|| panic!("a list of tasks: {rows}"))
        .iter()
        .map(id_of)
        .collect();
    ids.sort();
    ids
}

/// Every list the holder predicate reaches, as the ids each one lists.
///
/// Each list's count is asserted equal to its rows as it is read, so no test
/// can check one without the other: `meta.total` against the inbox page, as
/// in `a_caller_sees_their_own_roles_tasks_and_no_others`, and `tasksWaiting`
/// and `tasksOverdue` against the dashboard cards beside them.
struct Surfaces {
    open: Vec<Uuid>,
    overdue: Vec<Uuid>,
    waiting: Vec<Uuid>,
    late: Vec<Uuid>,
}

async fn surfaces_of(app: &TestApp, token: &str) -> Surfaces {
    let mut pages = Vec::new();

    for scope in ["open", "overdue"] {
        let page = inbox_of(app, token, &format!("scope={scope}")).await;
        assert_eq!(page.status, StatusCode::OK, "{}", page.body);

        let ids = ids_in(&page.body["data"]);
        assert_eq!(
            page.body["meta"]["total"],
            json!(ids.len()),
            "?scope={scope}: the count and the page disagree: {}",
            page.body
        );
        pages.push(ids);
    }

    let summary = app.get("/api/v1/dashboard/summary", Some(token)).await;
    assert_eq!(summary.status, StatusCode::OK, "{}", summary.body);
    let summary = &summary.body["data"];

    let waiting = ids_in(&summary["pendingTasks"]);
    assert_eq!(
        summary["tasksWaiting"],
        json!(waiting.len()),
        "the waiting count and its card disagree: {summary}"
    );
    let late = ids_in(&summary["overdueTasks"]);
    assert_eq!(
        summary["tasksOverdue"],
        json!(late.len()),
        "the overdue count and its card disagree: {summary}"
    );

    let overdue = pages.pop().expect("the overdue page");
    let open = pages.pop().expect("the open page");

    Surfaces {
        open,
        overdue,
        waiting,
        late,
    }
}

impl Surfaces {
    /// Asserts that every list holds exactly `expected`.
    fn assert_all(&self, expected: &[Uuid], context: &str) {
        let mut expected = expected.to_vec();
        expected.sort();

        for (surface, ids) in [
            ("GET /tasks?scope=open", &self.open),
            ("GET /tasks?scope=overdue", &self.overdue),
            ("pendingTasks", &self.waiting),
            ("overdueTasks", &self.late),
        ] {
            assert_eq!(ids, &expected, "{context}: {surface}");
        }
    }
}

async fn decision(app: &TestApp, token: &str, task: Uuid, action: &str) -> common::TestResponse {
    app.post(
        &format!("/api/v1/workflow/tasks/{task}/decision"),
        Some(token),
        json!({ "action": action }),
    )
    .await
}

/// `repository::inbox::is_visible_to`, the detail page's gate, asked directly.
async fn is_visible_to(app: &TestApp, user: Uuid, task: Uuid) -> bool {
    kelir_backend::modules::workflow::repository::inbox::is_visible_to(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        user,
        task,
    )
    .await
    .expect("ask the gate")
}

/// Asserts that `APPROVE` and `REJECT`, the two decisions `workflow_for`'s
/// edges offer, each answer `ASSIGNMENT_UNRESOLVED` on `task`.
async fn assert_every_decision_refused(app: &TestApp, token: &str, task: Uuid) {
    for action in ["APPROVE", "REJECT"] {
        let refused = decision(app, token, task, action).await;
        assert_eq!(
            refused.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{action}: {}",
            refused.body
        );
        assert!(
            refused.body.to_string().contains("ASSIGNMENT_UNRESOLVED"),
            "{action} is refused by the engine, as before: {}",
            refused.body
        );
    }
}

/// **A claimed task whose every decision edge names a deleted role stops
/// waiting for its holder** ([#530]).
///
/// Record 18's probe P3. The task is offered to `gone`, both of its edges are
/// allowedBy `gone`, the approver has claimed it, and it is late. #507 hid the
/// **unclaimed** arm of such a task and left this one. It stayed on the inbox
/// and its count, on both dashboard cards and their counts, and its page
/// opened, while deciding it refused as `ASSIGNMENT_UNRESOLVED`.
///
/// Two more stranded tasks sit beside it:
///
/// * **One names the role as `DEPARTMENT_ROLE`**, which `direct` refuses on a
///   deleted role exactly as it refuses `ROLE`.
/// * **One also has a `CANCEL` edge allowedBy the approver.** That edge
///   resolves, but `decide()` cannot fire it, so it must not keep the task.
///
/// **The second subject** is the same shape on a role that stays, claimed by
/// the same approver and as late. A clause that hid every claimed task, or
/// every late one, would pass the first half and fail on it.
///
/// **The decision is unchanged** (AC5). Each hidden task, decided by its
/// assignee, still refuses `APPROVE` and `REJECT` with `ASSIGNMENT_UNRESOLVED`.
/// The refusal comes from the engine and not from the gate, because nothing on
/// the decision path reads the inbox.
///
/// **Seen red, 2026-09-26**, one mutation of `repository::inbox` at a time:
///
/// * #530's clause dropped from `list_for_caller`: the open page lists all four
///   tasks over a `meta.total` of 1.
/// * Dropped from `count_for_caller`: `meta.total` says 4 over a page of 1.
/// * Dropped from `is_visible_to`: the gate admits the stranded tasks. The page
///   still answers 404, because `get_task` reads the row through
///   `list_for_caller` after the gate, which is why the gate is asked directly.
/// * `live.deleted_at IS NULL` dropped: every list holds all four.
/// * `IN ('ROLE', 'DEPARTMENT_ROLE')` narrowed to `= 'ROLE'`: the
///   `DEPARTMENT_ROLE` task is missing before the delete.
/// * The `APPROVE`/`REJECT`/`RETURN` filter dropped: the task beside a live
///   `CANCEL` edge stays on every list.
///
/// [#530]: https://github.com/sujanto-gaws/kelir/issues/530
#[tokio::test]
async fn a_claimed_task_whose_every_decision_edge_names_a_deleted_role_stops_waiting() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let gone = bare_role(&app, "TI-530-GONE").await;
    let kept = bare_role(&app, "TI-530-KEPT").await;
    let (approver_id, approver) =
        waiting_holder(&app, "TI-530-DASH", "ti.530", &[gone, kept]).await;

    let by_role = claimed_late_task(
        &app,
        &token,
        &approver,
        "ti_530_gone",
        workflow_for("ti_530_gone", "TI-530-GONE"),
    )
    .await;

    let department_role = json!({ "assigneeType": "DEPARTMENT_ROLE", "roleCode": "TI-530-GONE" });
    let by_department_role = claimed_late_task(
        &app,
        &token,
        &approver,
        "ti_530_dept",
        two_edge_workflow(
            "ti_530_dept",
            "TI-530-GONE",
            department_role.clone(),
            department_role,
        ),
    )
    .await;

    let mut with_cancel = workflow_for("ti_530_cancel", "TI-530-GONE");
    with_cancel["transitions"]
        .as_array_mut()
        .expect("transitions")
        .push(json!({
            "from": "MANAGER_APPROVAL", "to": "REJECTED", "action": "CANCEL",
            "allowedBy": { "assigneeType": "USER", "userId": approver_id }
        }));
    let beside_a_live_cancel =
        claimed_late_task(&app, &token, &approver, "ti_530_cancel", with_cancel).await;

    let live = claimed_late_task(
        &app,
        &token,
        &approver,
        "ti_530_kept",
        workflow_for("ti_530_kept", "TI-530-KEPT"),
    )
    .await;

    let stranded = [by_role, by_department_role, beside_a_live_cancel];

    surfaces_of(&app, &approver).await.assert_all(
        &[by_role, by_department_role, beside_a_live_cancel, live],
        "before the delete",
    );

    soft_delete_role(&app, gone).await;

    surfaces_of(&app, &approver)
        .await
        .assert_all(&[live], "a claimed task nobody can decide is still waiting");

    for task in stranded {
        let page = app.get(&format!("{TASKS}/{task}"), Some(&approver)).await;
        assert_eq!(
            page.status,
            StatusCode::NOT_FOUND,
            "the page of a task nobody can decide still opens: {}",
            page.body
        );

        // The gate itself, not only the page. `get_task` reads the row through
        // `list_for_caller` after the gate, so the 404 above holds with the
        // gate's clause gone; this does not.
        assert!(
            !is_visible_to(&app, approver_id, task).await,
            "the detail gate still admits a task nobody can decide"
        );

        assert_every_decision_refused(&app, &approver, task).await;
    }

    let page = app.get(&format!("{TASKS}/{live}"), Some(&approver)).await;
    assert_eq!(page.status, StatusCode::OK, "{}", page.body);
    assert!(is_visible_to(&app, approver_id, live).await);

    let decided = decision(&app, &approver, live, "APPROVE").await;
    assert_eq!(decided.status, StatusCode::OK, "{}", decided.body);
}

/// **A claimed task stays waiting while a role-based decision edge can still
/// resolve** ([#530]).
///
/// Four tasks, each claimed by the approver and late, and each with a deleted
/// role in it. None is certain to be refused, so none moves, and each is then
/// decided, which shows the edge that kept it was a real one:
///
/// * **Offered to a deleted role, decided by a live one** (record 18's P2),
///   twice, so that `APPROVE` and `REJECT` are each decided. A claimed task's
///   decision never reads `candidate_role_id`, so the delete of its own role
///   changes nothing.
/// * **Mixed edges.** `APPROVE` names a live role, and `REJECT` and a `CANCEL`
///   edge name the deleted one. Approved. The dead `CANCEL` edge does not hide
///   the task.
/// * **A live `DEPARTMENT_ROLE` edge**, beside a dead `ROLE` one. Approved.
///
/// **Seen red, 2026-09-26**, one mutation of `repository::inbox` at a time.
/// With `IN ('ROLE', 'DEPARTMENT_ROLE')` narrowed to `= 'ROLE'`, the live
/// `DEPARTMENT_ROLE` task leaves every list. Gated on `candidate_role_id`'s
/// role being live instead of on the edges, all four leave.
///
/// [#530]: https://github.com/sujanto-gaws/kelir/issues/530
#[tokio::test]
async fn a_claimed_task_stays_waiting_while_a_live_role_can_decide_it() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let queue = bare_role(&app, "TI-530-QUEUE").await;
    let edge = bare_role(&app, "TI-530-EDGE").await;
    let doomed = bare_role(&app, "TI-530-DOOMED").await;
    let (_, approver) = waiting_holder(
        &app,
        "TI-530-HOLDER",
        "ti.530.holder",
        &[queue, edge, doomed],
    )
    .await;

    let p2 = |key: &str| {
        two_edge_workflow(
            key,
            "TI-530-QUEUE",
            json!("ROLE:TI-530-EDGE"),
            json!("ROLE:TI-530-EDGE"),
        )
    };
    let to_approve =
        claimed_late_task(&app, &token, &approver, "ti_530_p2a", p2("ti_530_p2a")).await;
    let to_reject =
        claimed_late_task(&app, &token, &approver, "ti_530_p2r", p2("ti_530_p2r")).await;

    let mut mixed = two_edge_workflow(
        "ti_530_mixed",
        "TI-530-DOOMED",
        json!("ROLE:TI-530-EDGE"),
        json!("ROLE:TI-530-DOOMED"),
    );
    mixed["transitions"]
        .as_array_mut()
        .expect("transitions")
        .push(json!({
            "from": "MANAGER_APPROVAL", "to": "REJECTED", "action": "CANCEL",
            "allowedBy": "ROLE:TI-530-DOOMED"
        }));
    let mixed = claimed_late_task(&app, &token, &approver, "ti_530_mixed", mixed).await;

    let by_department_role = claimed_late_task(
        &app,
        &token,
        &approver,
        "ti_530_dlive",
        two_edge_workflow(
            "ti_530_dlive",
            "TI-530-DOOMED",
            json!({ "assigneeType": "DEPARTMENT_ROLE", "roleCode": "TI-530-EDGE" }),
            json!("ROLE:TI-530-DOOMED"),
        ),
    )
    .await;

    let all = [to_approve, to_reject, mixed, by_department_role];

    surfaces_of(&app, &approver)
        .await
        .assert_all(&all, "before the delete");

    soft_delete_role(&app, queue).await;
    soft_delete_role(&app, doomed).await;

    surfaces_of(&app, &approver)
        .await
        .assert_all(&all, "a claimed task a live role can decide left");

    for task in all {
        let page = app.get(&format!("{TASKS}/{task}"), Some(&approver)).await;
        assert_eq!(page.status, StatusCode::OK, "{}", page.body);
    }

    for (task, action) in [
        (to_approve, "APPROVE"),
        (to_reject, "REJECT"),
        (mixed, "APPROVE"),
        (by_department_role, "APPROVE"),
    ] {
        let decided = decision(&app, &approver, task, action).await;
        assert_eq!(
            decided.status,
            StatusCode::OK,
            "{action} on a task a live role can decide: {}",
            decided.body
        );
    }
}

/// **A claimed task stays waiting while a `USER` or `OWNER` decision edge can
/// still resolve, and so does one with no decision edge at all** ([#530]).
///
/// * **A `USER` edge.** `APPROVE` names the deleted role and `REJECT` names the
///   approver. Rejected.
/// * **An `OWNER` edge.** `APPROVE` names the deleted role and `REJECT` names
///   the document's owner, the administrator. The edge resolves, so the task
///   stays on the approver's surfaces. It is not decided here, because the
///   approver is not the owner.
/// * **A `RETURNED` correction task.** A return sends the document to a state
///   whose `OWNER` task is created already assigned, and whose only edge is
///   `RESUBMIT`, which `decide()` never fires. With no decision edge there is
///   nothing to judge, and the task stays on all five of the owner's
///   surfaces. This is deliberate, not a gap in the clause.
///
/// **Seen red, 2026-09-26**, one mutation of `repository::inbox` at a time.
/// With `IN ('USER', 'OWNER')` narrowed to `= 'USER'`, the `OWNER` task leaves
/// every list. With the "no decision edge" branch dropped, the correction task
/// leaves the owner's inbox. Gated on `candidate_role_id`, the `USER` and
/// `OWNER` tasks both leave.
///
/// [#530]: https://github.com/sujanto-gaws/kelir/issues/530
#[tokio::test]
async fn a_claimed_task_stays_waiting_while_a_person_can_decide_it() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let doomed = bare_role(&app, "TI-530-PERSON").await;
    let (approver_id, approver) =
        waiting_holder(&app, "TI-530-PEOPLE", "ti.530.people", &[doomed]).await;

    let by_user = claimed_late_task(
        &app,
        &token,
        &approver,
        "ti_530_user",
        two_edge_workflow(
            "ti_530_user",
            "TI-530-PERSON",
            json!("ROLE:TI-530-PERSON"),
            json!({ "assigneeType": "USER", "userId": approver_id }),
        ),
    )
    .await;
    let by_owner = claimed_late_task(
        &app,
        &token,
        &approver,
        "ti_530_owner",
        two_edge_workflow(
            "ti_530_owner",
            "TI-530-PERSON",
            json!("ROLE:TI-530-PERSON"),
            json!("OWNER"),
        ),
    )
    .await;

    let returning = json!({
        "workflowKey": "ti_530_return",
        "version": "1.0.0",
        "name": "Standard approval",
        "initialState": "MANAGER_APPROVAL",
        "states": [
            { "code": "MANAGER_APPROVAL", "name": "Manager approval",
              "mapsToDocumentStatus": "PENDING_APPROVAL",
              "task": { "taskDefinitionKey": "manager_approval", "taskName": "Decide",
                        "assignment": { "assigneeType": "ROLE", "roleCode": "TI-530-PEOPLE" } } },
            { "code": "RETURNED", "name": "Returned to the author",
              "mapsToDocumentStatus": "RETURNED",
              "task": { "taskDefinitionKey": "correct_it", "taskName": "Correct the request",
                        "assignment": { "assigneeType": "OWNER" } } },
            { "code": "COMPLETED", "name": "Completed", "mapsToDocumentStatus": "COMPLETED",
              "isFinal": true }
        ],
        "transitions": [
            { "from": "MANAGER_APPROVAL", "to": "COMPLETED", "action": "APPROVE",
              "allowedBy": "ROLE:TI-530-PEOPLE" },
            { "from": "MANAGER_APPROVAL", "to": "RETURNED", "action": "RETURN",
              "allowedBy": "ROLE:TI-530-PEOPLE" },
            { "from": "RETURNED", "to": "MANAGER_APPROVAL", "action": "RESUBMIT",
              "allowedBy": "OWNER" }
        ]
    });
    let to_return = claimed_late_task(&app, &token, &approver, "ti_530_return", returning).await;
    let returned = decision(&app, &approver, to_return, "RETURN").await;
    assert_eq!(returned.status, StatusCode::OK, "{}", returned.body);

    let correction: Uuid = sqlx::query_scalar(
        "SELECT id FROM workflow_tasks
         WHERE workflow_instance_id = (SELECT workflow_instance_id FROM workflow_tasks WHERE id = $1)
           AND status IN ('CREATED', 'ASSIGNED', 'IN_PROGRESS')",
    )
    .bind(to_return)
    .fetch_one(&app.pool)
    .await
    .expect("the correction task");

    soft_delete_role(&app, doomed).await;

    surfaces_of(&app, &approver).await.assert_all(
        &[by_user, by_owner],
        "a claimed task a person can decide left",
    );

    for task in [by_user, by_owner] {
        let page = app.get(&format!("{TASKS}/{task}"), Some(&approver)).await;
        assert_eq!(page.status, StatusCode::OK, "{}", page.body);
    }

    // The owner's correction task, late like the others, on all five surfaces.
    sqlx::query("UPDATE workflow_tasks SET due_at = now() - interval '1 day' WHERE id = $1")
        .bind(correction)
        .execute(&app.pool)
        .await
        .expect("date the correction task a day late");
    let owner: Uuid =
        sqlx::query_scalar("SELECT assignee_user_id FROM workflow_tasks WHERE id = $1")
            .bind(correction)
            .fetch_one(&app.pool)
            .await
            .expect("the correction task is assigned to the owner");

    surfaces_of(&app, &token).await.assert_all(
        &[correction],
        "the owner's correction task, which no decision edge leaves, left",
    );
    let page = app
        .get(&format!("{TASKS}/{correction}"), Some(&token))
        .await;
    assert_eq!(page.status, StatusCode::OK, "{}", page.body);
    assert!(
        is_visible_to(&app, owner, correction).await,
        "the detail gate hides the owner's correction task"
    );

    let rejected = decision(&app, &approver, by_user, "REJECT").await;
    assert_eq!(rejected.status, StatusCode::OK, "{}", rejected.body);
}

/// A decision state offered to `role_code`, for the #530 edge-shape tests.
fn decision_state(code: &str, role_code: &str) -> Value {
    json!({
        "code": code, "name": code, "mapsToDocumentStatus": "PENDING_APPROVAL",
        "task": { "taskDefinitionKey": code.to_lowercase(), "taskName": "Decide",
                  "assignment": { "assigneeType": "ROLE", "roleCode": role_code } }
    })
}

/// A workflow starting at `MANAGER_APPROVAL`, with `states` and the two final
/// states `COMPLETED` and `REJECTED`.
fn workflow_of(key: &str, mut states: Vec<Value>, transitions: Vec<Value>) -> Value {
    states.push(json!({ "code": "COMPLETED", "name": "Completed",
                        "mapsToDocumentStatus": "COMPLETED", "isFinal": true }));
    states.push(json!({ "code": "REJECTED", "name": "Rejected",
                        "mapsToDocumentStatus": "REJECTED", "isFinal": true }));

    json!({
        "workflowKey": key, "version": "1.0.0", "name": "Standard approval",
        "initialState": "MANAGER_APPROVAL", "states": states, "transitions": transitions
    })
}

/// An edge allowedBy `ROLE:<role_code>`.
fn role_edge(from: &str, to: &str, action: &str, role_code: &str) -> Value {
    json!({ "from": from, "to": to, "action": action, "allowedBy": format!("ROLE:{role_code}") })
}

/// **A live role of the same code in another tenant does not keep the task**
/// ([#530]).
///
/// The clause asks whether a live role holds an edge's `roleCode` *in the
/// task's tenant*. Here the system tenant's `TI-530-TWIN` is deleted while a
/// second tenant's `TI-530-TWIN` lives, and the claimed task still stops
/// waiting on every surface.
///
/// **Seen red, 2026-09-27**: with `live.tenant_id = t.tenant_id` dropped from
/// the clause, the task stays on every list.
///
/// [#530]: https://github.com/sujanto-gaws/kelir/issues/530
#[tokio::test]
async fn a_live_role_of_the_same_code_in_another_tenant_does_not_keep_the_task() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let gone = bare_role(&app, "TI-530-TWIN").await;
    let elsewhere = fixtures::create_tenant(&app.pool, "TNT-530", "Another tenant").await;
    fixtures::create_role_with_permissions(&app.pool, elsewhere, "TI-530-TWIN", &[]).await;
    let (_, approver) = waiting_holder(&app, "TI-530-TWINS", "ti.530.twin", &[gone]).await;

    let task = claimed_late_task(
        &app,
        &token,
        &approver,
        "ti_530_twin",
        workflow_for("ti_530_twin", "TI-530-TWIN"),
    )
    .await;

    surfaces_of(&app, &approver)
        .await
        .assert_all(&[task], "before the delete");

    soft_delete_role(&app, gone).await;

    surfaces_of(&app, &approver).await.assert_all(
        &[],
        "another tenant's live role of the same code kept the task",
    );
}

/// **A live `RETURN` edge keeps the task**, though `APPROVE` and `REJECT` name
/// the deleted role ([#530]). `RETURN` is a decision, and `decide()` fires it,
/// so the task stays on every surface and returning it answers 200.
///
/// **Seen red, 2026-09-27**: with `RETURN` dropped from the resolving edge's
/// actions, the task leaves every list.
///
/// [#530]: https://github.com/sujanto-gaws/kelir/issues/530
#[tokio::test]
async fn a_live_return_edge_keeps_a_claimed_task_waiting() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let gone = bare_role(&app, "TI-530-RGONE").await;
    let kept = bare_role(&app, "TI-530-RKEPT").await;
    let (_, approver) =
        waiting_holder(&app, "TI-530-RETURNER", "ti.530.returner", &[gone, kept]).await;

    let mut definition = workflow_of(
        "ti_530_rlive",
        vec![decision_state("MANAGER_APPROVAL", "TI-530-RGONE")],
        vec![
            role_edge("MANAGER_APPROVAL", "COMPLETED", "APPROVE", "TI-530-RGONE"),
            role_edge("MANAGER_APPROVAL", "REJECTED", "REJECT", "TI-530-RGONE"),
            role_edge("MANAGER_APPROVAL", "RETURNED", "RETURN", "TI-530-RKEPT"),
            json!({ "from": "RETURNED", "to": "MANAGER_APPROVAL", "action": "RESUBMIT",
                    "allowedBy": "OWNER" }),
        ],
    );
    definition["states"]
        .as_array_mut()
        .expect("states")
        .push(json!({
            "code": "RETURNED", "name": "Returned to the author",
            "mapsToDocumentStatus": "RETURNED",
            "task": { "taskDefinitionKey": "correct_it", "taskName": "Correct the request",
                      "assignment": { "assigneeType": "OWNER" } }
        }));
    let task = claimed_late_task(&app, &token, &approver, "ti_530_rlive", definition).await;

    soft_delete_role(&app, gone).await;

    surfaces_of(&app, &approver)
        .await
        .assert_all(&[task], "a task a live RETURN edge can decide left");

    let returned = decision(&app, &approver, task, "RETURN").await;
    assert_eq!(returned.status, StatusCode::OK, "{}", returned.body);
}

/// **Only the current state's edges are judged** ([#530]). The task waits in
/// `MANAGER_APPROVAL`, whose edges name the deleted role, while the next
/// state's edges name a live one. It stops waiting on every surface, and the
/// detail gate refuses it.
///
/// **Seen red, 2026-09-27**: with `tr.from_state = i.current_state` dropped
/// from the resolving edge in all three copies, the task stays on every list;
/// dropped from `is_visible_to`'s copy alone, the gate admits it while the
/// lists agree.
///
/// [#530]: https://github.com/sujanto-gaws/kelir/issues/530
#[tokio::test]
async fn only_the_current_states_edges_decide_whether_a_claimed_task_waits() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let gone = bare_role(&app, "TI-530-NOW").await;
    let kept = bare_role(&app, "TI-530-LATER").await;
    let (approver_id, approver) =
        waiting_holder(&app, "TI-530-STATES", "ti.530.states", &[gone, kept]).await;

    let definition = workflow_of(
        "ti_530_later",
        vec![
            decision_state("MANAGER_APPROVAL", "TI-530-NOW"),
            decision_state("FINANCE_APPROVAL", "TI-530-LATER"),
        ],
        vec![
            role_edge(
                "MANAGER_APPROVAL",
                "FINANCE_APPROVAL",
                "APPROVE",
                "TI-530-NOW",
            ),
            role_edge("MANAGER_APPROVAL", "REJECTED", "REJECT", "TI-530-NOW"),
            role_edge("FINANCE_APPROVAL", "COMPLETED", "APPROVE", "TI-530-LATER"),
            role_edge("FINANCE_APPROVAL", "REJECTED", "REJECT", "TI-530-LATER"),
        ],
    );
    let task = claimed_late_task(&app, &token, &approver, "ti_530_later", definition).await;

    soft_delete_role(&app, gone).await;

    surfaces_of(&app, &approver)
        .await
        .assert_all(&[], "a later state's live edges kept the task");
    assert!(
        !is_visible_to(&app, approver_id, task).await,
        "the detail gate admits a task only a later state's edges could decide"
    );
}

/// **A finished task is not judged** ([#530]). The approver decides the task,
/// the instance moves on to a state whose edges then name a deleted role, and
/// the finished task stays in the approver's `scope=completed` list, its
/// count, and the detail gate. The clause governs open tasks only.
///
/// **Seen red, 2026-09-27**: with the open-status condition dropped from all
/// three copies, the completed list is empty; dropped from `is_visible_to`'s
/// copy alone, the gate refuses the finished task.
///
/// [#530]: https://github.com/sujanto-gaws/kelir/issues/530
#[tokio::test]
async fn a_finished_task_stays_in_its_holders_completed_list() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let gone = bare_role(&app, "TI-530-FGONE").await;
    let kept = bare_role(&app, "TI-530-FKEPT").await;
    let (approver_id, approver) =
        waiting_holder(&app, "TI-530-FINISHER", "ti.530.finisher", &[gone, kept]).await;

    let definition = workflow_of(
        "ti_530_done",
        vec![
            decision_state("MANAGER_APPROVAL", "TI-530-FKEPT"),
            decision_state("FINANCE_APPROVAL", "TI-530-FKEPT"),
        ],
        vec![
            role_edge(
                "MANAGER_APPROVAL",
                "FINANCE_APPROVAL",
                "APPROVE",
                "TI-530-FKEPT",
            ),
            role_edge("MANAGER_APPROVAL", "REJECTED", "REJECT", "TI-530-FKEPT"),
            role_edge("FINANCE_APPROVAL", "COMPLETED", "APPROVE", "TI-530-FGONE"),
            role_edge("FINANCE_APPROVAL", "REJECTED", "REJECT", "TI-530-FGONE"),
        ],
    );
    let task = claimed_late_task(&app, &token, &approver, "ti_530_done", definition).await;

    let decided = decision(&app, &approver, task, "APPROVE").await;
    assert_eq!(decided.status, StatusCode::OK, "{}", decided.body);

    soft_delete_role(&app, gone).await;

    let completed = inbox_of(&app, &approver, "scope=completed").await;
    assert_eq!(completed.status, StatusCode::OK, "{}", completed.body);
    assert_eq!(
        ids_in(&completed.body["data"]),
        vec![task],
        "the finished task left the completed list: {}",
        completed.body
    );
    assert_eq!(
        completed.body["meta"]["total"],
        json!(1),
        "{}",
        completed.body
    );
    assert!(
        is_visible_to(&app, approver_id, task).await,
        "the detail gate refuses a finished task"
    );
}

/// **A condition that picks a live edge keeps the task** ([#530]). `APPROVE`
/// has two edges: one conditioned, on a live role, and a fallback on the
/// deleted one. The live edge can resolve, so the task stays on every surface,
/// and approving it answers 200.
///
/// The clause does not evaluate conditions. It asks whether *any* decision
/// edge resolves, which errs toward visible, as the module doc says.
///
/// [#530]: https://github.com/sujanto-gaws/kelir/issues/530
#[tokio::test]
async fn a_conditioned_live_edge_keeps_a_claimed_task_waiting() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let gone = bare_role(&app, "TI-530-CGONE").await;
    let kept = bare_role(&app, "TI-530-CKEPT").await;
    let (_, approver) =
        waiting_holder(&app, "TI-530-CHOOSER", "ti.530.chooser", &[gone, kept]).await;

    let mut conditioned = role_edge(
        "MANAGER_APPROVAL",
        "FINANCE_APPROVAL",
        "APPROVE",
        "TI-530-CKEPT",
    );
    conditioned["condition"] = json!({ "==": [1, 1] });
    let definition = workflow_of(
        "ti_530_cond",
        vec![
            decision_state("MANAGER_APPROVAL", "TI-530-CGONE"),
            decision_state("FINANCE_APPROVAL", "TI-530-CKEPT"),
        ],
        vec![
            conditioned,
            role_edge("MANAGER_APPROVAL", "COMPLETED", "APPROVE", "TI-530-CGONE"),
            role_edge("MANAGER_APPROVAL", "REJECTED", "REJECT", "TI-530-CGONE"),
            role_edge("FINANCE_APPROVAL", "COMPLETED", "APPROVE", "TI-530-CKEPT"),
        ],
    );
    let task = claimed_late_task(&app, &token, &approver, "ti_530_cond", definition).await;

    soft_delete_role(&app, gone).await;

    surfaces_of(&app, &approver)
        .await
        .assert_all(&[task], "a task a conditioned live edge can decide left");

    let approved = decision(&app, &approver, task, "APPROVE").await;
    assert_eq!(approved.status, StatusCode::OK, "{}", approved.body);
}

/// **An unclaimed task follows #507's arm, not this clause** ([#530]). Two
/// tasks on the same shape, offered to a live role whose edges name a deleted
/// one: the claimed task stops waiting, and the unclaimed one stays on every
/// surface of the role's holder and passes the detail gate. #507 judges an
/// unclaimed task by its offered role, which lives, and #530's clause is for
/// the claimed arm alone.
///
/// **Seen red, 2026-09-27**: with the claimed condition,
/// `t.assignee_user_id IS DISTINCT FROM $2`, set to `false` in all three
/// copies, the unclaimed task leaves every list too.
///
/// [#530]: https://github.com/sujanto-gaws/kelir/issues/530
#[tokio::test]
async fn an_unclaimed_task_is_judged_by_its_offered_role_and_not_by_the_clause() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let queue = bare_role(&app, "TI-530-OFFERED").await;
    let gone = bare_role(&app, "TI-530-DECIDER").await;
    let (approver_id, approver) =
        waiting_holder(&app, "TI-530-QUEUER", "ti.530.queuer", &[queue, gone]).await;

    let shape = |key: &str| {
        two_edge_workflow(
            key,
            "TI-530-OFFERED",
            json!("ROLE:TI-530-DECIDER"),
            json!("ROLE:TI-530-DECIDER"),
        )
    };
    let claimed = claimed_late_task(
        &app,
        &token,
        &approver,
        "ti_530_claimed",
        shape("ti_530_claimed"),
    )
    .await;

    let workflow =
        publish_workflow_definition(&app, &token, "ti_530_open", shape("ti_530_open")).await;
    let type_id = document_type(&app, &token, "TI_530_OPEN", workflow).await;
    let document = submitted_document(&app, &token, type_id, "ti_530_open").await;
    let unclaimed = open_task_of(&app, document).await;
    sqlx::query("UPDATE workflow_tasks SET due_at = now() - interval '1 day' WHERE id = $1")
        .bind(unclaimed)
        .execute(&app.pool)
        .await
        .expect("date the unclaimed task a day late");

    surfaces_of(&app, &approver)
        .await
        .assert_all(&[claimed, unclaimed], "before the delete");

    soft_delete_role(&app, gone).await;

    surfaces_of(&app, &approver)
        .await
        .assert_all(&[unclaimed], "the clause reached past the claimed arm");
    assert!(
        is_visible_to(&app, approver_id, unclaimed).await,
        "the detail gate refuses an unclaimed task on a live offered role"
    );
    assert!(!is_visible_to(&app, approver_id, claimed).await);
}

// ---------------------------------------------------------------------------
// #512 — an administrator reassigns an open task (FR-WF-017, D-91, ADR-0042)
// ---------------------------------------------------------------------------

// # Seen to fail (coding standard §2.9)
//
// Each mutation was applied alone, the named tests run, and the mutation
// reverted, on 2026-09-26, and the rows marked (2026-09-28) again after the
// campaign's round:
//
// | Mutation | Reddened |
// |---|---|
// | The closed-status 409 after `lock_task` removed from `service::task::reassign` (2026-09-28) | *a closed task whose instance moved on…* — 422 `TARGET_CANNOT_DECIDE` about `FINANCE_APPROVAL`. *A decided task is not reassigned* stays green: its instance is final, so no edge is judged and the write's predicate answers 409 |
// | The open-status predicate dropped from `repository::task::reassign` | Nothing since 2026-09-28: it is the backstop behind the 409 above, and under the task lock no test can reach it |
// | The tenant filter dropped from `repository::task::lock_task` (2026-09-28) | Nothing, and nothing can: `reassign` reads the task through the tenant-filtered `find_task` before its transaction, and answers 404 there. The row said 409 until 2026-09-28 |
// | The tenant filter dropped from `assignment::direct`'s user lookup | *a reassign stays in its tenant* — reassigned to the other tenant's user, 200 |
// | The tenant filter dropped from `assignment::direct`'s role lookup (2026-09-28) | *a reassign stays in its tenant* — `TARGET_CANNOT_DECIDE` rather than `ASSIGNMENT_UNRESOLVED`: the other tenant's role resolved, and only the decidability check refused it |
// | `deleted_at IS NULL` dropped from `direct`'s user lookup | *a reassign names one live target…* — a deleted user accepted |
// | `deleted_at IS NULL` dropped from `direct`'s role lookup | *a reassign names one live target…* — a deleted role accepted; *a reassign arriving during a role delete…* — 200 onto the deleted role |
// | `FOR KEY SHARE` dropped from `direct`'s role lookup | *a role delete arriving during a reassign…* — the delete never waits, 204; *a reassign arriving during a role delete…* — never seen waiting in its lock |
// | `assignee_user_id = COALESCE($3, assignee_user_id)` in `reassign` | *…to a role whose holders may claim it* — the claim stayed |
// | `candidate_role_id = COALESCE($4, candidate_role_id)` in `reassign` | *…to a user who then decides it* — the role stayed |
// | The decidability check's call removed from `service::task::reassign` | *a user target must satisfy…* and *a role target must be named…* — a target that could not decide accepted |
// | "At least one" decision edge turned into "every" | *a user target must satisfy…* — the mixed case's reject-only holder refused |
// | `DEPARTMENT_ROLE` scope dropped before `permits` for a user target | *a user target must satisfy…* — the holder scoped to another department accepted |
// | The audit event renamed, or its call's `.await` dropped (2026-09-28) | *a reassign is audited once…* — no record |
// | `FOR UPDATE` dropped from `lock_task` (2026-09-28) | *a claim arriving during a reassign…* — the claim did not wait, 200. **Green since #619** (2026-10-01): every task path now waits at the instance first, so the task lock no longer decides any race these tests stage; see the #619 table |
// | `COALESCE` on `candidate_department_id`, then on `delegated_from_user_id`, in `reassign` (2026-09-28) | *a reassign clears the department and the delegation* |
// | `lock_instance` dropped from `claim_task`, `delegate` and `reassign`, one at a time (#619) | See the #619 section's table |
// | `Return` dropped from the decision actions; `Cancel` added; an edge with no `allowedBy` made to `continue` (2026-09-28) | *the decidability check reads return and not cancel*, each case |

/// What every role in this section carries: the inbox's permissions, and not
/// `workflow:task:reassign`, which only the administrator holds.
const WORKER: &[&str] = &[
    "workflow:task:read",
    "workflow:task:execute",
    "workflow:instance:read",
    "document:read",
];

async fn reassign(app: &TestApp, token: &str, task: Uuid, body: Value) -> common::TestResponse {
    app.post(
        &format!("/api/v1/workflow/tasks/{task}/reassign"),
        Some(token),
        body,
    )
    .await
}

/// A reassign of `task`, not yet sent.
fn reassignment(
    app: &Arc<TestApp>,
    token: &str,
    task: Uuid,
    body: Value,
) -> impl FnOnce() -> tokio::task::JoinHandle<common::TestResponse> {
    let app = Arc::clone(app);
    let token = token.to_owned();
    move || tokio::spawn(async move { reassign(&app, &token, task, body).await })
}

async fn worker_role(app: &TestApp, code: &str) -> Uuid {
    fixtures::create_role_with_permissions(&app.pool, fixtures::SYSTEM_TENANT_ID, code, WORKER)
        .await
}

/// A user of the system tenant holding `roles`, and their token.
async fn worker(app: &TestApp, username: &str, roles: &[Uuid]) -> (Uuid, String) {
    let id = fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        username,
        &format!("{username}@example.test"),
        common::ADMIN_PASSWORD,
        roles,
    )
    .await;

    (id, app.sign_in(username, common::ADMIN_PASSWORD).await)
}

/// A definition whose task is offered to `queue` and whose edges are
/// `allowedBy` `edge` (JWSS §5 lets the two differ).
fn offered_to_and_decided_by(key: &str, queue: &str, edge: &str) -> Value {
    let mut definition = workflow_for(key, queue);
    for transition in definition["transitions"]
        .as_array_mut()
        .expect("transitions")
    {
        transition["allowedBy"] = json!(format!("ROLE:{edge}"));
    }
    definition
}

fn inbox_holds(inbox: &common::TestResponse, task: Uuid) -> bool {
    inbox.body["data"]
        .as_array()
        .expect("a page")
        .iter()
        .any(|row| row["id"] == json!(task))
}

async fn administrator_id(app: &TestApp) -> Uuid {
    sqlx::query_scalar("SELECT id FROM users WHERE username = $1")
        .bind(common::ADMIN_USERNAME)
        .fetch_one(&app.pool)
        .await
        .expect("read the administrator")
}

/// The task's `REASSIGN` history rows: old status, new status, comment, actor.
async fn reassign_history(
    app: &TestApp,
    task: Uuid,
) -> Vec<(Option<String>, String, Option<String>, Option<Uuid>)> {
    sqlx::query_as(
        "SELECT old_status, new_status, comment, actor_user_id FROM workflow_task_history
         WHERE task_id = $1 AND action = 'REASSIGN' ORDER BY created_at",
    )
    .bind(task)
    .fetch_all(&app.pool)
    .await
    .expect("read the task's history")
}

async fn workflow_history_rows(app: &TestApp, document: Uuid) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM workflow_history WHERE document_id = $1")
        .bind(document)
        .fetch_one(&app.pool)
        .await
        .expect("count the document's workflow history")
}

/// The task's holder columns and status, straight from the row.
async fn task_holder(app: &TestApp, task: Uuid) -> (Option<Uuid>, Option<Uuid>, String) {
    sqlx::query_as(
        "SELECT assignee_user_id, candidate_role_id, status FROM workflow_tasks WHERE id = $1",
    )
    .bind(task)
    .fetch_one(&app.pool)
    .await
    .expect("read the task")
}

fn detail_paths_and_codes(response: &common::TestResponse) -> Vec<(String, String)> {
    response.body["error"]["details"]
        .as_array()
        .expect("details")
        .iter()
        .map(|detail| {
            (
                detail["path"].as_str().expect("a path").to_owned(),
                detail["code"].as_str().expect("a code").to_owned(),
            )
        })
        .collect()
}

/// **A claimed task reassigned to a user is that user's to decide, and
/// nobody else's** (FR-WF-017, [#512]).
///
/// Two holders of the task's role. The first claims it, so the second cannot
/// decide it. The administrator reassigns it to the second with a reason, and
/// the row, the response, the history and both inboxes say so: the second
/// holds it `ASSIGNED`, the role column is cleared, one `REASSIGN` row records
/// the move with the reason trimmed and the administrator as actor, and no
/// `workflow_history` row is written, because the process did not move. Then
/// the second decides it.
///
/// [#512]: https://github.com/sujanto-gaws/kelir/issues/512
#[tokio::test]
async fn an_administrator_reassigns_a_claimed_task_to_a_user_who_then_decides_it() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let queue = worker_role(&app, "TI-RA-USER").await;
    let (_, first) = worker(&app, "ti.ra.first", &[queue]).await;
    let (second_id, second) = worker(&app, "ti.ra.second", &[queue]).await;

    let workflow = publish_workflow(&app, &token, "ti_ra_user", "TI-RA-USER").await;
    let type_id = document_type(&app, &token, "TI_RA_USER", workflow).await;
    let document = submitted_document(&app, &token, type_id, "Reassigned to a person").await;
    let task = open_task_of(&app, document).await;
    claim(&app, &first, task).await;

    let not_theirs = app
        .post(
            &format!("/api/v1/workflow/tasks/{task}/decision"),
            Some(&second),
            json!({ "action": "APPROVE" }),
        )
        .await;
    assert_eq!(
        not_theirs.status,
        StatusCode::FORBIDDEN,
        "a claimed task was decidable by another holder of its role: {}",
        not_theirs.body
    );

    let moves = workflow_history_rows(&app, document).await;

    let reassigned = reassign(
        &app,
        &token,
        task,
        json!({ "userId": second_id, "comment": "  Ani is on leave  " }),
    )
    .await;
    assert_eq!(reassigned.status, StatusCode::OK, "{}", reassigned.body);

    let data = reassigned.data();
    assert_eq!(data["assigneeUserId"], json!(second_id), "{data}");
    assert!(data["candidateRoleId"].is_null(), "the role stayed: {data}");
    assert_eq!(data["status"], "ASSIGNED", "{data}");
    assert!(data["delegatedFromUserId"].is_null(), "{data}");
    assert_eq!(
        task_holder(&app, task).await,
        (Some(second_id), None, "ASSIGNED".to_owned())
    );

    assert_eq!(
        reassign_history(&app, task).await,
        [(
            Some("ASSIGNED".to_owned()),
            "ASSIGNED".to_owned(),
            Some("Ani is on leave".to_owned()),
            Some(administrator_id(&app).await),
        )],
        "one REASSIGN row, with the reason and the administrator"
    );
    assert_eq!(
        workflow_history_rows(&app, document).await,
        moves,
        "a reassign wrote a workflow_history row, but the process did not move"
    );

    assert!(
        !inbox_holds(&app.get(TASKS, Some(&first)).await, task),
        "the previous holder still sees the task"
    );
    let inbox = app.get(TASKS, Some(&second)).await;
    assert!(inbox_holds(&inbox, task), "{}", inbox.body);

    decide(&app, &second, task).await;
}

/// **A claimed task reassigned to a role is unclaimed and offered to that
/// role, and the role delete counts it there** ([#512], **D-89**, [#529]).
///
/// The task is offered to `Q`; its `APPROVE` is `allowedBy` `R` and its
/// `REJECT` `allowedBy` `E`, so `R` is a role that can decide it. `A` holds `Q`
/// and `E` and claims it; `B` holds `R` and `E`. The administrator reassigns
/// it to `R`: the assignee is cleared, the status goes back to `CREATED`, and
/// the history records both ends. `A` no longer sees it; `B` sees it as a role
/// task, claims it, and decides it.
///
/// **Row 7's count, after the assignee is cleared.** Deleting `R` is refused:
/// the task is offered to it unclaimed, and an edge names it. (A reassign to a
/// role is only accepted to a role an edge names, so the edge alone would hold
/// it; the first clause is asserted by the row, in `task_holder`.) `Q`, which
/// neither offers the task any more nor names an edge, is needed by no open
/// task. Once `B` decides it, neither is `R`. Both are still named by the
/// published definition, so since **D-91** (3) ([#510]) each delete is refused
/// with that code instead, which is asked only once no open task needs the
/// role.
///
/// [#510]: https://github.com/sujanto-gaws/kelir/issues/510
///
/// [#512]: https://github.com/sujanto-gaws/kelir/issues/512
/// [#529]: https://github.com/sujanto-gaws/kelir/issues/529
#[tokio::test]
async fn an_administrator_reassigns_a_claimed_task_to_a_role_whose_holders_may_claim_it() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let queue = worker_role(&app, "TI-RA-Q").await;
    let edge = worker_role(&app, "TI-RA-E").await;
    let target = worker_role(&app, "TI-RA-R").await;
    let (_, a) = worker(&app, "ti.ra.a", &[queue, edge]).await;
    let (b_id, b) = worker(&app, "ti.ra.b", &[target, edge]).await;

    let mut definition = workflow_for("ti_ra_role", "TI-RA-Q");
    definition["transitions"][0]["allowedBy"] = json!("ROLE:TI-RA-R");
    definition["transitions"][1]["allowedBy"] = json!("ROLE:TI-RA-E");
    let workflow = publish_workflow_definition(&app, &token, "ti_ra_role", definition).await;
    let type_id = document_type(&app, &token, "TI_RA_ROLE", workflow).await;
    let document = submitted_document(&app, &token, type_id, "Reassigned to a role").await;
    let task = open_task_of(&app, document).await;
    claim(&app, &a, task).await;
    assert!(!inbox_holds(&app.get(TASKS, Some(&b)).await, task));

    let reassigned = reassign(
        &app,
        &token,
        task,
        json!({ "roleCode": "TI-RA-R", "comment": "Finance takes these now" }),
    )
    .await;
    assert_eq!(reassigned.status, StatusCode::OK, "{}", reassigned.body);

    let data = reassigned.data();
    assert!(data["assigneeUserId"].is_null(), "the claim stayed: {data}");
    assert_eq!(data["candidateRoleId"], json!(target), "{data}");
    assert_eq!(data["candidateRoleCode"], "TI-RA-R", "{data}");
    assert_eq!(data["status"], "CREATED", "{data}");
    assert_eq!(
        task_holder(&app, task).await,
        (None, Some(target), "CREATED".to_owned())
    );
    assert_eq!(
        reassign_history(&app, task).await,
        [(
            Some("ASSIGNED".to_owned()),
            "CREATED".to_owned(),
            Some("Finance takes these now".to_owned()),
            Some(administrator_id(&app).await),
        )]
    );

    assert!(
        !inbox_holds(&app.get(TASKS, Some(&a)).await, task),
        "the previous assignee still sees a task reassigned to a role they do not hold"
    );
    let inbox = app.get(TASKS, Some(&b)).await;
    let row = inbox.body["data"]
        .as_array()
        .expect("a page")
        .iter()
        .find(|row| row["id"] == json!(task))
        .unwrap_or_else(|| {
            panic!(
                "the new role's holder is not offered the task: {}",
                inbox.body
            )
        })
        .clone();
    assert_eq!(row["assignment"], "ROLE", "{row}");

    let target_refused = delete_role(&app, &token, target).await;
    assert_eq!(
        target_refused.status,
        StatusCode::CONFLICT,
        "the role a task was reassigned to did not hold it: {}",
        target_refused.body
    );
    assert_eq!(
        target_refused.body["error"]["code"], "ROLE_HAS_OPEN_TASKS",
        "{}",
        target_refused.body
    );
    assert!(
        target_refused.body["error"]["message"]
            .as_str()
            .is_some_and(|message| message.starts_with("1 open task needs")),
        "{}",
        target_refused.body
    );

    assert_needed_by_no_open_task(
        &app,
        &token,
        queue,
        "the role the task was reassigned away from still held it",
    )
    .await;

    claim(&app, &b, task).await;
    assert_eq!(
        task_holder(&app, task).await,
        (Some(b_id), Some(target), "ASSIGNED".to_owned())
    );
    decide(&app, &b, task).await;

    assert_needed_by_no_open_task(&app, &token, target, "the decided task still held its role")
        .await;
}

/// No open task needs `role`: its list is empty, and its delete is refused, if
/// at all, for the published definition that names it, **D-91** (3)'s refusal,
/// which is asked only after the open-task count.
async fn assert_needed_by_no_open_task(app: &TestApp, token: &str, role: Uuid, why: &str) {
    let listed = open_tasks(app, token, role, "").await;
    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);
    assert_eq!(listed.body["meta"]["total"], 0, "{why}: {}", listed.body);

    let refused = delete_role(app, token, role).await;
    assert_eq!(refused.status, StatusCode::CONFLICT, "{}", refused.body);
    assert_eq!(
        refused.body["error"]["code"], "ROLE_NAMED_BY_PUBLISHED_DEFINITION",
        "{why}: {}",
        refused.body
    );
}

/// **A reassign names exactly one live target, or nothing changes** ([#512]
/// AC3, AC4, AC9).
///
/// Both targets, neither, and a null one are refused before anything is read,
/// naming both fields. A user or a role that is deleted, or never existed, is
/// refused as `ASSIGNMENT_UNRESOLVED` on the field that named it, by the
/// engine's own checks. The task keeps its holder and gains no history row.
///
/// [#512]: https://github.com/sujanto-gaws/kelir/issues/512
#[tokio::test]
async fn a_reassign_names_one_live_target_or_changes_nothing() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let queue = worker_role(&app, "TI-RA-ONE").await;
    let (live, _) = worker(&app, "ti.ra.live", &[queue]).await;
    let (gone, _) = worker(&app, "ti.ra.gone", &[queue]).await;
    sqlx::query("UPDATE users SET deleted_at = now() WHERE id = $1")
        .bind(gone)
        .execute(&app.pool)
        .await
        .expect("delete the user");
    let deleted_role = worker_role(&app, "TI-RA-GONE").await;
    sqlx::query("UPDATE roles SET deleted_at = now() WHERE id = $1")
        .bind(deleted_role)
        .execute(&app.pool)
        .await
        .expect("delete the role");

    let workflow = publish_workflow(&app, &token, "ti_ra_one", "TI-RA-ONE").await;
    let type_id = document_type(&app, &token, "TI_RA_ONE", workflow).await;
    let document = submitted_document(&app, &token, type_id, "Nobody new holds this").await;
    let task = open_task_of(&app, document).await;

    let both = [
        ("userId".to_owned(), "ONE_TARGET_REQUIRED".to_owned()),
        ("roleCode".to_owned(), "ONE_TARGET_REQUIRED".to_owned()),
    ];
    let user = [("userId".to_owned(), "ASSIGNMENT_UNRESOLVED".to_owned())];
    let role = [("roleCode".to_owned(), "ASSIGNMENT_UNRESOLVED".to_owned())];

    for (body, expected) in [
        (
            json!({ "userId": live, "roleCode": "TI-RA-ONE" }),
            &both[..],
        ),
        (json!({}), &both[..]),
        (json!({ "userId": null, "comment": "to nobody" }), &both[..]),
        (json!({ "roleCode": "  " }), &both[..]),
        (json!({ "userId": gone }), &user[..]),
        (json!({ "userId": Uuid::now_v7() }), &user[..]),
        (json!({ "roleCode": "TI-RA-GONE" }), &role[..]),
        (json!({ "roleCode": "TI-RA-NEVER" }), &role[..]),
    ] {
        let refused = reassign(&app, &token, task, body.clone()).await;

        assert_eq!(
            refused.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{body} was not refused: {}",
            refused.body
        );
        assert_eq!(detail_paths_and_codes(&refused), expected, "{body}");
    }

    assert_eq!(
        task_holder(&app, task).await,
        (None, Some(queue), "CREATED".to_owned()),
        "a refused reassign changed the task"
    );
    assert!(reassign_history(&app, task).await.is_empty());
}

/// **Only an open task is reassigned** ([#512] AC6).
///
/// A decided task answers 409 naming its status, is still the decider's, and
/// gains no history row. The refusal comes straight after the task lock,
/// before the target is resolved or judged; the write's own predicate is the
/// backstop behind it. *A closed task whose instance moved on…* is the case
/// where the order matters.
///
/// [#512]: https://github.com/sujanto-gaws/kelir/issues/512
#[tokio::test]
async fn a_decided_task_is_not_reassigned() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let queue = worker_role(&app, "TI-RA-DONE").await;
    let (decider, approver) = worker(&app, "ti.ra.decider", &[queue]).await;
    let (other, _) = worker(&app, "ti.ra.other", &[queue]).await;

    let workflow = publish_workflow(&app, &token, "ti_ra_done", "TI-RA-DONE").await;
    let type_id = document_type(&app, &token, "TI_RA_DONE", workflow).await;
    let document = submitted_document(&app, &token, type_id, "Already decided").await;
    let task = open_task_of(&app, document).await;
    decide(&app, &approver, task).await;

    let refused = reassign(&app, &token, task, json!({ "userId": other })).await;

    assert_eq!(
        refused.status,
        StatusCode::CONFLICT,
        "a decided task was reassigned: {}",
        refused.body
    );
    assert!(
        refused.body["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("COMPLETED")),
        "{}",
        refused.body
    );
    assert_eq!(
        task_holder(&app, task).await,
        (Some(decider), Some(queue), "COMPLETED".to_owned())
    );
    assert!(reassign_history(&app, task).await.is_empty());
}

/// **The reassign has a permission of its own** ([#512] AC2, **D-91**).
///
/// The task's own holder, with `workflow:task:execute`, is refused 403: working
/// a task is not moving one. The administrator, whose `ROLE-ADMIN` `0048`
/// granted it, is answered 404 for a task that does not exist.
///
/// [#512]: https://github.com/sujanto-gaws/kelir/issues/512
#[tokio::test]
async fn a_reassign_requires_workflow_task_reassign() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let queue = worker_role(&app, "TI-RA-PERM").await;
    let (holder_id, holder) = worker(&app, "ti.ra.holder", &[queue]).await;

    let workflow = publish_workflow(&app, &token, "ti_ra_perm", "TI-RA-PERM").await;
    let type_id = document_type(&app, &token, "TI_RA_PERM", workflow).await;
    let document = submitted_document(&app, &token, type_id, "Not theirs to move").await;
    let task = open_task_of(&app, document).await;
    claim(&app, &holder, task).await;

    let refused = reassign(&app, &holder, task, json!({ "roleCode": "TI-RA-PERM" })).await;
    assert_eq!(
        refused.status,
        StatusCode::FORBIDDEN,
        "a holder without workflow:task:reassign reassigned their task: {}",
        refused.body
    );
    assert_eq!(
        task_holder(&app, task).await,
        (Some(holder_id), Some(queue), "ASSIGNED".to_owned())
    );

    let missing = reassign(&app, &token, Uuid::now_v7(), json!({ "userId": holder_id })).await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND, "{}", missing.body);
}

/// **A reassign stays in its tenant, and a provisioned tenant's administrator
/// holds it** ([#512], ADR-0042 §2).
///
/// Tenant B is provisioned through the real route, so its administrator holds
/// what provisioning grants, `workflow:task:reassign` among it. They are
/// answered 404 for the system tenant's task, which is untouched. The system
/// tenant's administrator cannot reassign their own task to B's user, or to a
/// role code only B has: both are 422, because the engine resolves a target
/// in the caller's tenant.
#[tokio::test]
async fn a_reassign_stays_in_its_tenant() {
    let app = TestApp::spawn_with(|config| config.multi_tenant = true).await;
    let token = app
        .sign_in_to("SYSTEM", common::ADMIN_USERNAME, common::ADMIN_PASSWORD)
        .await;

    let provisioned = app
        .post(
            "/api/v1/organization/tenants",
            Some(&token),
            json!({
                "tenantCode": "TNT-RA",
                "name": "Another Customer",
                "administrator": {
                    "username": "ti.ra.b.admin",
                    "email": "ti.ra.b.admin@example.test",
                    "displayName": "Tenant Administrator",
                    "password": "a-sufficiently-long-password",
                },
            }),
        )
        .await;
    assert_eq!(
        provisioned.status,
        StatusCode::CREATED,
        "{}",
        provisioned.body
    );
    let other = id_of(provisioned.data());
    let other_token = app
        .sign_in_to("TNT-RA", "ti.ra.b.admin", "a-sufficiently-long-password")
        .await;

    let profile = app.get("/api/v1/auth/me", Some(&other_token)).await;
    assert!(
        profile.data()["permissions"]
            .as_array()
            .expect("permissions")
            .contains(&json!("workflow:task:reassign")),
        "a provisioned tenant's administrator cannot reassign: {}",
        profile.body
    );

    let their_user = fixtures::create_user(
        &app.pool,
        other,
        "ti.ra.b.user",
        "ti.ra.b.user@example.test",
        common::ADMIN_PASSWORD,
        &[],
    )
    .await;
    fixtures::create_role_with_permissions(&app.pool, other, "TI-RA-B-ONLY", &[]).await;

    let queue = worker_role(&app, "TI-RA-XT").await;
    let workflow = publish_workflow(&app, &token, "ti_ra_xt", "TI-RA-XT").await;
    let type_id = document_type(&app, &token, "TI_RA_XT", workflow).await;
    let document = submitted_document(&app, &token, type_id, "Ours to move").await;
    let task = open_task_of(&app, document).await;

    let across = reassign(&app, &other_token, task, json!({ "userId": their_user })).await;
    assert_eq!(
        across.status,
        StatusCode::NOT_FOUND,
        "another tenant's administrator reached this task: {}",
        across.body
    );
    // The task's own 404, not the instance's or the document's.
    assert_eq!(
        across.body["error"]["message"], "Task not found",
        "{}",
        across.body
    );

    let to_their_user = reassign(&app, &token, task, json!({ "userId": their_user })).await;
    assert_eq!(
        to_their_user.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        to_their_user.body
    );
    assert_eq!(
        detail_paths_and_codes(&to_their_user),
        [("userId".to_owned(), "ASSIGNMENT_UNRESOLVED".to_owned())]
    );

    let to_their_role = reassign(&app, &token, task, json!({ "roleCode": "TI-RA-B-ONLY" })).await;
    assert_eq!(
        to_their_role.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        to_their_role.body
    );
    // Not `TARGET_CANNOT_DECIDE`: the code must not resolve at all here, or
    // another tenant's role is only refused because no edge names it.
    assert_eq!(
        detail_paths_and_codes(&to_their_role),
        [("roleCode".to_owned(), "ASSIGNMENT_UNRESOLVED".to_owned())]
    );

    assert_eq!(
        task_holder(&app, task).await,
        (None, Some(queue), "CREATED".to_owned()),
        "a refused reassign changed the task"
    );
    assert!(reassign_history(&app, task).await.is_empty());
}

/// **A real role delete waits on a reassign holding the role, then refuses**
/// ([#512] AC5, **D-89**).
///
/// The reassign moves an unclaimed task from `Q` to `R`, which its edges name.
/// It locks the task, reads `R` `FOR KEY SHARE`, and is held at its `UPDATE`
/// of the task, before it commits. `DELETE /api/v1/identity/roles/{R}` arrives
/// and must wait in its `FOR UPDATE` on `R`. Released, the reassign commits,
/// and the delete answers 409 with nothing deleted.
///
/// **The wait is what this asserts.** The delete's 409 would come anyway,
/// since the task's edges name `R`; the decidability check makes that true of
/// every accepted reassign to a role.
///
/// [#512]: https://github.com/sujanto-gaws/kelir/issues/512
#[tokio::test]
async fn a_role_delete_arriving_during_a_reassign_to_it_waits_and_refuses() {
    let app = Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;

    worker_role(&app, "TI-RA-RACE-Q").await;
    let target = worker_role(&app, "TI-RA-RACE-R").await;

    let definition = offered_to_and_decided_by("ti_ra_race", "TI-RA-RACE-Q", "TI-RA-RACE-R");
    let workflow = publish_workflow_definition(&app, &token, "ti_ra_race", definition).await;
    let type_id = document_type(&app, &token, "TI_RA_RACE", workflow).await;
    let document = submitted_document(&app, &token, type_id, "Reassigned during a delete").await;
    let task = open_task_of(&app, document).await;

    let (reassigned, deleted) = second_waits_on_first(
        &app,
        Gate {
            table: "workflow_tasks",
            event: "UPDATE",
        },
        (
            "the reassign",
            reassignment(&app, &token, task, json!({ "roleCode": "TI-RA-RACE-R" })),
        ),
        ("the delete", || spawn_delete_role(&app, &token, target)),
        "FOR UPDATE",
    )
    .await;

    assert_eq!(reassigned.status, StatusCode::OK, "{}", reassigned.body);
    assert_refused_for_one_open_task(&app, &deleted, target, document).await;
    assert_eq!(
        task_holder(&app, task).await,
        (None, Some(target), "CREATED".to_owned())
    );
}

/// **A reassign arriving during a role's delete waits, then finds the role
/// gone** ([#512] AC5, **D-89**): the order the test above reverses.
///
/// **The delete's half is held by the test**, as
/// `a_submission_racing_a_role_delete_waits_and_then_finds_the_role_gone`
/// holds it. The real route cannot be used here: a reassign to `R` is only
/// accepted when the task's edges name `R`, and then the real delete counts
/// the task and refuses before it writes anything a gate could stop. So the
/// test locks `R` `FOR UPDATE` in a transaction of its own, sends the reassign,
/// sees it blocked by that transaction in its `FOR KEY SHARE` on `roles`,
/// soft-deletes `R` and commits. The reassign then reads `R` again, finds it
/// deleted, and is refused as `ASSIGNMENT_UNRESOLVED`, with the task still
/// offered to `Q`.
///
/// [#512]: https://github.com/sujanto-gaws/kelir/issues/512
#[tokio::test]
async fn a_reassign_arriving_during_a_role_delete_waits_and_finds_the_role_gone() {
    use std::time::Duration;
    use tokio::time::Instant;

    let app = Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;

    let queue = worker_role(&app, "TI-RA-GATE-Q").await;
    let target = worker_role(&app, "TI-RA-GATE-R").await;

    let definition = offered_to_and_decided_by("ti_ra_gate", "TI-RA-GATE-Q", "TI-RA-GATE-R");
    let workflow = publish_workflow_definition(&app, &token, "ti_ra_gate", definition).await;
    let type_id = document_type(&app, &token, "TI_RA_GATE", workflow).await;
    let document = submitted_document(&app, &token, type_id, "Reassigned to a role going").await;
    let task = open_task_of(&app, document).await;

    let mut delete = app.pool.begin().await.expect("a transaction");
    let deleting: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *delete)
        .await
        .expect("the delete's backend");
    sqlx::query("SELECT id FROM roles WHERE id = $1 FOR UPDATE")
        .bind(target)
        .execute(&mut *delete)
        .await
        .expect("the delete's lock");

    let reassigned = reassignment(&app, &token, task, json!({ "roleCode": "TI-RA-GATE-R" }))();

    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let waiting: Option<i32> = sqlx::query_scalar(
            "SELECT pid FROM pg_stat_activity
             WHERE datname = current_database() AND $1 = ANY(pg_blocking_pids(pid))
               AND query LIKE '%FROM roles%' AND query LIKE '%FOR KEY SHARE%'",
        )
        .bind(deleting)
        .fetch_optional(&app.pool)
        .await
        .expect("read pg_stat_activity");
        if waiting.is_some() {
            break;
        }
        if reassigned.is_finished() {
            let answered = reassigned.await.expect("the reassign did not panic");
            panic!(
                "the reassign did not wait for the delete: {} {}",
                answered.status, answered.body
            );
        }
        assert!(
            Instant::now() < deadline,
            "the reassign was never seen waiting on the delete in its FOR KEY SHARE on a role"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    sqlx::query("UPDATE roles SET deleted_at = now() WHERE id = $1")
        .bind(target)
        .execute(&mut *delete)
        .await
        .expect("the delete");
    delete.commit().await.expect("the delete commits");

    let reassigned = reassigned.await.expect("the reassign did not panic");
    assert_eq!(
        reassigned.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a task was reassigned to a role deleted under it: {}",
        reassigned.body
    );
    assert_eq!(
        detail_paths_and_codes(&reassigned),
        [("roleCode".to_owned(), "ASSIGNMENT_UNRESOLVED".to_owned())]
    );
    assert_eq!(
        task_holder(&app, task).await,
        (None, Some(queue), "CREATED".to_owned()),
        "the task moved to a deleted role"
    );
    assert!(reassign_history(&app, task).await.is_empty());
}

/// A task offered to `TI-RA-DQ` whose `APPROVE` and `REJECT` are `allowedBy`
/// the two rules given, raised on a document of its own. Answers the task.
async fn task_decided_by(
    app: &TestApp,
    token: &str,
    code: &str,
    approve: Value,
    reject: Value,
) -> Uuid {
    let key = code.to_lowercase();
    let mut definition = workflow_for(&key, "TI-RA-DQ");
    definition["transitions"][0]["allowedBy"] = approve;
    definition["transitions"][1]["allowedBy"] = reject;

    let workflow = publish_workflow_definition(app, token, &key, definition).await;
    let type_id = document_type(app, token, code, workflow).await;
    let document = submitted_document(app, token, type_id, code).await;

    open_task_of(app, document).await
}

/// A department of the system tenant, by code.
async fn department(app: &TestApp, code: &str) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO departments (id, tenant_id, department_code, name) VALUES ($1, $2, $3, $3)",
    )
    .bind(id)
    .bind(fixtures::SYSTEM_TENANT_ID)
    .bind(code)
    .execute(&app.pool)
    .await
    .expect("insert the department");
    id
}

/// Every grant `user` holds is scoped to `department`.
async fn scope_grants(app: &TestApp, user: Uuid, department: Uuid) {
    sqlx::query("UPDATE user_roles SET department_id = $1 WHERE user_id = $2")
        .bind(department)
        .bind(user)
        .execute(&app.pool)
        .await
        .expect("scope the grants");
}

fn cannot_decide(response: &common::TestResponse, field: &str) {
    assert_eq!(
        response.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a reassign to somebody who could not decide the task was accepted: {}",
        response.body
    );
    assert_eq!(
        detail_paths_and_codes(response),
        [(field.to_owned(), "TARGET_CANNOT_DECIDE".to_owned())]
    );
}

/// **A user target must satisfy at least one decision edge, as the decision
/// itself would judge it** ([#512], the product owner's decision of
/// 2026-09-26).
///
/// One task per `allowedBy` form, each refused for somebody the form does not
/// admit and then accepted for somebody it does: `USER`, `ROLE`, `OWNER` (the
/// administrator raised every document), and `DEPARTMENT_ROLE`, whose scope is
/// checked rather than assumed — a holder of the role whose grant is scoped to
/// another department is refused. A refusal names the state and what its
/// decisions need, and leaves the task as it was.
///
/// **Mixed edges pass on one.** `APPROVE` is `allowedBy` another user and
/// `REJECT` a role the target holds: the target can reject, so the task is not
/// stranded, and the reassign is accepted.
#[tokio::test]
async fn a_user_target_must_satisfy_a_decision_edge() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let queue = worker_role(&app, "TI-RA-DQ").await;
    let edge = worker_role(&app, "TI-RA-DE").await;
    let scoped = worker_role(&app, "TI-RA-DD").await;
    let procurement = department(&app, "TI-RA-PROC").await;
    let finance = department(&app, "TI-RA-FIN").await;

    let (stranger, _) = worker(&app, "ti.ra.stranger", &[queue]).await;
    let (named, _) = worker(&app, "ti.ra.named", &[queue, edge]).await;
    let (inside, _) = worker(&app, "ti.ra.inside", &[scoped]).await;
    scope_grants(&app, inside, procurement).await;
    let (outside, _) = worker(&app, "ti.ra.outside", &[scoped]).await;
    scope_grants(&app, outside, finance).await;
    let creator = administrator_id(&app).await;

    let user_rule = json!(format!("USER:{named}"));
    let cases = [
        (
            "TI_RA_D_USER",
            user_rule.clone(),
            user_rule,
            stranger,
            named,
        ),
        (
            "TI_RA_D_ROLE",
            json!("ROLE:TI-RA-DE"),
            json!("ROLE:TI-RA-DE"),
            stranger,
            named,
        ),
        (
            "TI_RA_D_OWNER",
            json!("OWNER"),
            json!("OWNER"),
            named,
            creator,
        ),
        (
            "TI_RA_D_DEPT",
            json!({ "assigneeType": "DEPARTMENT_ROLE", "roleCode": "TI-RA-DD",
                    "departmentScope": "TI-RA-PROC" }),
            json!({ "assigneeType": "DEPARTMENT_ROLE", "roleCode": "TI-RA-DD",
                    "departmentScope": "TI-RA-PROC" }),
            outside,
            inside,
        ),
        (
            "TI_RA_D_MIXED",
            json!(format!("USER:{stranger}")),
            json!("ROLE:TI-RA-DE"),
            inside,
            named,
        ),
    ];

    for (code, approve, reject, refused_user, accepted_user) in cases {
        let task = task_decided_by(&app, &token, code, approve, reject).await;

        let refused = reassign(&app, &token, task, json!({ "userId": refused_user })).await;
        cannot_decide(&refused, "userId");
        assert!(
            refused.body["error"]["details"][0]["message"]
                .as_str()
                .is_some_and(|message| message.contains("`MANAGER_APPROVAL`")),
            "{code}: the refusal names the state: {}",
            refused.body
        );
        assert_eq!(
            task_holder(&app, task).await,
            (None, Some(queue), "CREATED".to_owned()),
            "{code}: a refused reassign changed the task"
        );

        let accepted = reassign(&app, &token, task, json!({ "userId": accepted_user })).await;
        assert_eq!(
            accepted.status,
            StatusCode::OK,
            "{code}: somebody who could decide the task was refused: {}",
            accepted.body
        );
        assert_eq!(accepted.data()["assigneeUserId"], json!(accepted_user));
    }
}

/// **A role target must be named by a decision edge** ([#512], ADR-0042 §2).
///
/// A role is a set of people who may change after the reassign, so it passes
/// an edge that names it — `ROLE`, or `DEPARTMENT_ROLE` whose department
/// resolves — and not an `OWNER` or `USER` edge, which no role satisfies by
/// being held. The task's own queue role, named by no edge, is refused; so is
/// any role on a task whose decisions are the owner's.
#[tokio::test]
async fn a_role_target_must_be_named_by_a_decision_edge() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let queue = worker_role(&app, "TI-RA-DQ").await;
    let edge = worker_role(&app, "TI-RA-RE").await;
    let scoped = worker_role(&app, "TI-RA-RD").await;
    department(&app, "TI-RA-RPROC").await;

    let task = task_decided_by(
        &app,
        &token,
        "TI_RA_R_ROLE",
        json!("ROLE:TI-RA-RE"),
        json!("OWNER"),
    )
    .await;
    let refused = reassign(&app, &token, task, json!({ "roleCode": "TI-RA-DQ" })).await;
    cannot_decide(&refused, "roleCode");
    let accepted = reassign(&app, &token, task, json!({ "roleCode": "TI-RA-RE" })).await;
    assert_eq!(accepted.status, StatusCode::OK, "{}", accepted.body);
    assert_eq!(accepted.data()["candidateRoleId"], json!(edge));

    let owners = task_decided_by(
        &app,
        &token,
        "TI_RA_R_OWNER",
        json!("OWNER"),
        json!("OWNER"),
    )
    .await;
    let refused = reassign(&app, &token, owners, json!({ "roleCode": "TI-RA-RE" })).await;
    cannot_decide(&refused, "roleCode");
    assert!(
        refused.body["error"]["details"][0]["message"]
            .as_str()
            .is_some_and(|message| message.contains("the document's creator")),
        "{}",
        refused.body
    );
    assert_eq!(
        task_holder(&app, owners).await,
        (None, Some(queue), "CREATED".to_owned())
    );

    let rule = json!({ "assigneeType": "DEPARTMENT_ROLE", "roleCode": "TI-RA-RD",
                       "departmentScope": "TI-RA-RPROC" });
    let departmental = task_decided_by(&app, &token, "TI_RA_R_DEPT", rule.clone(), rule).await;
    let accepted = reassign(
        &app,
        &token,
        departmental,
        json!({ "roleCode": "TI-RA-RD" }),
    )
    .await;
    assert_eq!(accepted.status, StatusCode::OK, "{}", accepted.body);
    assert_eq!(accepted.data()["candidateRoleId"], json!(scoped));
}

/// **A state with no decision edges is not judged** ([#512], ADR-0042 §2).
///
/// A return puts the document in `RETURNED`, whose task is the owner's
/// correction and whose only edge is `RESUBMIT`, taken on the document rather
/// than decided on the task. There is no decision to strand, so a reassign of
/// that task to a user no edge names is accepted.
#[tokio::test]
async fn a_task_with_no_decision_edges_is_reassigned_without_the_check() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let queue = worker_role(&app, "TI-RA-ZQ").await;
    let (_, approver) = worker(&app, "ti.ra.approver", &[queue]).await;
    let (anybody, _) = worker(&app, "ti.ra.anybody", &[]).await;

    let definition = json!({
        "workflowKey": "ti_ra_zero",
        "version": "1.0.0",
        "name": "With a return",
        "initialState": "MANAGER_APPROVAL",
        "states": [
            { "code": "MANAGER_APPROVAL", "name": "Manager approval",
              "mapsToDocumentStatus": "PENDING_APPROVAL",
              "task": { "taskDefinitionKey": "manager_approval", "taskName": "Decide",
                        "assignment": { "assigneeType": "ROLE", "roleCode": "TI-RA-ZQ" } } },
            { "code": "RETURNED", "name": "Returned to the author",
              "mapsToDocumentStatus": "RETURNED",
              "task": { "taskDefinitionKey": "correct_it", "taskName": "Correct the request",
                        "assignment": { "assigneeType": "OWNER" } } },
            { "code": "COMPLETED", "name": "Completed", "mapsToDocumentStatus": "COMPLETED",
              "isFinal": true }
        ],
        "transitions": [
            { "from": "MANAGER_APPROVAL", "to": "COMPLETED", "action": "APPROVE",
              "allowedBy": "ROLE:TI-RA-ZQ" },
            { "from": "MANAGER_APPROVAL", "to": "RETURNED", "action": "RETURN",
              "allowedBy": "ROLE:TI-RA-ZQ" },
            { "from": "RETURNED", "to": "MANAGER_APPROVAL", "action": "RESUBMIT",
              "allowedBy": "OWNER" }
        ]
    });
    let workflow = publish_workflow_definition(&app, &token, "ti_ra_zero", definition).await;
    let type_id = document_type(&app, &token, "TI_RA_ZERO", workflow).await;
    let document = submitted_document(&app, &token, type_id, "Returned for correction").await;
    let manager = open_task_of(&app, document).await;

    let returned = app
        .post(
            &format!("/api/v1/workflow/tasks/{manager}/decision"),
            Some(&approver),
            json!({ "action": "RETURN", "comment": "Fix the amount" }),
        )
        .await;
    assert_eq!(returned.status, StatusCode::OK, "{}", returned.body);

    let correction = open_task_of(&app, document).await;
    assert_ne!(correction, manager);

    let reassigned = reassign(&app, &token, correction, json!({ "userId": anybody })).await;
    assert_eq!(
        reassigned.status,
        StatusCode::OK,
        "a task with no decision to strand was refused: {}",
        reassigned.body
    );
    assert_eq!(reassigned.data()["assigneeUserId"], json!(anybody));
}

/// **A closed task is refused as closed, whatever the target** ([#512] AC6).
///
/// A two-stage workflow: the manager's task is decided, and the instance waits
/// at `FINANCE_APPROVAL` on a task of its own. The manager's task is closed,
/// so a reassign of it is a 409 naming `COMPLETED`, to a user, to a role, to a
/// deleted user and to a role code nothing holds. None of them is judged: the
/// target's decidability is asked of the instance's *current* state, which the
/// closed task has left, and a dead target is not looked up.
///
/// **Seen red** without the 409 after `lock_task`: the user and the role were
/// `TARGET_CANNOT_DECIDE` (*a decision in `FINANCE_APPROVAL` needs role
/// `TI-RA-MS-F`*), and the dead targets `ASSIGNMENT_UNRESOLVED`, each a 422.
///
/// [#512]: https://github.com/sujanto-gaws/kelir/issues/512
#[tokio::test]
async fn a_closed_task_whose_instance_moved_on_is_refused_as_closed() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let manager = worker_role(&app, "TI-RA-MS-M").await;
    worker_role(&app, "TI-RA-MS-F").await;
    let (manager_id, approver) = worker(&app, "ti.ra.ms.manager", &[manager]).await;
    let (other, _) = worker(&app, "ti.ra.ms.other", &[manager]).await;
    let (gone, _) = worker(&app, "ti.ra.ms.gone", &[manager]).await;
    sqlx::query("UPDATE users SET deleted_at = now() WHERE id = $1")
        .bind(gone)
        .execute(&app.pool)
        .await
        .expect("delete the user");

    let definition = two_stage("ti_ra_ms", "TI-RA-MS-M", "TI-RA-MS-F");
    let workflow = publish_workflow_definition(&app, &token, "ti_ra_ms", definition).await;
    let type_id = document_type(&app, &token, "TI_RA_MS", workflow).await;
    let document = submitted_document(&app, &token, type_id, "Past the manager").await;
    let task = open_task_of(&app, document).await;
    decide(&app, &approver, task).await;
    assert_ne!(
        open_task_of(&app, document).await,
        task,
        "the instance did not move on to finance"
    );

    for (target, body) in [
        ("a user", json!({ "userId": other })),
        ("a role", json!({ "roleCode": "TI-RA-MS-M" })),
        ("a deleted user", json!({ "userId": gone })),
        (
            "a role code nothing holds",
            json!({ "roleCode": "TI-RA-MS-NONE" }),
        ),
    ] {
        let refused = reassign(&app, &token, task, body).await;
        assert_eq!(
            refused.status,
            StatusCode::CONFLICT,
            "a closed task's reassign to {target} was not refused as closed: {}",
            refused.body
        );
        assert!(
            refused.body["error"]["message"]
                .as_str()
                .is_some_and(|message| message.contains("COMPLETED")),
            "{target}: {}",
            refused.body
        );
    }

    assert_eq!(
        task_holder(&app, task).await,
        (Some(manager_id), Some(manager), "COMPLETED".to_owned())
    );
    assert!(reassign_history(&app, task).await.is_empty());
}

/// An `audit_events` row: action, object type, actor, reason, old and new.
type AuditRow = (String, String, Option<Uuid>, Option<String>, Value, Value);

/// **A reassign is audited once, with both holders** ([#512], ADR-0042).
///
/// One `Workflow.TaskReassigned` record, `REASSIGN` on the `WORKFLOW_TASK`,
/// by the administrator, with no reason: the comment is prose about somebody's
/// document, so the record says only that there was one. The old value is the
/// claim, the new one the target.
///
/// **Seen red** with the event renamed `Workflow.TaskReassign`, and with the
/// audit call dropped: no record.
///
/// [#512]: https://github.com/sujanto-gaws/kelir/issues/512
#[tokio::test]
async fn a_reassign_is_audited_once_with_both_holders() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let queue = worker_role(&app, "TI-RA-AUD").await;
    let (first_id, first) = worker(&app, "ti.ra.aud.first", &[queue]).await;
    let (second_id, _) = worker(&app, "ti.ra.aud.second", &[queue]).await;

    let workflow = publish_workflow(&app, &token, "ti_ra_aud", "TI-RA-AUD").await;
    let type_id = document_type(&app, &token, "TI_RA_AUD", workflow).await;
    let document = submitted_document(&app, &token, type_id, "Audited").await;
    let task = open_task_of(&app, document).await;
    claim(&app, &first, task).await;

    let reassigned = reassign(
        &app,
        &token,
        task,
        json!({ "userId": second_id, "comment": "A private note" }),
    )
    .await;
    assert_eq!(reassigned.status, StatusCode::OK, "{}", reassigned.body);

    let records: Vec<AuditRow> = sqlx::query_as(
        "SELECT action, object_type, actor_user_id, reason, old_value_json, new_value_json
             FROM audit_events WHERE object_id = $1 AND event_type = 'Workflow.TaskReassigned'",
    )
    .bind(task)
    .fetch_all(&app.pool)
    .await
    .expect("read the audit");
    assert_eq!(records.len(), 1, "{records:#?}");

    let (action, object_type, actor, reason, old, new) = &records[0];
    assert_eq!(action, "REASSIGN");
    assert_eq!(object_type, "WORKFLOW_TASK");
    assert_eq!(*actor, Some(administrator_id(&app).await));
    assert_eq!(*reason, None, "the comment reached the audit");
    assert_eq!(old["assigneeUserId"], json!(first_id), "{old}");
    assert_eq!(old["candidateRoleId"], json!(queue), "{old}");
    assert_eq!(old["status"], "ASSIGNED", "{old}");
    assert_eq!(new["assigneeUserId"], json!(second_id), "{new}");
    assert!(new["candidateRoleId"].is_null(), "{new}");
    assert_eq!(new["status"], "ASSIGNED", "{new}");
    assert_eq!(new["documentId"], json!(document), "{new}");
    assert_eq!(new["commented"], true, "{new}");
    assert!(
        !new.to_string().contains("A private note"),
        "the comment reached the audit: {new}"
    );
}

/// The backend blocked by `blocker` in a statement matching `table` and `lock`.
async fn blocked_by(app: &TestApp, blocker: i32, table: &str, lock: &str) -> Option<i32> {
    sqlx::query_scalar(
        "SELECT pid FROM pg_stat_activity
         WHERE datname = current_database() AND $1 = ANY(pg_blocking_pids(pid))
           AND query LIKE $2 AND query LIKE $3",
    )
    .bind(blocker)
    .bind(format!("%{table}%"))
    .bind(format!("%{lock}%"))
    .fetch_optional(&app.pool)
    .await
    .expect("read pg_stat_activity")
}

/// **A claim arriving during a reassign waits for it** ([#512] AC5).
///
/// The reassign moves an unclaimed task from `Q` to `R`. The test holds `R`
/// `FOR UPDATE`, so the reassign stops in its `FOR KEY SHARE` on the role,
/// **after** it has locked the instance and the task. A claim by somebody
/// holding both roles is then sent, and must be seen blocked by the reassign
/// in its own `FOR UPDATE` on `workflow_instances`, which is where every task
/// path waits since [#619]. Released, the reassign commits, then the
/// claim does: the claimant holds the task under `R`, and the history reads
/// the reassign, then the claim.
///
/// [`second_waits_on_first`] cannot stage this: its gate is a row trigger, and
/// an `UPDATE`'s row trigger fires after the row is locked, so a gate at the
/// reassign's `UPDATE` would hold the task with or without `lock_task`'s lock.
/// The only point between that lock and the write is the role's.
///
/// **Seen red** with `FOR UPDATE` dropped from `lock_task`: the claim did not
/// wait, and answered 200 before the reassign overwrote it (2026-09-28). Since
/// [#619] that mutation leaves it green, because the claim waits at the
/// instance first; it is red with `lock_instance` dropped from `reassign` or
/// from `claim_task`, where the claim is seen waiting in `workflow_tasks`.
///
/// [#512]: https://github.com/sujanto-gaws/kelir/issues/512
/// [#619]: https://github.com/sujanto-gaws/kelir/issues/619
#[tokio::test]
async fn a_claim_arriving_during_a_reassign_waits_for_it() {
    use std::time::Duration;
    use tokio::time::Instant;

    let app = Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;

    let queue = worker_role(&app, "TI-RA-CL-Q").await;
    let target = worker_role(&app, "TI-RA-CL-R").await;
    let (claimant_id, claimant) = worker(&app, "ti.ra.cl.claimant", &[queue, target]).await;

    let definition = offered_to_and_decided_by("ti_ra_cl", "TI-RA-CL-Q", "TI-RA-CL-R");
    let workflow = publish_workflow_definition(&app, &token, "ti_ra_cl", definition).await;
    let type_id = document_type(&app, &token, "TI_RA_CL", workflow).await;
    let document = submitted_document(&app, &token, type_id, "Claimed mid-reassign").await;
    let task = open_task_of(&app, document).await;

    let mut holding = app.pool.begin().await.expect("a transaction");
    let holder: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *holding)
        .await
        .expect("the holder's backend");
    sqlx::query("SELECT id FROM roles WHERE id = $1 FOR UPDATE")
        .bind(target)
        .execute(&mut *holding)
        .await
        .expect("hold the role");

    let reassigned = reassignment(&app, &token, task, json!({ "roleCode": "TI-RA-CL-R" }))();

    let deadline = Instant::now() + Duration::from_secs(30);
    let reassigning = loop {
        if let Some(pid) = blocked_by(&app, holder, "FROM roles", "FOR KEY SHARE").await {
            break pid;
        }
        assert!(
            !reassigned.is_finished(),
            "the reassign did not stop at the role"
        );
        assert!(
            Instant::now() < deadline,
            "the reassign was never seen waiting on the role"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    };

    let claimed = {
        let app = Arc::clone(&app);
        tokio::spawn(async move {
            app.post(
                &format!("/api/v1/workflow/tasks/{task}/claim"),
                Some(&claimant),
                json!({}),
            )
            .await
        })
    };

    let deadline = Instant::now() + Duration::from_secs(30);
    while blocked_by(&app, reassigning, "FROM workflow_instances", "FOR UPDATE")
        .await
        .is_none()
    {
        if claimed.is_finished() {
            let answered = claimed.await.expect("the claim did not panic");
            panic!(
                "the claim did not wait for the reassign: {} {}",
                answered.status, answered.body
            );
        }
        assert!(
            Instant::now() < deadline,
            "the claim was never seen waiting on the reassign's instance lock"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    holding.rollback().await.expect("release the role");

    let reassigned = reassigned.await.expect("the reassign did not panic");
    assert_eq!(reassigned.status, StatusCode::OK, "{}", reassigned.body);
    let claimed = claimed.await.expect("the claim did not panic");
    assert_eq!(claimed.status, StatusCode::OK, "{}", claimed.body);

    assert_eq!(
        task_holder(&app, task).await,
        (Some(claimant_id), Some(target), "ASSIGNED".to_owned()),
        "the claim was overwritten"
    );

    let history: Vec<(Option<String>, Option<String>, String)> = sqlx::query_as(
        "SELECT action, old_status, new_status FROM workflow_task_history
         WHERE task_id = $1 AND old_status IS NOT NULL
         ORDER BY created_at, id",
    )
    .bind(task)
    .fetch_all(&app.pool)
    .await
    .expect("read the task's history");
    assert_eq!(
        history,
        [
            (
                Some("REASSIGN".to_owned()),
                Some("CREATED".to_owned()),
                "CREATED".to_owned()
            ),
            (None, Some("CREATED".to_owned()), "ASSIGNED".to_owned()),
        ]
    );
}

// ---------------------------------------------------------------------------
// #619 — every task path takes the instance, then the task (ADR-0042 §2)
// ---------------------------------------------------------------------------
//
// `claim_task`, `delegate` and `reassign` each insert a `workflow_task_history`
// row, and that row's foreign key to `workflow_instances` takes `FOR KEY
// SHARE` on the instance. Before #619 they locked only the task, so each of
// them took the task and then, through the key, the instance: the reverse of
// `decide`'s order. Record 20's P2 caught a decision and a reassign
// deadlocking, and the decision answering 500.
//
// # Seen red on the unfixed code, 2026-09-30
//
// | Test | How it went red |
// |---|---|
// | *a decision arriving during a reassign…* | The decision answered 500 `INTERNAL_ERROR`, *deadlock detected* in the log; the reassign answered 200 |
// | *a claim meeting the instance lock…* | The claim answered 500 `INTERNAL_ERROR`, *deadlock detected*: it waited in the history `INSERT`, holding the task |
// | *a hand-off meeting the instance lock…* | The test's task lock was aborted with *deadlock detected* (`40P01`): the hand-off waited in the history `INSERT`, holding the task |
//
// On the same code, with the settle wait before the status assertion and the
// test's own abort tolerated, `deadlocks_once_settled` read 1 against 0: the
// counter guard sees what it guards.
//
// # Seen to fail after the fix (coding standard §2.9), 2026-10-01
//
// Each mutation applied alone, the #619 tests and the two #512 race tests
// run, and the mutation reverted.
//
// | Mutation | Reddened |
// |---|---|
// | `lock_instance` dropped from `claim_task` (bare `lock_task`) | *a claim meeting…* — 500 deadlock; *a claim arriving during a reassign…* — never seen waiting on the instance; *…a missing task or instance…* — 409 rather than 404 "Workflow instance" |
// | `lock_instance` dropped from `delegate` | *a hand-off meeting…* — 500 deadlock; *…a missing task or instance…* — 200 rather than 404 |
// | `lock_instance` dropped from `reassign` | *a decision arriving during a reassign…* — 500 rather than 403; *a reassign meeting…* — 500; *a claim arriving during a reassign…* — never seen waiting on the instance |
// | `claim_task` takes the task, then the instance | *a claim meeting…* — 500; *a claim arriving during a reassign…* |
// | `delegate` takes the task, then the instance | *a hand-off meeting…* — 500 |
// | `reassign` takes the task, then the instance | *a reassign meeting…* — 500. *A decision arriving during a reassign…* stays green by construction: the parked reassign holds both rows in either order |
// | The instance-id equality check under the lock removed | Nothing, and nothing can: a task never changes instance, so the check is a backstop |
// | `FOR UPDATE` dropped from `lock_task` | Nothing: the instance lock now serializes every task path first |

/// The statement of the backend `blocker` holds up, if one is waiting on it.
async fn statement_blocked_by(app: &TestApp, blocker: i32) -> Option<String> {
    sqlx::query_scalar(
        "SELECT query FROM pg_stat_activity
         WHERE datname = current_database() AND $1 = ANY(pg_blocking_pids(pid))",
    )
    .bind(blocker)
    .fetch_optional(&app.pool)
    .await
    .expect("read pg_stat_activity")
}

/// Whether `statement` is `instance_repo::lock_instance`'s: a `SELECT` whose
/// one `FROM` is `workflow_instances` alone, ending in `FOR UPDATE`.
///
/// Read word by word, so that a statement locking another row while it joins
/// the instance or reads it in a subquery is not mistaken for this one: its
/// `FROM` would name another table first, or there would be two.
fn locks_the_instance(statement: &str) -> bool {
    let words: Vec<&str> = statement.split_whitespace().collect();
    let froms = words.iter().filter(|word| **word == "FROM").count();

    words.first() == Some(&"SELECT")
        && words.ends_with(&["FOR", "UPDATE"])
        && froms == 1
        && words
            .windows(3)
            .any(|three| three == ["FROM", "workflow_instances", "WHERE"])
}

/// **[`locks_the_instance`] knows `lock_instance` from its neighbours.** Every
/// race test that says where a request waited rests on it, and the substring
/// check it replaced would have passed a subquery that locks the document, or
/// a join from the instance that locks the document with it.
#[test]
fn locks_the_instance_is_lock_instance_and_nothing_near_it() {
    let lock_instance = "
        SELECT id, workflow_definition_id, document_id, current_state, status
        FROM workflow_instances
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
        FOR UPDATE
        ";
    assert!(locks_the_instance(lock_instance));

    for (statement, why) in [
        (
            "SELECT id FROM workflow_tasks WHERE tenant_id = $1 AND id = $2 FOR UPDATE",
            "the task's lock",
        ),
        (
            "SELECT d.id FROM documents d JOIN workflow_instances i ON i.id = d.process_instance_id
             WHERE d.id = $1 FOR UPDATE",
            "a join locking the document",
        ),
        (
            "SELECT i.id FROM workflow_instances i JOIN documents d ON d.id = i.document_id
             WHERE i.id = $1 FOR UPDATE",
            "a join from the instance, locking the document too",
        ),
        (
            "SELECT id FROM documents WHERE process_instance_id =
             (SELECT id FROM workflow_instances WHERE id = $1) FOR UPDATE",
            "a subquery locking the document",
        ),
        (
            "SELECT i.id FROM workflow_instances i WHERE i.id = $1 FOR UPDATE",
            "an aliased read, which `lock_instance` is not",
        ),
        (
            "SELECT id FROM workflow_instances WHERE id = $1 FOR KEY SHARE",
            "a weaker lock",
        ),
        (
            "SELECT id FROM workflow_instances WHERE id = $1 FOR NO KEY UPDATE",
            "a weaker lock",
        ),
    ] {
        assert!(!locks_the_instance(statement), "{why}: {statement}");
    }
}

/// The deadlocks PostgreSQL has counted in this test's database so far.
async fn deadlocks_so_far(app: &TestApp) -> i64 {
    sqlx::query_scalar("SELECT deadlocks FROM pg_stat_database WHERE datname = current_database()")
        .fetch_one(&app.pool)
        .await
        .expect("read pg_stat_database")
}

/// The deadlocks PostgreSQL has counted in `app`'s database, **read once every
/// client backend on it has exited**. Closes the application's pool.
///
/// A backend counts a deadlock in its own pending statistics and flushes them
/// when it has been idle long enough, which can be ten seconds after the
/// abort. A backend that exits flushes them before it leaves
/// `pg_stat_activity`, so waiting for the database's backends to be gone makes
/// the read final rather than early.
///
/// **An idle backend the closed pool left behind is ended here** ([#637]). In
/// CI one connection in four runs outlived `close()`: opened up to six seconds
/// before it, idle in `ClientRead`, having run no statement, with the pool
/// reporting itself closed and still counting one connection. Nothing was
/// going to hang it up, so waiting for it timed out. It holds no lock and no
/// transaction, and ending it flushes its statistics as any exit does.
///
/// **A backend that is not idle is never ended**: one that is running a
/// statement, is inside a transaction or waits on a lock is work the closed
/// pool should not have, and a lock wait there is #619's own shape. The guard
/// waits for it and, at the deadline, fails naming it.
///
/// [#637]: https://github.com/sujanto-gaws/kelir/issues/637
async fn deadlocks_once_settled(app: &TestApp) -> i64 {
    use sqlx::Connection;
    use std::time::Duration;
    use tokio::time::Instant;

    app.pool.close().await;

    let server = std::env::var("DATABASE_URL")
        .or_else(|_| std::env::var("KELIR_DATABASE_URL"))
        .expect("the server's URL");
    let mut connection = sqlx::PgConnection::connect(&server)
        .await
        .expect("reach the server");

    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        // Each backend that is left: whether it is idle, and one line that
        // says what it is doing, so a timeout names what it waited for.
        let left: Vec<(i32, bool, String)> = sqlx::query_as(
            "SELECT pid,
                    COALESCE(state = 'idle', false),
                    format(
                        'pid %s, %s, %s, waiting on %s/%s, started %s ago, in this state for %s: %s',
                        pid,
                        COALESCE(NULLIF(application_name, ''), 'no application name'),
                        COALESCE(state, 'no state'),
                        COALESCE(wait_event_type, '-'),
                        COALESCE(wait_event, '-'),
                        now() - backend_start,
                        now() - state_change,
                        left(query, 300))
             FROM pg_stat_activity
             WHERE datname = $1 AND backend_type = 'client backend'
             ORDER BY backend_start",
        )
        .bind(&app.database_name)
        .fetch_all(&mut connection)
        .await
        .expect("read pg_stat_activity");
        if left.is_empty() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the test database's backends never exited; the pool is closed: {},              and it counts {} connections, {} of them idle:
{}",
            app.pool.is_closed(),
            app.pool.size(),
            app.pool.num_idle(),
            left.iter()
                .map(|(_, _, line)| line.as_str())
                .collect::<Vec<_>>()
                .join("
")
        );
        for (pid, _, _) in left.iter().filter(|(_, idle, _)| *idle) {
            // Guarded by the state again, so a backend that took up work
            // between the read and here is left to finish it.
            sqlx::query(
                "SELECT pg_terminate_backend(pid) FROM pg_stat_activity
                 WHERE pid = $1 AND datname = $2 AND state = 'idle'",
            )
            .bind(pid)
            .bind(&app.database_name)
            .execute(&mut connection)
            .await
            .expect("end an idle backend the closed pool left");
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let deadlocks = sqlx::query_scalar("SELECT deadlocks FROM pg_stat_database WHERE datname = $1")
        .bind(&app.database_name)
        .fetch_one(&mut connection)
        .await
        .expect("read pg_stat_database");
    connection.close().await.expect("close the connection");
    deadlocks
}

/// **A decision arriving during a reassign waits for it, and does not
/// deadlock** ([#619], record 20's P2).
///
/// The task is offered to `Q`, whose `APPROVE` names `Q` and whose `REJECT`
/// names `R`. The claimant holds `Q` only, and claims it. The test holds `R`
/// `FOR UPDATE`, so a reassign to `R` stops in its `FOR KEY SHARE` on the role,
/// **after** it has locked the instance and the task. The claimant's
/// `APPROVE` is then sent, and must be seen blocked by the reassign in
/// `lock_instance`'s `FOR UPDATE` on `workflow_instances`. Released, the
/// reassign commits and answers 200. The decision then reads a task that is
/// no longer the claimant's and is offered to a role they do not hold, so it
/// answers **403**, and nothing is decided.
///
/// **The status is fixed by the staging.** A decision that ran first would be
/// 200; one judged after the reassign is 403; one aborted is 500.
///
/// [#619]: https://github.com/sujanto-gaws/kelir/issues/619
#[tokio::test]
async fn a_decision_arriving_during_a_reassign_waits_for_it() {
    use std::time::Duration;
    use tokio::time::Instant;

    let app = Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;
    let before = deadlocks_so_far(&app).await;

    let queue = worker_role(&app, "TI-LO-DE-Q").await;
    let target = worker_role(&app, "TI-LO-DE-R").await;
    let (_, claimant) = worker(&app, "ti.lo.de.claimant", &[queue]).await;

    let mut definition = workflow_for("ti_lo_de", "TI-LO-DE-Q");
    definition["transitions"][1]["allowedBy"] = json!("ROLE:TI-LO-DE-R");
    let workflow = publish_workflow_definition(&app, &token, "ti_lo_de", definition).await;
    let type_id = document_type(&app, &token, "TI_LO_DE", workflow).await;
    let document = submitted_document(&app, &token, type_id, "Decided mid-reassign").await;
    let task = open_task_of(&app, document).await;
    claim(&app, &claimant, task).await;
    let moves = workflow_history_rows(&app, document).await;

    let mut holding = app.pool.begin().await.expect("a transaction");
    let holder: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *holding)
        .await
        .expect("the holder's backend");
    sqlx::query("SELECT id FROM roles WHERE id = $1 FOR UPDATE")
        .bind(target)
        .execute(&mut *holding)
        .await
        .expect("hold the role");

    let reassigned = reassignment(&app, &token, task, json!({ "roleCode": "TI-LO-DE-R" }))();

    let deadline = Instant::now() + Duration::from_secs(30);
    let reassigning = loop {
        if let Some(pid) = blocked_by(&app, holder, "FROM roles", "FOR KEY SHARE").await {
            break pid;
        }
        assert!(
            !reassigned.is_finished(),
            "the reassign did not stop at the role"
        );
        assert!(
            Instant::now() < deadline,
            "the reassign was never seen waiting on the role"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    };

    let decided = {
        let app = Arc::clone(&app);
        tokio::spawn(async move {
            app.post(
                &format!("/api/v1/workflow/tasks/{task}/decision"),
                Some(&claimant),
                json!({ "action": "APPROVE" }),
            )
            .await
        })
    };

    let deadline = Instant::now() + Duration::from_secs(30);
    let waited_in = loop {
        if let Some(statement) = statement_blocked_by(&app, reassigning).await {
            break statement;
        }
        if decided.is_finished() {
            let answered = decided.await.expect("the decision did not panic");
            panic!(
                "the decision did not wait for the reassign: {} {}",
                answered.status, answered.body
            );
        }
        assert!(
            Instant::now() < deadline,
            "the decision was never seen waiting on the reassign"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    };

    holding.rollback().await.expect("release the role");

    let reassigned = reassigned.await.expect("the reassign did not panic");
    assert_eq!(reassigned.status, StatusCode::OK, "{}", reassigned.body);
    let decided = decided.await.expect("the decision did not panic");
    assert_eq!(
        decided.status,
        StatusCode::FORBIDDEN,
        "the decision was not judged after the reassign: {}",
        decided.body
    );

    assert!(
        locks_the_instance(&waited_in),
        "the decision waited somewhere other than the instance lock: {waited_in}"
    );
    assert_eq!(
        task_holder(&app, task).await,
        (None, Some(target), "CREATED".to_owned()),
        "the reassign did not hold"
    );
    let decisions: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM approval_decisions WHERE task_id = $1")
            .bind(task)
            .fetch_one(&app.pool)
            .await
            .expect("count the task's decisions");
    assert_eq!(decisions, 0, "a refused decision was recorded");
    assert_eq!(
        workflow_history_rows(&app, document).await,
        moves,
        "the process moved"
    );

    assert_eq!(
        deadlocks_once_settled(&app).await,
        before,
        "PostgreSQL counted a deadlock"
    );
}

/// **#619's inverted order, held by the test against one task route.**
///
/// The test takes the task's instance `FOR UPDATE` in a transaction of its
/// own, sends the route, and waits until the route is blocked by it. Then it
/// takes the task `FOR UPDATE` too, and commits.
///
/// A route that takes the instance first is blocked holding nothing, so the
/// test's task lock is granted at once and the route completes after the
/// commit. A route that takes the task first and reaches the instance only
/// through `workflow_task_history`'s foreign key holds the task while it
/// waits, and the test's task lock closes a cycle: PostgreSQL aborts one side
/// with `40P01`. Answers the route's response and the statement it waited in.
async fn instance_then_task_against(
    app: &Arc<TestApp>,
    task: Uuid,
    route: impl FnOnce() -> tokio::task::JoinHandle<common::TestResponse>,
) -> (common::TestResponse, String) {
    use std::time::Duration;
    use tokio::time::Instant;

    let instance: Uuid =
        sqlx::query_scalar("SELECT workflow_instance_id FROM workflow_tasks WHERE id = $1")
            .bind(task)
            .fetch_one(&app.pool)
            .await
            .expect("read the task's instance");

    let mut holding = app.pool.begin().await.expect("a transaction");
    let holder: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *holding)
        .await
        .expect("the holder's backend");
    sqlx::query("SELECT id FROM workflow_instances WHERE id = $1 FOR UPDATE")
        .bind(instance)
        .execute(&mut *holding)
        .await
        .expect("hold the instance");

    let sent = route();

    let deadline = Instant::now() + Duration::from_secs(30);
    let waited_in = loop {
        if let Some(statement) = statement_blocked_by(app, holder).await {
            break statement;
        }
        if sent.is_finished() {
            let answered = sent.await.expect("the route did not panic");
            panic!(
                "the route did not wait for the instance: {} {}",
                answered.status, answered.body
            );
        }
        assert!(
            Instant::now() < deadline,
            "the route was never seen waiting on the instance"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    };

    if let Err(error) = sqlx::query("SELECT id FROM workflow_tasks WHERE id = $1 FOR UPDATE")
        .bind(task)
        .execute(&mut *holding)
        .await
    {
        panic!("the test's task lock, taken after the route waited in `{waited_in}`: {error}");
    }
    holding
        .commit()
        .await
        .expect("release the instance and the task");

    (sent.await.expect("the route did not panic"), waited_in)
}

/// **A claim meeting the instance lock waits for it, and does not deadlock**
/// ([#619]).
///
/// Staged by [`instance_then_task_against`]. The claim is seen blocked in
/// `lock_instance`, then answers 200, and the claimant holds the task.
///
/// [#619]: https://github.com/sujanto-gaws/kelir/issues/619
#[tokio::test]
async fn a_claim_meeting_the_instance_lock_waits_for_it() {
    let app = Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;
    let before = deadlocks_so_far(&app).await;

    let queue = worker_role(&app, "TI-LO-CL").await;
    let (claimant_id, claimant) = worker(&app, "ti.lo.cl.claimant", &[queue]).await;

    let workflow = publish_workflow(&app, &token, "ti_lo_cl", "TI-LO-CL").await;
    let type_id = document_type(&app, &token, "TI_LO_CL", workflow).await;
    let document = submitted_document(&app, &token, type_id, "Claimed under a lock").await;
    let task = open_task_of(&app, document).await;

    let (claimed, waited_in) = instance_then_task_against(&app, task, || {
        let app = Arc::clone(&app);
        tokio::spawn(async move {
            app.post(
                &format!("/api/v1/workflow/tasks/{task}/claim"),
                Some(&claimant),
                json!({}),
            )
            .await
        })
    })
    .await;

    assert_eq!(claimed.status, StatusCode::OK, "{}", claimed.body);
    assert!(
        locks_the_instance(&waited_in),
        "the claim waited somewhere other than the instance lock: {waited_in}"
    );
    assert_eq!(
        task_holder(&app, task).await,
        (Some(claimant_id), Some(queue), "ASSIGNED".to_owned())
    );

    assert_eq!(
        deadlocks_once_settled(&app).await,
        before,
        "PostgreSQL counted a deadlock"
    );
}

/// **A hand-off meeting the instance lock waits for it, and does not
/// deadlock** ([#619]).
///
/// The holder claims the task, and hands it to a colleague. Staged by
/// [`instance_then_task_against`]. The hand-off is seen blocked in
/// `lock_instance`, then answers 200, and the colleague holds the task.
///
/// [#619]: https://github.com/sujanto-gaws/kelir/issues/619
#[tokio::test]
async fn a_hand_off_meeting_the_instance_lock_waits_for_it() {
    let app = Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;
    let before = deadlocks_so_far(&app).await;

    let queue = worker_role(&app, "TI-LO-HO").await;
    let (_, holder) = worker(&app, "ti.lo.ho.holder", &[queue]).await;
    let (colleague_id, _) = worker(&app, "ti.lo.ho.colleague", &[queue]).await;

    let workflow = publish_workflow(&app, &token, "ti_lo_ho", "TI-LO-HO").await;
    let type_id = document_type(&app, &token, "TI_LO_HO", workflow).await;
    let document = submitted_document(&app, &token, type_id, "Handed on under a lock").await;
    let task = open_task_of(&app, document).await;
    claim(&app, &holder, task).await;

    let (handed, waited_in) = instance_then_task_against(&app, task, || {
        let app = Arc::clone(&app);
        tokio::spawn(async move {
            app.post(
                &format!("/api/v1/workflow/tasks/{task}/delegation"),
                Some(&holder),
                json!({ "delegateUserId": colleague_id }),
            )
            .await
        })
    })
    .await;

    assert_eq!(handed.status, StatusCode::OK, "{}", handed.body);
    assert!(
        locks_the_instance(&waited_in),
        "the hand-off waited somewhere other than the instance lock: {waited_in}"
    );
    assert_eq!(
        task_holder(&app, task).await,
        (Some(colleague_id), Some(queue), "ASSIGNED".to_owned())
    );

    assert_eq!(
        deadlocks_once_settled(&app).await,
        before,
        "PostgreSQL counted a deadlock"
    );
}

/// **A reassign meeting the instance lock waits for it, and does not
/// deadlock** ([#619]).
///
/// Staged by [`instance_then_task_against`], which the role-held staging of
/// *a decision arriving during a reassign…* cannot replace: there the reassign
/// is parked holding both rows whatever order it took them in, so a reassign
/// that took the task first would stay green. Here it answers 200, is seen
/// blocked in `lock_instance`, and the task is offered to `R`.
///
/// [#619]: https://github.com/sujanto-gaws/kelir/issues/619
#[tokio::test]
async fn a_reassign_meeting_the_instance_lock_waits_for_it() {
    let app = Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;
    let before = deadlocks_so_far(&app).await;

    worker_role(&app, "TI-LO-RA-Q").await;
    let target = worker_role(&app, "TI-LO-RA-R").await;

    let definition = offered_to_and_decided_by("ti_lo_ra", "TI-LO-RA-Q", "TI-LO-RA-R");
    let workflow = publish_workflow_definition(&app, &token, "ti_lo_ra", definition).await;
    let type_id = document_type(&app, &token, "TI_LO_RA", workflow).await;
    let document = submitted_document(&app, &token, type_id, "Reassigned under a lock").await;
    let task = open_task_of(&app, document).await;

    let (reassigned, waited_in) = instance_then_task_against(
        &app,
        task,
        reassignment(&app, &token, task, json!({ "roleCode": "TI-LO-RA-R" })),
    )
    .await;

    assert_eq!(reassigned.status, StatusCode::OK, "{}", reassigned.body);
    assert!(
        locks_the_instance(&waited_in),
        "the reassign waited somewhere other than the instance lock: {waited_in}"
    );
    assert_eq!(
        task_holder(&app, task).await,
        (None, Some(target), "CREATED".to_owned())
    );

    assert_eq!(
        deadlocks_once_settled(&app).await,
        before,
        "PostgreSQL counted a deadlock"
    );
}

/// **The three task paths answer what they answered before the lock order
/// changed** ([#619] criterion 2).
///
/// An unknown task is a 404 about the task, read on the pool before the
/// transaction. A task whose instance is gone is a 404 about the workflow
/// instance, from `lock_instance`, as `decide` answers it. Neither writes.
///
/// [#619]: https://github.com/sujanto-gaws/kelir/issues/619
#[tokio::test]
async fn a_task_path_refuses_a_missing_task_or_instance_with_a_404() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let queue = worker_role(&app, "TI-LO-NF").await;
    let (_, holder) = worker(&app, "ti.lo.nf.holder", &[queue]).await;
    let (colleague_id, _) = worker(&app, "ti.lo.nf.colleague", &[queue]).await;

    let workflow = publish_workflow(&app, &token, "ti_lo_nf", "TI-LO-NF").await;
    let type_id = document_type(&app, &token, "TI_LO_NF", workflow).await;
    let document = submitted_document(&app, &token, type_id, "Missing pieces").await;
    let task = open_task_of(&app, document).await;
    claim(&app, &holder, task).await;

    let paths = |task: Uuid| {
        [
            (format!("/api/v1/workflow/tasks/{task}/claim"), json!({})),
            (
                format!("/api/v1/workflow/tasks/{task}/delegation"),
                json!({ "delegateUserId": colleague_id }),
            ),
            (
                format!("/api/v1/workflow/tasks/{task}/reassign"),
                json!({ "userId": colleague_id }),
            ),
        ]
    };

    for (path, body) in paths(Uuid::now_v7()) {
        let caller = if path.ends_with("/reassign") {
            &token
        } else {
            &holder
        };
        let answered = app.post(&path, Some(caller), body).await;
        assert_eq!(
            answered.status,
            StatusCode::NOT_FOUND,
            "{path}: {}",
            answered.body
        );
        assert_eq!(
            answered.body["error"]["message"], "Task not found",
            "{path}"
        );
    }

    sqlx::query(
        "UPDATE workflow_instances SET deleted_at = now()
         WHERE id = (SELECT workflow_instance_id FROM workflow_tasks WHERE id = $1)",
    )
    .bind(task)
    .execute(&app.pool)
    .await
    .expect("remove the instance");

    for (path, body) in paths(task) {
        let caller = if path.ends_with("/reassign") {
            &token
        } else {
            &holder
        };
        let answered = app.post(&path, Some(caller), body).await;
        assert_eq!(
            answered.status,
            StatusCode::NOT_FOUND,
            "{path}: {}",
            answered.body
        );
        assert_eq!(
            answered.body["error"]["message"], "Workflow instance not found",
            "{path}"
        );
    }

    let history: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM workflow_task_history WHERE task_id = $1")
            .bind(task)
            .fetch_one(&app.pool)
            .await
            .expect("count the task's history");
    assert_eq!(
        history, 2,
        "a refused path wrote history: creation and the claim only"
    );
}

// ---------------------------------------------------------------------------
// #663 — a resubmit takes the instance before the document
// ---------------------------------------------------------------------------
//
// A resubmit locked the document `FOR UPDATE`, then the instance. Every task
// path locks the instance, then the task, then inserts a
// `workflow_task_history` row, and that row's foreign key to `documents` takes
// `FOR KEY SHARE` on the document. A resubmit arriving while a task path held
// the instance took the document and waited for the instance; the task path
// then waited for the document, and PostgreSQL aborted one of them. Record
// 21's P1 caught it with a reassign of the owner's correction task, and the
// resubmit answered 500.
//
// # Seen red on the unfixed code, 2026-10-08
//
// Both tests below: the reassign answered 200, and the resubmit answered 500
// `INTERNAL_ERROR`, *deadlock detected* in the log. The submit now locks the
// instance before the document, and both are green.

/// A workflow whose `RETURN` sends the document to `RETURNED`, where the
/// owner's correction task waits. Its `RESUBMIT` goes back to
/// `MANAGER_APPROVAL`, which raises a task, or with `rechecked` to `RECHECK`,
/// which raises none.
fn returned_for_correction(key: &str, queue: &str, rechecked: bool) -> Value {
    let mut definition = json!({
        "workflowKey": key,
        "version": "1.0.0",
        "name": "With a correction",
        "initialState": "MANAGER_APPROVAL",
        "states": [
            { "code": "MANAGER_APPROVAL", "name": "Manager approval",
              "mapsToDocumentStatus": "PENDING_APPROVAL",
              "task": { "taskDefinitionKey": "manager_approval", "taskName": "Decide",
                        "assignment": { "assigneeType": "ROLE", "roleCode": queue } } },
            { "code": "RETURNED", "name": "Returned to the author",
              "mapsToDocumentStatus": "RETURNED",
              "task": { "taskDefinitionKey": "correct_it", "taskName": "Correct the request",
                        "assignment": { "assigneeType": "OWNER" } } },
            { "code": "COMPLETED", "name": "Completed", "mapsToDocumentStatus": "COMPLETED",
              "isFinal": true }
        ],
        "transitions": [
            { "from": "MANAGER_APPROVAL", "to": "COMPLETED", "action": "APPROVE",
              "allowedBy": format!("ROLE:{queue}") },
            { "from": "MANAGER_APPROVAL", "to": "RETURNED", "action": "RETURN",
              "allowedBy": format!("ROLE:{queue}") },
            { "from": "RETURNED", "to": "MANAGER_APPROVAL", "action": "RESUBMIT",
              "allowedBy": "OWNER" }
        ]
    });

    if rechecked {
        // S6 refuses a state nothing reaches, so `RECHECK` is declared only
        // where `RESUBMIT` goes to it.
        definition["states"].as_array_mut().expect("states").push(
            json!({ "code": "RECHECK", "name": "Rechecked without a task",
                          "mapsToDocumentStatus": "PENDING_APPROVAL" }),
        );
        definition["transitions"][2]["to"] = json!("RECHECK");
        definition["transitions"]
            .as_array_mut()
            .expect("transitions")
            .push(
                json!({ "from": "RECHECK", "to": "COMPLETED", "action": "APPROVE",
                          "allowedBy": format!("ROLE:{queue}") }),
            );
    }

    definition
}

/// A document the administrator created and submitted, returned by an
/// approver holding `queue`. Answers the document and the owner's correction
/// task.
async fn returned_document(
    app: &TestApp,
    token: &str,
    code: &str,
    queue: &str,
    rechecked: bool,
) -> (Uuid, Uuid) {
    let role = worker_role(app, queue).await;
    let (_, approver) = worker(app, &format!("{}.approver", code.to_lowercase()), &[role]).await;

    let key = code.to_lowercase();
    let definition = returned_for_correction(&key, queue, rechecked);
    let workflow = publish_workflow_definition(app, token, &key, definition).await;
    let type_id = document_type(app, token, code, workflow).await;
    let document = submitted_document(app, token, type_id, "Returned for correction").await;
    let manager = open_task_of(app, document).await;

    let returned = app
        .post(
            &format!("/api/v1/workflow/tasks/{manager}/decision"),
            Some(&approver),
            json!({ "action": "RETURN", "comment": "Fix the amount" }),
        )
        .await;
    assert_eq!(returned.status, StatusCode::OK, "{}", returned.body);

    let correction = open_task_of(app, document).await;
    assert_ne!(correction, manager);

    (document, correction)
}

/// **Record 21's P1, staged with real routes on both sides.**
///
/// The test holds `target` `FOR UPDATE`, so an administrator's reassign of the
/// correction task to it stops in its `FOR KEY SHARE` on the role, after it
/// has locked the instance and the task. The owner's resubmit is then sent,
/// and must be seen blocked by the reassign. The role is released. Answers the
/// reassign's response, the resubmit's, and the statement the resubmit waited
/// in.
async fn resubmit_during_a_reassign(
    app: &Arc<TestApp>,
    token: &str,
    document: Uuid,
    correction: Uuid,
    target: Uuid,
    target_code: &str,
) -> (common::TestResponse, common::TestResponse, String) {
    use std::time::Duration;
    use tokio::time::Instant;

    let mut holding = app.pool.begin().await.expect("a transaction");
    let holder: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *holding)
        .await
        .expect("the holder's backend");
    sqlx::query("SELECT id FROM roles WHERE id = $1 FOR UPDATE")
        .bind(target)
        .execute(&mut *holding)
        .await
        .expect("hold the role");

    let reassigned = reassignment(app, token, correction, json!({ "roleCode": target_code }))();

    let deadline = Instant::now() + Duration::from_secs(30);
    let reassigning = loop {
        if let Some(pid) = blocked_by(app, holder, "FROM roles", "FOR KEY SHARE").await {
            break pid;
        }
        assert!(
            !reassigned.is_finished(),
            "the reassign did not stop at the role"
        );
        assert!(
            Instant::now() < deadline,
            "the reassign was never seen waiting on the role"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    };

    let resubmitted = {
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

    let deadline = Instant::now() + Duration::from_secs(30);
    let waited_in = loop {
        if let Some(statement) = statement_blocked_by(app, reassigning).await {
            break statement;
        }
        if resubmitted.is_finished() {
            let answered = resubmitted.await.expect("the resubmit did not panic");
            panic!(
                "the resubmit did not wait for the reassign: {} {}",
                answered.status, answered.body
            );
        }
        assert!(
            Instant::now() < deadline,
            "the resubmit was never seen waiting on the reassign"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    };

    holding.rollback().await.expect("release the role");

    (
        reassigned.await.expect("the reassign did not panic"),
        resubmitted.await.expect("the resubmit did not panic"),
        waited_in,
    )
}

/// The document's status and its instance's state, straight from the rows.
async fn document_and_instance(app: &TestApp, document: Uuid) -> (String, String) {
    sqlx::query_as(
        "SELECT d.status, i.current_state
         FROM documents d JOIN workflow_instances i ON i.id = d.process_instance_id
         WHERE d.id = $1",
    )
    .bind(document)
    .fetch_one(&app.pool)
    .await
    .expect("read the document and its instance")
}

/// **A resubmit arriving during a reassign of the correction task waits for
/// it, and does not deadlock** ([#663], record 21's P1).
///
/// Staged by [`resubmit_during_a_reassign`]. The resubmit is seen blocked in
/// `lock_instance`'s `FOR UPDATE` on `workflow_instances`. Released, the
/// reassign commits and answers 200. The resubmit then fires `RESUBMIT` into
/// `MANAGER_APPROVAL`, whose task would sit beside the correction task that
/// is still open, so it answers **409**, the refusal [#667] is about. What
/// this test holds is that it is that 409 and not a 500.
///
/// [#663]: https://github.com/sujanto-gaws/kelir/issues/663
/// [#667]: https://github.com/sujanto-gaws/kelir/issues/667
#[tokio::test]
async fn a_resubmit_arriving_during_a_reassign_of_the_correction_waits_for_it() {
    let app = Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;
    let before = deadlocks_so_far(&app).await;

    let target = worker_role(&app, "TI-RS-RT-R").await;
    let (document, correction) =
        returned_document(&app, &token, "TI_RS_RT", "TI-RS-RT-Q", false).await;

    let (reassigned, resubmitted, waited_in) =
        resubmit_during_a_reassign(&app, &token, document, correction, target, "TI-RS-RT-R").await;

    assert_eq!(reassigned.status, StatusCode::OK, "{}", reassigned.body);
    assert_eq!(
        resubmitted.status,
        StatusCode::CONFLICT,
        "the resubmit was not judged after the reassign: {}",
        resubmitted.body
    );
    assert!(
        resubmitted.body["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("already has an open task")),
        "the resubmit was refused for another reason: {}",
        resubmitted.body
    );
    assert!(
        locks_the_instance(&waited_in),
        "the resubmit waited somewhere other than the instance lock: {waited_in}"
    );

    assert_eq!(
        task_holder(&app, correction).await,
        (None, Some(target), "CREATED".to_owned()),
        "the reassign did not hold"
    );
    assert_eq!(
        document_and_instance(&app, document).await,
        ("RETURNED".to_owned(), "RETURNED".to_owned()),
        "a refused resubmit moved something"
    );

    assert_eq!(
        deadlocks_once_settled(&app).await,
        before,
        "PostgreSQL counted a deadlock"
    );
}

/// **A resubmit to a state with no task, arriving during a reassign, waits
/// for it and then moves the process** ([#663]).
///
/// The shape [#667] does not refuse: `RESUBMIT` goes to `RECHECK`, which
/// raises no task. Staged by [`resubmit_during_a_reassign`]. The resubmit is
/// seen blocked in `lock_instance`. Released, the reassign answers 200, and
/// the resubmit answers 200 with the document `PENDING_APPROVAL` and the
/// instance at `RECHECK`.
///
/// [#663]: https://github.com/sujanto-gaws/kelir/issues/663
/// [#667]: https://github.com/sujanto-gaws/kelir/issues/667
#[tokio::test]
async fn a_resubmit_to_a_state_with_no_task_during_a_reassign_waits_and_moves() {
    let app = Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;
    let before = deadlocks_so_far(&app).await;

    let target = worker_role(&app, "TI-RS-NT-R").await;
    let (document, correction) =
        returned_document(&app, &token, "TI_RS_NT", "TI-RS-NT-Q", true).await;

    let (reassigned, resubmitted, waited_in) =
        resubmit_during_a_reassign(&app, &token, document, correction, target, "TI-RS-NT-R").await;

    assert_eq!(reassigned.status, StatusCode::OK, "{}", reassigned.body);
    assert_eq!(
        resubmitted.status,
        StatusCode::OK,
        "the resubmit did not go through after the reassign: {}",
        resubmitted.body
    );
    assert_eq!(resubmitted.data()["status"], json!("PENDING_APPROVAL"));
    assert!(
        locks_the_instance(&waited_in),
        "the resubmit waited somewhere other than the instance lock: {waited_in}"
    );

    assert_eq!(
        task_holder(&app, correction).await,
        (None, Some(target), "CREATED".to_owned()),
        "the reassign did not hold"
    );
    assert_eq!(
        document_and_instance(&app, document).await,
        ("PENDING_APPROVAL".to_owned(), "RECHECK".to_owned())
    );

    assert_eq!(
        deadlocks_once_settled(&app).await,
        before,
        "PostgreSQL counted a deadlock"
    );
}

// ---------------------------------------------------------------------------
// #663 — the resubmit meets every task path (plan 18 row 18's campaign)
// ---------------------------------------------------------------------------
//
// The two tests above race the resubmit against a reassign, parked at the
// role it is moving the task to. A claim, a hand-off and a decision take no
// lock between the instance and their history row that a test can hold, so
// these park each of them **at its own `workflow_task_history` `INSERT`**: a
// `BEFORE INSERT` row trigger waits on an advisory lock the test holds. A row
// trigger fires before the row's foreign keys are checked, so the task path is
// stopped holding the instance and the task, and not yet the document. That
// is the moment record 21's P1 needed, reached on every path.
//
// # Seen red on main's code (`da594d5`'s `src/` and `.sqlx/`), 2026-10-08
//
// | Test | How it went red |
// |---|---|
// | *…during a claim of the correction…* | The claim answered 200; the resubmit answered 500 `INTERNAL_ERROR`, *deadlock detected* in the log |
// | *…during a hand-off of the correction…* | The hand-off answered 200; the resubmit answered 500, *deadlock detected* |
// | *…during a decision on the correction…* | The decision answered 200; the resubmit answered 500 rather than 409, *deadlock detected* |
// | *a first submit overtaken by a submit and a return…* | 409, but from `start_workflow`'s *"this document already has a live approval"*: main has no instance check, so the overtaken submit took the first-submit path on a returned document, set a new number in its own transaction, and was refused only when it tried to start a second process. It rolled back |
//
// Each resubmit was seen blocked in `lock_instance`'s statement on main too: it
// held the document there, which is the difference the deadlock shows. The two
// reassign tests above went red on the same run, as their own header says.
//
// # Mutations of the fix, each applied alone and reverted, 2026-10-08
//
// The #663 tests above and below, and `workflow_engine`'s two resubmit
// tests, run against each.
//
// | Mutation | Result |
// |---|---|
// | The old order: the instance locked after the document, the check kept | Red: the claim, hand-off and decision tests here and both reassign tests answer 500, *deadlock detected* |
// | The instance check dropped | Red only in *a first submit overtaken…*, and **only by its message**: `start_workflow` refuses a second live process with a 409 of its own and the transaction rolls back. The check is a backstop that answers first and names the cause; nothing else observable changes |
// | The instance locked after the document on a first submit only | Green, and no test can redden it: a document read as a draft has no process (only a first submit writes `process_instance_id`, and nothing moves a document back to `DRAFT`), so a first submit locks no instance in either place |
// | `None` passed to `resubmit_workflow` for the instance | Red: every resubmit that reaches the workflow answers 500, *points at instance …, which does not exist*; the claim, hand-off and both reassign tests, and `workflow_engine`'s two resubmit tests. The decision test stays green, its resubmit refused as not editable before the workflow |

/// A workflow whose `RETURN` sends the document to `RETURNED`, where a
/// correction task is offered to `corrector`. A holder of `corrector` may claim
/// it, hand it on, or `REJECT` the document from there. `RESUBMIT` goes to
/// `RECHECK`, which raises no task, so [#667]'s refusal never decides the
/// outcome.
///
/// [#667]: https://github.com/sujanto-gaws/kelir/issues/667
fn corrected_by_a_role(key: &str, queue: &str, corrector: &str) -> Value {
    json!({
        "workflowKey": key,
        "version": "1.0.0",
        "name": "Corrected by a role",
        "initialState": "MANAGER_APPROVAL",
        "states": [
            { "code": "MANAGER_APPROVAL", "name": "Manager approval",
              "mapsToDocumentStatus": "PENDING_APPROVAL",
              "task": { "taskDefinitionKey": "manager_approval", "taskName": "Decide",
                        "assignment": { "assigneeType": "ROLE", "roleCode": queue } } },
            { "code": "RETURNED", "name": "Returned for correction",
              "mapsToDocumentStatus": "RETURNED",
              "task": { "taskDefinitionKey": "correct_it", "taskName": "Correct the request",
                        "assignment": { "assigneeType": "ROLE", "roleCode": corrector } } },
            { "code": "RECHECK", "name": "Rechecked without a task",
              "mapsToDocumentStatus": "PENDING_APPROVAL" },
            { "code": "REJECTED", "name": "Rejected", "mapsToDocumentStatus": "REJECTED",
              "isFinal": true },
            { "code": "COMPLETED", "name": "Completed", "mapsToDocumentStatus": "COMPLETED",
              "isFinal": true }
        ],
        "transitions": [
            { "from": "MANAGER_APPROVAL", "to": "COMPLETED", "action": "APPROVE",
              "allowedBy": format!("ROLE:{queue}") },
            { "from": "MANAGER_APPROVAL", "to": "RETURNED", "action": "RETURN",
              "allowedBy": format!("ROLE:{queue}") },
            { "from": "RETURNED", "to": "RECHECK", "action": "RESUBMIT",
              "allowedBy": "OWNER" },
            { "from": "RETURNED", "to": "REJECTED", "action": "REJECT",
              "allowedBy": format!("ROLE:{corrector}") },
            { "from": "RECHECK", "to": "COMPLETED", "action": "APPROVE",
              "allowedBy": format!("ROLE:{queue}") }
        ]
    })
}

/// A document [`corrected_by_a_role`] has returned, and who can work its
/// correction task.
struct Correction {
    document: Uuid,
    task: Uuid,
    role: Uuid,
    corrector_id: Uuid,
    corrector: String,
    colleague_id: Uuid,
}

/// The administrator creates and submits the document, an approver returns
/// it, and the correction task is offered to `<code>-C`, which a corrector and
/// a colleague hold.
async fn returned_to_a_role(app: &TestApp, token: &str, code: &str) -> Correction {
    let roles = code.replace('_', "-");
    let users = code.to_lowercase().replace('_', ".");
    let key = code.to_lowercase();

    let queue = worker_role(app, &format!("{roles}-Q")).await;
    let role = worker_role(app, &format!("{roles}-C")).await;
    let (_, approver) = worker(app, &format!("{users}.approver"), &[queue]).await;
    let (corrector_id, corrector) = worker(app, &format!("{users}.corrector"), &[role]).await;
    let (colleague_id, _) = worker(app, &format!("{users}.colleague"), &[role]).await;

    let definition = corrected_by_a_role(&key, &format!("{roles}-Q"), &format!("{roles}-C"));
    let workflow = publish_workflow_definition(app, token, &key, definition).await;
    let type_id = document_type(app, token, code, workflow).await;
    let document = submitted_document(app, token, type_id, "Returned to a role").await;
    let manager = open_task_of(app, document).await;

    let returned = app
        .post(
            &format!("/api/v1/workflow/tasks/{manager}/decision"),
            Some(&approver),
            json!({ "action": "RETURN", "comment": "Fix the amount" }),
        )
        .await;
    assert_eq!(returned.status, StatusCode::OK, "{}", returned.body);

    let task = open_task_of(app, document).await;
    assert_ne!(task, manager);

    Correction {
        document,
        task,
        role,
        corrector_id,
        corrector,
        colleague_id,
    }
}

/// Sends `first`, a task path on the correction task, and stops it at its
/// `workflow_task_history` `INSERT`, holding the instance and the task. Then
/// sends the owner's resubmit of `document`, and waits until it is seen
/// blocked by `first` in `pg_stat_activity`. Then opens the gate. Answers the
/// task path's response, the resubmit's, and the statement the resubmit
/// waited in.
///
/// The gate is [`second_waits_on_first`]'s: a trigger that takes a shared
/// advisory lock the test holds exclusively on a connection of its own, and
/// the first request seen stopped in `pg_locks`. It is created after the
/// staging, so only the raced requests meet it.
async fn resubmit_during_a_held_task_path(
    app: &Arc<TestApp>,
    token: &str,
    document: Uuid,
    first: impl FnOnce() -> tokio::task::JoinHandle<common::TestResponse>,
) -> (common::TestResponse, common::TestResponse, String) {
    use sqlx::{Connection, PgConnection};
    use std::time::Duration;
    use tokio::time::Instant;

    /// The advisory key the trigger waits on; the issue's number.
    const GATE: i64 = 663;
    const PATIENCE: Duration = Duration::from_secs(30);

    sqlx::query(&format!(
        "CREATE FUNCTION test_history_gate() RETURNS trigger LANGUAGE plpgsql AS $$
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
        "CREATE TRIGGER test_history_gate BEFORE INSERT ON workflow_task_history
         FOR EACH ROW EXECUTE FUNCTION test_history_gate()",
    )
    .execute(&app.pool)
    .await
    .expect("the gate's trigger");

    let mut gate = PgConnection::connect_with(&app.pool.connect_options())
        .await
        .expect("the gate's connection");
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(GATE)
        .execute(&mut gate)
        .await
        .expect("close the gate");

    let first = first();

    let deadline = Instant::now() + PATIENCE;
    let held: i32 = loop {
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
        if first.is_finished() {
            let answered = first.await.expect("the task path did not panic");
            panic!(
                "the task path finished without reaching its history row: {} {}",
                answered.status, answered.body
            );
        }
        assert!(
            Instant::now() < deadline,
            "the task path never reached its history row"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    };

    let resubmitted = submission(app, token, document)();

    let deadline = Instant::now() + PATIENCE;
    let waited_in = loop {
        if let Some(statement) = statement_blocked_by(app, held).await {
            break statement;
        }
        if resubmitted.is_finished() {
            let answered = resubmitted.await.expect("the resubmit did not panic");
            panic!(
                "the resubmit did not wait for the task path: {} {}",
                answered.status, answered.body
            );
        }
        assert!(
            Instant::now() < deadline,
            "the resubmit was never seen waiting on the task path"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    };

    sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(GATE)
        .execute(&mut gate)
        .await
        .expect("open the gate");
    gate.close().await.expect("close the gate's connection");

    let first = tokio::time::timeout(PATIENCE, first)
        .await
        .expect("the task path did not finish once the gate opened")
        .expect("the task path did not panic");
    let resubmitted = tokio::time::timeout(PATIENCE, resubmitted)
        .await
        .expect("the resubmit did not finish once the task path committed")
        .expect("the resubmit did not panic");

    (first, resubmitted, waited_in)
}

/// **A resubmit arriving during a claim of the correction task waits for it,
/// and does not deadlock** ([#663]).
///
/// The claim is stopped at its history row by
/// [`resubmit_during_a_held_task_path`], and the resubmit is seen blocked in
/// `lock_instance`. Released, both answer 200: the corrector holds the task,
/// and the process is at `RECHECK`.
///
/// [#663]: https://github.com/sujanto-gaws/kelir/issues/663
#[tokio::test]
async fn a_resubmit_arriving_during_a_claim_of_the_correction_waits_for_it() {
    let app = Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;
    let before = deadlocks_so_far(&app).await;

    let correction = returned_to_a_role(&app, &token, "TI_RS_CL").await;
    let task = correction.task;

    let (claimed, resubmitted, waited_in) =
        resubmit_during_a_held_task_path(&app, &token, correction.document, || {
            let app = Arc::clone(&app);
            let corrector = correction.corrector.clone();
            tokio::spawn(async move {
                app.post(
                    &format!("/api/v1/workflow/tasks/{task}/claim"),
                    Some(&corrector),
                    json!({}),
                )
                .await
            })
        })
        .await;

    assert_eq!(claimed.status, StatusCode::OK, "{}", claimed.body);
    assert_eq!(
        resubmitted.status,
        StatusCode::OK,
        "the resubmit did not go through after the claim: {}",
        resubmitted.body
    );
    assert!(
        locks_the_instance(&waited_in),
        "the resubmit waited somewhere other than the instance lock: {waited_in}"
    );

    assert_eq!(
        task_holder(&app, task).await,
        (
            Some(correction.corrector_id),
            Some(correction.role),
            "ASSIGNED".to_owned()
        ),
        "the claim did not hold"
    );
    assert_eq!(
        document_and_instance(&app, correction.document).await,
        ("PENDING_APPROVAL".to_owned(), "RECHECK".to_owned())
    );

    assert_eq!(
        deadlocks_once_settled(&app).await,
        before,
        "PostgreSQL counted a deadlock"
    );
}

/// **A resubmit arriving during a hand-off of the correction task waits for
/// it, and does not deadlock** ([#663]).
///
/// The corrector claims the task, then hands it to a colleague. The hand-off
/// is stopped at its history row by [`resubmit_during_a_held_task_path`], and
/// the resubmit is seen blocked in `lock_instance`. Released, both answer 200:
/// the colleague holds the task, and the process is at `RECHECK`.
///
/// [#663]: https://github.com/sujanto-gaws/kelir/issues/663
#[tokio::test]
async fn a_resubmit_arriving_during_a_hand_off_of_the_correction_waits_for_it() {
    let app = Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;
    let before = deadlocks_so_far(&app).await;

    let correction = returned_to_a_role(&app, &token, "TI_RS_HO").await;
    let task = correction.task;
    claim(&app, &correction.corrector, task).await;

    let (handed, resubmitted, waited_in) =
        resubmit_during_a_held_task_path(&app, &token, correction.document, || {
            let app = Arc::clone(&app);
            let corrector = correction.corrector.clone();
            let colleague = correction.colleague_id;
            tokio::spawn(async move {
                app.post(
                    &format!("/api/v1/workflow/tasks/{task}/delegation"),
                    Some(&corrector),
                    json!({ "delegateUserId": colleague }),
                )
                .await
            })
        })
        .await;

    assert_eq!(handed.status, StatusCode::OK, "{}", handed.body);
    assert_eq!(
        resubmitted.status,
        StatusCode::OK,
        "the resubmit did not go through after the hand-off: {}",
        resubmitted.body
    );
    assert!(
        locks_the_instance(&waited_in),
        "the resubmit waited somewhere other than the instance lock: {waited_in}"
    );

    assert_eq!(
        task_holder(&app, task).await,
        (
            Some(correction.colleague_id),
            Some(correction.role),
            "ASSIGNED".to_owned()
        ),
        "the hand-off did not hold"
    );
    assert_eq!(
        document_and_instance(&app, correction.document).await,
        ("PENDING_APPROVAL".to_owned(), "RECHECK".to_owned())
    );

    assert_eq!(
        deadlocks_once_settled(&app).await,
        before,
        "PostgreSQL counted a deadlock"
    );
}

/// **A resubmit arriving during a decision on the correction task waits for
/// it, and is refused with a 409** ([#663]).
///
/// The corrector claims the task and rejects the document. The decision is
/// stopped at its history row by [`resubmit_during_a_held_task_path`], before
/// it updates the document, and the resubmit is seen blocked in
/// `lock_instance`. Released, the decision answers 200 and the document is
/// `REJECTED`. The resubmit then reads a document that is not editable, so it
/// answers 409 and moves nothing.
///
/// [#663]: https://github.com/sujanto-gaws/kelir/issues/663
#[tokio::test]
async fn a_resubmit_arriving_during_a_decision_on_the_correction_waits_for_it() {
    let app = Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;
    let before = deadlocks_so_far(&app).await;

    let correction = returned_to_a_role(&app, &token, "TI_RS_DE").await;
    let task = correction.task;
    claim(&app, &correction.corrector, task).await;

    let (decided, resubmitted, waited_in) =
        resubmit_during_a_held_task_path(&app, &token, correction.document, || {
            let app = Arc::clone(&app);
            let corrector = correction.corrector.clone();
            tokio::spawn(async move {
                app.post(
                    &format!("/api/v1/workflow/tasks/{task}/decision"),
                    Some(&corrector),
                    json!({ "action": "REJECT", "comment": "Not worth correcting" }),
                )
                .await
            })
        })
        .await;

    assert_eq!(decided.status, StatusCode::OK, "{}", decided.body);
    assert_eq!(
        resubmitted.status,
        StatusCode::CONFLICT,
        "the resubmit was not judged after the decision: {}",
        resubmitted.body
    );
    assert!(
        resubmitted.body["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("only a draft or a returned document")),
        "the resubmit was refused for another reason: {}",
        resubmitted.body
    );
    assert!(
        locks_the_instance(&waited_in),
        "the resubmit waited somewhere other than the instance lock: {waited_in}"
    );

    assert_eq!(
        task_holder(&app, task).await,
        (
            Some(correction.corrector_id),
            Some(correction.role),
            "COMPLETED".to_owned()
        ),
        "the decision did not hold"
    );
    assert_eq!(
        document_and_instance(&app, correction.document).await,
        ("REJECTED".to_owned(), "REJECTED".to_owned()),
        "a refused resubmit moved something"
    );

    assert_eq!(
        deadlocks_once_settled(&app).await,
        before,
        "PostgreSQL counted a deadlock"
    );
}

/// A draft of `type_id` requested for `department`.
async fn draft_for(app: &TestApp, token: &str, type_id: Uuid, department: Uuid) -> Uuid {
    let created = app
        .post(
            "/api/v1/documents",
            Some(token),
            json!({
                "documentTypeId": type_id,
                "title": "Requested for a department",
                "requestedForDepartmentId": department,
                "formData": { "amount": 1_000 },
            }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    id_of(&created.body["data"])
}

/// A document's status, number and instance, straight from the row.
async fn status_number_and_instance(
    app: &TestApp,
    document: Uuid,
) -> (String, Option<String>, Option<Uuid>) {
    sqlx::query_as(
        "SELECT status, document_number, process_instance_id FROM documents WHERE id = $1",
    )
    .bind(document)
    .fetch_one(&app.pool)
    .await
    .expect("read the document")
}

/// **A first submit overtaken by another submit and a return is refused with
/// a 409, not a 500** ([#663], the fix's instance check).
///
/// The fix locks the instance the pool read named, and refuses a document that,
/// once locked, names another. Only a submit that starts a process between the
/// two reads can make them differ, and two first submits alone never reach
/// the check: the loser finds the document under approval and is refused as
/// not editable first. The document has to be editable again with a process,
/// which only a return makes it.
///
/// So the type numbers per department and tolerates gaps, and the test holds
/// the first department's bucket. The owner's first submit reads the draft,
/// with no process, and stops in `allocate_committed`, holding nothing on the
/// document. The owner then moves the draft to a second department, submits
/// it again, which takes the second department's number and starts a process,
/// and an approver returns it. Released, the first submit takes a number from
/// the first bucket, locks the document, finds it `RETURNED` on a process it
/// did not lock, and answers **409**. The document keeps the number, the
/// process and the status the overtaking submit and the return gave it.
///
/// [#663]: https://github.com/sujanto-gaws/kelir/issues/663
#[tokio::test]
async fn a_first_submit_overtaken_by_a_submit_and_a_return_is_refused_with_a_409() {
    use std::time::Duration;
    use tokio::time::Instant;

    let app = Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;
    let before = deadlocks_so_far(&app).await;

    let queue = worker_role(&app, "TI-RS-OT-Q").await;
    let (_, approver) = worker(&app, "ti.rs.ot.approver", &[queue]).await;
    let definition = returned_for_correction("ti_rs_ot", "TI-RS-OT-Q", false);
    let workflow = publish_workflow_definition(&app, &token, "ti_rs_ot", definition).await;
    let type_id = document_type(&app, &token, "TI_RS_OT", workflow).await;
    let rule = app
        .put(
            &format!("/api/v1/document-types/{type_id}/numbering-rule"),
            Some(&token),
            json!({
                "ruleTemplate": "TI_RS_OT-{department}-{year}-{sequence}",
                "sequenceScope": "DEPARTMENT_YEAR",
                "gapPolicy": "ALLOW_GAPS",
            }),
        )
        .await;
    assert_eq!(rule.status, StatusCode::OK, "{}", rule.body);

    let first_department = department(&app, "TI-RS-OT-1").await;
    let second_department = department(&app, "TI-RS-OT-2").await;

    // Another document opens the first department's bucket, so there is a
    // row to hold.
    let opener = draft_for(&app, &token, type_id, first_department).await;
    let opened = submission(&app, &token, opener)()
        .await
        .expect("the opener did not panic");
    assert_eq!(opened.status, StatusCode::OK, "{}", opened.body);

    let document = draft_for(&app, &token, type_id, first_department).await;

    let mut holding = app.pool.begin().await.expect("a transaction");
    let holder: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *holding)
        .await
        .expect("the holder's backend");
    sqlx::query(
        "SELECT id FROM document_type_sequence_buckets
         WHERE document_type_id = $1 AND sequence_key LIKE $2 FOR UPDATE",
    )
    .bind(type_id)
    .bind(format!("{first_department}:%"))
    .fetch_one(&mut *holding)
    .await
    .expect("hold the first department's bucket");

    let waiting = submission(&app, &token, document)();

    let deadline = Instant::now() + Duration::from_secs(30);
    let waited_in = loop {
        if let Some(statement) = statement_blocked_by(&app, holder).await {
            break statement;
        }
        if waiting.is_finished() {
            let answered = waiting.await.expect("the first submit did not panic");
            panic!(
                "the first submit did not wait at the bucket: {} {}",
                answered.status, answered.body
            );
        }
        assert!(
            Instant::now() < deadline,
            "the first submit was never seen waiting at the bucket"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    assert!(
        waited_in.contains("document_type_sequence_buckets"),
        "the first submit waited somewhere other than the bucket: {waited_in}"
    );

    let moved = app
        .put(
            &format!("/api/v1/documents/{document}"),
            Some(&token),
            json!({ "requestedForDepartmentId": second_department }),
        )
        .await;
    assert_eq!(moved.status, StatusCode::OK, "{}", moved.body);

    let overtaking = submission(&app, &token, document)()
        .await
        .expect("the overtaking submit did not panic");
    assert_eq!(overtaking.status, StatusCode::OK, "{}", overtaking.body);

    let manager = open_task_of(&app, document).await;
    let returned = app
        .post(
            &format!("/api/v1/workflow/tasks/{manager}/decision"),
            Some(&approver),
            json!({ "action": "RETURN", "comment": "Fix the amount" }),
        )
        .await;
    assert_eq!(returned.status, StatusCode::OK, "{}", returned.body);

    let (status, number, instance) = status_number_and_instance(&app, document).await;
    assert_eq!(status, "RETURNED");
    assert!(
        instance.is_some(),
        "the overtaking submit started no process"
    );

    holding.rollback().await.expect("release the bucket");

    let refused = tokio::time::timeout(Duration::from_secs(30), waiting)
        .await
        .expect("the first submit did not finish once the bucket was released")
        .expect("the first submit did not panic");
    assert_eq!(
        refused.status,
        StatusCode::CONFLICT,
        "the overtaken submit was not refused: {}",
        refused.body
    );
    assert!(
        refused.body["error"]["message"].as_str().is_some_and(
            |message| message.contains("workflow changed while it was being submitted")
        ),
        "the overtaken submit was refused for another reason: {}",
        refused.body
    );

    assert_eq!(
        status_number_and_instance(&app, document).await,
        ("RETURNED".to_owned(), number, instance),
        "a refused submit moved the document"
    );
    let instances: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM workflow_instances WHERE document_id = $1")
            .bind(document)
            .fetch_one(&app.pool)
            .await
            .expect("count the document's instances");
    assert_eq!(instances, 1, "a refused submit started a second process");

    assert_eq!(
        deadlocks_once_settled(&app).await,
        before,
        "PostgreSQL counted a deadlock"
    );
}

/// **A reassign writes every holder column, clearing what it does not name**
/// ([#512], ADR-0042 §2).
///
/// The task is claimed, scoped to a department, and carries a delegation's
/// `delegated_from_user_id`, all written directly. A reassign to a role names
/// no department and no delegation, so both are cleared along with the claim.
///
/// **Seen red** with `COALESCE($5, candidate_department_id)` and with
/// `COALESCE($6, delegated_from_user_id)` in `repository::task::reassign`.
///
/// [#512]: https://github.com/sujanto-gaws/kelir/issues/512
#[tokio::test]
async fn a_reassign_clears_the_department_and_the_delegation() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    worker_role(&app, "TI-RA-HC-Q").await;
    let target = worker_role(&app, "TI-RA-HC-R").await;
    let (holder, _) = worker(&app, "ti.ra.hc.holder", &[]).await;
    let (delegator, _) = worker(&app, "ti.ra.hc.delegator", &[]).await;
    let scope = department(&app, "TI-RA-HC").await;

    let definition = offered_to_and_decided_by("ti_ra_hc", "TI-RA-HC-Q", "TI-RA-HC-R");
    let workflow = publish_workflow_definition(&app, &token, "ti_ra_hc", definition).await;
    let type_id = document_type(&app, &token, "TI_RA_HC", workflow).await;
    let document = submitted_document(&app, &token, type_id, "Every column").await;
    let task = open_task_of(&app, document).await;

    sqlx::query(
        "UPDATE workflow_tasks SET assignee_user_id = $1, status = 'ASSIGNED',
             candidate_department_id = $2, delegated_from_user_id = $3
         WHERE id = $4",
    )
    .bind(holder)
    .bind(scope)
    .bind(delegator)
    .bind(task)
    .execute(&app.pool)
    .await
    .expect("scope and delegate the task");

    let reassigned = reassign(&app, &token, task, json!({ "roleCode": "TI-RA-HC-R" })).await;
    assert_eq!(reassigned.status, StatusCode::OK, "{}", reassigned.body);

    let columns: (Option<Uuid>, Option<Uuid>, Option<Uuid>, Option<Uuid>) = sqlx::query_as(
        "SELECT assignee_user_id, candidate_role_id, candidate_department_id,
                delegated_from_user_id
         FROM workflow_tasks WHERE id = $1",
    )
    .bind(task)
    .fetch_one(&app.pool)
    .await
    .expect("read the task");
    assert_eq!(
        columns,
        (None, Some(target), None, None),
        "a holder column the reassign did not name survived it"
    );
}

/// A task at `MANAGER_APPROVAL` offered to `TI-RA-EQ`, with `transitions`,
/// raised on a document of its own. Answers the task.
async fn task_with_edges(app: &TestApp, token: &str, code: &str, transitions: Value) -> Uuid {
    let key = code.to_lowercase();
    let mut definition = workflow_for(&key, "TI-RA-EQ");
    definition["transitions"] = transitions;

    let workflow = publish_workflow_definition(app, token, &key, definition).await;
    let type_id = document_type(app, token, code, workflow).await;
    let document = submitted_document(app, token, type_id, code).await;

    open_task_of(app, document).await
}

/// **Which edges the decidability check reads** ([#512], ADR-0042 §2).
///
/// * **`RETURN` is a decision.** A target who can take only the state's
///   `RETURN` is accepted. Seen red with `Return` dropped from the check's
///   actions: 422.
/// * **`CANCEL` is not.** A target who can take only the state's `CANCEL` is
///   refused. Seen red with `Cancel` added: 200.
/// * **An edge with no `allowedBy` admits anybody**, so the target passes.
///   JWSS S5 refuses such an edge at publish, so the published definition is
///   rewritten under the running instance. Seen red with that edge made to
///   `continue`: 422.
///
/// [#512]: https://github.com/sujanto-gaws/kelir/issues/512
#[tokio::test]
async fn the_decidability_check_reads_return_and_not_cancel() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let queue = worker_role(&app, "TI-RA-EQ").await;
    let (nobody, _) = worker(&app, "ti.ra.e.nobody", &[]).await;
    let (target, _) = worker(&app, "ti.ra.e.target", &[]).await;
    let only = |user: Uuid| json!(format!("USER:{user}"));

    // RETURN, to a state whose correction task is the owner's.
    let key = "ti_ra_e_return";
    let returns = json!({
        "workflowKey": key, "version": "1.0.0", "name": "With a return",
        "initialState": "MANAGER_APPROVAL",
        "states": [
            { "code": "MANAGER_APPROVAL", "name": "Manager approval",
              "mapsToDocumentStatus": "PENDING_APPROVAL",
              "task": { "taskDefinitionKey": "manager_approval", "taskName": "Decide",
                        "assignment": { "assigneeType": "ROLE", "roleCode": "TI-RA-EQ" } } },
            { "code": "RETURNED", "name": "Returned", "mapsToDocumentStatus": "RETURNED",
              "task": { "taskDefinitionKey": "correct_it", "taskName": "Correct",
                        "assignment": { "assigneeType": "OWNER" } } },
            { "code": "COMPLETED", "name": "Completed", "mapsToDocumentStatus": "COMPLETED",
              "isFinal": true }
        ],
        "transitions": [
            { "from": "MANAGER_APPROVAL", "to": "COMPLETED", "action": "APPROVE",
              "allowedBy": only(nobody) },
            { "from": "MANAGER_APPROVAL", "to": "RETURNED", "action": "RETURN",
              "allowedBy": only(target) },
            { "from": "RETURNED", "to": "MANAGER_APPROVAL", "action": "RESUBMIT",
              "allowedBy": "OWNER" }
        ]
    });
    let workflow = publish_workflow_definition(&app, &token, key, returns).await;
    let type_id = document_type(&app, &token, "TI_RA_E_RETURN", workflow).await;
    let document = submitted_document(&app, &token, type_id, "Only a return").await;
    let task = open_task_of(&app, document).await;
    let accepted = reassign(&app, &token, task, json!({ "userId": target })).await;
    assert_eq!(
        accepted.status,
        StatusCode::OK,
        "a target who can return the task was refused: {}",
        accepted.body
    );

    // CANCEL, beside decisions the target cannot take.
    let task = task_with_edges(
        &app,
        &token,
        "TI_RA_E_CANCEL",
        json!([
            { "from": "MANAGER_APPROVAL", "to": "COMPLETED", "action": "APPROVE",
              "allowedBy": only(nobody) },
            { "from": "MANAGER_APPROVAL", "to": "REJECTED", "action": "REJECT",
              "allowedBy": only(nobody) },
            { "from": "MANAGER_APPROVAL", "to": "REJECTED", "action": "CANCEL",
              "allowedBy": only(target) }
        ]),
    )
    .await;
    let refused = reassign(&app, &token, task, json!({ "userId": target })).await;
    cannot_decide(&refused, "userId");
    assert_eq!(
        task_holder(&app, task).await,
        (None, Some(queue), "CREATED".to_owned())
    );

    // No allowedBy on APPROVE, written under the running instance.
    let task = task_with_edges(
        &app,
        &token,
        "TI_RA_E_OPEN",
        json!([
            { "from": "MANAGER_APPROVAL", "to": "COMPLETED", "action": "APPROVE",
              "allowedBy": only(nobody) },
            { "from": "MANAGER_APPROVAL", "to": "REJECTED", "action": "REJECT",
              "allowedBy": only(nobody) }
        ]),
    )
    .await;
    sqlx::query(
        "UPDATE workflow_definitions d
         SET definition_json = jsonb_set(d.definition_json, '{transitions,0}',
                                         (d.definition_json #> '{transitions,0}') - 'allowedBy')
         FROM workflow_instances i JOIN workflow_tasks t ON t.workflow_instance_id = i.id
         WHERE t.id = $1 AND d.id = i.workflow_definition_id",
    )
    .bind(task)
    .execute(&app.pool)
    .await
    .expect("drop the edge's allowedBy");
    let accepted = reassign(&app, &token, task, json!({ "userId": target })).await;
    assert_eq!(
        accepted.status,
        StatusCode::OK,
        "an edge that admits anybody did not admit the target: {}",
        accepted.body
    );
}
