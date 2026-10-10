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
/// `delete_definition` takes no row lock of its own; its `UPDATE` waits on the
/// deprecate's `FOR UPDATE` and, the row still live when it ends, matches it.
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

/// **A submit that read the revision `ACTIVE` before a deprecate committed
/// starts on it, and that approval is an ordinary running approval**: it is
/// decided to the end. The next submit of the type is refused.
///
/// The builder said a submit racing a deprecate may start an instance on the
/// revision. It does: `engine::start` reads the revision's status without a
/// lock, and the instance's foreign key to the revision then waits on the
/// deprecate's `FOR UPDATE` and is satisfied once it commits. The result is
/// the state `running_approvals_carry_on_after_their_revision_is_deprecated`
/// reaches in order, an instance pinned to a deprecated revision, so it is
/// harmless: this holds that it runs.
#[tokio::test]
async fn a_submit_racing_a_deprecate_starts_an_approval_that_runs_to_the_end() {
    let app = Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;
    let tenant = fixtures::SYSTEM_TENANT_ID;
    let approver = approver(&app, "dep.race.approver").await;

    let workflow = published(&app, &token, "dep_submit_race").await;
    let type_id = document_type(&app, &token, "PR_DEP_RACE", workflow).await;
    let document = draft(&app, &token, type_id).await;

    let mut deprecating = app.pool.begin().await.expect("a transaction");
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

    let submitting = {
        let app = Arc::clone(&app);
        let token = token.clone();
        tokio::spawn(async move { submit(&app, &token, document).await })
    };

    let waited = waited_on_a_lock(&app, &submitting).await;

    deprecating.commit().await.expect("the deprecate commits");
    let submitted = submitting.await.expect("the submit finished");

    assert!(waited, "the submit did not wait on the deprecate in flight");
    assert_eq!(submitted.status, StatusCode::OK, "{}", submitted.body);
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
