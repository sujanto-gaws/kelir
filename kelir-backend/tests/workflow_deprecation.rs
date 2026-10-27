//! A published workflow revision is deprecated by a route ([#573], **D-101** B,
//! **D-108**).
//!
//! `POST /api/v1/workflow/definitions/{id}/deprecation` moves an `ACTIVE`
//! revision to `DEPRECATED` and nothing else. Before it, only SQL could, so a
//! tenant's old revisions stayed `ACTIVE` for good and a role one of them named
//! could not be deleted while it did (#510).
//!
//! **What deprecating does is mostly decided elsewhere**, and these tests pin
//! it from the route:
//!
//! * a running approval carries on, because an instance pins its revision;
//! * a document type bound to the revision stays bound, and its next
//!   submission is refused as `WORKFLOW_NOT_PUBLISHED` by `engine::start` until
//!   the type is bound to a published revision;
//! * a role the revision names stops being held by it.
//!
//! Every test here was seen red before the route existed, and the controls the
//! route adds were seen red again under the mutation each doc comment names.
//!
//! [#573]: https://github.com/sujanto-gaws/kelir/issues/573

mod common;

use std::sync::Arc;
use std::time::Duration;

use axum::http::{Method, StatusCode};
use common::{deadlocks_once_settled, deadlocks_so_far, fixtures, TestApp};
use kelir_backend::modules::workflow::repository::definition as definition_repo;
use serde_json::{json, Value};
use sqlx::{Connection, PgConnection};
use tokio::task::JoinHandle;
use uuid::Uuid;

const DEFINITIONS: &str = "/api/v1/workflow/definitions";
const APPROVER_ROLE: &str = "DEP-APPROVER";

fn id_of(value: &Value) -> Uuid {
    value["id"]
        .as_str()
        .expect("an id")
        .parse()
        .expect("a uuid")
}

/// One approval, approved or rejected, offered to [`APPROVER_ROLE`].
fn approval_workflow(key: &str) -> Value {
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
        ]
    })
}

/// The approver role in `tenant`, created once: a publish refuses a definition
/// naming a role that is not live (D-111).
async fn approver_role(app: &TestApp, tenant: Uuid) -> Uuid {
    if let Some(id) = fixtures::live_role(&app.pool, tenant, APPROVER_ROLE).await {
        return id;
    }

    fixtures::create_role_with_permissions(
        &app.pool,
        tenant,
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

async fn saved(app: &TestApp, token: &str, key: &str) -> Uuid {
    let created = app
        .post(
            DEFINITIONS,
            Some(token),
            json!({ "workflowKey": key, "name": "Standard approval",
                    "definition": approval_workflow(key) }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    id_of(&created.body["data"])
}

async fn publish(app: &TestApp, token: &str, id: Uuid) -> common::TestResponse {
    app.post(
        &format!("{DEFINITIONS}/{id}/publication"),
        Some(token),
        json!({}),
    )
    .await
}

async fn deprecate(app: &TestApp, token: &str, id: Uuid) -> common::TestResponse {
    app.post(
        &format!("{DEFINITIONS}/{id}/deprecation"),
        Some(token),
        json!({}),
    )
    .await
}

/// Creates and publishes a workflow in the system tenant.
async fn published(app: &TestApp, token: &str, key: &str) -> Uuid {
    approver_role(app, fixtures::SYSTEM_TENANT_ID).await;
    let id = saved(app, token, key).await;

    let publication = publish(app, token, id).await;
    assert_eq!(publication.status, StatusCode::OK, "{}", publication.body);

    id
}

async fn status_of(app: &TestApp, id: Uuid) -> String {
    sqlx::query_scalar("SELECT status FROM workflow_definitions WHERE id = $1")
        .bind(id)
        .fetch_one(&app.pool)
        .await
        .expect("read the revision's status")
}

/// A user in `tenant` holding a role that grants exactly `permissions`.
async fn caller_holding(
    app: &TestApp,
    tenant: Uuid,
    tenant_code: Option<&str>,
    username: &str,
    permissions: &[&str],
) -> String {
    let role = fixtures::create_role_with_permissions(
        &app.pool,
        tenant,
        &format!("ROLE-{}", username.to_uppercase().replace('.', "-")),
        permissions,
    )
    .await;
    fixtures::create_user(
        &app.pool,
        tenant,
        username,
        &format!("{username}@example.test"),
        common::ADMIN_PASSWORD,
        &[role],
    )
    .await;

    match tenant_code {
        Some(code) => app.sign_in_to(code, username, common::ADMIN_PASSWORD).await,
        None => app.sign_in(username, common::ADMIN_PASSWORD).await,
    }
}

/// What a deprecation must leave alone: the revision's number, its document,
/// who published it and when, and how many states it projected.
type RevisionFacts = (
    i32,
    Value,
    Option<chrono::DateTime<chrono::Utc>>,
    Option<Uuid>,
    i64,
);

async fn revision_facts(app: &TestApp, id: Uuid) -> RevisionFacts {
    sqlx::query_as(
        "SELECT version, definition_json, published_at, published_by, \
                (SELECT count(*) FROM workflow_states s WHERE s.workflow_definition_id = d.id) \
         FROM workflow_definitions d WHERE id = $1",
    )
    .bind(id)
    .fetch_one(&app.pool)
    .await
    .expect("read the revision")
}

/// An audit row: action, object type, actor, old value, new value.
type AuditRow = (String, String, Option<Uuid>, Value, Value);

/// The `Workflow.Deprecated` audit rows written about `id`.
async fn deprecation_records(app: &TestApp, id: Uuid) -> Vec<AuditRow> {
    sqlx::query_as(
        "SELECT action, object_type, actor_user_id, old_value_json, new_value_json \
         FROM audit_events WHERE object_id = $1 AND event_type = 'Workflow.Deprecated' \
         ORDER BY created_at, id",
    )
    .bind(id)
    .fetch_all(&app.pool)
    .await
    .expect("read the audit rows")
}

// ---------------------------------------------------------------------------
// The document side: a form, a type, a draft, a submit
// ---------------------------------------------------------------------------

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

/// A document type with a numbering rule, bound to `workflow`.
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

async fn document_status(app: &TestApp, id: Uuid) -> String {
    sqlx::query_scalar("SELECT status FROM documents WHERE id = $1")
        .bind(id)
        .fetch_one(&app.pool)
        .await
        .expect("read the document's status")
}

/// A document's instance: its revision, its state and its status.
async fn instance_of(app: &TestApp, document: Uuid) -> Option<(Uuid, String, String)> {
    sqlx::query_as(
        "SELECT workflow_definition_id, current_state, status \
         FROM workflow_instances WHERE document_id = $1",
    )
    .bind(document)
    .fetch_optional(&app.pool)
    .await
    .expect("read the instance")
}

async fn open_task_of(app: &TestApp, document: Uuid) -> Uuid {
    sqlx::query_scalar(
        "SELECT id FROM workflow_tasks \
         WHERE document_id = $1 AND status IN ('CREATED','ASSIGNED','IN_PROGRESS')",
    )
    .bind(document)
    .fetch_one(&app.pool)
    .await
    .expect("read the open task")
}

async fn approver(app: &TestApp, username: &str) -> String {
    let role = approver_role(app, fixtures::SYSTEM_TENANT_ID).await;

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
// The route
// ---------------------------------------------------------------------------

/// **An `ACTIVE` revision becomes `DEPRECATED`, and nothing else about it
/// moves**: its number, its document, who published it and when, and the
/// projections the instances still running it read. A second workflow's
/// revision is untouched.
///
/// **Seen red** before the route existed (404 from the router), and against
/// `repo::deprecate` writing `published_at = now()` beside the status: the
/// publication stamp moved.
#[tokio::test]
async fn an_active_revision_is_deprecated_and_nothing_else_about_it_moves() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let id = published(&app, &token, "dep_happy").await;
    let other = published(&app, &token, "dep_bystander").await;

    let before = revision_facts(&app, id).await;

    let response = deprecate(&app, &token, id).await;

    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    assert_eq!(response.body["success"], true);
    assert_eq!(response.body["data"]["id"], id.to_string());
    assert_eq!(response.body["data"]["status"], "DEPRECATED");

    let after = revision_facts(&app, id).await;

    assert_eq!(status_of(&app, id).await, "DEPRECATED");
    assert_eq!(after, before, "deprecating changed more than the status");
    assert_eq!(
        status_of(&app, other).await,
        "ACTIVE",
        "another workflow moved"
    );

    // The list's status filter finds it, so the chooser can leave it out.
    let listed = app
        .get(&format!("{DEFINITIONS}?status=DEPRECATED"), Some(&token))
        .await;
    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);
    let ids: Vec<&str> = listed.body["data"]
        .as_array()
        .expect("a page")
        .iter()
        .map(|row| row["id"].as_str().expect("an id"))
        .collect();
    assert_eq!(ids, vec![id.to_string().as_str()]);
}

/// **A draft and an already deprecated revision are refused with 409**, and
/// neither moves.
///
/// **Seen red** before the route existed, and against the service's status
/// `match`, its row lock and `repo::deprecate`'s `AND status = 'ACTIVE'` all
/// defeated: the draft was deprecated without ever being published.
#[tokio::test]
async fn a_draft_or_an_already_deprecated_revision_is_refused_with_409() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    approver_role(&app, fixtures::SYSTEM_TENANT_ID).await;
    let draft_id = saved(&app, &token, "dep_draft").await;

    let refused = deprecate(&app, &token, draft_id).await;
    assert_eq!(refused.status, StatusCode::CONFLICT, "{}", refused.body);
    assert_eq!(
        refused.body["error"]["code"], "CONFLICT",
        "{}",
        refused.body
    );
    assert!(
        refused.body["error"]["message"]
            .as_str()
            .expect("a message")
            .contains("is a draft"),
        "{}",
        refused.body
    );
    assert_eq!(status_of(&app, draft_id).await, "DRAFT");

    // Published, it deprecates once.
    let published = publish(&app, &token, draft_id).await;
    assert_eq!(published.status, StatusCode::OK, "{}", published.body);
    let first = deprecate(&app, &token, draft_id).await;
    assert_eq!(first.status, StatusCode::OK, "{}", first.body);

    let again = deprecate(&app, &token, draft_id).await;
    assert_eq!(again.status, StatusCode::CONFLICT, "{}", again.body);
    assert!(
        again.body["error"]["message"]
            .as_str()
            .expect("a message")
            .contains("already deprecated"),
        "{}",
        again.body
    );
    assert_eq!(status_of(&app, draft_id).await, "DEPRECATED");

    // And it cannot be published back: deprecating is not reversible.
    let republished = publish(&app, &token, draft_id).await;
    assert_eq!(
        republished.status,
        StatusCode::CONFLICT,
        "{}",
        republished.body
    );
    assert_eq!(status_of(&app, draft_id).await, "DEPRECATED");
}

/// **The route takes `workflow:definition:deprecate` and nothing else does**
/// (D-108). A caller holding every other definition permission is refused, and
/// the revision stays `ACTIVE`; one holding this one and `read` deprecates it.
///
/// **Seen red** before the route existed, and against
/// `caller.require(DEFINITION_PUBLISH)` in its place, the reuse D-108 rejected:
/// the first caller deprecated the revision.
#[tokio::test]
async fn deprecating_needs_its_own_permission() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let id = published(&app, &token, "dep_permission").await;

    let everything_else = caller_holding(
        &app,
        fixtures::SYSTEM_TENANT_ID,
        None,
        "dep.everything.else",
        &[
            "workflow:definition:read",
            "workflow:definition:create",
            "workflow:definition:update",
            "workflow:definition:publish",
            "workflow:definition:delete",
        ],
    )
    .await;

    let refused = deprecate(&app, &everything_else, id).await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN, "{}", refused.body);
    assert_eq!(status_of(&app, id).await, "ACTIVE");

    // 403 before existence: a missing id tells this caller nothing either.
    let missing = deprecate(&app, &everything_else, Uuid::now_v7()).await;
    assert_eq!(missing.status, StatusCode::FORBIDDEN, "{}", missing.body);

    let deprecator = caller_holding(
        &app,
        fixtures::SYSTEM_TENANT_ID,
        None,
        "dep.deprecator",
        &["workflow:definition:read", "workflow:definition:deprecate"],
    )
    .await;

    let accepted = deprecate(&app, &deprecator, id).await;
    assert_eq!(accepted.status, StatusCode::OK, "{}", accepted.body);
    assert_eq!(status_of(&app, id).await, "DEPRECATED");

    // And without a token at all, 401.
    let anonymous = app
        .send(
            Method::POST,
            &format!("{DEFINITIONS}/{id}/deprecation"),
            None,
            None,
        )
        .await;
    assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);
}

/// **A revision in another tenant is not found**, and stays `ACTIVE`; nor is
/// an id nothing has. The outsider deprecates its own, so the 404 is about the
/// tenant rather than about the outsider reaching nothing.
///
/// **Seen red** before the route existed (the positive control), and against
/// the service reading the tenant to scope by from the row rather than the
/// caller: the outsider deprecated the first tenant's revision.
#[tokio::test]
async fn a_revision_in_another_tenant_is_not_found() {
    let app = TestApp::spawn_with(|config| config.multi_tenant = true).await;
    let token = app
        .sign_in_to("SYSTEM", common::ADMIN_USERNAME, common::ADMIN_PASSWORD)
        .await;

    let mine = published(&app, &token, "dep_mine").await;

    let other = fixtures::create_tenant(&app.pool, "OTHER-DEP", "Other tenant").await;
    let outsider = caller_holding(
        &app,
        other,
        Some("OTHER-DEP"),
        "other.deprecator",
        &[
            "workflow:definition:read",
            "workflow:definition:create",
            "workflow:definition:publish",
            "workflow:definition:deprecate",
        ],
    )
    .await;

    let refused = deprecate(&app, &outsider, mine).await;
    assert_eq!(
        refused.status,
        StatusCode::NOT_FOUND,
        "another tenant's revision was deprecated: {}",
        refused.body
    );
    assert_eq!(status_of(&app, mine).await, "ACTIVE");
    assert!(deprecation_records(&app, mine).await.is_empty());

    let nothing = deprecate(&app, &outsider, Uuid::now_v7()).await;
    assert_eq!(nothing.status, StatusCode::NOT_FOUND, "{}", nothing.body);

    // The outsider's own revision deprecates.
    approver_role(&app, other).await;
    let theirs = saved(&app, &outsider, "dep_theirs").await;
    let published = publish(&app, &outsider, theirs).await;
    assert_eq!(published.status, StatusCode::OK, "{}", published.body);
    let accepted = deprecate(&app, &outsider, theirs).await;
    assert_eq!(accepted.status, StatusCode::OK, "{}", accepted.body);
}

/// **A deprecation writes one audit row, and a refusal writes none**, in the
/// shape `Workflow.Published` takes: an `UPDATE` of a `WORKFLOW_DEFINITION`,
/// by the caller, from its old status to its new one.
///
/// **Seen red** before the route existed, and against the service's
/// `audit::record_or_warn` call never awaited: no row.
#[tokio::test]
async fn a_deprecation_writes_one_audit_row_and_a_refusal_writes_none() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let id = published(&app, &token, "dep_audited").await;

    let administrator: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE username = $1")
        .bind(common::ADMIN_USERNAME)
        .fetch_one(&app.pool)
        .await
        .expect("the administrator");

    assert_eq!(deprecate(&app, &token, id).await.status, StatusCode::OK);
    assert_eq!(
        deprecate(&app, &token, id).await.status,
        StatusCode::CONFLICT
    );

    let records = deprecation_records(&app, id).await;
    assert_eq!(records.len(), 1, "{records:?}");

    let (action, object_type, actor, old_value, new_value) = &records[0];
    assert_eq!(action, "UPDATE");
    assert_eq!(object_type, "WORKFLOW_DEFINITION");
    assert_eq!(*actor, Some(administrator));
    assert_eq!(old_value, &json!({ "status": "ACTIVE" }));
    assert_eq!(
        new_value,
        &json!({ "status": "DEPRECATED", "workflowKey": "dep_audited", "version": 1 })
    );
}

// ---------------------------------------------------------------------------
// What deprecating does to what already uses the revision
// ---------------------------------------------------------------------------

/// **An approval already running on the revision carries on**: its instance
/// keeps the revision, the state and the open task it had, and the task is
/// decided to the end. **The delete refused over that approval names this
/// route**, and following it works.
///
/// **Nothing on the decision path takes the revision's lock** ([#712] AC8). A
/// decision sent while a deprecate, and then a delete, holds the revision
/// `FOR UPDATE` uncommitted answers 200 without waiting for either: only the
/// submit and the three definition routes lock the row, and a running approval
/// reads its revision plain (`repo::definition_of_instance`).
///
/// **Seen red** before the route existed: the deprecate answered 404. The
/// decision path reads no revision status, so no mutation of this route's code
/// makes the rest red; it pins that deprecating stays a write to one column.
/// The held locks were seen red against `definition_of_instance` reading
/// `FOR SHARE`: the first decision waited on the deprecate.
///
/// [#712]: https://github.com/sujanto-gaws/kelir/issues/712
#[tokio::test]
async fn running_approvals_carry_on_after_their_revision_is_deprecated() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let approver = approver(&app, "dep.approver").await;
    let deadlocks = deadlocks_so_far(&app).await;

    let workflow = published(&app, &token, "dep_running").await;
    let type_id = document_type(&app, &token, "PR_DEP_RUNNING", workflow).await;
    let document = draft(&app, &token, type_id).await;
    let submitted = submit(&app, &token, document).await;
    assert_eq!(submitted.status, StatusCode::OK, "{}", submitted.body);

    let before = instance_of(&app, document).await.expect("an instance");
    let task = open_task_of(&app, document).await;

    // Two more approvals on the revision, each decided while a lock on it is
    // held: one under a deprecate, one under a delete. Both are rolled back.
    let mut held_tasks = Vec::new();
    for _ in 0..2 {
        let other = draft(&app, &token, type_id).await;
        let submitted = submit(&app, &token, other).await;
        assert_eq!(submitted.status, StatusCode::OK, "{}", submitted.body);
        held_tasks.push((other, open_task_of(&app, other).await));
    }
    let tenant = fixtures::SYSTEM_TENANT_ID;

    let mut deprecating = app.pool.begin().await.expect("a transaction");
    assert!(
        definition_repo::lock_for_publish(&mut deprecating, tenant, workflow)
            .await
            .expect("the deprecate's lock")
    );
    assert_eq!(
        definition_repo::deprecate(&mut *deprecating, tenant, workflow, None)
            .await
            .expect("the deprecate runs"),
        1
    );
    let (held, held_task) = held_tasks[0];
    let under_deprecate = tokio::time::timeout(
        Duration::from_secs(10),
        decide(&app, &approver, held_task, json!({ "action": "APPROVE" })),
    )
    .await;
    deprecating
        .rollback()
        .await
        .expect("the deprecate rolls back");
    let under_deprecate =
        under_deprecate.expect("a decision waited on a deprecate held uncommitted");
    assert_eq!(
        under_deprecate.status,
        StatusCode::OK,
        "{}",
        under_deprecate.body
    );
    assert_eq!(document_status(&app, held).await, "COMPLETED");

    let mut deleting = app.pool.begin().await.expect("a transaction");
    assert!(
        definition_repo::lock_for_publish(&mut deleting, tenant, workflow)
            .await
            .expect("the delete's lock")
    );
    assert_eq!(
        definition_repo::soft_delete(&mut *deleting, tenant, workflow, None)
            .await
            .expect("the delete runs"),
        1
    );
    let (held, held_task) = held_tasks[1];
    let under_delete = tokio::time::timeout(
        Duration::from_secs(10),
        decide(&app, &approver, held_task, json!({ "action": "APPROVE" })),
    )
    .await;
    deleting.rollback().await.expect("the delete rolls back");
    let under_delete = under_delete.expect("a decision waited on a delete held uncommitted");
    assert_eq!(under_delete.status, StatusCode::OK, "{}", under_delete.body);
    assert_eq!(document_status(&app, held).await, "COMPLETED");
    assert_eq!(status_of(&app, workflow).await, "ACTIVE");

    let delete_refused = app
        .delete(&format!("{DEFINITIONS}/{workflow}"), Some(&token))
        .await;
    assert_eq!(
        delete_refused.status,
        StatusCode::CONFLICT,
        "{}",
        delete_refused.body
    );
    assert!(
        delete_refused.body["error"]["message"]
            .as_str()
            .expect("a message")
            .contains(&format!("{DEFINITIONS}/{workflow}/deprecation")),
        "the delete refusal names no route that exists: {}",
        delete_refused.body
    );

    let deprecated = deprecate(&app, &token, workflow).await;
    assert_eq!(deprecated.status, StatusCode::OK, "{}", deprecated.body);

    assert_eq!(
        instance_of(&app, document).await.expect("the instance"),
        before,
        "deprecating moved a running instance"
    );
    assert_eq!(before.0, workflow, "the instance pins the revision");
    assert_eq!(open_task_of(&app, document).await, task);

    let decided = app
        .post(
            &format!("/api/v1/workflow/tasks/{task}/decision"),
            Some(&approver),
            json!({ "action": "APPROVE" }),
        )
        .await;
    assert_eq!(decided.status, StatusCode::OK, "{}", decided.body);
    assert_eq!(document_status(&app, document).await, "COMPLETED");

    assert_eq!(
        deadlocks_once_settled(&app).await,
        deadlocks,
        "PostgreSQL counted a deadlock"
    );
}

/// **A document type bound to a deprecated revision stays bound, and its next
/// submission is refused until it is rebound** — the bound-revision rule #573
/// asked to be stated and tested.
///
/// * The binding row is not touched: the type still names the revision.
/// * A new submission is refused as `WORKFLOW_NOT_PUBLISHED` (422) at
///   `documentTypeId`, by `engine::start`'s `ACTIVE` check, and writes
///   nothing: the document stays a draft with no instance.
/// * A binding naming the deprecated revision is refused as `NOT_PUBLISHED`,
///   on a new type or on this one. On this one it is sent at another priority:
///   since #716 the stored binding sent back unchanged is not re-checked
///   (`a_rename_that_sends_the_deprecated_binding_back_unchanged_still_cannot_submit`).
/// * Rebound to the next revision, published, the same draft submits and runs
///   on that revision.
///
/// **Seen red** before the route existed, and against `engine::start` without
/// its `status != "ACTIVE"` check: the submission started an approval on the
/// deprecated revision.
#[tokio::test]
async fn a_type_bound_to_a_deprecated_revision_stays_bound_and_refuses_submissions_until_rebound() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let first = published(&app, &token, "dep_bound").await;
    let type_id = document_type(&app, &token, "PR_DEP_BOUND", first).await;

    assert_eq!(deprecate(&app, &token, first).await.status, StatusCode::OK);

    // Still bound.
    let read = app
        .get(&format!("/api/v1/document-types/{type_id}"), Some(&token))
        .await;
    assert_eq!(read.status, StatusCode::OK, "{}", read.body);
    assert_eq!(
        read.body["data"]["workflows"][0]["workflowDefinitionId"],
        first.to_string(),
        "{}",
        read.body
    );

    // The next submission is refused, and writes nothing.
    let document = draft(&app, &token, type_id).await;
    let refused = submit(&app, &token, document).await;
    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a submission started an approval on a deprecated revision: {}",
        refused.body
    );
    assert_eq!(
        refused.body["error"]["details"][0]["code"], "WORKFLOW_NOT_PUBLISHED",
        "{}",
        refused.body
    );
    assert_eq!(
        refused.body["error"]["details"][0]["path"], "documentTypeId",
        "{}",
        refused.body
    );
    assert_eq!(document_status(&app, document).await, "DRAFT");
    assert!(instance_of(&app, document).await.is_none());

    // A binding naming it is refused, here and on a new type. Here it is an
    // edit of the stored binding, which is priority 1: sent back unchanged it
    // binds nothing new and is not re-checked (#716).
    let rebinding_to_it = app
        .put(
            &format!("/api/v1/document-types/{type_id}"),
            Some(&token),
            json!({ "workflows": [{ "workflowDefinitionId": first, "priority": 2 }] }),
        )
        .await;
    assert_eq!(
        rebinding_to_it.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        rebinding_to_it.body
    );
    assert_eq!(
        rebinding_to_it.body["error"]["details"][0]["code"], "NOT_PUBLISHED",
        "{}",
        rebinding_to_it.body
    );

    let new_type = app
        .post(
            "/api/v1/document-types",
            Some(&token),
            json!({
                "typeCode": "PR_DEP_NEW",
                "name": "PR_DEP_NEW",
                "workflows": [{ "workflowDefinitionId": first }],
            }),
        )
        .await;
    assert_eq!(
        new_type.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        new_type.body
    );
    assert_eq!(
        new_type.body["error"]["details"][0]["code"], "NOT_PUBLISHED",
        "{}",
        new_type.body
    );

    // Rebound to the next revision, the same draft submits and runs on it.
    let revision = app
        .post(
            &format!("{DEFINITIONS}/{first}/revisions"),
            Some(&token),
            json!({}),
        )
        .await;
    assert_eq!(revision.status, StatusCode::CREATED, "{}", revision.body);
    let second = id_of(&revision.body["data"]);
    assert_eq!(publish(&app, &token, second).await.status, StatusCode::OK);

    let rebound = app
        .put(
            &format!("/api/v1/document-types/{type_id}"),
            Some(&token),
            json!({ "workflows": [{ "workflowDefinitionId": second }] }),
        )
        .await;
    assert_eq!(rebound.status, StatusCode::OK, "{}", rebound.body);

    let submitted = submit(&app, &token, document).await;
    assert_eq!(submitted.status, StatusCode::OK, "{}", submitted.body);
    assert_eq!(
        instance_of(&app, document).await.expect("an instance").0,
        second
    );
}

/// **A type renamed with its deprecated binding sent back unchanged saves, and
/// still cannot submit** (#716 AC1).
///
/// The rename is a full-body PUT, as an API client sends one: the stored
/// binding read back and returned in all five fields. It is not re-checked, so
/// it saves and the type is still bound to the deprecated revision. What the
/// caller *sets* is still checked at `workflows.0.workflowDefinitionId`:
/// another deprecated revision (`NOT_PUBLISHED`), one that names nothing
/// (`NOT_FOUND`), and the same revision edited (`NOT_PUBLISHED`). And nothing
/// became routable: the renamed type's next submission is refused as
/// `WORKFLOW_NOT_PUBLISHED` at `documentTypeId`, writes no instance, and leaves
/// the draft a draft.
///
/// **Seen red** against #716's parent (5781320): the rename was refused
/// `NOT_PUBLISHED` at `workflows.0.workflowDefinitionId`.
#[tokio::test]
async fn a_rename_that_sends_the_deprecated_binding_back_unchanged_still_cannot_submit() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let first = published(&app, &token, "dep_unchanged").await;
    let other = published(&app, &token, "dep_unchanged_other").await;
    let type_id = document_type_bound(
        &app,
        &token,
        "PR_DEP_UNCHANGED",
        json!([{ "workflowDefinitionId": first, "priority": 3 }]),
    )
    .await;

    assert_eq!(deprecate(&app, &token, first).await.status, StatusCode::OK);
    assert_eq!(deprecate(&app, &token, other).await.status, StatusCode::OK);

    let read = app
        .get(&format!("/api/v1/document-types/{type_id}"), Some(&token))
        .await;
    assert_eq!(read.status, StatusCode::OK, "{}", read.body);
    let stored = read.body["data"]["workflows"].clone();
    assert_eq!(stored[0]["workflowDefinitionId"], first.to_string());

    let renamed = app
        .put(
            &format!("/api/v1/document-types/{type_id}"),
            Some(&token),
            json!({ "name": "Renamed, still deprecated", "workflows": stored }),
        )
        .await;
    assert_eq!(
        renamed.status,
        StatusCode::OK,
        "the stored binding was sent back unchanged: {}",
        renamed.body
    );
    assert_eq!(renamed.body["data"]["name"], "Renamed, still deprecated");
    assert_eq!(
        renamed.body["data"]["workflows"], stored,
        "{}",
        renamed.body
    );

    let mut edited = stored[0].clone();
    edited["priority"] = json!(4);

    for (label, binding, code) in [
        (
            "another deprecated revision",
            json!({ "workflowDefinitionId": other, "priority": 3 }),
            "NOT_PUBLISHED",
        ),
        (
            "a revision that names nothing",
            json!({ "workflowDefinitionId": Uuid::now_v7(), "priority": 3 }),
            "NOT_FOUND",
        ),
        ("the binding edited", edited, "NOT_PUBLISHED"),
    ] {
        let refused = app
            .put(
                &format!("/api/v1/document-types/{type_id}"),
                Some(&token),
                json!({ "name": "Refused", "workflows": [binding] }),
            )
            .await;
        assert_eq!(
            refused.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{label}: {}",
            refused.body
        );
        assert_eq!(
            refused.body["error"]["details"][0]["path"], "workflows.0.workflowDefinitionId",
            "{label}: {}",
            refused.body
        );
        assert_eq!(
            refused.body["error"]["details"][0]["code"], code,
            "{label}: {}",
            refused.body
        );
    }

    let read = app
        .get(&format!("/api/v1/document-types/{type_id}"), Some(&token))
        .await;
    assert_eq!(read.body["data"]["name"], "Renamed, still deprecated");
    assert_eq!(read.body["data"]["workflows"], stored);

    // Nothing became routable: the renamed type still refuses a submission.
    let document = draft(&app, &token, type_id).await;
    let refused = submit(&app, &token, document).await;
    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a submission started an approval on a deprecated revision: {}",
        refused.body
    );
    assert_eq!(
        refused.body["error"]["details"][0]["code"], "WORKFLOW_NOT_PUBLISHED",
        "{}",
        refused.body
    );
    assert_eq!(
        refused.body["error"]["details"][0]["path"], "documentTypeId",
        "{}",
        refused.body
    );
    assert_eq!(document_status(&app, document).await, "DRAFT");
    assert!(instance_of(&app, document).await.is_none());
}

/// **A role named only by a deprecated revision can be deleted** once no
/// approval runs on it (**D-91** (3)), which is what #573 was raised for: the
/// way through User Manual §11.2 no longer needs the old revision deleted.
///
/// **Seen red** before the route existed: the deprecate answered 404 and the
/// role delete stayed refused.
#[tokio::test]
async fn a_role_named_only_by_a_deprecated_revision_can_be_deleted() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let workflow = published(&app, &token, "dep_role").await;
    let role = approver_role(&app, fixtures::SYSTEM_TENANT_ID).await;

    let refused = app
        .delete(&format!("/api/v1/identity/roles/{role}"), Some(&token))
        .await;
    assert_eq!(refused.status, StatusCode::CONFLICT, "{}", refused.body);
    assert_eq!(
        refused.body["error"]["code"], "ROLE_NAMED_BY_PUBLISHED_DEFINITION",
        "{}",
        refused.body
    );

    assert_eq!(
        deprecate(&app, &token, workflow).await.status,
        StatusCode::OK
    );

    let deleted = app
        .delete(&format!("/api/v1/identity/roles/{role}"), Some(&token))
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);
}

// ---------------------------------------------------------------------------
// Races: a second deprecate, and a publish in flight
// ---------------------------------------------------------------------------

/// How many sessions in this test's database are waiting on a lock.
async fn lock_waiters(app: &TestApp) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM pg_stat_activity \
         WHERE datname = current_database() AND wait_event_type = 'Lock'",
    )
    .fetch_one(&app.pool)
    .await
    .expect("count the sessions waiting on a lock")
}

/// Waits until a session is waiting on a lock, and answers whether one was.
/// It gives up, answering `false`, when `running` finishes first, or after ten
/// seconds. Polled rather than slept (coding standard §2.5's technique).
async fn waited_on_a_lock<T>(app: &TestApp, running: &tokio::task::JoinHandle<T>) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);

    while std::time::Instant::now() < deadline {
        if lock_waiters(app).await >= 1 {
            return true;
        }

        if running.is_finished() {
            return false;
        }

        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }

    false
}

/// **Eight deprecations of one revision produce one deprecator** and one
/// audit row; the other seven are refused with 409.
///
/// **Seen red** before the route existed, and against the service without
/// `repo::lock_for_publish` and with `repo::deprecate`'s `AND status =
/// 'ACTIVE'` defeated: all eight callers were told they deprecated it.
#[tokio::test]
async fn eight_deprecations_of_one_revision_produce_one_deprecator() {
    let app = Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;

    let id = published(&app, &token, "dep_eight").await;

    let mut handles = Vec::new();

    for _ in 0..8 {
        let app = Arc::clone(&app);
        let token = token.clone();

        handles.push(tokio::spawn(
            async move { deprecate(&app, &token, id).await },
        ));
    }

    let mut deprecated = 0usize;
    let mut refused = 0usize;

    for handle in handles {
        let response = handle.await.expect("a deprecate finished");

        match response.status {
            StatusCode::OK => deprecated += 1,
            StatusCode::CONFLICT => refused += 1,
            other => panic!(
                "a deprecate answered {other}, which is neither deprecating nor losing: {}",
                response.body
            ),
        }
    }

    assert_eq!(
        deprecated, 1,
        "{deprecated} callers deprecated one revision"
    );
    assert_eq!(refused, 7);
    assert_eq!(deprecation_records(&app, id).await.len(), 1);
}

/// **A deprecate sent while a publish of the same draft is in flight waits for
/// it, and deprecates the revision it published.**
///
/// The publish is held uncommitted in a transaction that has flipped the row
/// to `ACTIVE`, and so holds its lock. The deprecate's `FOR UPDATE` waits on
/// it; when the publish commits, the deprecate reads `ACTIVE` and moves it.
/// Either order is consistent; this one is what the lock gives, and it means
/// the status the deprecate checks is the status it changes.
///
/// **Seen red** against the service reading the revision without
/// `repo::lock_for_publish`: the deprecate did not wait, read the committed
/// `DRAFT` and was refused.
#[tokio::test]
async fn a_deprecate_sent_while_a_publish_is_in_flight_waits_and_deprecates_it() {
    let app = Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;
    let tenant = fixtures::SYSTEM_TENANT_ID;

    approver_role(&app, tenant).await;
    let id = saved(&app, &token, "dep_mid_publish").await;

    let mut publishing = app.pool.begin().await.expect("a transaction");
    let flipped = definition_repo::publish(&mut *publishing, tenant, id, None)
        .await
        .expect("the publish runs");
    assert_eq!(flipped, 1);

    let deprecating = {
        let app = Arc::clone(&app);
        let token = token.clone();
        tokio::spawn(async move { deprecate(&app, &token, id).await })
    };

    let waited = waited_on_a_lock(&app, &deprecating).await;

    publishing.commit().await.expect("the publish commits");
    let response = deprecating.await.expect("the deprecate finished");

    assert!(
        waited,
        "the deprecate did not wait on the publish in flight"
    );
    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    assert_eq!(status_of(&app, id).await, "DEPRECATED");
}

// ===========================================================================
// The independent campaign (test-engineer, PR #711, 2026-10-10)
// ===========================================================================
//
// What the builder's tests above did not hold, found by reading the route
// against #573's criteria and plan 19 row 5: the routing of a type bound at
// two priorities, the columns a deprecation stamps, a revision soft-deleted
// under the route, the races with a delete and a submit, a key with several
// revisions, RESUBMIT on a deprecated revision, and who holds the permission
// on either side of `0051`. Each test names the mutations seen red against it;
// the campaign's table is in the PR.
//
// **Three mutations survived, all equivalent** (2026-10-10, built online
// against a throwaway database): `deleted_at IS NULL` dropped from
// `repo::lock_for_publish`, and `deleted_at IS NULL` or `status = 'ACTIVE'`
// dropped from `repo::deprecate`. The first is masked by `find_definition`
// re-reading the locked row with its own filter; the other two by that read
// and the service's status `match` running under the lock, which is what
// `repo::deprecate`'s documentation calls its second line. No test can tell
// them apart while the lock is taken.

/// A document type with a numbering rule and the given `workflows` bindings.
async fn document_type_bound(app: &TestApp, token: &str, code: &str, workflows: Value) -> Uuid {
    let form = published_form(app, token, &code.to_lowercase().replace('_', "-")).await;

    let created = app
        .post(
            "/api/v1/document-types",
            Some(token),
            json!({
                "typeCode": code,
                "name": code,
                "formId": form,
                "workflows": workflows,
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

/// The next revision of `from`, as a draft.
async fn revision_of(app: &TestApp, token: &str, from: Uuid) -> Uuid {
    let revision = app
        .post(
            &format!("{DEFINITIONS}/{from}/revisions"),
            Some(token),
            json!({}),
        )
        .await;
    assert_eq!(revision.status, StatusCode::CREATED, "{}", revision.body);

    id_of(&revision.body["data"])
}

async fn user_id(app: &TestApp, username: &str) -> Uuid {
    sqlx::query_scalar("SELECT id FROM users WHERE username = $1")
        .bind(username)
        .fetch_one(&app.pool)
        .await
        .expect("the user")
}

/// The revisions of `key` in the system tenant, by number: `(version, status)`.
async fn revisions_of_key(app: &TestApp, key: &str) -> Vec<(i32, String)> {
    sqlx::query_as(
        "SELECT version, status FROM workflow_definitions \
         WHERE workflow_key = $1 AND deleted_at IS NULL ORDER BY version",
    )
    .bind(key)
    .fetch_all(&app.pool)
    .await
    .expect("read the key's revisions")
}

/// The columns a deprecation may and may not stamp.
#[derive(Debug, PartialEq, sqlx::FromRow)]
struct Stamps {
    status: String,
    published_at: Option<chrono::DateTime<chrono::Utc>>,
    published_by: Option<Uuid>,
    updated_by: Option<Uuid>,
    updated_at: chrono::DateTime<chrono::Utc>,
    deleted_at: Option<chrono::DateTime<chrono::Utc>>,
}

async fn stamps(app: &TestApp, id: Uuid) -> Stamps {
    sqlx::query_as(
        "SELECT status, published_at, published_by, updated_by, updated_at, deleted_at \
         FROM workflow_definitions WHERE id = $1",
    )
    .bind(id)
    .fetch_one(&app.pool)
    .await
    .expect("read the revision's stamps")
}

/// A workflow whose approver can send the document back, and whose owner
/// resubmits it (`workflow_engine.rs`'s `returnable_workflow`, on this file's
/// role).
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

async fn decide(app: &TestApp, token: &str, task: Uuid, body: Value) -> common::TestResponse {
    app.post(
        &format!("/api/v1/workflow/tasks/{task}/decision"),
        Some(token),
        body,
    )
    .await
}

/// **A deprecated first-priority binding refuses the submit; it does not fall
/// through to the binding below it** (#573 criterion 4, the requirements
/// trace's gap on #711).
///
/// `document_type::repository::workflow_binding` takes `ORDER BY priority,
/// created_at LIMIT 1` without reading the revision's status, and
/// `engine::start` refuses what it took. So a type bound at priority 1 to a
/// deprecated revision and at priority 2 to an `ACTIVE` one refuses with 422
/// `WORKFLOW_NOT_PUBLISHED` at `documentTypeId`, writes no instance, and
/// leaves the draft a draft. The administrator rebinds; routing does not
/// quietly change under them.
///
/// The converse is held on a second type: deprecating the priority-2 revision
/// changes nothing for a submit, which still starts on priority 1.
///
/// **Seen red** against `workflow_binding` also requiring the bound revision
/// to be `ACTIVE` (the fall-through, built online): the first submit started
/// an approval on the priority-2 revision and the document went to
/// `PENDING_APPROVAL`.
#[tokio::test]
async fn a_deprecated_first_priority_binding_refuses_the_submit_without_falling_through() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let first = published(&app, &token, "dep_prio_first").await;
    let second = published(&app, &token, "dep_prio_second").await;

    let type_id = document_type_bound(
        &app,
        &token,
        "PR_DEP_PRIO",
        json!([
            { "workflowDefinitionId": first, "priority": 1 },
            { "workflowDefinitionId": second, "priority": 2 },
        ]),
    )
    .await;

    assert_eq!(deprecate(&app, &token, first).await.status, StatusCode::OK);

    let document = draft(&app, &token, type_id).await;
    let refused = submit(&app, &token, document).await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "the submit fell through to the priority-2 binding: {}",
        refused.body
    );
    assert_eq!(
        refused.body["error"]["details"][0]["code"], "WORKFLOW_NOT_PUBLISHED",
        "{}",
        refused.body
    );
    assert_eq!(
        refused.body["error"]["details"][0]["path"], "documentTypeId",
        "{}",
        refused.body
    );
    assert!(instance_of(&app, document).await.is_none());
    assert_eq!(document_status(&app, document).await, "DRAFT");

    let numbered: Option<String> =
        sqlx::query_scalar("SELECT document_number FROM documents WHERE id = $1")
            .bind(document)
            .fetch_one(&app.pool)
            .await
            .expect("read the number");
    assert_eq!(numbered, None, "a refused submit kept a number");

    // The converse: the lower priority deprecated, the higher one still routes.
    let upper = published(&app, &token, "dep_prio_upper").await;
    let lower = published(&app, &token, "dep_prio_lower").await;
    let other_type = document_type_bound(
        &app,
        &token,
        "PR_DEP_PRIO_LOW",
        json!([
            { "workflowDefinitionId": upper, "priority": 1 },
            { "workflowDefinitionId": lower, "priority": 2 },
        ]),
    )
    .await;

    assert_eq!(deprecate(&app, &token, lower).await.status, StatusCode::OK);

    let routed = draft(&app, &token, other_type).await;
    let submitted = submit(&app, &token, routed).await;
    assert_eq!(submitted.status, StatusCode::OK, "{}", submitted.body);
    assert_eq!(
        instance_of(&app, routed).await.expect("an instance").0,
        upper
    );
}

/// **The deprecator is stamped as the updater, and the publisher stays the
/// publisher.** The builder's facts test publishes and deprecates as one user,
/// so a deprecation that wrote `published_by` would not move it; here a second
/// user, holding only `read` and `deprecate`, deprecates.
///
/// Also held: `updated_at` moves, the envelope's `publishedBy` and
/// `publishedAt` are the publisher's, and the audit row's actor and tenant are
/// the deprecator's.
///
/// **Seen red** against `repo::deprecate` setting `published_by = $3`, and
/// against it without `updated_by = $3` (both built online against a
/// throwaway database, since the statement's text keys the offline entry).
#[tokio::test]
async fn the_deprecator_is_stamped_as_the_updater_and_the_publisher_is_kept() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let id = published(&app, &token, "dep_stamps").await;
    let administrator = user_id(&app, common::ADMIN_USERNAME).await;

    let deprecator_token = caller_holding(
        &app,
        fixtures::SYSTEM_TENANT_ID,
        None,
        "dep.stamper",
        &["workflow:definition:read", "workflow:definition:deprecate"],
    )
    .await;
    let deprecator = user_id(&app, "dep.stamper").await;

    let before = stamps(&app, id).await;
    assert_eq!(before.published_by, Some(administrator));

    let response = deprecate(&app, &deprecator_token, id).await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.body);

    let after = stamps(&app, id).await;

    assert_eq!(after.status, "DEPRECATED");
    assert_eq!(
        after.published_by,
        Some(administrator),
        "the deprecator was recorded as the publisher"
    );
    assert_eq!(after.published_at, before.published_at);
    assert_eq!(
        after.updated_by,
        Some(deprecator),
        "the updater is not the deprecator"
    );
    assert!(
        after.updated_at > before.updated_at,
        "updated_at did not move: {:?} then {:?}",
        before.updated_at,
        after.updated_at
    );
    assert_eq!(after.deleted_at, None);

    assert_eq!(
        response.body["data"]["publishedBy"],
        administrator.to_string(),
        "{}",
        response.body
    );

    let records = deprecation_records(&app, id).await;
    assert_eq!(records.len(), 1, "{records:?}");
    assert_eq!(records[0].2, Some(deprecator));

    let tenant: Uuid = sqlx::query_scalar(
        "SELECT tenant_id FROM audit_events \
         WHERE object_id = $1 AND event_type = 'Workflow.Deprecated'",
    )
    .bind(id)
    .fetch_one(&app.pool)
    .await
    .expect("the audit row's tenant");
    assert_eq!(tenant, fixtures::SYSTEM_TENANT_ID);
}

/// **A soft-deleted revision is not found**, whatever its status was: a
/// published revision with no running approval is deleted, and deprecating
/// it then answers 404, writes no audit row, and leaves the row as the delete
/// left it.
#[tokio::test]
async fn a_soft_deleted_revision_cannot_be_deprecated() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let id = published(&app, &token, "dep_deleted").await;

    let deleted = app
        .delete(&format!("{DEFINITIONS}/{id}"), Some(&token))
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);

    let before = stamps(&app, id).await;

    let refused = deprecate(&app, &token, id).await;
    assert_eq!(refused.status, StatusCode::NOT_FOUND, "{}", refused.body);

    assert_eq!(stamps(&app, id).await, before, "a deleted revision moved");
    assert_eq!(before.status, "ACTIVE");
    assert!(deprecation_records(&app, id).await.is_empty());
}

/// **A deprecate that waits on a delete in flight finds nothing**: 404, no
/// audit row, and the row stays `ACTIVE` as the delete left it.
///
/// The delete is held uncommitted (`repo::soft_delete` in an open
/// transaction). The deprecate's `FOR UPDATE` waits on it and, once it
/// commits, re-reads the row, which no longer matches `deleted_at IS NULL`.
///
/// **Seen red** against the service without `repo::lock_for_publish`: the
/// deprecate read the committed live row, `repo::deprecate` waited and then
/// matched nothing, and the caller was told 409 "deprecated by another
/// request" about a revision nobody deprecated.
#[tokio::test]
async fn a_deprecate_waiting_on_a_delete_in_flight_finds_nothing() {
    let app = Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;
    let tenant = fixtures::SYSTEM_TENANT_ID;

    let id = published(&app, &token, "dep_mid_delete").await;

    let mut deleting = app.pool.begin().await.expect("a transaction");
    let removed = definition_repo::soft_delete(&mut *deleting, tenant, id, None)
        .await
        .expect("the delete runs");
    assert_eq!(removed, 1);

    let deprecating = {
        let app = Arc::clone(&app);
        let token = token.clone();
        tokio::spawn(async move { deprecate(&app, &token, id).await })
    };

    let waited = waited_on_a_lock(&app, &deprecating).await;

    deleting.commit().await.expect("the delete commits");
    let response = deprecating.await.expect("the deprecate finished");

    assert!(waited, "the deprecate did not wait on the delete in flight");
    assert_eq!(
        response.status,
        StatusCode::NOT_FOUND,
        "a deprecate deprecated, or misreported, a deleted revision: {}",
        response.body
    );

    let after = stamps(&app, id).await;
    assert_eq!(after.status, "ACTIVE");
    assert!(after.deleted_at.is_some());
    assert!(deprecation_records(&app, id).await.is_empty());
}

/// **A delete sent while a deprecate is in flight waits for it, then
/// deletes**: the revision ends `DEPRECATED` and deleted, and each act wrote
/// its own audit row. Neither is lost and neither fails.
///
/// `delete_definition` takes the row `FOR UPDATE` before it reads it
/// (`repo::lock_for_publish`, [#712]), so it waits on the deprecate's lock and,
/// the row still live when the deprecate commits, reads it `DEPRECATED` and
/// deletes it. What its audit row then says is
/// `a_delete_that_waited_on_a_deprecate_audits_it_as_deprecated`'s.
///
/// [#712]: https://github.com/sujanto-gaws/kelir/issues/712
#[tokio::test]
async fn a_delete_sent_while_a_deprecate_is_in_flight_waits_then_deletes() {
    let app = Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;
    let tenant = fixtures::SYSTEM_TENANT_ID;

    let id = published(&app, &token, "dep_then_delete").await;

    let mut deprecating = app.pool.begin().await.expect("a transaction");
    assert!(
        definition_repo::lock_for_publish(&mut deprecating, tenant, id)
            .await
            .expect("the lock")
    );
    assert_eq!(
        definition_repo::deprecate(&mut *deprecating, tenant, id, None)
            .await
            .expect("the deprecate runs"),
        1
    );

    let deleting = {
        let app = Arc::clone(&app);
        let token = token.clone();
        tokio::spawn(async move {
            app.delete(&format!("{DEFINITIONS}/{id}"), Some(&token))
                .await
        })
    };

    let waited = waited_on_a_lock(&app, &deleting).await;

    deprecating.commit().await.expect("the deprecate commits");
    let response = deleting.await.expect("the delete finished");

    assert!(waited, "the delete did not wait on the deprecate in flight");
    assert_eq!(response.status, StatusCode::NO_CONTENT, "{}", response.body);

    let after = stamps(&app, id).await;
    assert_eq!(after.status, "DEPRECATED");
    assert!(after.deleted_at.is_some());

    let deleted_records: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events \
         WHERE object_id = $1 AND event_type = 'Workflow.Deleted'",
    )
    .bind(id)
    .fetch_one(&app.pool)
    .await
    .expect("count the delete's audit rows");
    assert_eq!(deleted_records, 1);
}

/// **A submit that waits on a deprecate is refused, and starts nothing**
/// ([#712] AC7). The deprecate's statements are held uncommitted; the submit
/// is seen waiting on them in `engine::start`'s `FOR SHARE` on the revision;
/// the deprecate commits, and the submit, reading the revision's status under
/// that lock, finds it `DEPRECATED` and answers 422 `WORKFLOW_NOT_PUBLISHED` at
/// `documentTypeId`. The document stays a draft with no instance and no
/// number. The other order is
/// `a_deprecate_waiting_on_a_submit_still_deprecates_and_its_approval_runs_to_the_end`.
///
/// **This test held the opposite until #712.** It was
/// `a_submit_racing_a_deprecate_starts_an_approval_that_runs_to_the_end`: the
/// start read the status with no lock, so a submit that read `ACTIVE` started
/// on a revision whose deprecate committed a moment later, and the campaign on
/// #711 pinned that as harmless. Record 22 §5.2 left it to row 5's criteria.
/// #712 decides it: no approval starts on a revision once its deprecate has
/// committed.
///
/// [#712]: https://github.com/sujanto-gaws/kelir/issues/712
#[tokio::test]
async fn a_submit_waiting_on_a_deprecate_is_refused_and_starts_nothing() {
    let app = Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;
    let tenant = fixtures::SYSTEM_TENANT_ID;
    let deadlocks = deadlocks_so_far(&app).await;

    let workflow = published(&app, &token, "dep_submit_race").await;
    let type_id = document_type(&app, &token, "PR_DEP_RACE", workflow).await;
    let document = draft(&app, &token, type_id).await;

    let mut deprecating = app.pool.begin().await.expect("a transaction");
    let deprecator: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *deprecating)
        .await
        .expect("the deprecate's backend");
    assert!(
        definition_repo::lock_for_publish(&mut deprecating, tenant, workflow)
            .await
            .expect("the lock")
    );
    assert_eq!(
        definition_repo::deprecate(&mut *deprecating, tenant, workflow, None)
            .await
            .expect("the deprecate runs"),
        1
    );

    let submitting = sent(&app, {
        let token = token.clone();
        move |app| async move { submit(&app, &token, document).await }
    });
    let (waited_in, submitting) = waits_on(&app, deprecator, submitting, "the submit").await;

    deprecating.commit().await.expect("the deprecate commits");
    let submitted = answer(submitting, "the submit").await;

    assert!(
        locks_the_revision(&waited_in, &["FOR", "SHARE"]),
        "the submit waited somewhere other than the start's lock: {waited_in}"
    );
    assert_eq!(
        submitted.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a submit started an approval on a revision whose deprecate committed first: {}",
        submitted.body
    );
    assert_eq!(
        submitted.body["error"]["details"][0]["code"], "WORKFLOW_NOT_PUBLISHED",
        "{}",
        submitted.body
    );
    assert_eq!(
        submitted.body["error"]["details"][0]["path"], "documentTypeId",
        "{}",
        submitted.body
    );
    assert_eq!(status_of(&app, workflow).await, "DEPRECATED");
    assert_eq!(document_status(&app, document).await, "DRAFT");
    assert!(instance_of(&app, document).await.is_none());
    assert_eq!(document_number(&app, document).await, None);

    assert_eq!(
        deadlocks_once_settled(&app).await,
        deadlocks,
        "PostgreSQL counted a deadlock"
    );
}

/// **One revision of several is deprecated, and only that one.** Publishing
/// revision 2 leaves revision 1 `ACTIVE` (User Manual §10.6, Database Schema
/// §7.1), so a key can hold two; deprecating revision 1 leaves revision 2 as
/// it was. Then **a new revision is created from the deprecated one**, which
/// `create_revision` allows: it takes number 3, opens as a draft carrying the
/// deprecated revision's document, and publishes. Revision 1 stays deprecated.
///
/// **Seen red** against `repo::deprecate` matching the key's `ACTIVE`
/// revisions rather than the one id (built online): revision 2 was deprecated
/// beside revision 1.
#[tokio::test]
async fn one_revision_of_several_is_deprecated_and_a_revision_can_be_made_from_it() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let first = published(&app, &token, "dep_several").await;
    let second = revision_of(&app, &token, first).await;
    assert_eq!(publish(&app, &token, second).await.status, StatusCode::OK);

    assert_eq!(
        revisions_of_key(&app, "dep_several").await,
        vec![(1, "ACTIVE".to_owned()), (2, "ACTIVE".to_owned())],
        "publishing revision 2 deprecated revision 1, which no document says it does"
    );

    let second_before = stamps(&app, second).await;

    assert_eq!(deprecate(&app, &token, first).await.status, StatusCode::OK);

    assert_eq!(
        revisions_of_key(&app, "dep_several").await,
        vec![(1, "DEPRECATED".to_owned()), (2, "ACTIVE".to_owned())]
    );
    assert_eq!(stamps(&app, second).await, second_before);

    // A revision made from the deprecated one.
    let revived = app
        .post(
            &format!("{DEFINITIONS}/{first}/revisions"),
            Some(&token),
            json!({}),
        )
        .await;
    assert_eq!(revived.status, StatusCode::CREATED, "{}", revived.body);
    assert_eq!(revived.body["data"]["version"], 3);
    assert_eq!(revived.body["data"]["status"], "DRAFT");
    let third = id_of(&revived.body["data"]);

    let copied: (Value, Value) = sqlx::query_as(
        "SELECT (SELECT definition_json FROM workflow_definitions WHERE id = $1), \
                (SELECT definition_json FROM workflow_definitions WHERE id = $2)",
    )
    .bind(first)
    .bind(third)
    .fetch_one(&app.pool)
    .await
    .expect("read both documents");
    assert_eq!(
        copied.0, copied.1,
        "the revision did not carry the deprecated one's document"
    );

    assert_eq!(publish(&app, &token, third).await.status, StatusCode::OK);
    assert_eq!(
        revisions_of_key(&app, "dep_several").await,
        vec![
            (1, "DEPRECATED".to_owned()),
            (2, "ACTIVE".to_owned()),
            (3, "ACTIVE".to_owned()),
        ]
    );
}

/// **Revision 1 deprecated while revision 2 is a draft; revision 2 then
/// published: the key ends with exactly one `ACTIVE` revision**, the new one.
/// Publishing does not revive the deprecated one, and deprecating did not
/// touch the draft.
#[tokio::test]
async fn deprecating_while_the_next_revision_is_a_draft_leaves_one_active_once_it_publishes() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let first = published(&app, &token, "dep_handover").await;
    let second = revision_of(&app, &token, first).await;

    let draft_before = stamps(&app, second).await;
    assert_eq!(deprecate(&app, &token, first).await.status, StatusCode::OK);
    assert_eq!(
        stamps(&app, second).await,
        draft_before,
        "deprecating moved the draft"
    );

    assert_eq!(publish(&app, &token, second).await.status, StatusCode::OK);

    assert_eq!(
        revisions_of_key(&app, "dep_handover").await,
        vec![(1, "DEPRECATED".to_owned()), (2, "ACTIVE".to_owned())]
    );
}

/// **RESUBMIT and decisions on a deprecated revision's running approval carry
/// on, and the revision keeps holding its role while they do** (D-91 (3)).
///
/// A document is submitted and returned, so its instance sits in `RETURNED`
/// with no open task. The revision is deprecated. The role it names cannot be
/// deleted, because a deprecated revision with a running instance still holds
/// it, and with no open task that is the refusal that answers. The owner
/// resubmits, which `resubmit_workflow` fires on the pinned revision without
/// asking its status, and the approver approves to the end.
///
/// **Seen red** against `definitions_naming_role` reduced to `d.status =
/// 'ACTIVE'` (built online): the role was deleted under the running approval.
/// And against `resubmit_workflow` refusing a revision that is not `ACTIVE`:
/// the owner's resubmission was refused.
#[tokio::test]
async fn resubmit_and_decisions_on_a_deprecated_revisions_running_approval_carry_on() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let approver = approver(&app, "dep.resubmit.approver").await;
    let role = approver_role(&app, fixtures::SYSTEM_TENANT_ID).await;

    let saved_return = app
        .post(
            DEFINITIONS,
            Some(&token),
            json!({ "workflowKey": "dep_return", "name": "Approval that can send back",
                    "definition": returnable_workflow("dep_return") }),
        )
        .await;
    assert_eq!(
        saved_return.status,
        StatusCode::CREATED,
        "{}",
        saved_return.body
    );
    let workflow = id_of(&saved_return.body["data"]);
    assert_eq!(publish(&app, &token, workflow).await.status, StatusCode::OK);

    let type_id = document_type(&app, &token, "PR_DEP_RETURN", workflow).await;
    let document = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, document).await.status, StatusCode::OK);

    let task = open_task_of(&app, document).await;
    let returned = decide(
        &app,
        &approver,
        task,
        json!({ "action": "RETURN", "comment": "The quotation is for 12 desks, not 2." }),
    )
    .await;
    assert_eq!(returned.status, StatusCode::OK, "{}", returned.body);
    assert_eq!(document_status(&app, document).await, "RETURNED");

    assert_eq!(
        deprecate(&app, &token, workflow).await.status,
        StatusCode::OK
    );

    // Held by the deprecated revision while its approval runs.
    let role_refused = app
        .delete(&format!("/api/v1/identity/roles/{role}"), Some(&token))
        .await;
    assert_eq!(
        role_refused.status,
        StatusCode::CONFLICT,
        "{}",
        role_refused.body
    );
    assert_eq!(
        role_refused.body["error"]["code"], "ROLE_NAMED_BY_PUBLISHED_DEFINITION",
        "{}",
        role_refused.body
    );

    // The owner resubmits on the deprecated revision.
    let resubmitted = submit(&app, &token, document).await;
    assert_eq!(resubmitted.status, StatusCode::OK, "{}", resubmitted.body);

    let (pinned, state, status) = instance_of(&app, document).await.expect("the instance");
    assert_eq!(
        pinned, workflow,
        "the resubmission moved to another revision"
    );
    assert_eq!(state, "MANAGER_APPROVAL");
    assert_eq!(status, "RUNNING");

    let task = open_task_of(&app, document).await;
    let approved = decide(&app, &approver, task, json!({ "action": "APPROVE" })).await;
    assert_eq!(approved.status, StatusCode::OK, "{}", approved.body);
    assert_eq!(document_status(&app, document).await, "COMPLETED");
    assert_eq!(status_of(&app, workflow).await, "DEPRECATED");
}

/// **A tenant provisioned after `0051` has an administrator who deprecates**,
/// because provisioning grants the catalogue minus the withheld families and
/// `workflow:definition:*` is not one. That administrator deprecates a
/// revision in their own tenant, and the audit row is filed under it.
///
/// **Across tenants, 403 comes before 404**: a user of that tenant without the
/// permission is refused 403 on the system tenant's revision, as on an id
/// nothing has, so a refusal says nothing about what exists elsewhere; the
/// administrator, holding it, is told 404.
///
/// **Seen red** against `WITHHELD_FROM_A_PROVISIONED_TENANT` gaining
/// `"workflow:definition:deprecate"`: the provisioned administrator did not
/// hold the code and was refused. Against the audit entry filed under the
/// system tenant rather than the caller's. And against the service checking
/// the permission after the lookup: the reader was told 404.
#[tokio::test]
async fn a_tenant_provisioned_after_0051_has_an_administrator_who_deprecates() {
    let app = TestApp::spawn_with(|config| config.multi_tenant = true).await;
    let system = app
        .sign_in_to("SYSTEM", common::ADMIN_USERNAME, common::ADMIN_PASSWORD)
        .await;

    let systems_revision = published(&app, &system, "dep_system_side").await;

    let created = app
        .post(
            "/api/v1/organization/tenants",
            Some(&system),
            json!({
                "tenantCode": "TNT-DEP",
                "name": "TNT-DEP Limited",
                "administrator": {
                    "username": "tntdep.admin",
                    "email": "tntdep.admin@example.test",
                    "displayName": "Tenant Administrator",
                    "password": "a-sufficiently-long-password",
                },
            }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let tenant: Uuid = created.body["data"]["id"]
        .as_str()
        .expect("an id")
        .parse()
        .expect("a uuid");

    let administrator = app
        .sign_in_to("TNT-DEP", "tntdep.admin", "a-sufficiently-long-password")
        .await;

    let profile = app.get("/api/v1/auth/me", Some(&administrator)).await;
    assert_eq!(profile.status, StatusCode::OK, "{}", profile.body);
    assert!(
        profile.body["data"]["permissions"]
            .as_array()
            .expect("permissions")
            .iter()
            .any(|code| code == "workflow:definition:deprecate"),
        "a provisioned tenant's administrator does not hold the code: {}",
        profile.body
    );

    approver_role(&app, tenant).await;
    let theirs = saved(&app, &administrator, "dep_provisioned").await;
    assert_eq!(
        publish(&app, &administrator, theirs).await.status,
        StatusCode::OK
    );

    let accepted = deprecate(&app, &administrator, theirs).await;
    assert_eq!(accepted.status, StatusCode::OK, "{}", accepted.body);
    assert_eq!(status_of(&app, theirs).await, "DEPRECATED");

    let filed_under: Uuid = sqlx::query_scalar(
        "SELECT tenant_id FROM audit_events \
         WHERE object_id = $1 AND event_type = 'Workflow.Deprecated'",
    )
    .bind(theirs)
    .fetch_one(&app.pool)
    .await
    .expect("the audit row");
    assert_eq!(filed_under, tenant);

    // 403 before 404, across tenants.
    let reader = caller_holding(
        &app,
        tenant,
        Some("TNT-DEP"),
        "tntdep.reader",
        &["workflow:definition:read"],
    )
    .await;
    let forbidden = deprecate(&app, &reader, systems_revision).await;
    assert_eq!(
        forbidden.status,
        StatusCode::FORBIDDEN,
        "{}",
        forbidden.body
    );

    let not_found = deprecate(&app, &administrator, systems_revision).await;
    assert_eq!(
        not_found.status,
        StatusCode::NOT_FOUND,
        "{}",
        not_found.body
    );
    assert_eq!(status_of(&app, systems_revision).await, "ACTIVE");
}

/// **`0051` grants the code to the system tenant's `ROLE-ADMIN` alone, so a
/// tenant provisioned before it runs does not hold it**, and rewords
/// `workflow:definition:delete`.
///
/// The upgrade is replayed: the catalogue row and its grant are removed, a
/// tenant is provisioned (by the API, so its administrator holds the catalogue
/// as it then stood), and `0051`'s own text is run again. The system
/// administrator then holds the code and the old tenant's does not, and is
/// refused the route, as the migration's header says. That is the precedent
/// `0049` and `0050` set, and `CHANGELOG.md`'s upgrade note tells operators.
///
/// **Seen red** against `0051` granting every tenant's `ROLE-ADMIN`, and
/// against it leaving the delete code's description as it was.
#[tokio::test]
async fn migration_0051_grants_the_system_administrator_and_not_an_older_tenants() {
    let app = TestApp::spawn_with(|config| config.multi_tenant = true).await;
    let system = app
        .sign_in_to("SYSTEM", common::ADMIN_USERNAME, common::ADMIN_PASSWORD)
        .await;

    // As migrated: one row, one grant, the delete reworded.
    let (id, module, holders): (Uuid, String, Vec<Uuid>) = sqlx::query_as(
        "SELECT p.id, p.module, \
                array(SELECT rp.role_id FROM role_permissions rp \
                      WHERE rp.permission_id = p.id AND rp.deleted_at IS NULL) \
         FROM permissions p WHERE p.permission_code = 'workflow:definition:deprecate'",
    )
    .fetch_one(&app.pool)
    .await
    .expect("the catalogue row");
    assert_eq!(id, uuid::uuid!("00000000-0000-0000-0001-000000000078"));
    assert_eq!(module, "workflow");
    assert_eq!(holders, vec![fixtures::ADMIN_ROLE_ID]);

    let delete_text: String = sqlx::query_scalar(
        "SELECT description FROM permissions WHERE permission_code = 'workflow:definition:delete'",
    )
    .fetch_one(&app.pool)
    .await
    .expect("the delete code's text");
    assert_eq!(
        delete_text,
        "Delete a workflow revision that no running approval uses"
    );

    // Back to before 0051, with a tenant provisioned then.
    sqlx::query("DELETE FROM role_permissions WHERE permission_id = $1")
        .bind(id)
        .execute(&app.pool)
        .await
        .expect("remove the grant");
    sqlx::query("DELETE FROM permissions WHERE id = $1")
        .bind(id)
        .execute(&app.pool)
        .await
        .expect("remove the catalogue row");

    let created = app
        .post(
            "/api/v1/organization/tenants",
            Some(&system),
            json!({
                "tenantCode": "TNT-OLD",
                "name": "TNT-OLD Limited",
                "administrator": {
                    "username": "tntold.admin",
                    "email": "tntold.admin@example.test",
                    "displayName": "Tenant Administrator",
                    "password": "a-sufficiently-long-password",
                },
            }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let old_tenant: Uuid = created.body["data"]["id"]
        .as_str()
        .expect("an id")
        .parse()
        .expect("a uuid");

    sqlx::raw_sql(include_str!(
        "../migrations/0051_workflow_definition_deprecate_permission.sql"
    ))
    .execute(&app.pool)
    .await
    .expect("0051 runs again");

    let holders: Vec<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT rp.tenant_id, rp.role_id FROM role_permissions rp \
         JOIN permissions p ON p.id = rp.permission_id \
         WHERE p.permission_code = 'workflow:definition:deprecate' AND rp.deleted_at IS NULL",
    )
    .fetch_all(&app.pool)
    .await
    .expect("the grants");
    assert_eq!(
        holders,
        vec![(fixtures::SYSTEM_TENANT_ID, fixtures::ADMIN_ROLE_ID)],
        "0051 granted beyond the system administrator"
    );

    let old_administrator = app
        .sign_in_to("TNT-OLD", "tntold.admin", "a-sufficiently-long-password")
        .await;
    approver_role(&app, old_tenant).await;
    let theirs = saved(&app, &old_administrator, "dep_old_tenant").await;
    assert_eq!(
        publish(&app, &old_administrator, theirs).await.status,
        StatusCode::OK
    );

    let refused = deprecate(&app, &old_administrator, theirs).await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN, "{}", refused.body);
    assert_eq!(status_of(&app, theirs).await, "ACTIVE");
}

// ===========================================================================
// A revision being deleted, raced (#712)
// ===========================================================================
//
// `delete_definition` holds the revision `FOR UPDATE` before it reads it, as
// the publish and the deprecate do, and runs its reads and its soft-delete in
// that transaction. `engine::start` holds the revision `FOR SHARE` before the
// instance `INSERT`, and refuses one that is gone. Between them a submit and a
// delete of one revision do not both succeed, whichever is first, and the
// delete's audit row records the status the revision had when it was deleted.
//
// The races are staged with `task_inbox.rs`'s gate (#663): a trigger that
// stops the first request at a chosen statement, holding every lock it took
// before it, while the second is seen waiting on it in `pg_stat_activity`.
//
// # Seen red on the unfixed code (`64920ce`), 2026-10-20
//
// | Test | How it went red |
// |---|---|
// | *a delete that waited on a deprecate…* | Recorded `ACTIVE` |
// | *a delete that waited on a publish…* | Recorded `DRAFT` |
// | *a submit waiting on a delete…* | The submit did not wait: 200. The delete's `UPDATE` takes `FOR NO KEY UPDATE`, which the instance key's `FOR KEY SHARE` does not conflict with |
// | *a delete waiting on a submit…* | The delete did not wait: 204, under the approval just started |
// | *a deprecate waiting on a submit…* | The deprecate did not wait: 200 |
// | *a submit of a type bound to a deleted revision…* | 200, an approval started on the deleted revision |
// | *twenty submits racing deletes…* | Round 0: 200 and 204 |
// | *a submit waiting on a deprecate…* | The submit waited in the instance `INSERT`, not the start's lock. Until #712 this test pinned the opposite outcome |
//
// # Seen to fail after the fix, 2026-10-20
//
// Each mutation applied alone, these tests run, and the mutation reverted.
//
// | Mutation | Red |
// |---|---|
// | `lock_for_publish` dropped from `delete_definition` | *…waited on a deprecate…* (`ACTIVE`); *…waited on a publish…* (`DRAFT`); *a delete waiting on a submit…* (waited in the `UPDATE`); *twenty…* (round 4, 200 and 204). *A submit waiting on a delete…* stays green: the start's share lock alone waits on the soft-delete's row lock |
// | `find_definition` moved before the lock, on the pool | *…waited on a deprecate…*; *…waited on a publish…* |
// | `has_live_instances` moved before the lock, on the pool | *a delete waiting on a submit…* (204); *twenty…* |
// | `soft_delete` on the pool | *…waited on a deprecate…* and *…on a publish…* never answered; *a submit waiting on a delete…* never reached the gate; *eight deletes…* answered 500 |
// | `lock_for_start` dropped from `engine::start` | *a submit waiting on a delete…* (waited in the instance `INSERT`); *a delete waiting on a submit…*; *a deprecate waiting on a submit…*; *…bound to a deleted revision…*; *twenty…* |
// | `definition_of_instance` reads `FOR SHARE` | *running approvals carry on…*: the decision waited on the held deprecate |
// | `engine::start` checks the status read before its lock (`loaded.status`) | *a submit waiting on a deprecate…*: 200, an approval started on the revision just deprecated |

/// The `old_value` of every `Workflow.Deleted` row written about `id`.
async fn deletion_records(app: &TestApp, id: Uuid) -> Vec<Value> {
    sqlx::query_scalar(
        "SELECT old_value_json FROM audit_events \
         WHERE object_id = $1 AND event_type = 'Workflow.Deleted' ORDER BY created_at, id",
    )
    .bind(id)
    .fetch_all(&app.pool)
    .await
    .expect("read the delete's audit rows")
}

async fn delete_revision(app: &TestApp, token: &str, id: Uuid) -> common::TestResponse {
    app.delete(&format!("{DEFINITIONS}/{id}"), Some(token))
        .await
}

/// The live instances standing on a deleted revision, which must be none: an
/// approval whose revision is gone cannot move.
async fn approvals_on_deleted_revisions(app: &TestApp) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM workflow_instances i \
         JOIN workflow_definitions d ON d.id = i.workflow_definition_id \
         WHERE i.status IN ('STARTED', 'RUNNING', 'SUSPENDED') AND i.deleted_at IS NULL \
           AND d.deleted_at IS NOT NULL",
    )
    .fetch_one(&app.pool)
    .await
    .expect("count the approvals on deleted revisions")
}

async fn document_number(app: &TestApp, id: Uuid) -> Option<String> {
    sqlx::query_scalar("SELECT document_number FROM documents WHERE id = $1")
        .bind(id)
        .fetch_one(&app.pool)
        .await
        .expect("read the document's number")
}

/// How long a raced request is given to reach a gate, to be seen waiting, or
/// to answer once the gate opens.
const PATIENCE: Duration = Duration::from_secs(30);

/// `running`'s answer, failing rather than hanging when it never comes.
async fn answer(running: JoinHandle<common::TestResponse>, what: &str) -> common::TestResponse {
    tokio::time::timeout(PATIENCE, running)
        .await
        .unwrap_or_else(|_| panic!("{what} never answered"))
        .unwrap_or_else(|error| panic!("{what} panicked: {error}"))
}

/// A request sent in the background, as the raced half of a test.
fn sent<F>(
    app: &Arc<TestApp>,
    send: impl FnOnce(Arc<TestApp>) -> F,
) -> JoinHandle<common::TestResponse>
where
    F: std::future::Future<Output = common::TestResponse> + Send + 'static,
{
    tokio::spawn(send(Arc::clone(app)))
}

/// The advisory key the gate waits on; the issue's number.
const GATE: i64 = 712;

/// The advisory key [`Gate::close_reads`] waits on; its issue's number.
const READ_GATE: i64 = 785;

/// A `BEFORE <event>` trigger on a table that takes a shared advisory lock
/// this test holds exclusively on a connection of its own, so the first
/// request to write that table stops there holding every lock it took before
/// it. `task_inbox.rs`'s technique ([#663]). Closed after the staging, so only
/// the raced requests meet it.
///
/// [#663]: https://github.com/sujanto-gaws/kelir/issues/663
struct Gate {
    connection: PgConnection,
    /// The advisory key the gate's function waits on.
    key: i64,
}

impl Gate {
    async fn close(app: &TestApp, event: &str, table: &str) -> Self {
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
            "CREATE TRIGGER test_gate BEFORE {event} ON {table}
             FOR EACH ROW EXECUTE FUNCTION test_gate()"
        ))
        .execute(&app.pool)
        .await
        .expect("the gate's trigger");

        let mut connection = PgConnection::connect_with(&app.pool.connect_options())
            .await
            .expect("the gate's connection");
        sqlx::query("SELECT pg_advisory_lock($1)")
            .bind(GATE)
            .execute(&mut connection)
            .await
            .expect("close the gate");

        Gate {
            connection,
            key: GATE,
        }
    }

    /// A gate on the plain reads of the `workflow_definitions` rows that
    /// `predicate` matches ([#785]). `predicate` is SQL over `row_status` and
    /// `row_name`.
    ///
    /// **It stops the read a request runs on the pool after its commit**, and
    /// nothing else: a statement with no transaction id that takes no row
    /// lock (`FOR UPDATE`, `FOR NO KEY UPDATE`, `FOR SHARE`, `FOR KEY SHARE`). A statement in a transaction that has written or locked a
    /// row has an id, and a row lock is taken before its recheck reads the
    /// new row, so neither stops.
    ///
    /// **A stopped read then sees what a read run at that moment would.** It
    /// took its snapshot before it was stopped, so the row it returns is the
    /// row as it was then. Once the gate opens, the function asks again, with
    /// the fresh snapshot each statement in it takes, whether the row has been
    /// deleted since, and drops it if so.
    ///
    /// A trigger cannot stop a `SELECT`, and row-level security does not apply
    /// to a superuser, which CI's role is. So the table is renamed and a view
    /// of its name put in its place, whose `WHERE` calls the gate's function.
    /// The service's statements name `workflow_definitions`, so they read,
    /// lock and write through the view; the foreign keys and any trigger stay
    /// on the table. Close a [`Gate::close`] first, since its trigger goes on
    /// the table by name.
    ///
    /// [#785]: https://github.com/sujanto-gaws/kelir/issues/785
    async fn close_reads(app: &TestApp, predicate: &str) -> Self {
        for statement in [
            format!(
                "CREATE FUNCTION test_read_gate(row_id uuid, row_status text, row_name text)
                 RETURNS boolean LANGUAGE plpgsql AS $$
                 BEGIN
                     IF ({predicate})
                        AND txid_current_if_assigned() IS NULL
                        AND current_query()
                            !~* '\\mFOR\\s+(NO\\s+KEY\\s+UPDATE|UPDATE|KEY\\s+SHARE|SHARE)\\M' THEN
                         PERFORM pg_advisory_xact_lock_shared({READ_GATE});
                         RETURN NOT EXISTS (
                             SELECT 1 FROM workflow_definitions_stored
                             WHERE id = row_id AND deleted_at IS NOT NULL
                         );
                     END IF;
                     RETURN true;
                 END
                 $$"
            ),
            "ALTER TABLE workflow_definitions RENAME TO workflow_definitions_stored".to_owned(),
            "CREATE VIEW workflow_definitions AS
             SELECT * FROM workflow_definitions_stored WHERE test_read_gate(id, status, name)"
                .to_owned(),
        ] {
            sqlx::query(&statement)
                .execute(&app.pool)
                .await
                .unwrap_or_else(|error| {
                    panic!(
                        "the read gate: {error}
{statement}"
                    )
                });
        }

        let mut connection = PgConnection::connect_with(&app.pool.connect_options())
            .await
            .expect("the read gate's connection");
        sqlx::query("SELECT pg_advisory_lock($1)")
            .bind(READ_GATE)
            .execute(&mut connection)
            .await
            .expect("close the read gate");

        Gate {
            connection,
            key: READ_GATE,
        }
    }

    /// The backend of the request `running`, once it is stopped at the gate.
    async fn parked(
        &self,
        app: &TestApp,
        running: &JoinHandle<common::TestResponse>,
        what: &str,
    ) -> i32 {
        let deadline = tokio::time::Instant::now() + PATIENCE;
        loop {
            let waiting: Option<i32> = sqlx::query_scalar(
                "SELECT pid FROM pg_locks
                 WHERE locktype = 'advisory' AND objid::text::bigint = $1 AND objsubid = 1
                   AND NOT granted
                   AND database = (SELECT oid FROM pg_database WHERE datname = current_database())",
            )
            .bind(self.key)
            .fetch_optional(&app.pool)
            .await
            .expect("read pg_locks");
            if let Some(pid) = waiting {
                return pid;
            }
            assert!(
                !running.is_finished(),
                "{what} finished without reaching the gate"
            );
            assert!(
                tokio::time::Instant::now() < deadline,
                "{what} never reached the gate"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// Whether a request is stopped at the gate now.
    async fn holds_a_waiter(&self, app: &TestApp) -> bool {
        sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM pg_locks
              WHERE locktype = 'advisory' AND objid::text::bigint = $1 AND objsubid = 1
                AND NOT granted
                AND database = (SELECT oid FROM pg_database WHERE datname = current_database()))",
        )
        .bind(self.key)
        .fetch_one(&app.pool)
        .await
        .expect("read pg_locks")
    }

    async fn open(mut self) {
        sqlx::query("SELECT pg_advisory_unlock($1)")
            .bind(self.key)
            .execute(&mut self.connection)
            .await
            .expect("open the gate");
        self.connection
            .close()
            .await
            .expect("close the gate's connection");
    }
}

/// The statement `running` is seen waiting in, blocked by the backend
/// `blocker`. A request that answers first is a failure, and says what it
/// answered.
async fn waits_on(
    app: &TestApp,
    blocker: i32,
    running: JoinHandle<common::TestResponse>,
    what: &str,
) -> (String, JoinHandle<common::TestResponse>) {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let statement: Option<String> = sqlx::query_scalar(
            "SELECT query FROM pg_stat_activity
             WHERE datname = current_database() AND $1 = ANY(pg_blocking_pids(pid))",
        )
        .bind(blocker)
        .fetch_optional(&app.pool)
        .await
        .expect("read pg_stat_activity");
        if let Some(statement) = statement {
            return (statement, running);
        }
        if running.is_finished() {
            let answered = answer(running, what).await;
            panic!(
                "{what} did not wait: it answered {} {}",
                answered.status, answered.body
            );
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "{what} was never seen waiting"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Whether `statement` locks one `workflow_definitions` row in `mode`: a
/// `SELECT` whose one `FROM` is that table alone, ending in the lock clause.
/// Read word by word, so a statement that only joins the table, or names it in
/// a subquery, is not mistaken for it (`task_inbox.rs`'s `locks_the_instance`).
fn locks_the_revision(statement: &str, mode: &[&str]) -> bool {
    let words: Vec<&str> = statement.split_whitespace().collect();
    let froms = words.iter().filter(|word| **word == "FROM").count();

    words.first() == Some(&"SELECT")
        && words.ends_with(mode)
        && froms == 1
        && words
            .windows(3)
            .any(|three| three == ["FROM", "workflow_definitions", "WHERE"])
}

#[test]
fn locks_the_revision_knows_the_two_locks_from_their_neighbours() {
    let lock_for_publish = "
        SELECT 1 AS \"found!\" FROM workflow_definitions
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
        FOR UPDATE
        ";
    let lock_for_start = "
        SELECT status FROM workflow_definitions
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
        FOR SHARE
        ";
    assert!(locks_the_revision(lock_for_publish, &["FOR", "UPDATE"]));
    assert!(locks_the_revision(lock_for_start, &["FOR", "SHARE"]));
    assert!(!locks_the_revision(lock_for_publish, &["FOR", "SHARE"]));
    assert!(!locks_the_revision(lock_for_start, &["FOR", "UPDATE"]));

    for (statement, why) in [
        (
            "UPDATE workflow_definitions SET deleted_at = now() WHERE tenant_id = $1 AND id = $2",
            "the soft-delete's own write",
        ),
        (
            "INSERT INTO workflow_instances (id, workflow_definition_id) VALUES ($1, $2)",
            "the instance's foreign key",
        ),
        (
            "SELECT t.id FROM document_types t JOIN workflow_definitions d ON d.id = $1
             WHERE t.id = $2 FOR SHARE",
            "a join locking another row with it",
        ),
    ] {
        assert!(
            !locks_the_revision(statement, &["FOR", "SHARE"]),
            "{why}: {statement}"
        );
        assert!(
            !locks_the_revision(statement, &["FOR", "UPDATE"]),
            "{why}: {statement}"
        );
    }
}

/// **A delete that waited on a deprecate records the revision as
/// `DEPRECATED`** ([#712] AC2), the status it had when it was deleted, in its
/// one audit row.
///
/// Staged as `a_delete_sent_while_a_deprecate_is_in_flight_waits_then_deletes`
/// is: the deprecate's statements held uncommitted, the delete sent and seen
/// waiting, the deprecate committed.
///
/// [#712]: https://github.com/sujanto-gaws/kelir/issues/712
#[tokio::test]
async fn a_delete_that_waited_on_a_deprecate_audits_it_as_deprecated() {
    let app = Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;
    let tenant = fixtures::SYSTEM_TENANT_ID;
    let deadlocks = deadlocks_so_far(&app).await;

    let id = published(&app, &token, "del_after_deprecate").await;

    let mut deprecating = app.pool.begin().await.expect("a transaction");
    assert!(
        definition_repo::lock_for_publish(&mut deprecating, tenant, id)
            .await
            .expect("the lock")
    );
    assert_eq!(
        definition_repo::deprecate(&mut *deprecating, tenant, id, None)
            .await
            .expect("the deprecate runs"),
        1
    );

    let deleting = sent(&app, {
        let token = token.clone();
        move |app| async move { delete_revision(&app, &token, id).await }
    });

    let waited = waited_on_a_lock(&app, &deleting).await;

    deprecating.commit().await.expect("the deprecate commits");
    let deleted = answer(deleting, "the delete").await;

    assert!(waited, "the delete did not wait on the deprecate in flight");
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);

    let records = deletion_records(&app, id).await;
    assert_eq!(records.len(), 1, "{records:?}");
    assert_eq!(
        records[0]["status"], "DEPRECATED",
        "the delete recorded a status the revision no longer had: {}",
        records[0]
    );

    assert_eq!(
        deadlocks_once_settled(&app).await,
        deadlocks,
        "PostgreSQL counted a deadlock"
    );
}

/// **A delete that waited on a publish records the revision as `ACTIVE`**
/// ([#712] AC3). The draft is held mid-publish, flipped to `ACTIVE` and
/// uncommitted; the delete waits for it, finds no running approval, deletes
/// the revision and records it as the published revision it deleted.
///
/// [#712]: https://github.com/sujanto-gaws/kelir/issues/712
#[tokio::test]
async fn a_delete_that_waited_on_a_publish_audits_it_as_active() {
    let app = Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;
    let tenant = fixtures::SYSTEM_TENANT_ID;
    let deadlocks = deadlocks_so_far(&app).await;

    approver_role(&app, tenant).await;
    let id = saved(&app, &token, "del_after_publish").await;

    let mut publishing = app.pool.begin().await.expect("a transaction");
    assert!(
        definition_repo::lock_for_publish(&mut publishing, tenant, id)
            .await
            .expect("the lock")
    );
    assert_eq!(
        definition_repo::publish(&mut *publishing, tenant, id, None)
            .await
            .expect("the publish runs"),
        1
    );

    let deleting = sent(&app, {
        let token = token.clone();
        move |app| async move { delete_revision(&app, &token, id).await }
    });

    let waited = waited_on_a_lock(&app, &deleting).await;

    publishing.commit().await.expect("the publish commits");
    let deleted = answer(deleting, "the delete").await;

    assert!(waited, "the delete did not wait on the publish in flight");
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);
    assert!(stamps(&app, id).await.deleted_at.is_some());

    let records = deletion_records(&app, id).await;
    assert_eq!(records.len(), 1, "{records:?}");
    assert_eq!(
        records[0]["status"], "ACTIVE",
        "the delete recorded a status the revision no longer had: {}",
        records[0]
    );

    assert_eq!(
        deadlocks_once_settled(&app).await,
        deadlocks,
        "PostgreSQL counted a deadlock"
    );
}

/// **A submit that waits on a delete is refused, and starts nothing**
/// ([#712] AC4, AC6).
///
/// The delete is stopped at its soft-delete `UPDATE` by a gate, holding the
/// revision `FOR UPDATE`. The submit is sent and seen waiting in
/// `engine::start`'s `FOR SHARE` on the revision. When the gate opens, the
/// delete answers 204, and the submit, re-reading a row that is gone, answers
/// 422 `WORKFLOW_NOT_PUBLISHED` at `documentTypeId`. The document stays a
/// draft with no instance and no number.
///
/// [#712]: https://github.com/sujanto-gaws/kelir/issues/712
#[tokio::test]
async fn a_submit_waiting_on_a_delete_is_refused_and_starts_nothing() {
    let app = Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;
    let deadlocks = deadlocks_so_far(&app).await;

    let workflow = published(&app, &token, "del_then_submit").await;
    let type_id = document_type(&app, &token, "PR_DEL_THEN_SUBMIT", workflow).await;
    let document = draft(&app, &token, type_id).await;

    let gate = Gate::close(&app, "UPDATE", "workflow_definitions").await;

    let deleting = sent(&app, {
        let token = token.clone();
        move |app| async move { delete_revision(&app, &token, workflow).await }
    });
    let deleter = gate.parked(&app, &deleting, "the delete").await;

    let submitting = sent(&app, {
        let token = token.clone();
        move |app| async move { submit(&app, &token, document).await }
    });
    let (waited_in, submitting) = waits_on(&app, deleter, submitting, "the submit").await;

    gate.open().await;
    let deleted = answer(deleting, "the delete").await;
    let submitted = answer(submitting, "the submit").await;

    assert!(
        locks_the_revision(&waited_in, &["FOR", "SHARE"]),
        "the submit waited somewhere other than the start's lock: {waited_in}"
    );
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);
    assert_eq!(
        submitted.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a submit started an approval on a revision being deleted: {}",
        submitted.body
    );
    assert_eq!(
        submitted.body["error"]["details"][0]["code"], "WORKFLOW_NOT_PUBLISHED",
        "{}",
        submitted.body
    );
    assert_eq!(
        submitted.body["error"]["details"][0]["path"], "documentTypeId",
        "{}",
        submitted.body
    );

    assert_eq!(document_status(&app, document).await, "DRAFT");
    assert!(instance_of(&app, document).await.is_none());
    assert_eq!(document_number(&app, document).await, None);
    assert_eq!(deletion_records(&app, workflow).await.len(), 1);
    assert_eq!(approvals_on_deleted_revisions(&app).await, 0);

    assert_eq!(
        deadlocks_once_settled(&app).await,
        deadlocks,
        "PostgreSQL counted a deadlock"
    );
}

/// **A delete that waits on a submit is refused for the approval it started**
/// ([#712] AC5, AC6).
///
/// The submit is stopped at its instance `INSERT` by a gate, after
/// `engine::start` has taken the revision `FOR SHARE` and before the
/// instance's foreign key is checked. The delete is sent and seen waiting in
/// `repo::lock_for_publish`'s `FOR UPDATE`. When the gate opens, the submit
/// answers 200, and the delete, finding the approval it started, answers 409.
/// The revision is not deleted and no `Workflow.Deleted` row is written.
///
/// [#712]: https://github.com/sujanto-gaws/kelir/issues/712
#[tokio::test]
async fn a_delete_waiting_on_a_submit_is_refused_for_its_approval() {
    let app = Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;
    let deadlocks = deadlocks_so_far(&app).await;

    let workflow = published(&app, &token, "submit_then_del").await;
    let type_id = document_type(&app, &token, "PR_SUBMIT_THEN_DEL", workflow).await;
    let document = draft(&app, &token, type_id).await;

    let gate = Gate::close(&app, "INSERT", "workflow_instances").await;

    let submitting = sent(&app, {
        let token = token.clone();
        move |app| async move { submit(&app, &token, document).await }
    });
    let submitter = gate.parked(&app, &submitting, "the submit").await;

    let deleting = sent(&app, {
        let token = token.clone();
        move |app| async move { delete_revision(&app, &token, workflow).await }
    });
    let (waited_in, deleting) = waits_on(&app, submitter, deleting, "the delete").await;

    gate.open().await;
    let submitted = answer(submitting, "the submit").await;
    let deleted = answer(deleting, "the delete").await;

    assert!(
        locks_the_revision(&waited_in, &["FOR", "UPDATE"]),
        "the delete waited somewhere other than its lock: {waited_in}"
    );
    assert_eq!(submitted.status, StatusCode::OK, "{}", submitted.body);
    assert_eq!(
        deleted.status,
        StatusCode::CONFLICT,
        "a delete removed the revision an approval had just started on: {}",
        deleted.body
    );
    assert!(
        deleted.body["error"]["message"]
            .as_str()
            .expect("a message")
            .contains("has running approvals"),
        "{}",
        deleted.body
    );

    let after = stamps(&app, workflow).await;
    assert!(after.deleted_at.is_none(), "the revision was deleted");
    assert_eq!(after.status, "ACTIVE");
    assert!(deletion_records(&app, workflow).await.is_empty());

    let (pinned, _, status) = instance_of(&app, document).await.expect("an instance");
    assert_eq!(pinned, workflow);
    assert_eq!(status, "RUNNING");
    assert_eq!(approvals_on_deleted_revisions(&app).await, 0);

    assert_eq!(
        deadlocks_once_settled(&app).await,
        deadlocks,
        "PostgreSQL counted a deadlock"
    );
}

/// **A deprecate that waits on a submit still deprecates, and that approval
/// runs to the end** ([#712] AC7). The other order is
/// `a_submit_waiting_on_a_deprecate_is_refused_and_starts_nothing`.
///
/// The submit is stopped at its instance `INSERT`, holding the revision
/// `FOR SHARE`. The deprecate is seen waiting in its `FOR UPDATE`. When the
/// gate opens, the submit answers 200 and the deprecate 200, and the approval
/// started on the revision is decided to the end. The type's next submission
/// is refused.
///
/// [#712]: https://github.com/sujanto-gaws/kelir/issues/712
#[tokio::test]
async fn a_deprecate_waiting_on_a_submit_still_deprecates_and_its_approval_runs_to_the_end() {
    let app = Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;
    let approver = approver(&app, "dep.wait.approver").await;
    let deadlocks = deadlocks_so_far(&app).await;

    let workflow = published(&app, &token, "submit_then_dep").await;
    let type_id = document_type(&app, &token, "PR_SUBMIT_THEN_DEP", workflow).await;
    let document = draft(&app, &token, type_id).await;

    let gate = Gate::close(&app, "INSERT", "workflow_instances").await;

    let submitting = sent(&app, {
        let token = token.clone();
        move |app| async move { submit(&app, &token, document).await }
    });
    let submitter = gate.parked(&app, &submitting, "the submit").await;

    let deprecating = sent(&app, {
        let token = token.clone();
        move |app| async move { deprecate(&app, &token, workflow).await }
    });
    let (waited_in, deprecating) = waits_on(&app, submitter, deprecating, "the deprecate").await;

    gate.open().await;
    let submitted = answer(submitting, "the submit").await;
    let deprecated = answer(deprecating, "the deprecate").await;

    assert!(
        locks_the_revision(&waited_in, &["FOR", "UPDATE"]),
        "the deprecate waited somewhere other than its lock: {waited_in}"
    );
    assert_eq!(submitted.status, StatusCode::OK, "{}", submitted.body);
    assert_eq!(deprecated.status, StatusCode::OK, "{}", deprecated.body);
    assert_eq!(status_of(&app, workflow).await, "DEPRECATED");

    let (pinned, state, status) = instance_of(&app, document).await.expect("an instance");
    assert_eq!(pinned, workflow);
    assert_eq!(state, "MANAGER_APPROVAL");
    assert_eq!(status, "RUNNING");

    let task = open_task_of(&app, document).await;
    let decided = decide(&app, &approver, task, json!({ "action": "APPROVE" })).await;
    assert_eq!(decided.status, StatusCode::OK, "{}", decided.body);
    assert_eq!(document_status(&app, document).await, "COMPLETED");

    let next = draft(&app, &token, type_id).await;
    let refused = submit(&app, &token, next).await;
    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );

    assert_eq!(
        deadlocks_once_settled(&app).await,
        deadlocks,
        "PostgreSQL counted a deadlock"
    );
}

/// **A type bound to a deleted revision refuses its submission** ([#712]
/// AC6). Nothing refuses the delete of a bound revision with no running
/// approval, and the binding is not checked again at submit, so the refusal is
/// `engine::start`'s: 422 `WORKFLOW_NOT_PUBLISHED` at `documentTypeId`, the
/// draft left a draft with no instance and no number.
///
/// [#712]: https://github.com/sujanto-gaws/kelir/issues/712
#[tokio::test]
async fn a_submit_of_a_type_bound_to_a_deleted_revision_is_refused() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let workflow = published(&app, &token, "del_bound").await;
    let type_id = document_type(&app, &token, "PR_DEL_BOUND", workflow).await;

    let deleted = delete_revision(&app, &token, workflow).await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);

    let document = draft(&app, &token, type_id).await;
    let refused = submit(&app, &token, document).await;
    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a submission started an approval on a deleted revision: {}",
        refused.body
    );
    assert_eq!(
        refused.body["error"]["details"][0]["code"], "WORKFLOW_NOT_PUBLISHED",
        "{}",
        refused.body
    );
    assert_eq!(
        refused.body["error"]["details"][0]["path"], "documentTypeId",
        "{}",
        refused.body
    );
    assert_eq!(document_status(&app, document).await, "DRAFT");
    assert!(instance_of(&app, document).await.is_none());
    assert_eq!(document_number(&app, document).await, None);
    assert_eq!(approvals_on_deleted_revisions(&app).await, 0);
}

/// **Twenty submits, each sent with a delete of its revision, never both
/// succeed** ([#712] AC6), unstaged: each pair is sent at once, in either
/// order, and whichever takes the revision's lock first wins. A submit that
/// wins answers 200 and its delete 409; a delete that wins answers 204 and its
/// submit 422. No request answers 500, no approval is left on a deleted
/// revision, and PostgreSQL counts no deadlock.
///
/// It supplements the staged tests above and does not replace them: an
/// unstaged race may land every pair the same way.
///
/// [#712]: https://github.com/sujanto-gaws/kelir/issues/712
#[tokio::test]
async fn twenty_submits_racing_deletes_of_their_revision_never_both_succeed() {
    let app = Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;
    let deadlocks = deadlocks_so_far(&app).await;

    let mut submitted_first = 0usize;
    let mut deleted_first = 0usize;

    for round in 0..20u64 {
        let workflow = published(&app, &token, &format!("del_race_{round:02}")).await;
        let type_id =
            document_type(&app, &token, &format!("PR_DEL_RACE_{round:02}"), workflow).await;
        let document = draft(&app, &token, type_id).await;

        // The first sent alternates, and the second follows after a lag that
        // varies, so the pairs do not all land one way. A submit does more
        // before it reaches the revision than a delete does, so a delete sent
        // with it all but always wins; on the rounds the submit goes first,
        // the delete's lag is long enough to let it lose.
        let submit_first = round % 2 == 0;
        let lag = if submit_first {
            Duration::from_millis(10 * (round % 8))
        } else {
            Duration::from_millis(round % 4)
        };
        let submitting = sent(&app, {
            let token = token.clone();
            move |app| async move {
                if !submit_first {
                    tokio::time::sleep(lag).await;
                }
                submit(&app, &token, document).await
            }
        });
        let deleting = sent(&app, {
            let token = token.clone();
            move |app| async move {
                if submit_first {
                    tokio::time::sleep(lag).await;
                }
                delete_revision(&app, &token, workflow).await
            }
        });

        let submitted = answer(submitting, "a submit").await;
        let deleted = answer(deleting, "a delete").await;

        match (submitted.status, deleted.status) {
            (StatusCode::OK, StatusCode::CONFLICT) => submitted_first += 1,
            (StatusCode::UNPROCESSABLE_ENTITY, StatusCode::NO_CONTENT) => deleted_first += 1,
            (submit_status, delete_status) => panic!(
                "round {round}: the submit answered {submit_status} {} and the delete \
                 {delete_status} {}",
                submitted.body, deleted.body
            ),
        }
    }

    eprintln!("submits first: {submitted_first}; deletes first: {deleted_first}");
    assert_eq!(submitted_first + deleted_first, 20);
    assert_eq!(approvals_on_deleted_revisions(&app).await, 0);

    assert_eq!(
        deadlocks_once_settled(&app).await,
        deadlocks,
        "PostgreSQL counted a deadlock"
    );
}

/// **The delete's refusals are unchanged by its lock** ([#712] AC9). Without
/// the permission the answer is 403, for a revision that exists and for an id
/// nothing has, so the lock is never reached. An id nothing has, and a
/// revision already deleted, are 404 and write no audit row. Another tenant's
/// revision is `workflow_definitions.rs`'s
/// `a_workflow_in_another_tenant_cannot_be_retired`, and the 409 naming
/// `/deprecation` is `running_approvals_carry_on_after_their_revision_is_deprecated`'s.
///
/// [#712]: https://github.com/sujanto-gaws/kelir/issues/712
#[tokio::test]
async fn a_delete_without_the_permission_or_of_nothing_is_refused_before_it_locks() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let id = published(&app, &token, "del_refusals").await;

    let everything_else = caller_holding(
        &app,
        fixtures::SYSTEM_TENANT_ID,
        None,
        "del.everything.else",
        &[
            "workflow:definition:read",
            "workflow:definition:create",
            "workflow:definition:update",
            "workflow:definition:publish",
            "workflow:definition:deprecate",
        ],
    )
    .await;

    let refused = delete_revision(&app, &everything_else, id).await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN, "{}", refused.body);
    let missing = delete_revision(&app, &everything_else, Uuid::now_v7()).await;
    assert_eq!(missing.status, StatusCode::FORBIDDEN, "{}", missing.body);
    assert!(stamps(&app, id).await.deleted_at.is_none());

    let unknown = delete_revision(&app, &token, Uuid::now_v7()).await;
    assert_eq!(unknown.status, StatusCode::NOT_FOUND, "{}", unknown.body);

    let deleted = delete_revision(&app, &token, id).await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);
    let again = delete_revision(&app, &token, id).await;
    assert_eq!(again.status, StatusCode::NOT_FOUND, "{}", again.body);

    assert_eq!(deletion_records(&app, id).await.len(), 1);
}

/// **Eight deletes of one revision produce one deleter** ([#712] AC9): one
/// 204, seven 404, and one audit row.
///
/// [#712]: https://github.com/sujanto-gaws/kelir/issues/712
#[tokio::test]
async fn eight_deletes_of_one_revision_produce_one_deleter() {
    let app = Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;
    let deadlocks = deadlocks_so_far(&app).await;

    let id = published(&app, &token, "del_eight").await;

    let deletes: Vec<_> = (0..8)
        .map(|_| {
            sent(&app, {
                let token = token.clone();
                move |app| async move { delete_revision(&app, &token, id).await }
            })
        })
        .collect();

    let mut deleted = 0usize;
    let mut missing = 0usize;
    for running in deletes {
        let response = answer(running, "a delete").await;
        match response.status {
            StatusCode::NO_CONTENT => deleted += 1,
            StatusCode::NOT_FOUND => missing += 1,
            other => panic!(
                "a delete answered {other}, which is neither deleting nor finding nothing: {}",
                response.body
            ),
        }
    }

    assert_eq!(deleted, 1, "{deleted} callers deleted one revision");
    assert_eq!(missing, 7);
    assert_eq!(deletion_records(&app, id).await.len(), 1);

    assert_eq!(
        deadlocks_once_settled(&app).await,
        deadlocks,
        "PostgreSQL counted a deadlock"
    );
}

// ===========================================================================
// #712's closing campaign: the paths the races above do not stage
// ===========================================================================
//
// Added by the independent campaign on #712. Each test holds with and without
// the AC7 commit (`engine::start` deciding the status under its lock), so that
// commit stays droppable.
//
// # Seen to fail, 2026-10-20
//
// Each mutation applied alone, the whole file run, and the mutation reverted.
// Only this section's tests are named.
//
// | Mutation | Red |
// |---|---|
// | `delete_definition` without `lock_for_publish` | *a delete waiting on a deprecate…*: the delete waited in its `UPDATE`, and the deprecate answered 500. *A deprecate waiting on a delete…* stays green: the gate's `BEFORE UPDATE` trigger fires once the soft-delete has locked the row, so the deprecate waits on that lock instead |
// | `delete_definition` reads the row before its lock | *a delete waiting on a deprecate…*: recorded `ACTIVE` |
// | `soft_delete` on the pool | *a deprecate waiting on a delete…* and *a delete waiting on a deprecate…* |
// | `lock_for_start` dropped from `engine::start` | *a resubmit under a held delete…*: the type's first submit on the deleted revision answered 200 |
// | `lock_for_start` holds every revision of the key | *a submit and a publish of the next revision…*: the publish of revision 2 waited |
// | `resubmit_workflow` takes `lock_for_start` before it fires | *a resubmit under a held delete…*: the resubmit waited on the delete |
// | `definition_of_instance` reads `FOR SHARE` | *a resubmit under a held delete…*: the resubmit waited on the delete |
// | `definition_of_instance` filters `deleted_at IS NULL` | *a resubmit under a held delete…*: the resubmit on the revision deleted under it answered 500 |
// | `definition_of_instance` drops its tenant check | *a type bound across tenants…*: 422, a start read another tenant's revision |
//
// **One survivor.** *The deadlock guard…* stays green with `deadlocks_so_far`
// in place of the settled read: here the aborted transaction's statistics are
// flushed before the read. The settled read is for the slower flush #637 saw.

/// **A delete parked at its write makes a deprecate wait, and the deprecate
/// then finds nothing** ([#712] AC2, AC9). Both are the real routes:
/// `a_deprecate_waiting_on_a_delete_in_flight_finds_nothing` holds the
/// delete's `UPDATE` alone, which is not the delete the route now runs.
///
/// The delete is stopped at its soft-delete `UPDATE` by a gate, holding the
/// revision `FOR UPDATE`. The deprecate is seen waiting in its own
/// `FOR UPDATE`. When the gate opens, the delete answers 204 and records the
/// revision as `ACTIVE`, and the deprecate answers 404 and records nothing.
///
/// [#712]: https://github.com/sujanto-gaws/kelir/issues/712
#[tokio::test]
async fn a_deprecate_waiting_on_a_delete_finds_nothing_and_audits_nothing() {
    let app = Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;
    let deadlocks = deadlocks_so_far(&app).await;

    let id = published(&app, &token, "del_then_dep").await;

    let gate = Gate::close(&app, "UPDATE", "workflow_definitions").await;

    let deleting = sent(&app, {
        let token = token.clone();
        move |app| async move { delete_revision(&app, &token, id).await }
    });
    let deleter = gate.parked(&app, &deleting, "the delete").await;

    let deprecating = sent(&app, {
        let token = token.clone();
        move |app| async move { deprecate(&app, &token, id).await }
    });
    let (waited_in, deprecating) = waits_on(&app, deleter, deprecating, "the deprecate").await;

    gate.open().await;
    let deleted = answer(deleting, "the delete").await;
    let deprecated = answer(deprecating, "the deprecate").await;

    assert!(
        locks_the_revision(&waited_in, &["FOR", "UPDATE"]),
        "the deprecate waited somewhere other than its lock: {waited_in}"
    );
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);
    assert_eq!(
        deprecated.status,
        StatusCode::NOT_FOUND,
        "a deprecate deprecated, or misreported, a revision deleted first: {}",
        deprecated.body
    );

    let after = stamps(&app, id).await;
    assert_eq!(after.status, "ACTIVE");
    assert!(after.deleted_at.is_some());

    let deletions = deletion_records(&app, id).await;
    assert_eq!(deletions.len(), 1, "{deletions:?}");
    assert_eq!(deletions[0]["status"], "ACTIVE", "{}", deletions[0]);
    assert!(deprecation_records(&app, id).await.is_empty());

    assert_eq!(
        deadlocks_once_settled(&app).await,
        deadlocks,
        "PostgreSQL counted a deadlock"
    );
}

/// **A deprecate parked at its write makes a delete wait, and each then
/// writes its one audit row** ([#712] AC2). Both are the real routes:
/// `a_delete_that_waited_on_a_deprecate_audits_it_as_deprecated` holds the
/// deprecate's statements, so it never sees the deprecate's own audit row.
///
/// The deprecate is stopped at its `UPDATE` by a gate, holding the revision
/// `FOR UPDATE`. The delete is seen waiting in its own `FOR UPDATE`. When the
/// gate opens, the deprecate answers 200 and records `ACTIVE` to
/// `DEPRECATED`, and the delete answers 204 and records `DEPRECATED`.
///
/// [#712]: https://github.com/sujanto-gaws/kelir/issues/712
#[tokio::test]
async fn a_delete_waiting_on_a_deprecate_deletes_it_and_each_audits_once() {
    let app = Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;
    let deadlocks = deadlocks_so_far(&app).await;

    let id = published(&app, &token, "dep_then_del").await;

    let gate = Gate::close(&app, "UPDATE", "workflow_definitions").await;

    let deprecating = sent(&app, {
        let token = token.clone();
        move |app| async move { deprecate(&app, &token, id).await }
    });
    let deprecator = gate.parked(&app, &deprecating, "the deprecate").await;

    let deleting = sent(&app, {
        let token = token.clone();
        move |app| async move { delete_revision(&app, &token, id).await }
    });
    let (waited_in, deleting) = waits_on(&app, deprecator, deleting, "the delete").await;

    gate.open().await;
    let deprecated = answer(deprecating, "the deprecate").await;
    let deleted = answer(deleting, "the delete").await;

    assert!(
        locks_the_revision(&waited_in, &["FOR", "UPDATE"]),
        "the delete waited somewhere other than its lock: {waited_in}"
    );
    assert_eq!(deprecated.status, StatusCode::OK, "{}", deprecated.body);
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);

    let after = stamps(&app, id).await;
    assert_eq!(after.status, "DEPRECATED");
    assert!(after.deleted_at.is_some());

    let deprecations = deprecation_records(&app, id).await;
    assert_eq!(deprecations.len(), 1, "{deprecations:?}");
    assert_eq!(deprecations[0].3["status"], "ACTIVE");
    assert_eq!(deprecations[0].4["status"], "DEPRECATED");

    let deletions = deletion_records(&app, id).await;
    assert_eq!(deletions.len(), 1, "{deletions:?}");
    assert_eq!(
        deletions[0]["status"], "DEPRECATED",
        "the delete recorded a status the revision no longer had: {}",
        deletions[0]
    );

    assert_eq!(
        deadlocks_once_settled(&app).await,
        deadlocks,
        "PostgreSQL counted a deadlock"
    );
}

/// **A submit on one revision and a publish of the next do not wait on each
/// other** ([#712] AC8, record 22 §5.2 rule 3). The start's share lock is on
/// the revision the type is bound to, by id, and the publish locks its own
/// revision and then the roles it names `FOR KEY SHARE`, which no submit
/// takes in a mode that conflicts.
///
/// First, a submit is stopped at its instance `INSERT`, holding revision 1
/// `FOR SHARE`, and revision 2 is published through the route. Then the
/// publish of revision 3 is held uncommitted, its revision `FOR UPDATE` and its
/// roles `FOR KEY SHARE`, and a second submit is sent. Each answers 200 within
/// ten seconds, and both approvals start on revision 1.
///
/// [#712]: https://github.com/sujanto-gaws/kelir/issues/712
#[tokio::test]
async fn a_submit_and_a_publish_of_the_next_revision_do_not_wait_on_each_other() {
    let app = Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;
    let tenant = fixtures::SYSTEM_TENANT_ID;
    let deadlocks = deadlocks_so_far(&app).await;

    let first = published(&app, &token, "pub_beside_submit").await;
    let type_id = document_type(&app, &token, "PR_PUB_BESIDE", first).await;
    let early = draft(&app, &token, type_id).await;
    let late = draft(&app, &token, type_id).await;
    let second = revision_of(&app, &token, first).await;

    // A submit holding revision 1, and revision 2 published.
    let gate = Gate::close(&app, "INSERT", "workflow_instances").await;
    let submitting = sent(&app, {
        let token = token.clone();
        move |app| async move { submit(&app, &token, early).await }
    });
    gate.parked(&app, &submitting, "the submit").await;

    let publication = tokio::time::timeout(Duration::from_secs(10), publish(&app, &token, second))
        .await
        .expect("a publish of revision 2 waited on a submit holding revision 1");
    assert_eq!(publication.status, StatusCode::OK, "{}", publication.body);

    gate.open().await;
    let submitted = answer(submitting, "the submit").await;
    assert_eq!(submitted.status, StatusCode::OK, "{}", submitted.body);

    // Revision 3's publish held, and a submit sent.
    let third = revision_of(&app, &token, first).await;
    let mut publishing = app.pool.begin().await.expect("a transaction");
    assert!(
        definition_repo::lock_for_publish(&mut publishing, tenant, third)
            .await
            .expect("the publish's lock")
    );
    assert_eq!(
        definition_repo::lock_live_roles(&mut publishing, tenant, &[APPROVER_ROLE.to_owned()])
            .await
            .expect("the publish's roles"),
        vec![APPROVER_ROLE.to_owned()]
    );
    assert_eq!(
        definition_repo::publish(&mut *publishing, tenant, third, None)
            .await
            .expect("the publish runs"),
        1
    );
    let under_publish =
        tokio::time::timeout(Duration::from_secs(10), submit(&app, &token, late)).await;
    publishing.commit().await.expect("the publish commits");
    let under_publish =
        under_publish.expect("a submit on revision 1 waited on a publish of revision 3");
    assert_eq!(
        under_publish.status,
        StatusCode::OK,
        "{}",
        under_publish.body
    );

    for document in [early, late] {
        let (pinned, _, status) = instance_of(&app, document).await.expect("an instance");
        assert_eq!(pinned, first, "an approval started on another revision");
        assert_eq!(status, "RUNNING");
    }
    for revision in [first, second, third] {
        assert_eq!(status_of(&app, revision).await, "ACTIVE");
    }

    assert_eq!(
        deadlocks_once_settled(&app).await,
        deadlocks,
        "PostgreSQL counted a deadlock"
    );
}

/// **A resubmit carries on while a delete holds its revision, and on a
/// revision deleted under its running approval** ([#712] AC8). A resubmit
/// fires the instance it has; it does not start one, so it takes no lock on
/// the revision and does not ask whether the revision is live.
///
/// The second half is the shape the unfixed delete could leave behind: an
/// approval running on a revision whose `deleted_at` is set, written here by
/// SQL because the route now refuses it. That approval still moves to the
/// end, and only a first submit of the type is refused.
///
/// [#712]: https://github.com/sujanto-gaws/kelir/issues/712
#[tokio::test]
async fn a_resubmit_under_a_held_delete_or_on_a_deleted_revision_carries_on() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let tenant = fixtures::SYSTEM_TENANT_ID;
    let approver = approver(&app, "del.resubmit.approver").await;
    let deadlocks = deadlocks_so_far(&app).await;

    let saved_return = app
        .post(
            DEFINITIONS,
            Some(&token),
            json!({ "workflowKey": "del_return", "name": "Approval that can send back",
                    "definition": returnable_workflow("del_return") }),
        )
        .await;
    assert_eq!(
        saved_return.status,
        StatusCode::CREATED,
        "{}",
        saved_return.body
    );
    let workflow = id_of(&saved_return.body["data"]);
    assert_eq!(publish(&app, &token, workflow).await.status, StatusCode::OK);

    let type_id = document_type(&app, &token, "PR_DEL_RETURN", workflow).await;
    let document = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, document).await.status, StatusCode::OK);

    // A resubmit while a delete holds the revision.
    send_back(
        &app,
        &approver,
        document,
        "The quotation is for 12 desks, not 2.",
    )
    .await;
    let mut deleting = app.pool.begin().await.expect("a transaction");
    assert!(
        definition_repo::lock_for_publish(&mut deleting, tenant, workflow)
            .await
            .expect("the delete's lock")
    );
    let under_delete =
        tokio::time::timeout(Duration::from_secs(10), submit(&app, &token, document)).await;
    deleting.rollback().await.expect("the delete rolls back");
    let under_delete = under_delete.expect("a resubmit waited on a delete held uncommitted");
    assert_eq!(under_delete.status, StatusCode::OK, "{}", under_delete.body);

    let refused = delete_revision(&app, &token, workflow).await;
    assert_eq!(refused.status, StatusCode::CONFLICT, "{}", refused.body);

    // A resubmit on a revision deleted under its approval.
    send_back(&app, &approver, document, "Attach the second quotation.").await;
    sqlx::query("UPDATE workflow_definitions SET deleted_at = now() WHERE id = $1")
        .bind(workflow)
        .execute(&app.pool)
        .await
        .expect("delete the revision under its approval");

    let resubmitted = submit(&app, &token, document).await;
    assert_eq!(
        resubmitted.status,
        StatusCode::OK,
        "a running approval stopped because its revision was deleted: {}",
        resubmitted.body
    );
    let (pinned, state, status) = instance_of(&app, document).await.expect("the instance");
    assert_eq!(pinned, workflow);
    assert_eq!(state, "MANAGER_APPROVAL");
    assert_eq!(status, "RUNNING");

    let task = open_task_of(&app, document).await;
    let approved = decide(&app, &approver, task, json!({ "action": "APPROVE" })).await;
    assert_eq!(approved.status, StatusCode::OK, "{}", approved.body);
    assert_eq!(document_status(&app, document).await, "COMPLETED");

    // The type's next document is a first submit, and is refused.
    let next = draft(&app, &token, type_id).await;
    let first_submit = submit(&app, &token, next).await;
    assert_eq!(
        first_submit.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        first_submit.body
    );
    assert_eq!(
        first_submit.body["error"]["details"][0]["code"], "WORKFLOW_NOT_PUBLISHED",
        "{}",
        first_submit.body
    );
    assert!(instance_of(&app, next).await.is_none());

    assert_eq!(
        deadlocks_once_settled(&app).await,
        deadlocks,
        "PostgreSQL counted a deadlock"
    );
}

/// The approver returns `document` with `reason`, and it is `RETURNED`.
async fn send_back(app: &TestApp, approver: &str, document: Uuid, reason: &str) {
    let task = open_task_of(app, document).await;
    let returned = decide(
        app,
        approver,
        task,
        json!({ "action": "RETURN", "comment": reason }),
    )
    .await;
    assert_eq!(returned.status, StatusCode::OK, "{}", returned.body);
    assert_eq!(document_status(app, document).await, "RETURNED");
}

/// **A type bound to another tenant's revision fails its submit with 500 and
/// starts nothing** ([#712], the builder's first untested path). Pinned, not
/// endorsed.
///
/// No route writes such a binding: the binding is checked in the caller's
/// tenant, and its foreign key names the revision by id alone, so only SQL
/// can. `engine::start`'s lock is scoped to the caller's tenant, so it finds
/// no row and holds nothing in the other tenant, whether it runs before
/// `repo::definition_of_instance` or after it. That read then logs both ends
/// of the broken link and answers `INTERNAL_ERROR`, as it did before #712: the
/// submit is rolled back whole, the document stays a draft with no number, and
/// the other tenant's revision is untouched. Whether the answer should be a
/// 422 is not #712's to decide.
///
/// [#712]: https://github.com/sujanto-gaws/kelir/issues/712
#[tokio::test]
async fn a_type_bound_across_tenants_fails_its_submit_and_starts_nothing() {
    let app = TestApp::spawn_with(|config| config.multi_tenant = true).await;
    let token = app
        .sign_in_to("SYSTEM", common::ADMIN_USERNAME, common::ADMIN_PASSWORD)
        .await;

    let mine = published(&app, &token, "del_mine").await;
    let type_id = document_type(&app, &token, "PR_DEL_ACROSS", mine).await;
    let document = draft(&app, &token, type_id).await;

    let other = fixtures::create_tenant(&app.pool, "OTHER-START", "Other tenant").await;
    let outsider = caller_holding(
        &app,
        other,
        Some("OTHER-START"),
        "other.publisher",
        &[
            "workflow:definition:read",
            "workflow:definition:create",
            "workflow:definition:publish",
        ],
    )
    .await;
    approver_role(&app, other).await;
    let theirs = saved(&app, &outsider, "del_theirs").await;
    let published = publish(&app, &outsider, theirs).await;
    assert_eq!(published.status, StatusCode::OK, "{}", published.body);

    sqlx::query(
        "UPDATE document_type_workflows SET workflow_definition_id = $1 \
         WHERE document_type_id = $2",
    )
    .bind(theirs)
    .bind(type_id)
    .execute(&app.pool)
    .await
    .expect("bind the type across tenants");

    let failed = submit(&app, &token, document).await;
    assert_eq!(
        failed.status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "a submit bound across tenants answered other than before #712: {}",
        failed.body
    );
    assert_eq!(
        failed.error_code(),
        Some("INTERNAL_ERROR"),
        "{}",
        failed.body
    );

    assert_eq!(document_status(&app, document).await, "DRAFT");
    assert!(instance_of(&app, document).await.is_none());
    assert_eq!(document_number(&app, document).await, None);

    let theirs_after = stamps(&app, theirs).await;
    assert_eq!(theirs_after.status, "ACTIVE");
    assert!(theirs_after.deleted_at.is_none());
    let started_on_theirs: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_instances WHERE workflow_definition_id = $1",
    )
    .bind(theirs)
    .fetch_one(&app.pool)
    .await
    .expect("count the instances on their revision");
    assert_eq!(started_on_theirs, 0);
}

/// **The deadlock guard counts a deadlock it is shown** ([#712] AC10). Every
/// race above asserts that `deadlocks_once_settled` equals `deadlocks_so_far`
/// read at the start, which would pass as well if nothing were ever counted.
/// Here two transactions take two revisions `FOR UPDATE` in opposite orders,
/// PostgreSQL aborts one with `40P01`, and the settled read is one more than
/// the read before it.
///
/// [#712]: https://github.com/sujanto-gaws/kelir/issues/712
#[tokio::test]
async fn the_deadlock_guard_counts_a_deadlock_it_is_shown() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let tenant = fixtures::SYSTEM_TENANT_ID;
    let deadlocks = deadlocks_so_far(&app).await;

    let first = published(&app, &token, "guard_first").await;
    let second = published(&app, &token, "guard_second").await;

    let mut one = app.pool.begin().await.expect("a transaction");
    let mut two = app.pool.begin().await.expect("a transaction");
    assert!(definition_repo::lock_for_publish(&mut one, tenant, first)
        .await
        .expect("one's first lock"));
    assert!(definition_repo::lock_for_publish(&mut two, tenant, second)
        .await
        .expect("two's first lock"));

    let (crossed_one, crossed_two) = tokio::time::timeout(Duration::from_secs(30), async {
        tokio::join!(
            definition_repo::lock_for_publish(&mut one, tenant, second),
            definition_repo::lock_for_publish(&mut two, tenant, first),
        )
    })
    .await
    .expect("PostgreSQL never broke the cycle");

    let aborted = [&crossed_one, &crossed_two]
        .into_iter()
        .filter(|crossed| {
            crossed
                .as_ref()
                .err()
                .and_then(|error| error.as_database_error())
                .and_then(|error| error.code())
                .as_deref()
                == Some("40P01")
        })
        .count();
    assert_eq!(aborted, 1, "{crossed_one:?} {crossed_two:?}");

    one.rollback().await.expect("one rolls back");
    two.rollback().await.expect("two rolls back");

    assert_eq!(
        deadlocks_once_settled(&app).await,
        deadlocks + 1,
        "the guard did not count the deadlock PostgreSQL detected"
    );
}

// ===========================================================================
// #785: a request reads back the row it changed before it commits
// ===========================================================================
//
// A publish, a deprecate and a draft's edit used to commit and then read their
// row back on the pool, for the answer and the audit row. A delete that
// committed in between left that read with nothing, and the request answered
// 500 for a change that had committed, with no audit row. CI met it once, in
// `a_delete_waiting_on_a_deprecate_deletes_it_and_each_audits_once`, where the
// gap is open only by chance. [`Gate::close_reads`] holds it open: it stops
// that read, and only that read, until the delete has answered.

/// The `event_type` audit rows written about `id`.
async fn audit_rows(app: &TestApp, id: Uuid, event_type: &str) -> Vec<AuditRow> {
    sqlx::query_as(
        "SELECT action, object_type, actor_user_id, old_value_json, new_value_json \
         FROM audit_events WHERE object_id = $1 AND event_type = $2 \
         ORDER BY created_at, id",
    )
    .bind(id)
    .bind(event_type)
    .fetch_all(&app.pool)
    .await
    .expect("read the audit rows")
}

/// **A deprecate a waiting delete follows still answers 200 and audits**
/// ([#785]). The gate holds the deprecate at its `UPDATE`, the delete waits on
/// its `FOR UPDATE`, and the read gate stops any read of the `DEPRECATED`
/// row on the pool, so the delete commits before such a read could run. The
/// deprecate's answer and its `Workflow.Deprecated` row come from its own
/// transaction.
///
/// [#785]: https://github.com/sujanto-gaws/kelir/issues/785
#[tokio::test]
async fn a_deprecate_a_waiting_delete_follows_answers_and_audits() {
    let app = Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;

    let id = published(&app, &token, "dep_read_back").await;

    let gate = Gate::close(&app, "UPDATE", "workflow_definitions").await;
    let reads = Gate::close_reads(&app, "row_status = 'DEPRECATED'").await;

    let deprecating = sent(&app, {
        let token = token.clone();
        move |app| async move { deprecate(&app, &token, id).await }
    });
    let deprecator = gate.parked(&app, &deprecating, "the deprecate").await;

    let deleting = sent(&app, {
        let token = token.clone();
        move |app| async move { delete_revision(&app, &token, id).await }
    });
    let (_, deleting) = waits_on(&app, deprecator, deleting, "the delete").await;

    gate.open().await;
    let deleted = answer(deleting, "the delete").await;
    reads.open().await;
    let deprecated = answer(deprecating, "the deprecate").await;

    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);
    assert_eq!(
        deprecated.status,
        StatusCode::OK,
        "a deprecate that committed answered as if it had not: {}",
        deprecated.body
    );
    assert_eq!(deprecated.body["data"]["status"], "DEPRECATED");

    let deprecations = deprecation_records(&app, id).await;
    assert_eq!(deprecations.len(), 1, "{deprecations:?}");
    assert_eq!(deprecations[0].4["status"], "DEPRECATED");

    // The deprecate committed before the delete read the row, so the answer
    // and the audit row are not a change that was rolled back.
    let deletions = deletion_records(&app, id).await;
    assert_eq!(deletions.len(), 1, "{deletions:?}");
    assert_eq!(deletions[0]["status"], "DEPRECATED", "{}", deletions[0]);
}

/// **A publish a waiting delete follows still answers 200 and audits**
/// ([#785]). The deprecate's race on the publish path: the gate holds the
/// publish at its `UPDATE`, the delete waits on its `FOR UPDATE`, and the
/// read gate stops any read of the `ACTIVE` row on the pool. A revision no
/// approval runs on can be deleted once it is published, so the delete
/// deletes it.
///
/// [#785]: https://github.com/sujanto-gaws/kelir/issues/785
#[tokio::test]
async fn a_publish_a_waiting_delete_follows_answers_and_audits() {
    let app = Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;

    approver_role(&app, fixtures::SYSTEM_TENANT_ID).await;
    let id = saved(&app, &token, "pub_read_back").await;

    let gate = Gate::close(&app, "UPDATE", "workflow_definitions").await;
    let reads = Gate::close_reads(&app, "row_status = 'ACTIVE'").await;

    let publishing = sent(&app, {
        let token = token.clone();
        move |app| async move { publish(&app, &token, id).await }
    });
    let publisher = gate.parked(&app, &publishing, "the publish").await;

    let deleting = sent(&app, {
        let token = token.clone();
        move |app| async move { delete_revision(&app, &token, id).await }
    });
    let (_, deleting) = waits_on(&app, publisher, deleting, "the delete").await;

    gate.open().await;
    let deleted = answer(deleting, "the delete").await;
    reads.open().await;
    let published = answer(publishing, "the publish").await;

    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);
    assert_eq!(
        published.status,
        StatusCode::OK,
        "a publish that committed answered as if it had not: {}",
        published.body
    );
    assert_eq!(published.body["data"]["status"], "ACTIVE");

    let publications = audit_rows(&app, id, "Workflow.Published").await;
    assert_eq!(publications.len(), 1, "{publications:?}");
    assert_eq!(publications[0].4["status"], "ACTIVE");

    let deletions = deletion_records(&app, id).await;
    assert_eq!(deletions.len(), 1, "{deletions:?}");
    assert_eq!(deletions[0]["status"], "ACTIVE", "{}", deletions[0]);
}

/// **A draft's edit a delete follows at once still answers 200 and audits**
/// ([#785]). An edit holds no lock a delete waits on before it writes, so the
/// delete is sent once the edit has either answered or been stopped at a read
/// of its renamed row on the pool. Either way the delete commits first, and
/// the edit's answer and its `Workflow.Updated` row come from its own
/// transaction.
///
/// [#785]: https://github.com/sujanto-gaws/kelir/issues/785
#[tokio::test]
async fn a_draft_edit_a_delete_follows_answers_and_audits() {
    const RENAMED: &str = "Renamed before the delete";

    let app = Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;

    let id = saved(&app, &token, "edit_read_back").await;

    let reads = Gate::close_reads(&app, &format!("row_name = '{RENAMED}'")).await;

    let editing = sent(&app, {
        let token = token.clone();
        move |app| async move {
            app.put(
                &format!("{DEFINITIONS}/{id}"),
                Some(&token),
                json!({ "name": RENAMED }),
            )
            .await
        }
    });
    let deadline = tokio::time::Instant::now() + PATIENCE;
    while !editing.is_finished() && !reads.holds_a_waiter(&app).await {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the edit neither answered nor reached the read gate"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    // Sent and awaited with a deadline: a delete the read gate stopped by
    // mistake would otherwise hang the test, since the gate opens after it.
    let deleting = sent(&app, {
        let token = token.clone();
        move |app| async move { delete_revision(&app, &token, id).await }
    });
    let deleted = answer(deleting, "the delete").await;
    reads.open().await;
    let edited = answer(editing, "the edit").await;

    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);
    assert_eq!(
        edited.status,
        StatusCode::OK,
        "an edit that committed answered as if it had not: {}",
        edited.body
    );
    assert_eq!(edited.body["data"]["name"], RENAMED);

    let updates = audit_rows(&app, id, "Workflow.Updated").await;
    assert_eq!(updates.len(), 1, "{updates:?}");
    assert_eq!(updates[0].4["name"], RENAMED);

    // The edit committed: the deleted row carries the new name. Read from the
    // table, since the view stands in for it and hides a deleted row.
    let stored: String =
        sqlx::query_scalar("SELECT name FROM workflow_definitions_stored WHERE id = $1")
            .bind(id)
            .fetch_one(&app.pool)
            .await
            .expect("read the deleted row");
    assert_eq!(stored, RENAMED, "an edit answered 200 and was rolled back");
}
