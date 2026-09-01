//! The engine: an instance starts, a task is generated, somebody decides it,
//! and the document's status follows (#175, #176, #177, #178, #187).
//!
//! **This file is about the seam as much as the engine.** Every test below
//! asserts on the *row* rather than the response where it can, because what
//! these items are about is what is durable: a document whose status disagrees
//! with the process driving it is the defect #178 exists to prevent, and it is
//! invisible from a response body that reports what the caller asked for.
//!
//! Every test that names a control has been seen to fail against a build with
//! that control removed (coding standard §2.9), and the doc comment on each says
//! what the mutation was and what it produced.

mod common;

use std::collections::HashSet;
use std::sync::Arc;

use axum::http::{Method, StatusCode};
use common::{fixtures, TestApp};
use serde_json::{json, Value};
use uuid::Uuid;

/// Above the test pool's ceiling (`TEST_POOL_MAX_CONNECTIONS` is 5), which is
/// the concurrency [#118] taught this project a harness has to actually reach:
/// its own tests could not, so a fix that closed a race and opened a
/// pool-exhaustion deadlock passed them all.
///
/// [#118]: https://github.com/sujanto-gaws/kelir/issues/118
const CONCURRENT_CALLERS: usize = 24;

const APPROVER_ROLE: &str = "WF-APPROVER";

fn id_of(value: &Value) -> Uuid {
    value["id"]
        .as_str()
        .expect("an id")
        .parse()
        .expect("a uuid")
}

/// A workflow whose one task is offered to a **role**, which is the case
/// [#176] AC2 is about: a role task has no assignee until somebody claims it.
fn role_workflow(key: &str) -> Value {
    json!({
        "workflowKey": key,
        "version": "1.0.0",
        "name": "Standard approval",
        "initialState": "MANAGER_APPROVAL",
        "states": [
            { "code": "MANAGER_APPROVAL", "name": "Manager approval",
              "mapsToDocumentStatus": "PENDING_APPROVAL",
              "task": { "taskDefinitionKey": "manager_approval", "taskName": "Approve the request",
                        "assignment": { "assigneeType": "ROLE", "roleCode": APPROVER_ROLE } } },
            { "code": "COMPLETED", "name": "Completed", "mapsToDocumentStatus": "COMPLETED",
              "isFinal": true },
            { "code": "REJECTED", "name": "Rejected", "mapsToDocumentStatus": "REJECTED",
              "isFinal": true }
        ],
        "transitions": [
            { "from": "MANAGER_APPROVAL", "to": "COMPLETED", "action": "APPROVE",
              "allowedBy": format!("ROLE:{APPROVER_ROLE}") },
            { "from": "MANAGER_APPROVAL", "to": "REJECTED", "action": "REJECT",
              "allowedBy": format!("ROLE:{APPROVER_ROLE}") }
        ],
        "variables": [
            { "key": "amount", "dataType": "NUMBER",
              "source": { "var": "formData.amount" } }
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

/// A form with the one field the workflow's variable reads.
///
/// A document type has to bind a published form before its documents can hold
/// any data at all — the definition is what a write is validated against — and
/// the variable `source` in [`role_workflow`] reads `formData.amount`, so this
/// is the smallest form that makes the seam observable.
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

/// A document type with a numbering rule and, optionally, a workflow binding.
///
/// **A second document type is created by every test that asserts anything is
/// scoped** — coding standard §2.9 as of Sprint 8, and [#218]'s single root
/// cause: one subject cannot distinguish *scoped* from *unscoped*.
///
/// [#218]: https://github.com/sujanto-gaws/kelir/issues/218
async fn document_type(app: &TestApp, token: &str, code: &str, workflow: Option<Uuid>) -> Uuid {
    let form = published_form(app, token, &code.to_lowercase().replace('_', "-")).await;
    let mut body = json!({ "typeCode": code, "name": code, "formId": form });

    if let Some(workflow) = workflow {
        // The role the workflow assigns to has to exist before anything submits
        // against this type — see `approver_role`.
        approver_role(app).await;
        body["workflows"] = json!([{ "workflowDefinitionId": workflow }]);
    }

    let created = app.post("/api/v1/document-types", Some(token), body).await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    let type_id = id_of(&created.body["data"]);

    let rule = app
        .put(
            &format!("/api/v1/document-types/{type_id}/numbering-rule"),
            Some(token),
            json!({
                // The template carries the type code, and that is not
                // decoration. `uq_documents_tenant_id_document_number` is
                // tenant-wide while a numbering *bucket* is per type, so two
                // types sharing one template both issue `PR-2026-000001` and the
                // second submit collides — as a 500, because a unique violation
                // on this path is not mapped. Recorded as a finding of this
                // sprint's pass; the fixture works around it so that these tests
                // are about the workflow rather than about #158's surface.
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

/// Reads a document's status from the row rather than from a response.
async fn stored_status(app: &TestApp, id: Uuid) -> String {
    sqlx::query_scalar("SELECT status FROM documents WHERE id = $1")
        .bind(id)
        .fetch_one(&app.pool)
        .await
        .expect("read the document's status")
}

/// The instance of a document, straight from the table.
async fn instance_of(app: &TestApp, document_id: Uuid) -> Option<(Uuid, String, String)> {
    sqlx::query_as(
        "SELECT id, current_state, status FROM workflow_instances WHERE document_id = $1",
    )
    .bind(document_id)
    .fetch_optional(&app.pool)
    .await
    .expect("read the instance")
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

/// The approver role, created once per database.
///
/// **It has to exist before the first submit, not before the first approver.**
/// An assignment that resolves to nobody fails the transition ([#176] AC3's
/// sibling rule), so a workflow naming a role no tenant holds cannot start at
/// all — which is the behaviour
/// `a_workflow_naming_a_role_nobody_holds_refuses_the_submit` asserts on
/// purpose, and which every other test here has to set up around.
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

/// A user holding the approver role plus everything the workflow surface needs.
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

// ---------------------------------------------------------------------------
// #178, #187 — the seam: a submit starts the process its type binds
// ---------------------------------------------------------------------------

/// **The whole seam in one test**: the submit starts an instance, links it,
/// generates the task, and the document's status is the *workflow's* initial
/// state rather than `SUBMITTED`.
///
/// The last clause is the one that matters. `mapsToDocumentStatus` on the
/// initial state says `PENDING_APPROVAL`, so that is where the document is at
/// the end of the submit's own transaction — which is the projection being real
/// rather than described (#178 AC2, AC4).
///
/// **Seen red** against `engine::enter` with its `project_document_status` call
/// removed: the instance runs in `MANAGER_APPROVAL` while the document says
/// `SUBMITTED`, which is exactly the disagreement #178 exists to prevent.
#[tokio::test]
async fn submitting_starts_the_workflow_and_the_documents_status_follows_it() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let workflow = publish_workflow(&app, &token, role_workflow("wf_seam")).await;
    let type_id = document_type(&app, &token, "PR_SEAM", Some(workflow)).await;
    let id = draft(&app, &token, type_id).await;

    let submitted = submit(&app, &token, id).await;
    assert_eq!(submitted.status, StatusCode::OK, "{}", submitted.body);

    let (instance_id, state, status) = instance_of(&app, id).await.expect("an instance started");
    assert_eq!(state, "MANAGER_APPROVAL");
    assert_eq!(status, "RUNNING");

    assert_eq!(
        stored_status(&app, id).await,
        "PENDING_APPROVAL",
        "the document's status is a projection of the instance's state, not SUBMITTED"
    );

    // FR-DOC-012: the document points at the process deciding it, written in the
    // same transaction.
    let linked: Option<Uuid> =
        sqlx::query_scalar("SELECT process_instance_id FROM documents WHERE id = $1")
            .bind(id)
            .fetch_one(&app.pool)
            .await
            .expect("read the link");
    assert_eq!(linked, Some(instance_id));

    // #176 AC1: the state declared a task, so the task exists — in the same
    // transaction, not eventually.
    let task_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workflow_tasks WHERE document_id = $1")
            .bind(id)
            .fetch_one(&app.pool)
            .await
            .expect("count the tasks");
    assert_eq!(task_count, 1);
}

/// **A type with no workflow submits and starts nothing** ([#187] AC4).
///
/// Not every document is approved, and a null binding is a valid configuration
/// rather than a missing one. The second type in this test is what makes the
/// assertion mean something: one type cannot tell *bound* from *unbound*.
#[tokio::test]
async fn a_document_type_with_no_workflow_still_submits() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let workflow = publish_workflow(&app, &token, role_workflow("wf_unbound")).await;
    let bound = document_type(&app, &token, "PR_BOUND", Some(workflow)).await;
    let unbound = document_type(&app, &token, "PR_UNBOUND", None).await;

    let routed = draft(&app, &token, bound).await;
    let plain = draft(&app, &token, unbound).await;

    assert_eq!(submit(&app, &token, routed).await.status, StatusCode::OK);
    let submitted = submit(&app, &token, plain).await;
    assert_eq!(submitted.status, StatusCode::OK, "{}", submitted.body);

    assert!(instance_of(&app, routed).await.is_some());
    assert!(
        instance_of(&app, plain).await.is_none(),
        "a type binding no workflow started one"
    );
    assert_eq!(
        stored_status(&app, plain).await,
        "SUBMITTED",
        "an unrouted document keeps the status the submit gave it"
    );
}

/// **The synchronization is one-way** ([#178] AC2).
///
/// A workflow transition sets the document's status; setting the document's
/// status does not move the workflow. So the transition route refuses a document
/// a process is deciding, naming the instance — and lets it through again once
/// the process has finished, which is not a loophole: nothing is deciding the
/// document any more.
///
/// **Seen red** against `service::status::transition` with the
/// `refuse_while_a_workflow_is_deciding` call removed: the document is moved to
/// `APPROVED` while its instance still says `MANAGER_APPROVAL`, and the next
/// decision overwrites it — a status that disagreed with its process and then
/// silently stopped disagreeing.
#[tokio::test]
async fn a_document_under_a_workflow_cannot_have_its_status_set_by_hand() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let workflow = publish_workflow(&app, &token, role_workflow("wf_oneway")).await;
    let type_id = document_type(&app, &token, "PR_ONEWAY", Some(workflow)).await;
    let id = draft(&app, &token, type_id).await;

    assert_eq!(submit(&app, &token, id).await.status, StatusCode::OK);

    let refused = app
        .put(
            &format!("/api/v1/documents/{id}/status"),
            Some(&token),
            json!({ "status": "APPROVED" }),
        )
        .await;

    assert_eq!(
        refused.status,
        StatusCode::CONFLICT,
        "a manual status change moved a document a workflow was deciding: {}",
        refused.body
    );
    assert_eq!(
        stored_status(&app, id).await,
        "PENDING_APPROVAL",
        "the refusal must leave the row where the process put it"
    );

    // And the other half: an unrouted document is unaffected, so the refusal is
    // about the workflow rather than about the route being broken.
    let unbound = document_type(&app, &token, "PR_ONEWAY_FREE", None).await;
    let free = draft(&app, &token, unbound).await;
    assert_eq!(submit(&app, &token, free).await.status, StatusCode::OK);

    let moved = app
        .put(
            &format!("/api/v1/documents/{free}/status"),
            Some(&token),
            json!({ "status": "APPROVED" }),
        )
        .await;
    assert_eq!(moved.status, StatusCode::OK, "{}", moved.body);
}

// ---------------------------------------------------------------------------
// #175 — the instance, its version pin and its variables
// ---------------------------------------------------------------------------

/// **An instance runs the revision it started against** ([#175] AC1), and
/// **rebinding the type leaves it there** ([#187] AC3).
///
/// The two acceptance criteria are one test because they are one claim: the
/// instance pins a revision row, so nothing reachable through the *type* can
/// move it. Which is also why there is no `guard_rebinding` for workflows —
/// `workflow_instances.workflow_definition_id` is `NOT NULL`, so no instance can
/// be in the unpinned condition **D-30** had to guard against for forms.
#[tokio::test]
async fn rebinding_the_type_leaves_a_running_approval_on_the_revision_it_started() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let first = publish_workflow(&app, &token, role_workflow("wf_pinned")).await;
    let type_id = document_type(&app, &token, "PR_PINNED", Some(first)).await;
    let id = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, id).await.status, StatusCode::OK);

    let (instance_id, state_before, _) = instance_of(&app, id).await.expect("an instance");

    // A second, different workflow, and the type is re-pointed at it.
    let second = publish_workflow(&app, &token, role_workflow("wf_pinned_next")).await;
    let rebound = app
        .put(
            &format!("/api/v1/document-types/{type_id}"),
            Some(&token),
            json!({ "workflows": [{ "workflowDefinitionId": second }] }),
        )
        .await;
    assert_eq!(
        rebound.status,
        StatusCode::OK,
        "rebinding a type with a running approval was refused: {}",
        rebound.body
    );

    let pinned: Uuid =
        sqlx::query_scalar("SELECT workflow_definition_id FROM workflow_instances WHERE id = $1")
            .bind(instance_id)
            .fetch_one(&app.pool)
            .await
            .expect("read the pin");

    assert_eq!(
        pinned, first,
        "a running approval changed shape underneath itself"
    );

    let (_, state_after, _) = instance_of(&app, id).await.expect("still running");
    assert_eq!(state_after, state_before, "its state moved as well");

    // And the third move: the *next* document of that type routes to the new
    // binding, which is what "future submissions use the new binding" means.
    let next = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, next).await.status, StatusCode::OK);

    let next_pin: Uuid = sqlx::query_scalar(
        "SELECT workflow_definition_id FROM workflow_instances WHERE document_id = $1",
    )
    .bind(next)
    .fetch_one(&app.pool)
    .await
    .expect("read the second pin");

    assert_eq!(next_pin, second);
}

/// **A workflow variable is computed at start and stored with its declared
/// type** ([#175] AC2).
#[tokio::test]
async fn an_instance_carries_the_variables_its_definition_declares() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let workflow = publish_workflow(&app, &token, role_workflow("wf_variables")).await;
    let type_id = document_type(&app, &token, "PR_VARIABLES", Some(workflow)).await;
    let id = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, id).await.status, StatusCode::OK);

    let (instance_id, _, _) = instance_of(&app, id).await.expect("an instance");

    let stored: (String, String) = sqlx::query_as(
        "SELECT variable_value, data_type FROM workflow_variables WHERE workflow_instance_id = $1 AND variable_key = 'amount'",
    )
    .bind(instance_id)
    .fetch_one(&app.pool)
    .await
    .expect("read the variable");

    assert_eq!(stored.1, "NUMBER");
    assert_eq!(stored.0, "45000000");

    // And it comes back through the API typed, rather than as the string it is
    // stored as — the whole reason `data_type` sits beside the value.
    let read = app
        .get(&format!("/api/v1/documents/{id}/workflow"), Some(&token))
        .await;
    assert_eq!(read.status, StatusCode::OK, "{}", read.body);
    assert_eq!(
        read.body["data"]["instance"]["variables"][0]["key"],
        "amount"
    );
    assert_eq!(
        read.body["data"]["instance"]["variables"][0]["value"],
        json!(45_000_000.0)
    );
    assert_eq!(
        read.body["data"]["instance"]["definitionVersion"], 1,
        "the revision the approval is running, joined rather than stored twice"
    );
}

/// **A second live instance is refused** ([#178] AC1).
///
/// The service refuses it after reading; `uq_workflow_instances_live_document`
/// is what makes the refusal true under concurrency. This reaches the **index**
/// rather than the service, by starting a second process for a document that
/// already has one — which is what a second submit would do if the draft check
/// were ever relaxed.
///
/// **Seen red** against `0025_workflow.sql` with
/// `uq_workflow_instances_live_document` dropped: the insert succeeds and the
/// document has two processes deciding it.
#[tokio::test]
async fn a_document_cannot_have_two_live_processes() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let workflow = publish_workflow(&app, &token, role_workflow("wf_second")).await;
    let type_id = document_type(&app, &token, "PR_SECOND", Some(workflow)).await;
    let id = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, id).await.status, StatusCode::OK);

    let refused = sqlx::query(
        r#"
        INSERT INTO workflow_instances
            (id, tenant_id, instance_ref, workflow_definition_id, document_id,
             status, current_state)
        VALUES ($1, $2, 'WFI-2026-999999', $3, $4, 'RUNNING', 'MANAGER_APPROVAL')
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(fixtures::SYSTEM_TENANT_ID)
    .bind(workflow)
    .bind(id)
    .execute(&app.pool)
    .await;

    let error = refused.expect_err("a second live instance was accepted");
    assert!(
        error
            .as_database_error()
            .is_some_and(|e| e.is_unique_violation()),
        "expected the live-instance index to refuse it, got {error}"
    );
}

/// **An instance cannot be in a state its definition does not declare**
/// ([#175] AC4), enforced by the database rather than by convention.
///
/// **Seen red** against `0025_workflow.sql` with
/// `fk_workflow_instances_current_state` dropped: the update succeeds and the
/// instance sits in a state no transition leaves.
#[tokio::test]
async fn an_instance_cannot_be_moved_to_a_state_the_definition_does_not_have() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let workflow = publish_workflow(&app, &token, role_workflow("wf_state_fk")).await;
    let type_id = document_type(&app, &token, "PR_STATE_FK", Some(workflow)).await;
    let id = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, id).await.status, StatusCode::OK);

    let (instance_id, _, _) = instance_of(&app, id).await.expect("an instance");

    let refused =
        sqlx::query("UPDATE workflow_instances SET current_state = 'INVENTED' WHERE id = $1")
            .bind(instance_id)
            .execute(&app.pool)
            .await;

    let error = refused.expect_err("an invented state was accepted");
    assert!(
        error
            .as_database_error()
            .is_some_and(|e| e.is_foreign_key_violation()),
        "expected the state foreign key to refuse it, got {error}"
    );
}

// ---------------------------------------------------------------------------
// #176 — the task, its assignment, and the claim
// ---------------------------------------------------------------------------

/// **A role task has no assignee until somebody claims it** ([#176] AC2).
#[tokio::test]
async fn a_role_task_is_unassigned_and_names_the_role_it_is_offered_to() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let _approver = approver(&app, "wf.assignee").await;

    let workflow = publish_workflow(&app, &token, role_workflow("wf_assignment")).await;
    let type_id = document_type(&app, &token, "PR_ASSIGNMENT", Some(workflow)).await;
    let id = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, id).await.status, StatusCode::OK);

    let row: (Option<Uuid>, Option<Uuid>, String) = sqlx::query_as(
        "SELECT assignee_user_id, candidate_role_id, status FROM workflow_tasks WHERE document_id = $1",
    )
    .bind(id)
    .fetch_one(&app.pool)
    .await
    .expect("read the task");

    assert!(row.0.is_none(), "a role task was written with an assignee");
    assert!(row.1.is_some(), "a role task named no role");
    assert_eq!(row.2, "CREATED");
}

/// **An assignment that resolves to nobody fails the transition** rather than
/// leaving an approval that has silently stopped.
///
/// The role the definition names does not exist in this tenant, so the submit is
/// refused with the whole transaction rolled back — no document number burned,
/// no instance, no task.
#[tokio::test]
async fn a_workflow_naming_a_role_nobody_holds_refuses_the_submit() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let mut definition = role_workflow("wf_no_role");
    definition["states"][0]["task"]["assignment"]["roleCode"] = json!("ROLE-THAT-IS-NOT-THERE");

    let workflow = publish_workflow(&app, &token, definition).await;
    let type_id = document_type(&app, &token, "PR_NO_ROLE", Some(workflow)).await;
    let id = draft(&app, &token, type_id).await;

    let refused = submit(&app, &token, id).await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a task assigned to nobody was created: {}",
        refused.body
    );
    assert!(
        refused.body.to_string().contains("ASSIGNMENT_UNRESOLVED"),
        "{}",
        refused.body
    );

    assert_eq!(
        stored_status(&app, id).await,
        "DRAFT",
        "the whole submit must roll back, not half of it"
    );
    assert!(instance_of(&app, id).await.is_none());
}

/// **Two simultaneous claims produce one winner and one 409** ([#176] AC3).
///
/// Driven at a concurrency **above the pool ceiling**, which is the level [#118]
/// showed a fix's own tests can fail to reach. It enumerates every status the
/// product may legitimately answer rather than asserting `409` and treating
/// anything else as a pass — the [Sprint 9 retrospective]'s second action.
///
/// **Seen red** against `repository::task::claim` with its
/// `assignee_user_id IS NULL AND status = 'CREATED'` predicate removed: every
/// one of the twenty-four callers is told it won, and the task ends up assigned
/// to whichever wrote last.
///
/// [Sprint 9 retrospective]: ../../projects/retrospectives/07.%20Sprint%209%20Retrospective.md
#[tokio::test]
async fn two_users_claiming_one_task_produce_one_owner() {
    let app = Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;

    let workflow = publish_workflow(&app, &token, role_workflow("wf_claim")).await;
    let type_id = document_type(&app, &token, "PR_CLAIM", Some(workflow)).await;
    let id = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, id).await.status, StatusCode::OK);

    let task = open_task_of(&app, id).await;

    // One role, many holders of it — which is the situation a queue is.
    let mut tokens = Vec::new();
    for index in 0..CONCURRENT_CALLERS {
        tokens.push(approver(&app, &format!("wf.claimant{index}")).await);
    }

    let mut handles = Vec::new();

    for token in tokens {
        let app = Arc::clone(&app);
        handles.push(tokio::spawn(async move {
            app.post(
                &format!("/api/v1/workflow/tasks/{task}/claim"),
                Some(&token),
                json!({}),
            )
            .await
        }));
    }

    let mut won = 0usize;
    let mut lost = 0usize;

    for handle in handles {
        let response = handle.await.expect("a claim finished");

        match response.status {
            StatusCode::OK => won += 1,
            StatusCode::CONFLICT => lost += 1,
            other => panic!(
                "a claim answered {other}, which is neither winning nor losing: {}",
                response.body
            ),
        }
    }

    assert_eq!(won, 1, "{won} callers were told they claimed one task");
    assert_eq!(lost, CONCURRENT_CALLERS - 1);

    let owners: i64 = sqlx::query_scalar(
        "SELECT count(DISTINCT assignee_user_id) FROM workflow_tasks WHERE id = $1",
    )
    .bind(task)
    .fetch_one(&app.pool)
    .await
    .expect("count the owners");

    assert_eq!(owners, 1, "one task ended with more than one owner");
}

// ---------------------------------------------------------------------------
// #177 — approve, reject, and the decision that must not happen twice
// ---------------------------------------------------------------------------

/// **Approving moves the instance and the document's status follows**
/// ([#177] AC1, [#178] AC2, AC3).
#[tokio::test]
async fn approving_completes_the_process_and_the_document() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let approver = approver(&app, "wf.approver").await;

    let workflow = publish_workflow(&app, &token, role_workflow("wf_approve")).await;
    let type_id = document_type(&app, &token, "PR_APPROVE", Some(workflow)).await;
    let id = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, id).await.status, StatusCode::OK);

    let task = open_task_of(&app, id).await;

    let decided = app
        .post(
            &format!("/api/v1/workflow/tasks/{task}/decision"),
            Some(&approver),
            json!({ "action": "APPROVE" }),
        )
        .await;

    assert_eq!(decided.status, StatusCode::OK, "{}", decided.body);
    assert_eq!(decided.body["data"]["previousState"], "MANAGER_APPROVAL");
    assert_eq!(decided.body["data"]["currentState"], "COMPLETED");
    assert_eq!(decided.body["data"]["documentStatus"], "COMPLETED");

    let (_, state, status) = instance_of(&app, id).await.expect("the instance");
    assert_eq!(state, "COMPLETED");
    assert_eq!(status, "COMPLETED", "a final state ends the instance");

    assert_eq!(stored_status(&app, id).await, "COMPLETED");

    // The formal record, and the task's own history, both written in that
    // transaction — and they are two rows because they answer two questions.
    let decisions: i64 =
        sqlx::query_scalar("SELECT count(*) FROM approval_decisions WHERE task_id = $1")
            .bind(task)
            .fetch_one(&app.pool)
            .await
            .expect("count the decisions");
    assert_eq!(decisions, 1);

    let history: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workflow_task_history WHERE task_id = $1")
            .bind(task)
            .fetch_one(&app.pool)
            .await
            .expect("count the history");
    assert_eq!(history, 2, "created, then completed");
}

/// **Rejecting takes the other transition**, and the document lands on the
/// status the definition mapped that state to.
#[tokio::test]
async fn rejecting_takes_the_definitions_other_edge() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let approver = approver(&app, "wf.rejecter").await;

    let workflow = publish_workflow(&app, &token, role_workflow("wf_reject")).await;
    let type_id = document_type(&app, &token, "PR_REJECT", Some(workflow)).await;
    let id = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, id).await.status, StatusCode::OK);

    let task = open_task_of(&app, id).await;

    let decided = app
        .post(
            &format!("/api/v1/workflow/tasks/{task}/decision"),
            Some(&approver),
            json!({ "action": "REJECT" }),
        )
        .await;

    assert_eq!(decided.status, StatusCode::OK, "{}", decided.body);
    assert_eq!(decided.body["data"]["currentState"], "REJECTED");
    assert_eq!(stored_status(&app, id).await, "REJECTED");

    let outcome: Option<String> =
        sqlx::query_scalar("SELECT outcome FROM workflow_instances WHERE document_id = $1")
            .bind(id)
            .fetch_one(&app.pool)
            .await
            .expect("read the outcome");
    assert_eq!(outcome.as_deref(), Some("REJECTED"));
}

/// **A decided task cannot be decided again** ([#177] AC2, AC4).
///
/// Twenty-four concurrent callers, half approving and half rejecting, above the
/// pool ceiling. Exactly one must win, and the instance, the document and the
/// decision record must all agree with whichever it was. It enumerates every
/// status the product may legitimately answer.
///
/// **Seen red** against `repository::task::complete` with its
/// `status IN ('CREATED','ASSIGNED','IN_PROGRESS')` predicate removed: several
/// callers are told they decided the task, `approval_decisions` holds several
/// rows for one task, and the document's status is whichever transition
/// committed last.
#[tokio::test]
async fn concurrent_decisions_on_one_task_resolve_to_exactly_one_outcome() {
    let app = Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;

    let workflow = publish_workflow(&app, &token, role_workflow("wf_race")).await;
    let type_id = document_type(&app, &token, "PR_RACE", Some(workflow)).await;
    let id = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, id).await.status, StatusCode::OK);

    let task = open_task_of(&app, id).await;

    let mut tokens = Vec::new();
    for index in 0..CONCURRENT_CALLERS {
        tokens.push(approver(&app, &format!("wf.decider{index}")).await);
    }

    let mut handles = Vec::new();

    for (index, token) in tokens.into_iter().enumerate() {
        let app = Arc::clone(&app);
        let action = if index % 2 == 0 { "APPROVE" } else { "REJECT" };

        handles.push(tokio::spawn(async move {
            app.post(
                &format!("/api/v1/workflow/tasks/{task}/decision"),
                Some(&token),
                json!({ "action": action }),
            )
            .await
        }));
    }

    let mut winners = HashSet::new();
    let mut lost = 0usize;

    for handle in handles {
        let response = handle.await.expect("a decision finished");

        match response.status {
            StatusCode::OK => {
                winners.insert(
                    response.body["data"]["currentState"]
                        .as_str()
                        .expect("a state")
                        .to_owned(),
                );
            }
            // The task was already decided, or the process moved underneath the
            // decision. Both are correct answers to losing.
            StatusCode::CONFLICT => lost += 1,
            other => panic!(
                "a decision answered {other}, which is neither winning nor losing: {}",
                response.body
            ),
        }
    }

    assert_eq!(
        winners.len(),
        1,
        "more than one caller decided one task: {winners:?}"
    );
    assert_eq!(lost, CONCURRENT_CALLERS - 1);

    let decisions: i64 =
        sqlx::query_scalar("SELECT count(*) FROM approval_decisions WHERE task_id = $1")
            .bind(task)
            .fetch_one(&app.pool)
            .await
            .expect("count the decisions");
    assert_eq!(
        decisions, 1,
        "one task collected {decisions} decision records"
    );

    // And everything agrees: the instance's state, the document's status and the
    // one decision that was recorded.
    let winning_state = winners.into_iter().next().expect("a winner");
    let (_, state, _) = instance_of(&app, id).await.expect("the instance");
    assert_eq!(state, winning_state);

    let expected_document_status = if winning_state == "COMPLETED" {
        "COMPLETED"
    } else {
        "REJECTED"
    };
    assert_eq!(stored_status(&app, id).await, expected_document_status);
}

// ---------------------------------------------------------------------------
// Workflow history (#181, FR-WF-012)
// ---------------------------------------------------------------------------

/// One document's history, as the workspace reads it.
async fn history_of(app: &TestApp, token: &str, document: Uuid) -> Value {
    let response = app
        .get(
            &format!("/api/v1/documents/{document}/workflow/history"),
            Some(token),
        )
        .await;

    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    response.body.clone()
}

/// **The whole of FR-WF-012 in one process**: the submit's row, the decision's
/// row, both ends of each, who moved it and from which task.
///
/// **Seen red** against `engine::fire`'s `history::record` call removed: the
/// approval completes and the history stops at the submit, which is the gap
/// #181 AC1 is about — a transition that committed without its history.
#[tokio::test]
async fn every_transition_is_recorded_with_both_ends_and_its_actor() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let approver_token = approver(&app, "wf.historian").await;

    let workflow = publish_workflow(&app, &token, role_workflow("wf_history")).await;
    let type_id = document_type(&app, &token, "PR_HISTORY", Some(workflow)).await;
    let id = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, id).await.status, StatusCode::OK);

    // The submit's own row: into the initial state, from nowhere.
    let started = history_of(&app, &token, id).await;
    let rows = started["data"].as_array().expect("a list");
    assert_eq!(rows.len(), 1, "the submit wrote no history: {started}");
    assert_eq!(rows[0]["fromState"], Value::Null);
    assert_eq!(rows[0]["toState"], "MANAGER_APPROVAL");
    assert_eq!(rows[0]["action"], Value::Null);
    assert_eq!(rows[0]["taskId"], Value::Null);

    let task = open_task_of(&app, id).await;
    let decided = app
        .post(
            &format!("/api/v1/workflow/tasks/{task}/decision"),
            Some(&approver_token),
            json!({ "action": "APPROVE" }),
        )
        .await;
    assert_eq!(decided.status, StatusCode::OK, "{}", decided.body);

    let after = history_of(&app, &token, id).await;
    let rows = after["data"].as_array().expect("a list");
    assert_eq!(rows.len(), 2, "the decision wrote no history: {after}");

    // Oldest first, so the decision is second — and it carries both ends, the
    // action, the task it came from and who took it.
    let moved = &rows[1];
    assert_eq!(moved["fromState"], "MANAGER_APPROVAL");
    assert_eq!(moved["toState"], "COMPLETED");
    assert_eq!(moved["action"], "APPROVE");
    assert_eq!(moved["taskId"], json!(task.to_string()));
    assert_eq!(moved["actorUsername"], "wf.historian");
    assert!(
        moved["occurredAt"].is_string(),
        "a history entry with no timestamp: {after}"
    );

    // No comment was sent, so none is recorded. The `requiresComment` tests
    // below are where the filled case is asserted.
    assert_eq!(moved["comment"], Value::Null);
}

/// The history and the transition are one transaction (#181 AC1).
///
/// A refused decision leaves no row behind: the `allowedBy` check in
/// `engine::fire` (#226) rejects *after* the transition is chosen, so a history
/// row written outside the transaction would survive the rollback and claim a
/// transition that never occurred.
#[tokio::test]
async fn a_refused_transition_records_nothing() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    given_bare_role(&app, "WF-EDGE-ONLY").await;
    let approver_token = approver(&app, "wf.refused").await;

    let workflow = publish_workflow(
        &app,
        &token,
        split_control_workflow("wf_history_refused", "WF-EDGE-ONLY"),
    )
    .await;
    let type_id = document_type(&app, &token, "PR_HIST_REFUSED", Some(workflow)).await;
    let id = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, id).await.status, StatusCode::OK);

    let task = open_task_of(&app, id).await;
    let refused = app
        .post(
            &format!("/api/v1/workflow/tasks/{task}/decision"),
            Some(&approver_token),
            json!({ "action": "APPROVE" }),
        )
        .await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN, "{}", refused.body);

    let after = history_of(&app, &token, id).await;
    assert_eq!(
        after["data"].as_array().expect("a list").len(),
        1,
        "a refused transition left a history row claiming it happened: {after}"
    );
}

/// The read is paginated (#181 AC3), and its order is total.
///
/// **The ids across the pages must be distinct rows** — an `ORDER BY
/// created_at` alone is not a total order when rows share a transaction's
/// timestamp, and the failure it produces is a row appearing on two pages while
/// another appears on none.
#[tokio::test]
async fn the_history_is_paginated_and_its_order_is_total() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let approver_token = approver(&app, "wf.pager").await;

    let workflow = publish_workflow(&app, &token, role_workflow("wf_history_paged")).await;
    let type_id = document_type(&app, &token, "PR_HIST_PAGED", Some(workflow)).await;
    let id = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, id).await.status, StatusCode::OK);

    let task = open_task_of(&app, id).await;
    app.post(
        &format!("/api/v1/workflow/tasks/{task}/decision"),
        Some(&approver_token),
        json!({ "action": "APPROVE" }),
    )
    .await;

    let whole = history_of(&app, &token, id).await;
    let total = whole["data"].as_array().expect("a list").len();
    assert_eq!(total, 2);
    assert_eq!(whole["meta"]["total"], json!(total));

    let mut seen: Vec<String> = Vec::new();
    for page in 1..=total {
        let response = app
            .get(
                &format!("/api/v1/documents/{id}/workflow/history?page={page}&pageSize=1"),
                Some(&token),
            )
            .await;
        assert_eq!(response.status, StatusCode::OK, "{}", response.body);

        let rows = response.body["data"].as_array().expect("a list");
        assert_eq!(
            rows.len(),
            1,
            "page {page} of a one-per-page read: {}",
            response.body
        );
        seen.push(rows[0]["id"].as_str().expect("an id").to_owned());
    }

    let mut distinct = seen.clone();
    distinct.sort();
    distinct.dedup();
    assert_eq!(
        distinct.len(),
        total,
        "paging repeated a row and skipped another — the order is not total: {seen:?}"
    );
}

/// #181 AC4: the history is **not** behind the governance permission.
///
/// An approver holds `workflow:instance:read` and no audit permission at all,
/// and they are the person the history is for. A caller holding neither is
/// refused, so the route is not simply open.
#[tokio::test]
async fn the_history_is_read_with_the_workflow_permission_not_the_audit_one() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let approver_token = approver(&app, "wf.reader").await;

    let workflow = publish_workflow(&app, &token, role_workflow("wf_history_perm")).await;
    let type_id = document_type(&app, &token, "PR_HIST_PERM", Some(workflow)).await;
    let id = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, id).await.status, StatusCode::OK);

    let theirs = app
        .get(
            &format!("/api/v1/documents/{id}/workflow/history"),
            Some(&approver_token),
        )
        .await;
    assert_eq!(
        theirs.status,
        StatusCode::OK,
        "the approver was refused the history of their own approval: {}",
        theirs.body
    );

    let bare = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "WF-NO-INSTANCE",
        &["document:read"],
    )
    .await;
    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "wf.nothing",
        "wf.nothing@example.test",
        common::ADMIN_PASSWORD,
        &[bare],
    )
    .await;
    let outsider = app.sign_in("wf.nothing", common::ADMIN_PASSWORD).await;

    let refused = app
        .get(
            &format!("/api/v1/documents/{id}/workflow/history"),
            Some(&outsider),
        )
        .await;
    assert_eq!(
        refused.status,
        StatusCode::FORBIDDEN,
        "the history was readable without workflow:instance:read: {}",
        refused.body
    );
}

/// #181 AC6: a history record is never edited or deleted, and the storage is
/// shaped so that no route could.
///
/// **Asserted against the schema rather than against the router**, because a
/// route that does not exist today is a route somebody adds tomorrow. The table
/// has no `deleted_at`, no `updated_at` and no `updated_by`: a soft delete has
/// nowhere to write and an edit has nothing to stamp, which is the argument
/// `0027`'s header makes. This fails when somebody adds one back.
#[tokio::test]
async fn the_history_table_cannot_record_an_edit_or_a_deletion() {
    let app = TestApp::spawn().await;

    let mutable: Vec<String> = sqlx::query_scalar(
        "SELECT column_name FROM information_schema.columns
          WHERE table_name = 'workflow_history'
            AND column_name IN ('deleted_at', 'updated_at', 'updated_by')",
    )
    .fetch_all(&app.pool)
    .await
    .expect("read the columns");

    assert!(
        mutable.is_empty(),
        "workflow_history grew a column that lets a row be changed or hidden: {mutable:?}"
    );
}

// ---------------------------------------------------------------------------
// allowedBy — the edge's own control (#226)
// ---------------------------------------------------------------------------

/// [`role_workflow`] with its **APPROVE edge** handed to a different role than
/// the task.
///
/// The shape that separates the two controls: the state's task is offered to
/// `APPROVER_ROLE`, so a holder of that role may work the task, and the APPROVE
/// transition out of it is `allowedBy` somebody else. REJECT is left alone, so
/// one task carries one edge the caller may take and one they may not.
fn split_control_workflow(key: &str, edge_role: &str) -> Value {
    let mut definition = role_workflow(key);

    definition["transitions"][0]["allowedBy"] = json!(format!("ROLE:{edge_role}"));

    definition
}

/// A role that exists and grants nothing, so `allowedBy` resolves and the
/// refusal is the check's rather than the resolver's.
async fn given_bare_role(app: &TestApp, code: &str) -> Uuid {
    fixtures::create_role_with_permissions(&app.pool, fixtures::SYSTEM_TENANT_ID, code, &[]).await
}

/// **A transition the task permits and `allowedBy` does not is refused** (#226).
///
/// The task's `assignment` and the transition's `allowedBy` are two controls and
/// both apply. This caller passes the first — they hold the role the task is
/// offered to, and `refuse_unless_theirs` lets them through — and fails the
/// second on the APPROVE edge only.
///
/// **REJECT from the same task still succeeds**, which is the assertion that
/// makes this about the edge rather than about the task. A check that had landed
/// on the task by mistake would refuse both.
///
/// **Seen red** (coding standard §2.9) against the `allowed_by` block removed
/// from `engine::fire`: the approver takes an edge the definition handed to
/// somebody else and the document reaches `COMPLETED`.
#[tokio::test]
async fn a_transition_the_task_permits_but_allowed_by_refuses_is_forbidden() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    given_bare_role(&app, "WF-EDGE-ONLY").await;
    let approver = approver(&app, "wf.taskonly").await;

    let workflow = publish_workflow(
        &app,
        &token,
        split_control_workflow("wf_allowed_by", "WF-EDGE-ONLY"),
    )
    .await;
    let type_id = document_type(&app, &token, "PR_ALLOWED_BY", Some(workflow)).await;
    let id = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, id).await.status, StatusCode::OK);

    let task = open_task_of(&app, id).await;
    let decision = format!("/api/v1/workflow/tasks/{task}/decision");

    let refused = app
        .post(&decision, Some(&approver), json!({ "action": "APPROVE" }))
        .await;

    assert_eq!(
        refused.status,
        StatusCode::FORBIDDEN,
        "the approver took an APPROVE edge the definition handed to another role: {}",
        refused.body
    );
    assert_eq!(stored_status(&app, id).await, "PENDING_APPROVAL");

    // The other edge out of the same state, which this caller *is* allowed.
    let rejected = app
        .post(&decision, Some(&approver), json!({ "action": "REJECT" }))
        .await;

    assert_eq!(
        rejected.status,
        StatusCode::OK,
        "the refusal reached an edge it was not about: {}",
        rejected.body
    );
    assert_eq!(stored_status(&app, id).await, "REJECTED");
}

/// The other half: **the role the edge names can take it.**
///
/// Without this, `a_transition_the_task_permits_but_allowed_by_refuses_is_forbidden`
/// passes against a check that refuses everybody, which would make every
/// `allowedBy` a dead end rather than a control.
#[tokio::test]
async fn a_transition_is_taken_by_the_role_its_allowed_by_names() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let edge_role = given_bare_role(&app, "WF-EDGE-ONLY").await;

    // Holds the task's role and the edge's, which is the deployment that works.
    let both = approver(&app, "wf.both").await;
    sqlx::query(
        "INSERT INTO user_roles (id, tenant_id, user_id, role_id)
         SELECT gen_random_uuid(), $1, u.id, $2 FROM users u WHERE u.username = 'wf.both'",
    )
    .bind(fixtures::SYSTEM_TENANT_ID)
    .bind(edge_role)
    .execute(&app.pool)
    .await
    .expect("grant the edge role");

    let workflow = publish_workflow(
        &app,
        &token,
        split_control_workflow("wf_allowed_by_ok", "WF-EDGE-ONLY"),
    )
    .await;
    let type_id = document_type(&app, &token, "PR_ALLOWED_BY_OK", Some(workflow)).await;
    let id = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, id).await.status, StatusCode::OK);

    let task = open_task_of(&app, id).await;

    let approved = app
        .post(
            &format!("/api/v1/workflow/tasks/{task}/decision"),
            Some(&both),
            json!({ "action": "APPROVE" }),
        )
        .await;

    assert_eq!(
        approved.status,
        StatusCode::OK,
        "the role the edge names was refused its own transition: {}",
        approved.body
    );
    assert_eq!(stored_status(&app, id).await, "COMPLETED");
}

// ---------------------------------------------------------------------------
// DEPARTMENT_ROLE — the department half of the grant
// ---------------------------------------------------------------------------

/// A workflow whose task is offered to a role **within one named department**.
fn department_workflow(key: &str, department_code: &str) -> Value {
    let mut definition = role_workflow(key);

    definition["states"][0]["task"]["assignment"] = json!({
        "assigneeType": "DEPARTMENT_ROLE",
        "roleCode": APPROVER_ROLE,
        "departmentScope": department_code,
    });

    definition
}

/// A department, by code.
async fn department(app: &TestApp, code: &str, name: &str) -> Uuid {
    let id = Uuid::now_v7();

    sqlx::query(
        "INSERT INTO departments (id, tenant_id, department_code, name)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(id)
    .bind(fixtures::SYSTEM_TENANT_ID)
    .bind(code)
    .bind(name)
    .execute(&app.pool)
    .await
    .expect("insert the department");

    id
}

/// Scopes a user's approver grant to one department.
///
/// `user_roles.department_id` is the "optional department-scoped grant" `0002`
/// created and [JSON Workflow Schema](../../docs/schema/JSON%20Workflow%20Schema.md)
/// §5.3 names as the column `DEPARTMENT_ROLE` resolves against.
async fn scope_grant_to(app: &TestApp, username: &str, department: Uuid) {
    let updated = sqlx::query(
        "UPDATE user_roles SET department_id = $1
          WHERE user_id = (SELECT id FROM users WHERE username = $2)",
    )
    .bind(department)
    .bind(username)
    .execute(&app.pool)
    .await
    .expect("scope the grant");

    assert_eq!(
        updated.rows_affected(),
        1,
        "the approver's grant was not scoped — the fixture is not testing what it says"
    );
}

/// **A `DEPARTMENT_ROLE` task is decided by an approver from another department.**
///
/// [JSON Workflow Schema](../../docs/schema/JSON%20Workflow%20Schema.md) §5.3 is
/// explicit about what this assignee type resolves against — *"`roles` plus
/// `user_roles.department_id`, which has carried a department-scoped grant since
/// `0002`"* — and §5.1's own worked example is a `FINANCE_APPROVER` scoped to
/// `REQUESTED_DEPARTMENT`.
///
/// The resolver honours it: `assignment::resolve` looks the department up,
/// refuses a code that names no live department, and stores the id on the task
/// as `candidate_department_id`. **Nothing reads it after that.**
/// `repository::task::holds_role` filters on `user_roles` by tenant, user, role
/// and validity window and not by department, and the inbox's candidate arm
/// matches `candidate_role_id` alone. So the column is written, displayed, and
/// never enforced.
///
/// A mutation campaign over `WHERE` predicates cannot find this: the defect is
/// an **absent** predicate rather than a wrong one, and there is no clause to
/// mutate. That is why 34 mutations at 68% red went past it, and why
/// [record 08](../../projects/verifications/08.%20Sprint%2010%20Independent%20Pass.md)
/// exists.
///
/// **Quarantined red for one commit, and lifted by the fix.** It was committed
/// failing so the defect was executable rather than only described — the
/// quarantine `identity_users.rs` used for the tenant boundary — and the
/// `#[ignore]` named #225 closing as the condition. That has happened;
/// `a_department_scoped_task_is_decidable_by_that_department` is the positive
/// case it asked for, so the fix cannot be "refuse everyone".
///
/// **Seen red** (coding standard §2.9) against `holds_role`'s department
/// predicate removed: the Procurement approver decides Finance's task and the
/// document reaches `COMPLETED`, which is the 200 this asserts is a 403.
#[tokio::test]
async fn a_department_scoped_task_is_not_decidable_from_another_department() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let finance = department(&app, "DEPT-FIN", "Finance").await;
    let procurement = department(&app, "DEPT-PROC", "Procurement").await;

    // The task is offered to the approver role *in Finance*.
    let workflow = publish_workflow(
        &app,
        &token,
        department_workflow("wf_department_scope", "DEPT-FIN"),
    )
    .await;
    let type_id = document_type(&app, &token, "PR_DEPT_SCOPE", Some(workflow)).await;

    // This approver holds the role, but their grant is Procurement's.
    let outsider = approver(&app, "wf.procurement").await;
    scope_grant_to(&app, "wf.procurement", procurement).await;

    let id = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, id).await.status, StatusCode::OK);

    let task = open_task_of(&app, id).await;

    // The resolver did its half: the task carries Finance.
    let carried: Option<Uuid> =
        sqlx::query_scalar("SELECT candidate_department_id FROM workflow_tasks WHERE id = $1")
            .bind(task)
            .fetch_one(&app.pool)
            .await
            .expect("read the task's department");
    assert_eq!(
        carried,
        Some(finance),
        "the assignment did not record the department it resolved"
    );

    let refused = app
        .post(
            &format!("/api/v1/workflow/tasks/{task}/decision"),
            Some(&outsider),
            json!({ "action": "APPROVE" }),
        )
        .await;

    assert_eq!(
        refused.status,
        StatusCode::FORBIDDEN,
        "an approver whose grant is Procurement's decided a task scoped to Finance: {}",
        refused.body
    );
    assert_eq!(stored_status(&app, id).await, "PENDING_APPROVAL");
}

/// The other half of [#225](https://github.com/sujanto-gaws/kelir/issues/225):
/// **the approver the definition meant can still decide.**
///
/// Without this the fix passes its own negative test by refusing everybody,
/// which would turn an authorization gap into a stalled process — the outcome
/// [JWSS §5.3](../../docs/schema/JSON%20Workflow%20Schema.md) refuses assignee
/// types at *save* time to avoid.
#[tokio::test]
async fn a_department_scoped_task_is_decidable_by_that_department() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let finance = department(&app, "DEPT-FIN", "Finance").await;

    let workflow = publish_workflow(
        &app,
        &token,
        department_workflow("wf_department_right", "DEPT-FIN"),
    )
    .await;
    let type_id = document_type(&app, &token, "PR_DEPT_RIGHT", Some(workflow)).await;

    let insider = approver(&app, "wf.finance").await;
    scope_grant_to(&app, "wf.finance", finance).await;

    let id = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, id).await.status, StatusCode::OK);

    let task = open_task_of(&app, id).await;

    let decided = app
        .post(
            &format!("/api/v1/workflow/tasks/{task}/decision"),
            Some(&insider),
            json!({ "action": "APPROVE" }),
        )
        .await;

    assert_eq!(
        decided.status,
        StatusCode::OK,
        "the department's own approver was refused their task: {}",
        decided.body
    );
    assert_eq!(stored_status(&app, id).await, "COMPLETED");
}

/// **The inbox and the decision answer the same question** (#225 AC2).
///
/// A queue that lists work the API then refuses is its own defect, so the
/// department predicate has to reach both. This asserts the listing half: the
/// outsider's inbox does not carry a task they could not decide, and the
/// insider's does.
///
/// **Seen red** against the `candidate_department_id` clause removed from
/// `repository::inbox`'s candidate arm: the Procurement approver's inbox lists
/// Finance's task.
#[tokio::test]
async fn a_department_scoped_task_is_listed_only_to_that_department() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let finance = department(&app, "DEPT-FIN", "Finance").await;
    let procurement = department(&app, "DEPT-PROC", "Procurement").await;

    let workflow = publish_workflow(
        &app,
        &token,
        department_workflow("wf_department_inbox", "DEPT-FIN"),
    )
    .await;
    let type_id = document_type(&app, &token, "PR_DEPT_INBOX", Some(workflow)).await;

    let insider = approver(&app, "wf.fin.inbox").await;
    scope_grant_to(&app, "wf.fin.inbox", finance).await;
    let outsider = approver(&app, "wf.proc.inbox").await;
    scope_grant_to(&app, "wf.proc.inbox", procurement).await;

    let id = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, id).await.status, StatusCode::OK);
    let task = open_task_of(&app, id).await;

    let listed = |body: &Value| -> bool {
        body["data"]
            .as_array()
            .expect("the inbox is a list")
            .iter()
            .any(|row| row["id"] == json!(task.to_string()))
    };

    let theirs = app.get("/api/v1/tasks", Some(&outsider)).await;
    assert_eq!(theirs.status, StatusCode::OK, "{}", theirs.body);
    assert!(
        !listed(&theirs.body),
        "Procurement's approver was offered Finance's task: {}",
        theirs.body
    );

    let ours = app.get("/api/v1/tasks", Some(&insider)).await;
    assert_eq!(ours.status, StatusCode::OK, "{}", ours.body);
    assert!(
        listed(&ours.body),
        "Finance's approver was not offered their own task: {}",
        ours.body
    );
}

/// **Only the task's assignee, or a holder of the role it is offered to, may
/// decide it** ([#177] AC5).
///
/// The third party below holds `workflow:task:execute` and not the role, which
/// is the distinction that matters: the permission says they may work tasks at
/// all, and the row says whether *this* one is theirs.
///
/// **Seen red** against `domain::task::refuse_unless_theirs` returning `Ok(())`
/// unconditionally: the stranger decides somebody else's approval and the
/// permission test in this file stays green, because a suite that only ever
/// calls with the assignee cannot tell the two apart.
#[tokio::test]
async fn a_third_party_holding_the_permission_cannot_decide_somebody_elses_task() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let approver = approver(&app, "wf.rightful").await;

    // Everything the approver has, except the approver role.
    let role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "WF-BYSTANDER",
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
        "wf.bystander",
        "wf.bystander@example.test",
        common::ADMIN_PASSWORD,
        &[role],
    )
    .await;
    let bystander = app.sign_in("wf.bystander", common::ADMIN_PASSWORD).await;

    let workflow = publish_workflow(&app, &token, role_workflow("wf_third_party")).await;
    let type_id = document_type(&app, &token, "PR_THIRD_PARTY", Some(workflow)).await;
    let id = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, id).await.status, StatusCode::OK);

    let task = open_task_of(&app, id).await;

    let refused = app
        .post(
            &format!("/api/v1/workflow/tasks/{task}/decision"),
            Some(&bystander),
            json!({ "action": "APPROVE" }),
        )
        .await;

    assert_eq!(
        refused.status,
        StatusCode::FORBIDDEN,
        "a third party decided somebody else's approval: {}",
        refused.body
    );
    assert_eq!(stored_status(&app, id).await, "PENDING_APPROVAL");

    // And the rightful holder can, so the refusal above is about the row rather
    // than about the endpoint refusing everybody — the gate §2.9 warns about.
    let decided = app
        .post(
            &format!("/api/v1/workflow/tasks/{task}/decision"),
            Some(&approver),
            json!({ "action": "APPROVE" }),
        )
        .await;
    assert_eq!(decided.status, StatusCode::OK, "{}", decided.body);
}

/// **An action the definition does not offer is a 422, not a 409.**
///
/// The request names something the process cannot do *from where it is*, which
/// is a property of the payload against the resource; a 409 is what a concurrent
/// change earns. The same split `DocumentStatus::check_move_to` makes one module
/// over, and a caller fixes the two differently.
#[tokio::test]
async fn an_action_the_definition_does_not_offer_names_the_ones_it_does() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let approver = approver(&app, "wf.limited").await;

    // A workflow that can only be rejected from its first state.
    let definition = json!({
        "workflowKey": "wf_reject_only",
        "version": "1.0.0",
        "name": "Reject only",
        "initialState": "MANAGER_APPROVAL",
        "states": [
            { "code": "MANAGER_APPROVAL", "name": "Manager approval",
              "mapsToDocumentStatus": "PENDING_APPROVAL",
              "task": { "taskDefinitionKey": "manager_approval", "taskName": "Decide",
                        "assignment": { "assigneeType": "ROLE", "roleCode": APPROVER_ROLE } } },
            { "code": "REJECTED", "name": "Rejected", "mapsToDocumentStatus": "REJECTED",
              "isFinal": true },
            { "code": "CANCELLED", "name": "Cancelled", "mapsToDocumentStatus": "CANCELLED",
              "isFinal": true }
        ],
        "transitions": [
            { "from": "MANAGER_APPROVAL", "to": "REJECTED", "action": "REJECT",
              "allowedBy": format!("ROLE:{APPROVER_ROLE}") },
            { "from": "MANAGER_APPROVAL", "to": "CANCELLED", "action": "CANCEL",
              "allowedBy": "OWNER" }
        ]
    });

    let workflow = publish_workflow(&app, &token, definition).await;
    let type_id = document_type(&app, &token, "PR_REJECT_ONLY", Some(workflow)).await;
    let id = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, id).await.status, StatusCode::OK);

    let task = open_task_of(&app, id).await;

    let refused = app
        .post(
            &format!("/api/v1/workflow/tasks/{task}/decision"),
            Some(&approver),
            json!({ "action": "APPROVE" }),
        )
        .await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );

    let body = refused.body.to_string();
    assert!(body.contains("NO_SUCH_TRANSITION"), "{body}");
    assert!(
        body.contains("REJECT"),
        "the refusal must name what is possible from here: {body}"
    );
}

// ---------------------------------------------------------------------------
// #187 — the binding, checked against a definition that exists and is published
// ---------------------------------------------------------------------------

/// **A binding must name a definition that exists and is `ACTIVE`**
/// ([#187] AC2).
///
/// Both refusals, because they are different problems: an id that names nothing
/// is a typo, and a draft is a workflow somebody has not finished. A draft bound
/// to a type could still change under documents already routed by it, which is
/// what publication exists to prevent — `check_bindings`' `NOT_PUBLISHED` arm,
/// restated for the other artefact.
///
/// **Seen red** against `service::check_workflow_bindings` returning `Ok(())`
/// before its loop: a document type is bound to a workflow that does not exist,
/// and the failure surfaces at somebody's submit as a 500.
#[tokio::test]
async fn a_type_cannot_bind_a_workflow_that_does_not_exist_or_is_a_draft() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let missing = app
        .post(
            "/api/v1/document-types",
            Some(&token),
            json!({
                "typeCode": "PR_BAD_BINDING",
                "name": "PR_BAD_BINDING",
                "workflows": [{ "workflowDefinitionId": Uuid::now_v7() }],
            }),
        )
        .await;

    assert_eq!(
        missing.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        missing.body
    );
    assert_eq!(
        missing.body["error"]["details"][0]["code"], "NOT_FOUND",
        "{}",
        missing.body
    );
    assert_eq!(
        missing.body["error"]["details"][0]["path"], "workflows.0.workflowDefinitionId",
        "{}",
        missing.body
    );

    // A draft: it exists, and binding it would bind a definition that can still
    // change under documents already routed by it.
    let created = app
        .post(
            "/api/v1/workflow/definitions",
            Some(&token),
            json!({
                "workflowKey": "wf_draft_binding",
                "name": "Draft",
                "definition": role_workflow("wf_draft_binding"),
            }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let draft_id = id_of(&created.body["data"]);

    let refused = app
        .post(
            "/api/v1/document-types",
            Some(&token),
            json!({
                "typeCode": "PR_DRAFT_BINDING",
                "name": "PR_DRAFT_BINDING",
                "workflows": [{ "workflowDefinitionId": draft_id }],
            }),
        )
        .await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a draft workflow was bound to a document type: {}",
        refused.body
    );
    assert_eq!(
        refused.body["error"]["details"][0]["code"], "NOT_PUBLISHED",
        "{}",
        refused.body
    );

    // And a published one binds, so the two refusals above are not green because
    // every binding is refused.
    let published = publish_workflow(&app, &token, role_workflow("wf_good_binding")).await;
    let accepted = app
        .post(
            "/api/v1/document-types",
            Some(&token),
            json!({
                "typeCode": "PR_GOOD_BINDING",
                "name": "PR_GOOD_BINDING",
                "workflows": [{ "workflowDefinitionId": published }],
            }),
        )
        .await;
    assert_eq!(accepted.status, StatusCode::CREATED, "{}", accepted.body);
}

/// **A definition with running approvals cannot be retired.**
///
/// `delete_type`'s decision one module over, and for its reason: an instance
/// *is* its definition, so retiring one under a running approval leaves that
/// approval unable to move.
#[tokio::test]
async fn a_workflow_with_running_approvals_cannot_be_retired() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let workflow = publish_workflow(&app, &token, role_workflow("wf_in_use")).await;
    let spare = publish_workflow(&app, &token, role_workflow("wf_spare")).await;
    let type_id = document_type(&app, &token, "PR_IN_USE", Some(workflow)).await;
    let id = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, id).await.status, StatusCode::OK);

    let refused = app
        .delete(
            &format!("/api/v1/workflow/definitions/{workflow}"),
            Some(&token),
        )
        .await;

    assert_eq!(refused.status, StatusCode::CONFLICT, "{}", refused.body);

    // The unused one retires, so the refusal is about the running approval
    // rather than about deletion being broken.
    let deleted = app
        .delete(
            &format!("/api/v1/workflow/definitions/{spare}"),
            Some(&token),
        )
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);
}

// ---------------------------------------------------------------------------
// Closing what the mutation campaign found nothing held
// ---------------------------------------------------------------------------

/// **A stale move writes nothing** (M11).
///
/// `move_state` carries `AND current_state = $3` so that two callers who both
/// chose a transition from one state produce one update of one row and one
/// update of none. The campaign found the predicate unheld, and the reason is a
/// **gate**: every decision reaches it through `workflow_tasks`' own
/// `status IN (…)` guard, which refuses the loser one statement earlier. One
/// test appeared to cover both and covered the first.
///
/// So this drives the repository, where the task guard is not in the way — the
/// shape `documents_status.rs`'s `a_stale_transition_writes_nothing` uses one
/// module over, and for the identical reason.
///
/// **Seen red** against the predicate defeated: the stale move updates one row
/// and the instance is put into a state the definition does not reach from where
/// it actually was.
#[tokio::test]
async fn a_stale_move_writes_nothing() {
    use kelir_backend::modules::workflow::domain::InstanceOutcome;
    use kelir_backend::modules::workflow::repository::instance as instance_repo;

    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let workflow = publish_workflow(&app, &token, role_workflow("wf_stale")).await;
    let type_id = document_type(&app, &token, "PR_STALE", Some(workflow)).await;
    let id = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, id).await.status, StatusCode::OK);

    let (instance_id, state, _) = instance_of(&app, id).await.expect("an instance");
    assert_eq!(state, "MANAGER_APPROVAL");

    let mut transaction = app.pool.begin().await.expect("a transaction");

    // The first move wins, from the state the instance is actually in.
    let won = instance_repo::move_state(
        &mut transaction,
        fixtures::SYSTEM_TENANT_ID,
        &instance_repo::StateMove {
            id: instance_id,
            from: "MANAGER_APPROVAL",
            to: "COMPLETED",
            final_state: true,
            outcome: Some(InstanceOutcome::Approved),
        },
        None,
    )
    .await
    .expect("the move runs");
    assert_eq!(won, 1);

    // The second was decided against `MANAGER_APPROVAL` too, and the process has
    // left it. It must write nothing rather than overwrite the first decision.
    let lost = instance_repo::move_state(
        &mut transaction,
        fixtures::SYSTEM_TENANT_ID,
        &instance_repo::StateMove {
            id: instance_id,
            from: "MANAGER_APPROVAL",
            to: "REJECTED",
            final_state: true,
            outcome: Some(InstanceOutcome::Rejected),
        },
        None,
    )
    .await
    .expect("the stale move runs");

    assert_eq!(
        lost, 0,
        "a decision taken against a state the process had left overwrote the one that won"
    );

    transaction.commit().await.expect("commit");

    let (_, state, _) = instance_of(&app, id).await.expect("the instance");
    assert_eq!(
        state, "COMPLETED",
        "the losing move changed the state anyway"
    );
}

/// **A final state ends the instance whatever its status maps to.**
///
/// Found by re-reading `fire` against `move_state`: `isFinal` and the outcome
/// were one argument, so a final state mapping to `IN_REVIEW` — which the JWSS
/// meta-schema permits — left the instance `RUNNING` in a state nothing can
/// leave. It would hold the one-live-instance index against its document
/// forever, and the document could never be transitioned by hand either, because
/// the seam refuses while a process is live.
///
/// **Seen red** against `move_state`'s `$5` reverted to the outcome: the
/// instance is `RUNNING` after entering a final state, and `is_live()` is true.
#[tokio::test]
async fn a_final_state_ends_the_instance_whatever_its_status_maps_to() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let approver = approver(&app, "wf.finality").await;

    // `REVIEWED` is final and maps to `IN_REVIEW`, which yields no outcome.
    let definition = json!({
        "workflowKey": "wf_finality",
        "version": "1.0.0",
        "name": "Ends in review",
        "initialState": "MANAGER_APPROVAL",
        "states": [
            { "code": "MANAGER_APPROVAL", "name": "Manager approval",
              "mapsToDocumentStatus": "PENDING_APPROVAL",
              "task": { "taskDefinitionKey": "manager_approval", "taskName": "Decide",
                        "assignment": { "assigneeType": "ROLE", "roleCode": APPROVER_ROLE } } },
            { "code": "REVIEWED", "name": "Reviewed", "mapsToDocumentStatus": "IN_REVIEW",
              "isFinal": true },
            { "code": "CANCELLED", "name": "Cancelled", "mapsToDocumentStatus": "CANCELLED",
              "isFinal": true }
        ],
        "transitions": [
            { "from": "MANAGER_APPROVAL", "to": "REVIEWED", "action": "APPROVE",
              "allowedBy": format!("ROLE:{APPROVER_ROLE}") },
            { "from": "MANAGER_APPROVAL", "to": "CANCELLED", "action": "REJECT",
              "allowedBy": format!("ROLE:{APPROVER_ROLE}") }
        ]
    });

    let workflow = publish_workflow(&app, &token, definition).await;
    let type_id = document_type(&app, &token, "PR_FINALITY", Some(workflow)).await;
    let id = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, id).await.status, StatusCode::OK);

    let task = open_task_of(&app, id).await;

    let decided = app
        .post(
            &format!("/api/v1/workflow/tasks/{task}/decision"),
            Some(&approver),
            json!({ "action": "APPROVE" }),
        )
        .await;
    assert_eq!(decided.status, StatusCode::OK, "{}", decided.body);

    let (_, state, status) = instance_of(&app, id).await.expect("the instance");

    assert_eq!(state, "REVIEWED");
    assert_eq!(
        status, "COMPLETED",
        "an instance in a final state was left running, so nothing could ever move it \
         and its document could never be transitioned by hand either"
    );
    assert_eq!(stored_status(&app, id).await, "IN_REVIEW");
}

/// **A document type bound to a deprecated workflow refuses the submit rather
/// than starting one.**
///
/// #187 refuses a *binding* to anything but an `ACTIVE` definition, and a
/// definition can be deprecated afterwards — at which point every later
/// submission of that type would start a process against a revision nobody
/// stands behind. `engine::start`'s documentation claimed this check and the
/// code did not make it, which a re-read found.
///
/// **Approvals already running are unaffected**, and that is the other half of
/// the decision: an instance pins its revision, so refusing at the decision
/// would strand every approval in flight the moment an administrator retired the
/// workflow.
///
/// **Seen red** against `engine::start` with the status check removed: the
/// submit succeeds and a process starts against a `DEPRECATED` definition.
#[tokio::test]
async fn a_deprecated_workflow_refuses_the_submit_and_leaves_running_approvals_alone() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let approver = approver(&app, "wf.deprecated").await;

    let workflow = publish_workflow(&app, &token, role_workflow("wf_deprecated")).await;
    let type_id = document_type(&app, &token, "PR_DEPRECATED", Some(workflow)).await;

    // One approval already running against it.
    let running = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, running).await.status, StatusCode::OK);

    sqlx::query("UPDATE workflow_definitions SET status = 'DEPRECATED' WHERE id = $1")
        .bind(workflow)
        .execute(&app.pool)
        .await
        .expect("deprecate the definition");

    // A new document of that type cannot start one.
    let next = draft(&app, &token, type_id).await;
    let refused = submit(&app, &token, next).await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a process started against a deprecated definition: {}",
        refused.body
    );
    assert!(
        refused.body.to_string().contains("WORKFLOW_NOT_PUBLISHED"),
        "{}",
        refused.body
    );
    assert_eq!(
        stored_status(&app, next).await,
        "DRAFT",
        "the whole submit must roll back"
    );

    // And the approval already in flight is still decidable, because it pinned
    // the revision it started against.
    let task = open_task_of(&app, running).await;
    let decided = app
        .post(
            &format!("/api/v1/workflow/tasks/{task}/decision"),
            Some(&approver),
            json!({ "action": "APPROVE" }),
        )
        .await;

    assert_eq!(
        decided.status,
        StatusCode::OK,
        "deprecating a workflow stranded an approval that was already running: {}",
        decided.body
    );
    assert_eq!(stored_status(&app, running).await, "COMPLETED");
}

// ---------------------------------------------------------------------------
// The decision comment (#182, FR-TASK-006), and `requiresComment` (JWSS §4.1)
// ---------------------------------------------------------------------------

/// A workflow whose `REJECT` edge demands a reason and whose `APPROVE` does not.
///
/// **The asymmetry is the subject.** A definition that marked both would let a
/// test pass against an implementation that required a comment for every
/// decision, which is the hard-coded rule JWSS §4.1 exists to avoid — so the two
/// edges differ, and the tests below assert on both.
fn comment_workflow(key: &str) -> Value {
    json!({
        "workflowKey": key,
        "version": "1.0.0",
        "name": "Approval with a reason",
        "initialState": "MANAGER_APPROVAL",
        "states": [
            { "code": "MANAGER_APPROVAL", "name": "Manager approval",
              "mapsToDocumentStatus": "PENDING_APPROVAL",
              "task": { "taskDefinitionKey": "manager_approval", "taskName": "Decide",
                        "assignment": { "assigneeType": "ROLE", "roleCode": APPROVER_ROLE } } },
            { "code": "COMPLETED", "name": "Completed", "mapsToDocumentStatus": "COMPLETED",
              "isFinal": true },
            { "code": "REJECTED", "name": "Rejected", "mapsToDocumentStatus": "REJECTED",
              "isFinal": true }
        ],
        "transitions": [
            { "from": "MANAGER_APPROVAL", "to": "COMPLETED", "action": "APPROVE",
              "allowedBy": format!("ROLE:{APPROVER_ROLE}") },
            { "from": "MANAGER_APPROVAL", "to": "REJECTED", "action": "REJECT",
              "allowedBy": format!("ROLE:{APPROVER_ROLE}"), "requiresComment": true }
        ]
    })
}

/// **One comment, three rows, one transaction** (#182 AC1, AC2).
///
/// The reason an approver gives lands on the task, on the formal decision record
/// and on the history — and the three are asserted separately rather than
/// through the response, because what this item is about is what is durable.
/// The history is the one a person reads, which is why AC2 names it.
///
/// **Seen red** against `repository::history::record` binding `None` for
/// `comment`: the task and the decision record carry the reason, the two
/// assertions above pass, and the account somebody actually opens is blank.
///
/// That mutation rather than the more obvious one — `None` in the
/// `DecisionProvenance` — because the obvious one is not isolating. `fire`
/// reads the same field to enforce `requiresComment`, so blanking it there
/// makes this test fail at the decision with a 422 and says nothing about
/// whether the history was written.
#[tokio::test]
async fn a_decision_comment_reaches_the_task_the_record_and_the_history() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let approver_token = approver(&app, "wf.commenter").await;

    let workflow = publish_workflow(&app, &token, comment_workflow("wf_comment")).await;
    let type_id = document_type(&app, &token, "PR_COMMENT", Some(workflow)).await;
    let id = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, id).await.status, StatusCode::OK);

    let task = open_task_of(&app, id).await;

    let decided = app
        .post(
            &format!("/api/v1/workflow/tasks/{task}/decision"),
            Some(&approver_token),
            json!({
                "action": "REJECT",
                "comment": "  The figure does not match the quotation.  "
            }),
        )
        .await;
    assert_eq!(decided.status, StatusCode::OK, "{}", decided.body);

    // Trimmed on the way in, so the stored value is the sentence rather than the
    // sentence plus whatever the textarea kept around it.
    let expected = "The figure does not match the quotation.";

    let on_task: Option<String> =
        sqlx::query_scalar("SELECT comment FROM workflow_tasks WHERE id = $1")
            .bind(task)
            .fetch_one(&app.pool)
            .await
            .expect("read the task");
    assert_eq!(
        on_task.as_deref(),
        Some(expected),
        "the task kept no reason"
    );

    let on_record: Option<String> =
        sqlx::query_scalar("SELECT comment FROM approval_decisions WHERE task_id = $1")
            .bind(task)
            .fetch_one(&app.pool)
            .await
            .expect("read the decision record");
    assert_eq!(
        on_record.as_deref(),
        Some(expected),
        "the formal decision record kept no reason"
    );

    // AC2, and the one that matters most: the reason is visible where the
    // decision is, through the API a person's screen reads.
    let history = history_of(&app, &token, id).await;
    let rows = history["data"].as_array().expect("a list");
    let moved = rows.last().expect("the decision's row");

    assert_eq!(moved["action"], "REJECT");
    assert_eq!(
        moved["comment"], expected,
        "the history does not carry the reason: {history}"
    );
}

/// **A required comment is refused, and the refusal names the field** (AC4).
///
/// A 422 rather than a 409 or a 403: nothing has changed underneath the caller
/// and they may take this edge — what is missing is something they can supply.
/// The path is `comment`, which is what makes the server's refusal and the
/// screen's the same rule expressed twice rather than two rules.
///
/// **Seen red** against `engine::fire` with the `requires_comment` check
/// removed: the rejection is recorded with no reason on it, which is precisely
/// the outcome the Sprint 10 construction plan §7.5 named as this item's cost.
#[tokio::test]
async fn a_transition_that_requires_a_reason_refuses_a_decision_without_one() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let approver_token = approver(&app, "wf.silent").await;

    let workflow = publish_workflow(&app, &token, comment_workflow("wf_comment_required")).await;
    let type_id = document_type(&app, &token, "PR_COMMENT_REQUIRED", Some(workflow)).await;
    let id = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, id).await.status, StatusCode::OK);

    let task = open_task_of(&app, id).await;

    let refused = app
        .post(
            &format!("/api/v1/workflow/tasks/{task}/decision"),
            Some(&approver_token),
            json!({ "action": "REJECT" }),
        )
        .await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a rejection was recorded with no reason on it: {}",
        refused.body
    );

    let body = refused.body.to_string();
    assert!(body.contains("COMMENT_REQUIRED"), "{body}");
    assert!(
        body.contains("MANAGER_APPROVAL") && body.contains("REJECT"),
        "the refusal must say which decision wanted a reason: {body}"
    );

    // Nothing happened. The refusal is raised inside the transaction, so the
    // task is still open and the document has not moved — a refusal that left
    // the task completed would be worse than the missing comment.
    assert_eq!(stored_status(&app, id).await, "PENDING_APPROVAL");
    assert_eq!(open_task_of(&app, id).await, task);

    // And a whitespace-only comment is the same as none, which is the rule that
    // stops the requirement being satisfied by the space bar.
    let blank = app
        .post(
            &format!("/api/v1/workflow/tasks/{task}/decision"),
            Some(&approver_token),
            json!({ "action": "REJECT", "comment": "   \n  " }),
        )
        .await;
    assert_eq!(
        blank.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a box full of spaces satisfied a required reason: {}",
        blank.body
    );

    // The same edge, with a reason, goes through — so the refusal above is about
    // the missing comment rather than about the edge refusing everybody, which
    // is the gate coding standard §2.9 warns about.
    let decided = app
        .post(
            &format!("/api/v1/workflow/tasks/{task}/decision"),
            Some(&approver_token),
            json!({ "action": "REJECT", "comment": "Duplicate of PR-2026-000004." }),
        )
        .await;
    assert_eq!(decided.status, StatusCode::OK, "{}", decided.body);
    assert_eq!(stored_status(&app, id).await, "REJECTED");
}

/// **An edge the definition did not mark takes a decision with no reason** (AC4).
///
/// The other half of the asymmetry, and the reason it is a separate test: an
/// implementation that required a comment for every decision would pass the one
/// above. The `APPROVE` edge of [`comment_workflow`] is unmarked, and a comment
/// given anyway is still recorded — optional does not mean discarded.
#[tokio::test]
async fn an_unmarked_transition_needs_no_reason_and_still_keeps_one() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let approver_token = approver(&app, "wf.optional").await;

    let workflow = publish_workflow(&app, &token, comment_workflow("wf_comment_optional")).await;
    let type_id = document_type(&app, &token, "PR_COMMENT_OPTIONAL", Some(workflow)).await;

    // No reason at all, on the edge that does not ask for one.
    let bare = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, bare).await.status, StatusCode::OK);

    let bare_task = open_task_of(&app, bare).await;
    let decided = app
        .post(
            &format!("/api/v1/workflow/tasks/{bare_task}/decision"),
            Some(&approver_token),
            json!({ "action": "APPROVE" }),
        )
        .await;
    assert_eq!(
        decided.status,
        StatusCode::OK,
        "an unmarked edge demanded a reason: {}",
        decided.body
    );
    assert_eq!(stored_status(&app, bare).await, "COMPLETED");

    // And one given anyway on the same edge is kept.
    let told = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, told).await.status, StatusCode::OK);

    let told_task = open_task_of(&app, told).await;
    let decided = app
        .post(
            &format!("/api/v1/workflow/tasks/{told_task}/decision"),
            Some(&approver_token),
            json!({ "action": "APPROVE", "comment": "Within the department budget." }),
        )
        .await;
    assert_eq!(decided.status, StatusCode::OK, "{}", decided.body);

    let history = history_of(&app, &token, told).await;
    let rows = history["data"].as_array().expect("a list");
    assert_eq!(
        rows.last().expect("the decision's row")["comment"],
        "Within the department budget.",
        "an optional reason was discarded: {history}"
    );
}

/// **The comment is bounded, and the refusal names the field** (#182).
///
/// `workflow_tasks.comment` is `TEXT`, so nothing in the schema bounds it and a
/// caller could otherwise write an unbounded row into a table [#181] AC6 makes
/// impossible to edit or delete. A 422 naming `comment`, not a `sqlx` error
/// surfacing as a 500.
#[tokio::test]
async fn a_comment_longer_than_the_limit_is_refused_before_anything_is_written() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let approver_token = approver(&app, "wf.verbose").await;

    let workflow = publish_workflow(&app, &token, role_workflow("wf_comment_long")).await;
    let type_id = document_type(&app, &token, "PR_COMMENT_LONG", Some(workflow)).await;
    let id = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, id).await.status, StatusCode::OK);

    let task = open_task_of(&app, id).await;

    let refused = app
        .post(
            &format!("/api/v1/workflow/tasks/{task}/decision"),
            Some(&approver_token),
            json!({ "action": "APPROVE", "comment": "x".repeat(4001) }),
        )
        .await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert!(
        refused.body.to_string().contains("TOO_LONG"),
        "{}",
        refused.body
    );
    assert_eq!(stored_status(&app, id).await, "PENDING_APPROVAL");
}

/// **The task detail tells the screen which decisions need a reason** (AC4).
///
/// This is what makes *both ends agree* true by construction rather than by two
/// implementations happening to match: the definition marks the edge, the detail
/// carries the mark, and the screen reads it. A client deriving the rule for
/// itself would disagree with the server the first time a workflow marked an
/// `APPROVE` — so the payload is asserted here, on the field the screen binds
/// to.
#[tokio::test]
async fn the_task_detail_says_which_decisions_require_a_reason() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let approver_token = approver(&app, "wf.informed").await;

    let workflow = publish_workflow(&app, &token, comment_workflow("wf_comment_detail")).await;
    let type_id = document_type(&app, &token, "PR_COMMENT_DETAIL", Some(workflow)).await;
    let id = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, id).await.status, StatusCode::OK);

    let task = open_task_of(&app, id).await;

    let detail = app
        .get(&format!("/api/v1/tasks/{task}"), Some(&approver_token))
        .await;
    assert_eq!(detail.status, StatusCode::OK, "{}", detail.body);

    let decisions = detail.body["data"]["decisions"]
        .as_array()
        .expect("the decisions on offer");

    let approve = decisions
        .iter()
        .find(|decision| decision["action"] == "APPROVE")
        .expect("the APPROVE edge");
    let reject = decisions
        .iter()
        .find(|decision| decision["action"] == "REJECT")
        .expect("the REJECT edge");

    assert_eq!(
        approve["requiresComment"], false,
        "an unmarked edge reported as needing a reason: {}",
        detail.body
    );
    assert_eq!(
        reject["requiresComment"], true,
        "a marked edge did not reach the screen: {}",
        detail.body
    );
}

/// **`requiresComment: true` on an `AUTO` transition is refused at save** (S12).
///
/// An `AUTO` transition fires without a caller, so an edge asking one for a
/// reason is an edge that can never fire — a stalled instance nobody is told
/// about, produced by a definition that published cleanly. It is the failure
/// **D-37** refuses two assignee types at save time to avoid, and it is refused
/// the same way and for the same reason.
#[tokio::test]
async fn an_auto_transition_cannot_demand_a_reason_nobody_can_give() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let definition = json!({
        "workflowKey": "wf_auto_comment",
        "version": "1.0.0",
        "name": "Auto with a reason",
        "initialState": "MANAGER_APPROVAL",
        "states": [
            { "code": "MANAGER_APPROVAL", "name": "Manager approval",
              "mapsToDocumentStatus": "PENDING_APPROVAL",
              "task": { "taskDefinitionKey": "manager_approval", "taskName": "Decide",
                        "assignment": { "assigneeType": "ROLE", "roleCode": APPROVER_ROLE } } },
            { "code": "COMPLETED", "name": "Completed", "mapsToDocumentStatus": "COMPLETED",
              "isFinal": true }
        ],
        "transitions": [
            { "from": "MANAGER_APPROVAL", "to": "COMPLETED", "action": "AUTO",
              "requiresComment": true }
        ]
    });

    let refused = app
        .post(
            "/api/v1/workflow/definitions",
            Some(&token),
            json!({ "workflowKey": "wf_auto_comment", "name": "Auto", "definition": definition }),
        )
        .await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "an AUTO edge demanding a reason was stored: {}",
        refused.body
    );

    // And the same definition without the flag is accepted, so the refusal is
    // about `requiresComment` rather than about `AUTO`.
    let mut allowed = definition.clone();
    allowed["transitions"][0]
        .as_object_mut()
        .expect("the transition")
        .remove("requiresComment");
    allowed["workflowKey"] = json!("wf_auto_plain");

    let stored = app
        .post(
            "/api/v1/workflow/definitions",
            Some(&token),
            json!({ "workflowKey": "wf_auto_plain", "name": "Auto", "definition": allowed }),
        )
        .await;
    assert_eq!(stored.status, StatusCode::CREATED, "{}", stored.body);
}

// ---------------------------------------------------------------------------
// The return action (#183, FR-WF-008), and the resubmission that closes the loop
// ---------------------------------------------------------------------------

/// A workflow with a return target, and the resubmission that comes back up.
///
/// `RETURNED` **declares no task**, which is JWSS §10's own shape and the reason
/// this item needed a path that fires a transition without one: the document is
/// with its author, not in anybody's queue. The `RESUBMIT` edge out of it is
/// `allowedBy: "OWNER"`, so the owner is who may send it back up — and #226's
/// `permits` is what enforces that.
///
/// `RETURN` requires a comment (#182), because *"why is this back with me"* is
/// the question return exists to answer and the definition is where that is
/// said.
fn returnable_workflow(key: &str) -> Value {
    json!({
        "workflowKey": key,
        "version": "1.0.0",
        "name": "Approval that can send back",
        "initialState": "MANAGER_APPROVAL",
        "states": [
            { "code": "MANAGER_APPROVAL", "name": "Manager approval",
              "mapsToDocumentStatus": "PENDING_APPROVAL",
              "task": { "taskDefinitionKey": "manager_approval", "taskName": "Decide",
                        "assignment": { "assigneeType": "ROLE", "roleCode": APPROVER_ROLE } } },
            { "code": "RETURNED", "name": "Sent back", "mapsToDocumentStatus": "RETURNED" },
            { "code": "COMPLETED", "name": "Completed", "mapsToDocumentStatus": "COMPLETED",
              "isFinal": true },
            { "code": "REJECTED", "name": "Rejected", "mapsToDocumentStatus": "REJECTED",
              "isFinal": true }
        ],
        "transitions": [
            { "from": "MANAGER_APPROVAL", "to": "COMPLETED", "action": "APPROVE",
              "allowedBy": format!("ROLE:{APPROVER_ROLE}") },
            { "from": "MANAGER_APPROVAL", "to": "REJECTED", "action": "REJECT",
              "allowedBy": format!("ROLE:{APPROVER_ROLE}"), "requiresComment": true },
            { "from": "MANAGER_APPROVAL", "to": "RETURNED", "action": "RETURN",
              "allowedBy": format!("ROLE:{APPROVER_ROLE}"), "requiresComment": true },
            { "from": "RETURNED", "to": "MANAGER_APPROVAL", "action": "RESUBMIT",
              "allowedBy": "OWNER" }
        ]
    })
}

/// The document's number, straight from the row.
async fn stored_number(app: &TestApp, id: Uuid) -> Option<String> {
    sqlx::query_scalar("SELECT document_number FROM documents WHERE id = $1")
        .bind(id)
        .fetch_one(&app.pool)
        .await
        .expect("read the document number")
}

/// **The whole loop** (#183 AC1, AC4, AC5): approve is not the only way out.
///
/// A document is submitted, returned with a reason, corrected by its owner,
/// sent back up, and approved. The number is captured before the return and
/// asserted after the resubmission, because **keeping it is the outcome return
/// exists to preserve** — a document that came back with a new number would have
/// lost its place in every report and every conversation about it.
///
/// **Seen red** against `repository::document::mark_submitted` with
/// `COALESCE($3, document_number)` reduced to `$3`: the resubmission writes
/// `NULL` over the number and the document comes back up anonymous.
#[tokio::test]
async fn a_returned_document_is_corrected_resubmitted_and_keeps_its_number() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let approver_token = approver(&app, "wf.returner").await;

    let workflow = publish_workflow(&app, &token, returnable_workflow("wf_return")).await;
    let type_id = document_type(&app, &token, "PR_RETURN", Some(workflow)).await;
    let id = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, id).await.status, StatusCode::OK);

    let number = stored_number(&app, id).await.expect("a number at submit");
    let task = open_task_of(&app, id).await;

    // --- The approver sends it back, with the reason -----------------------
    let returned = app
        .post(
            &format!("/api/v1/workflow/tasks/{task}/decision"),
            Some(&approver_token),
            json!({ "action": "RETURN", "comment": "The quotation is for 12 chairs, not 10." }),
        )
        .await;
    assert_eq!(returned.status, StatusCode::OK, "{}", returned.body);
    assert_eq!(returned.body["data"]["currentState"], "RETURNED");
    assert_eq!(returned.body["data"]["documentStatus"], "RETURNED");
    assert_eq!(stored_status(&app, id).await, "RETURNED");

    // **The instance is still running.** That is the whole difference between a
    // return and a rejection: nothing ended, so there is something to come back
    // to.
    let (_, state, instance_status) = instance_of(&app, id).await.expect("the instance");
    assert_eq!(state, "RETURNED");
    assert_eq!(
        instance_status, "RUNNING",
        "a return ended the process, which makes it a rejection with a softer name"
    );

    // --- AC1: it is editable again -----------------------------------------
    let corrected = app
        .put(
            &format!("/api/v1/documents/{id}"),
            Some(&token),
            json!({ "formData": { "amount": 12000 } }),
        )
        .await;
    assert_eq!(
        corrected.status,
        StatusCode::OK,
        "a returned document could not be corrected, which makes return a rejection: {}",
        corrected.body
    );

    // --- AC5: the owner sends it back up, and the number does not move ------
    let resubmitted = submit(&app, &token, id).await;
    assert_eq!(resubmitted.status, StatusCode::OK, "{}", resubmitted.body);

    assert_eq!(
        stored_number(&app, id).await.as_deref(),
        Some(number.as_str()),
        "the resubmission changed the document's number, which is what return exists to avoid"
    );
    assert_eq!(stored_status(&app, id).await, "PENDING_APPROVAL");

    let (_, state, _) = instance_of(&app, id).await.expect("the instance");
    assert_eq!(
        state, "MANAGER_APPROVAL",
        "the resubmission did not move the process"
    );

    // --- AC4: the history says how it got here, and why ---------------------
    let history = history_of(&app, &token, id).await;
    let rows = history["data"].as_array().expect("a list");

    let sent_back = rows
        .iter()
        .find(|row| row["action"] == "RETURN")
        .unwrap_or_else(|| panic!("the return is not in the history: {history}"));

    assert_eq!(sent_back["fromState"], "MANAGER_APPROVAL");
    assert_eq!(
        sent_back["toState"], "RETURNED",
        "the target is the definition's, not inferred"
    );
    assert_eq!(
        sent_back["comment"], "The quotation is for 12 chairs, not 10.",
        "the reason a document is back with its author is the question history answers"
    );

    let came_back = rows
        .iter()
        .find(|row| row["action"] == "RESUBMIT")
        .unwrap_or_else(|| panic!("the resubmission is not in the history: {history}"));

    assert_eq!(came_back["fromState"], "RETURNED");
    assert_eq!(came_back["toState"], "MANAGER_APPROVAL");

    // --- And the loop closes: the new task approves ------------------------
    let next_task = open_task_of(&app, id).await;
    assert_ne!(next_task, task, "the resubmission reused the decided task");

    let approved = app
        .post(
            &format!("/api/v1/workflow/tasks/{next_task}/decision"),
            Some(&approver_token),
            json!({ "action": "APPROVE" }),
        )
        .await;
    assert_eq!(approved.status, StatusCode::OK, "{}", approved.body);
    assert_eq!(stored_status(&app, id).await, "COMPLETED");
    assert_eq!(
        stored_number(&app, id).await.as_deref(),
        Some(number.as_str()),
        "the number moved somewhere in the round trip"
    );
}

/// **AC6: return is refused where the definition names no return target.**
///
/// The error envelope rather than a silent no-op, and the same 422 an
/// unavailable `APPROVE` earns — the request names an action the process cannot
/// take *from where it is*, which is a property of the payload against the
/// resource.
///
/// The second half is what makes the first mean something: the same workflow
/// takes a `REJECT` from the same task, so the refusal is about the missing
/// `RETURN` edge rather than about the task refusing everything.
#[tokio::test]
async fn return_is_refused_where_the_definition_names_no_return_target() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let approver_token = approver(&app, "wf.noreturn").await;

    // `role_workflow` offers APPROVE and REJECT and no way back.
    let workflow = publish_workflow(&app, &token, role_workflow("wf_no_return")).await;
    let type_id = document_type(&app, &token, "PR_NO_RETURN", Some(workflow)).await;
    let id = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, id).await.status, StatusCode::OK);

    let task = open_task_of(&app, id).await;

    let refused = app
        .post(
            &format!("/api/v1/workflow/tasks/{task}/decision"),
            Some(&approver_token),
            json!({ "action": "RETURN" }),
        )
        .await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a return fired against a definition with no return target: {}",
        refused.body
    );

    let body = refused.body.to_string();
    assert!(body.contains("NO_SUCH_TRANSITION"), "{body}");
    assert!(
        body.contains("APPROVE") && body.contains("REJECT"),
        "the refusal must name what is possible from here: {body}"
    );

    // Nothing happened, and the task is still there to decide.
    assert_eq!(stored_status(&app, id).await, "PENDING_APPROVAL");
    assert_eq!(open_task_of(&app, id).await, task);

    let decided = app
        .post(
            &format!("/api/v1/workflow/tasks/{task}/decision"),
            Some(&approver_token),
            json!({ "action": "REJECT", "comment": "No." }),
        )
        .await;
    assert_eq!(decided.status, StatusCode::OK, "{}", decided.body);
}

/// **AC3: one decision per task, and return is not an exception.**
///
/// The same compare-and-swap #177 established, at the same concurrency, with
/// `RETURN` in the mix — which is the point: a verb that reached the write
/// through a different branch would be a verb the predicate did not cover, and
/// the failure would be two decisions recorded against one task.
///
/// **Seen red** against `repository::task::complete` with its status predicate
/// removed **and** `service::task::decide`'s locked `refuse_unless_open` gone:
/// several callers win and the winners disagree about where the process went.
///
/// **Both, and the reason is the lock.** `FOR UPDATE` serialises these callers,
/// so the second one reads a `COMPLETED` task and is refused by the service
/// before it ever reaches the statement — which is the same thing `engine::fire`
/// says about its own compare-and-swap. The statement's predicate is what still
/// holds if somebody writes a second caller that forgets the check, so removing
/// only one of the two leaves a build that is still correct.
#[tokio::test]
async fn concurrent_returns_and_approvals_still_resolve_to_one_outcome() {
    let app = Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;

    let workflow = publish_workflow(&app, &token, returnable_workflow("wf_return_race")).await;
    let type_id = document_type(&app, &token, "PR_RETURN_RACE", Some(workflow)).await;
    let id = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, id).await.status, StatusCode::OK);

    let task = open_task_of(&app, id).await;

    let mut tokens = Vec::new();
    for index in 0..CONCURRENT_CALLERS {
        tokens.push(approver(&app, &format!("wf.returner{index}")).await);
    }

    let mut handles = Vec::new();

    for (index, token) in tokens.into_iter().enumerate() {
        let app = Arc::clone(&app);
        // Every third caller sends it back; the rest approve. Both verbs reach
        // the same statement, which is what this is about.
        let body = if index % 3 == 0 {
            json!({ "action": "RETURN", "comment": "Sent back." })
        } else {
            json!({ "action": "APPROVE" })
        };

        handles.push(tokio::spawn(async move {
            app.post(
                &format!("/api/v1/workflow/tasks/{task}/decision"),
                Some(&token),
                body,
            )
            .await
        }));
    }

    let mut winners = HashSet::new();
    let mut lost = 0usize;

    for handle in handles {
        let response = handle.await.expect("a decision finished");

        match response.status {
            StatusCode::OK => {
                winners.insert(
                    response.body["data"]["currentState"]
                        .as_str()
                        .expect("a state")
                        .to_owned(),
                );
            }
            StatusCode::CONFLICT => lost += 1,
            other => panic!(
                "a decision answered {other}, which is neither winning nor losing: {}",
                response.body
            ),
        }
    }

    assert_eq!(
        winners.len(),
        1,
        "more than one caller decided one task: {winners:?}"
    );
    assert_eq!(lost, CONCURRENT_CALLERS - 1, "every loser must be told");

    // And the row agrees with the one winner, whichever verb it was.
    let recorded: i64 =
        sqlx::query_scalar("SELECT count(*) FROM approval_decisions WHERE task_id = $1")
            .bind(task)
            .fetch_one(&app.pool)
            .await
            .expect("count the decisions");
    assert_eq!(recorded, 1, "one task, one formal decision");
}

/// **A returned document is corrected, not discarded** ([#183], `is_discardable`).
///
/// The half of the editable predicate that did **not** widen. A returned
/// document has a number, a status history and a live process waiting for it, so
/// deleting it would strand the instance that returned it — and the refusal says
/// so rather than repeating "only a draft" at somebody who has just been told
/// they may edit it.
///
/// **Seen red** against `is_discardable` returning `is_editable()` **and**
/// `repository::document::soft_delete`'s `WHERE` widened to match: the delete
/// succeeds and the workflow instance is left pointing at a soft-deleted
/// document.
///
/// **Both, because either alone still refuses**, which is worth knowing rather
/// than hiding behind a one-line mutation. The service checks the predicate and
/// the statement carries its own — `soft_delete` says why it is not redundant —
/// so a build with one of them opened is still correct, and only a build with
/// both opened is the defect this asserts against.
#[tokio::test]
async fn a_returned_document_may_be_edited_and_may_not_be_deleted() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let approver_token = approver(&app, "wf.undeletable").await;

    let workflow = publish_workflow(&app, &token, returnable_workflow("wf_return_delete")).await;
    let type_id = document_type(&app, &token, "PR_RETURN_DELETE", Some(workflow)).await;
    let id = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, id).await.status, StatusCode::OK);

    let task = open_task_of(&app, id).await;
    let returned = app
        .post(
            &format!("/api/v1/workflow/tasks/{task}/decision"),
            Some(&approver_token),
            json!({ "action": "RETURN", "comment": "Please attach the quotation." }),
        )
        .await;
    assert_eq!(returned.status, StatusCode::OK, "{}", returned.body);

    let refused = app
        .delete(&format!("/api/v1/documents/{id}"), Some(&token))
        .await;

    assert_eq!(
        refused.status,
        StatusCode::CONFLICT,
        "a returned document was deleted, stranding the process that returned it: {}",
        refused.body
    );
    assert!(
        refused.body.to_string().contains("RETURNED"),
        "the refusal must name where the document is: {}",
        refused.body
    );

    // Still there, and still correctable — which is the pair that makes the two
    // predicates different rather than one of them simply narrower.
    assert_eq!(stored_status(&app, id).await, "RETURNED");

    let corrected = app
        .put(
            &format!("/api/v1/documents/{id}"),
            Some(&token),
            json!({ "title": "Twelve ergonomic chairs" }),
        )
        .await;
    assert_eq!(corrected.status, StatusCode::OK, "{}", corrected.body);
}

/// **A resubmission takes no number from the sequence** ([#183] AC5).
///
/// Stronger than *the document keeps its number*, and the failure it catches is
/// different: a submit that allocated and then discarded would leave the
/// document correct and the **sequence** short by one per correction round. On a
/// gap-tolerant rule that hole is permanent.
///
/// Asserted through the next document's number rather than through the counter,
/// because the counter is an implementation detail and the number a person sees
/// is not.
#[tokio::test]
async fn a_resubmission_does_not_consume_a_number_from_the_sequence() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let approver_token = approver(&app, "wf.sequence").await;

    let workflow = publish_workflow(&app, &token, returnable_workflow("wf_return_sequence")).await;
    let type_id = document_type(&app, &token, "PR_RETURN_SEQ", Some(workflow)).await;

    let first = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, first).await.status, StatusCode::OK);
    let first_number = stored_number(&app, first).await.expect("a number");

    // Send it back and up again, twice, so a leak would be unmistakable.
    for round in 0..2 {
        let task = open_task_of(&app, first).await;
        let returned = app
            .post(
                &format!("/api/v1/workflow/tasks/{task}/decision"),
                Some(&approver_token),
                json!({ "action": "RETURN", "comment": format!("Round {round}.") }),
            )
            .await;
        assert_eq!(returned.status, StatusCode::OK, "{}", returned.body);
        assert_eq!(submit(&app, &token, first).await.status, StatusCode::OK);
    }

    // The next document takes the number immediately after the first one's. If
    // a resubmission had allocated, this would be three higher.
    let second = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, second).await.status, StatusCode::OK);
    let second_number = stored_number(&app, second).await.expect("a number");

    let next_of = |number: &str| -> u64 {
        number
            .rsplit('-')
            .next()
            .expect("a numeric tail")
            .parse()
            .expect("a number")
    };

    assert_eq!(
        next_of(&second_number),
        next_of(&first_number) + 1,
        "two resubmissions consumed {} numbers from the sequence ({first_number} then \
         {second_number})",
        next_of(&second_number) - next_of(&first_number) - 1
    );
}

// ---------------------------------------------------------------------------
// #278 — a discard cannot strand a live approval
// ---------------------------------------------------------------------------

/// A workflow whose **non-final initial state maps to `DRAFT`**.
///
/// This is [#278]'s precondition, and it is one word in a definition rather
/// than a contrivance: `DRAFT` is in the platform enum, in the meta-schema and
/// in `jwss::DOCUMENT_STATUSES`; S9 constrains only that *some final* state
/// maps to `COMPLETED` or `CANCELLED`; and [JWSS §10]'s own worked example maps
/// its initial state to `DRAFT`. **D-46** decided that stays permitted, so this
/// definition publishes — the assertion is inside [`publish_workflow`] — and
/// what changed is the guard downstream of it.
///
/// No other definition in this repository maps a state to `DRAFT`. That is why
/// the defect survived to Sprint 12: it was a trap laid for the first author
/// reaching for the status that means *editable again* without knowing
/// `RETURNED` is the one this product was built around.
///
/// [#278]: https://github.com/sujanto-gaws/kelir/issues/278
/// [JWSS §10]: ../../docs/schema/JSON%20Workflow%20Schema.md
fn draft_mapping_workflow(key: &str) -> Value {
    json!({
        "workflowKey": key,
        "version": "1.0.0",
        "name": "An approval that leaves the document editable",
        "initialState": "MANAGER_APPROVAL",
        "states": [
            { "code": "MANAGER_APPROVAL", "name": "Manager approval",
              "mapsToDocumentStatus": "DRAFT",
              "task": { "taskDefinitionKey": "manager_approval", "taskName": "Approve the request",
                        "assignment": { "assigneeType": "ROLE", "roleCode": APPROVER_ROLE } } },
            { "code": "COMPLETED", "name": "Completed", "mapsToDocumentStatus": "COMPLETED",
              "isFinal": true },
            { "code": "REJECTED", "name": "Rejected", "mapsToDocumentStatus": "REJECTED",
              "isFinal": true }
        ],
        "transitions": [
            { "from": "MANAGER_APPROVAL", "to": "COMPLETED", "action": "APPROVE",
              "allowedBy": format!("ROLE:{APPROVER_ROLE}") },
            { "from": "MANAGER_APPROVAL", "to": "REJECTED", "action": "REJECT",
              "allowedBy": format!("ROLE:{APPROVER_ROLE}") }
        ]
    })
}

/// **A document a workflow is deciding cannot be discarded, whatever its
/// status** ([#278] AC1, AC2, AC3).
///
/// The guard `delete_document` had was `status = 'DRAFT'`, a **proxy** for *has
/// no live process*, and the projection is what makes the proxy false: this
/// document is `DRAFT` with a number, a `RUNNING` instance and an open task.
///
/// **Reproduced before it was fixed** (AC5), on 2026-09-01, because the finding
/// was traced in source rather than executed — [record 09] §7 says so in its
/// own header. What the run showed, against `main` at `3767a44`:
///
/// ```text
/// delete answered 204 No Content
/// document deleted_at = Some(…), instance status = RUNNING
/// claim answered 200 OK          <- the task is still claimable
/// decision answered 404 Not Found: "Document not found"
/// ```
///
/// The claim succeeding is the part the issue did not predict and the part that
/// makes it worst: an approver is handed the task, takes it, and only then
/// finds the document gone. Nothing could move that instance again — not the
/// approver, not an administrator — because `find_document` filters
/// `deleted_at IS NULL` and every later decision reads through it.
///
/// **Seen red, 2026-09-01**, with the `refuse_while_a_workflow_is_deciding`
/// call deleted from `service::document::delete_document`: the delete answers
/// 204 and the assertions below fail on the first one.
///
/// [#278]: https://github.com/sujanto-gaws/kelir/issues/278
/// [record 09]: ../../projects/verifications/09.%20Sprint%2011%20Independent%20Pass.md
#[tokio::test]
async fn a_document_a_workflow_is_deciding_cannot_be_discarded() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let _ = approver_role(&app).await;

    let workflow = publish_workflow(&app, &token, draft_mapping_workflow("wf_278_guard")).await;
    let type_id = document_type(&app, &token, "PR_278_GUARD", Some(workflow)).await;
    let id = draft(&app, &token, type_id).await;

    assert_eq!(submit(&app, &token, id).await.status, StatusCode::OK);

    // The precondition, asserted rather than assumed: the projection put this
    // document back in `DRAFT` while its approval runs.
    assert_eq!(
        stored_status(&app, id).await,
        "DRAFT",
        "the precondition is gone: this test is no longer about #278"
    );
    let (instance, _, instance_status) = instance_of(&app, id).await.expect("a live instance");
    assert_eq!(instance_status, "RUNNING");
    let task = open_task_of(&app, id).await;

    let number: Option<String> =
        sqlx::query_scalar("SELECT document_number FROM documents WHERE id = $1")
            .bind(id)
            .fetch_one(&app.pool)
            .await
            .expect("the document row");
    assert!(
        number.is_some(),
        "a submitted document holds a number, which is half of what a discard would retire"
    );

    let refused = app
        .send(
            Method::DELETE,
            &format!("/api/v1/documents/{id}"),
            Some(&token),
            None,
        )
        .await;

    assert_eq!(
        refused.status,
        StatusCode::CONFLICT,
        "a document with a live approval was discarded: {}",
        refused.body
    );

    // AC2 — the refusal names the instance, which is the status route's shape.
    // A caller told only "no" cannot find the process they have to act on.
    let message = refused.body["error"]["message"]
        .as_str()
        .expect("a message");
    assert!(
        message.contains(&instance.to_string()),
        "the refusal does not name the instance a caller has to act on: {message}"
    );

    let deleted: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT deleted_at FROM documents WHERE id = $1")
            .bind(id)
            .fetch_one(&app.pool)
            .await
            .expect("the document row");
    assert!(deleted.is_none(), "the refusal wrote `deleted_at` anyway");

    // **And the process is unharmed**, which is the other half of the claim: a
    // refusal that left the approval undecidable would be this defect reached
    // by a different route.
    let approver_token = approver(&app, "wf-278-approver").await;
    assert_eq!(
        app.post(
            &format!("/api/v1/workflow/tasks/{task}/claim"),
            Some(&approver_token),
            json!({}),
        )
        .await
        .status,
        StatusCode::OK
    );

    let decided = app
        .post(
            &format!("/api/v1/workflow/tasks/{task}/decision"),
            Some(&approver_token),
            json!({ "action": "APPROVE", "comment": "Approved" }),
        )
        .await;
    assert_eq!(decided.status, StatusCode::OK, "{}", decided.body);
    assert_eq!(stored_status(&app, id).await, "COMPLETED");
}

/// The other half: **the guard is about the process, not about the delete**.
///
/// A draft under a type that binds no workflow is still discarded, so the
/// refusal above is a live instance being found rather than
/// `delete_document` having stopped working — the shape
/// `a_document_under_a_workflow_cannot_have_its_status_set_by_hand` uses for
/// the same reason one rule over.
#[tokio::test]
async fn a_draft_with_no_process_behind_it_is_still_discarded() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let type_id = document_type(&app, &token, "PR_278_FREE", None).await;
    let id = draft(&app, &token, type_id).await;

    assert!(
        instance_of(&app, id).await.is_none(),
        "this document was supposed to have no process"
    );

    let discarded = app
        .send(
            Method::DELETE,
            &format!("/api/v1/documents/{id}"),
            Some(&token),
            None,
        )
        .await;

    assert_eq!(
        discarded.status,
        StatusCode::NO_CONTENT,
        "the live-instance guard refused a document with no instance: {}",
        discarded.body
    );
}
