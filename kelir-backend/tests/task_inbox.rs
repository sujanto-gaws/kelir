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
    let created = app
        .post(
            "/api/v1/workflow/definitions",
            Some(token),
            json!({
                "workflowKey": key,
                "name": "Standard approval",
                "definition": workflow_for(key, role_code),
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
