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

use axum::http::{Method, StatusCode};
use common::{fixtures, TestApp};
use kelir_backend::modules::workflow::repository::definition as definition_repo;
use serde_json::{json, Value};
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
/// **Seen red** before the route existed: the deprecate answered 404. The
/// decision path reads no revision status, so no mutation of this route's code
/// makes the rest red; it pins that deprecating stays a write to one column.
#[tokio::test]
async fn running_approvals_carry_on_after_their_revision_is_deprecated() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let approver = approver(&app, "dep.approver").await;

    let workflow = published(&app, &token, "dep_running").await;
    let type_id = document_type(&app, &token, "PR_DEP_RUNNING", workflow).await;
    let document = draft(&app, &token, type_id).await;
    let submitted = submit(&app, &token, document).await;
    assert_eq!(submitted.status, StatusCode::OK, "{}", submitted.body);

    let before = instance_of(&app, document).await.expect("an instance");
    let task = open_task_of(&app, document).await;

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
///   on a new type or on this one.
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

    // A binding naming it is refused, here and on a new type.
    let rebinding_to_it = app
        .put(
            &format!("/api/v1/document-types/{type_id}"),
            Some(&token),
            json!({ "workflows": [{ "workflowDefinitionId": first }] }),
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
