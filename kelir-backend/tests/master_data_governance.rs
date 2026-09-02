//! Master-data changes routed through the document workflow (FR-MDM-010;
//! [#255], **D-55**).
//!
//! # What these tests are about
//!
//! A supplier's details are changed by raising a **document**, not by a `PUT`.
//! The record parks at `PENDING_APPROVAL` while the change is decided, direct
//! edits are refused while it is parked, an approval applies the change in the
//! transaction that closes the process, and a rejection leaves the record
//! exactly as it was.
//!
//! # Seen to fail (coding standard §2.9)
//!
//! Four mutations, run 2026-09-02:
//!
//! | Mutation | Reddened |
//! |---|---|
//! | `governance::raise` not called from the submit | **six of the nine** — nothing parks, so nothing is refused, applied or put back |
//! | `refuse_if_awaiting_approval` forced to `Ok` | *a direct edit is refused while a change is awaiting approval* |
//! | `settle` returning before it applies | *an approval applies the change…* |
//! | The rejection branch putting the record at `DRAFT` rather than back | *a rejected change leaves an active record active* |
//!
//! [#255]: https://github.com/sujanto-gaws/kelir/issues/255

mod common;

use axum::http::{Method, StatusCode};
use serde_json::{json, Value};
use uuid::Uuid;

use common::{fixtures, TestApp};

fn id_of(value: &Value) -> Uuid {
    value["id"]
        .as_str()
        .expect("an id")
        .parse()
        .expect("a uuid")
}

/// An approver, and a token for them.
struct Approver {
    id: Uuid,
    token: String,
}

async fn approver(app: &TestApp, username: &str) -> Approver {
    let role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        &format!("ROLE-{}", username.to_uppercase()),
        &[
            "document:read",
            "workflow:task:read",
            "workflow:task:execute",
            "workflow:instance:read",
        ],
    )
    .await;

    let id = fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        username,
        &format!("{username}@example.test"),
        common::ADMIN_PASSWORD,
        &[role],
    )
    .await;

    let token = app.sign_in(username, common::ADMIN_PASSWORD).await;

    Approver { id, token }
}

/// The workflow a change document runs: one approval, approve or reject.
fn approval_workflow(key: &str, assignee: Uuid) -> Value {
    json!({
        "workflowKey": key,
        "version": "1.0.0",
        "name": "Master-data change approval",
        "initialState": "REVIEW",
        "states": [
            { "code": "REVIEW", "name": "Review the change",
              "mapsToDocumentStatus": "PENDING_APPROVAL",
              "task": { "taskDefinitionKey": "review", "taskName": "Review the change",
                        "assignment": { "assigneeType": "USER", "userId": assignee.to_string() } } },
            // `COMPLETED` rather than `APPROVED`: JWSS S9 requires a state
            // mapping to COMPLETED or CANCELLED, and the document module treats
            // both terminal-approval statuses the same way.
            { "code": "APPROVED", "name": "Approved", "mapsToDocumentStatus": "COMPLETED",
              "isFinal": true },
            { "code": "REJECTED", "name": "Rejected", "mapsToDocumentStatus": "REJECTED",
              "isFinal": true }
        ],
        "transitions": [
            { "from": "REVIEW", "to": "APPROVED", "action": "APPROVE",
              "allowedBy": { "assigneeType": "USER", "userId": assignee.to_string() } },
            { "from": "REVIEW", "to": "REJECTED", "action": "REJECT",
              "allowedBy": { "assigneeType": "USER", "userId": assignee.to_string() } }
        ]
    })
}

async fn publish_workflow(app: &TestApp, token: &str, definition: Value) -> Uuid {
    let created = app
        .post(
            "/api/v1/workflow/definitions",
            Some(token),
            json!({
                "workflowKey": definition["workflowKey"],
                "name": definition["name"],
                "definition": definition,
            }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    let id = id_of(&created.body["data"]);

    let published = app
        .post(
            &format!("/api/v1/workflow/definitions/{id}/publication"),
            Some(token),
            json!({}),
        )
        .await;
    assert_eq!(published.status, StatusCode::OK, "{}", published.body);

    id
}

/// A form whose fields **are** the record's fields, which is what makes a
/// change document a change: the payload the person fills in is deserialized
/// into the record's own update shape.
async fn change_form(app: &TestApp, token: &str, key: &str) -> Uuid {
    let created = app
        .post(
            "/api/v1/rad/forms",
            Some(token),
            json!({
                "formKey": key,
                "title": "Supplier change",
                "definition": {
                    "formId": key,
                    "version": "2.0.1",
                    "title": "Supplier change",
                    "components": [
                        { "id": "external-id", "role": "data", "type": "textfield",
                          "key": "externalId", "label": "External id",
                          "validation": { "type": "string" } },
                        { "id": "description", "role": "data", "type": "textfield",
                          "key": "description", "label": "Description",
                          "validation": { "type": "string" } }
                    ]
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

/// A document type that governs a party's changes: a form, a workflow, and
/// `targetEntityType` — which is the whole of the configuration (#255 AC3).
async fn governed_type(
    app: &TestApp,
    token: &str,
    code: &str,
    workflow: Uuid,
    entity: Option<&str>,
) -> Uuid {
    let form = change_form(app, token, &code.to_lowercase().replace('_', "-")).await;
    let mut body = json!({
        "typeCode": code,
        "name": code,
        "formId": form,
        "workflows": [{ "workflowDefinitionId": workflow }],
    });

    if let Some(entity) = entity {
        body["targetEntityType"] = json!(entity);
    }

    let created = app.post("/api/v1/document-types", Some(token), body).await;
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

async fn party(app: &TestApp, token: &str, code: &str) -> Uuid {
    let created = app
        .post(
            "/api/v1/master-data/parties",
            Some(token),
            json!({
                "partyId": code,
                "partyTypeId": "PARTY_GROUP",
                "partyGroup": { "groupName": format!("{code} Supplies") },
            }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    id_of(&created.body["data"])
}

/// A change document: linked to the record, carrying the proposed values.
async fn change_document(
    app: &TestApp,
    token: &str,
    type_id: Uuid,
    supplier: Uuid,
    form_data: Value,
) -> Uuid {
    let created = app
        .post(
            "/api/v1/documents",
            Some(token),
            json!({
                "documentTypeId": type_id,
                "title": "Change of supplier details",
                "entityType": "PARTY",
                "entityId": supplier,
                "formData": form_data,
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

async fn record_status(app: &TestApp, supplier: Uuid) -> String {
    sqlx::query_scalar("SELECT record_status FROM mdm_parties WHERE id = $1")
        .bind(supplier)
        .fetch_one(&app.pool)
        .await
        .expect("the record")
}

async fn decide(app: &TestApp, approver: &Approver, document: Uuid, action: &str) {
    let task: Uuid = sqlx::query_scalar(
        "SELECT id FROM workflow_tasks WHERE document_id = $1 AND status <> 'COMPLETED'",
    )
    .bind(document)
    .fetch_one(&app.pool)
    .await
    .expect("the open task");

    let decided = app
        .post(
            &format!("/api/v1/workflow/tasks/{task}/decision"),
            Some(&approver.token),
            json!({ "action": action }),
        )
        .await;
    assert_eq!(decided.status, StatusCode::OK, "{}", decided.body);
}

/// Puts a party at `ACTIVE`, which is where a real supplier lives.
async fn activate(app: &TestApp, token: &str, supplier: Uuid) {
    let moved = app
        .post(
            &format!("/api/v1/master-data/parties/{supplier}/transition"),
            Some(token),
            json!({ "recordStatusId": "ACTIVE" }),
        )
        .await;
    assert_eq!(moved.status, StatusCode::OK, "{}", moved.body);
}

// ---------------------------------------------------------------------------
// AC1, AC2 — the change is raised, the record parks, and nothing is written yet
// ---------------------------------------------------------------------------

/// **The record is not altered until the approval completes** (AC1), and the
/// two statuses are not two records of one fact (AC2): the document is
/// `PENDING_APPROVAL` because a process is running, and the record is
/// `PENDING_APPROVAL` because it is not editable — one is about the change, the
/// other about the record.
#[tokio::test]
async fn a_governed_change_parks_the_record_and_writes_nothing_to_it() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let reviewer = approver(&app, "mdm-park-reviewer").await;

    let workflow =
        publish_workflow(&app, &token, approval_workflow("wf_mdm_park", reviewer.id)).await;
    let type_id = governed_type(&app, &token, "MDM_PARK", workflow, Some("PARTY")).await;
    let supplier = party(&app, &token, "MDM-PARK-1").await;
    activate(&app, &token, supplier).await;

    let document = change_document(
        &app,
        &token,
        type_id,
        supplier,
        json!({ "externalId": "SUP-9001" }),
    )
    .await;

    assert_eq!(record_status(&app, supplier).await, "ACTIVE", "not yet");

    let submitted = submit(&app, &token, document).await;
    assert_eq!(submitted.status, StatusCode::OK, "{}", submitted.body);

    assert_eq!(
        record_status(&app, supplier).await,
        "PENDING_APPROVAL",
        "the record parks while its change is decided"
    );

    // AC1's second half: nothing is written to the record.
    let external: Option<String> =
        sqlx::query_scalar("SELECT external_id FROM mdm_parties WHERE id = $1")
            .bind(supplier)
            .fetch_one(&app.pool)
            .await
            .expect("the record");

    assert!(
        external.is_none(),
        "the change is proposed, not applied: an approver has not seen it yet"
    );

    // And the change is on the record's own history as an open attempt (AC4).
    let attempts = app
        .get(
            &format!("/api/v1/master-data/parties/{supplier}/change-requests"),
            Some(&token),
        )
        .await;

    assert_eq!(attempts.status, StatusCode::OK, "{}", attempts.body);
    assert_eq!(attempts.body["data"][0]["documentId"], document.to_string());
    assert!(attempts.body["data"][0]["outcome"].is_null(), "still open");
}

/// **A direct edit is refused while a change is awaiting approval** (AC1).
///
/// Without this the governance is theatre: anybody with the update permission
/// could apply the change the approver is still looking at, or a different one.
#[tokio::test]
async fn a_direct_edit_is_refused_while_a_change_is_awaiting_approval() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let reviewer = approver(&app, "mdm-lock-reviewer").await;

    let workflow =
        publish_workflow(&app, &token, approval_workflow("wf_mdm_lock", reviewer.id)).await;
    let type_id = governed_type(&app, &token, "MDM_LOCK", workflow, Some("PARTY")).await;
    let supplier = party(&app, &token, "MDM-LOCK-1").await;

    let document = change_document(
        &app,
        &token,
        type_id,
        supplier,
        json!({ "description": "moved to the new estate" }),
    )
    .await;
    let submitted = submit(&app, &token, document).await;
    assert_eq!(submitted.status, StatusCode::OK, "{}", submitted.body);

    let refused = app
        .put(
            &format!("/api/v1/master-data/parties/{supplier}"),
            Some(&token),
            json!({ "description": "edited around the approval" }),
        )
        .await;

    assert_eq!(refused.status, StatusCode::CONFLICT, "{}", refused.body);

    let description: Option<String> =
        sqlx::query_scalar("SELECT description FROM mdm_parties WHERE id = $1")
            .bind(supplier)
            .fetch_one(&app.pool)
            .await
            .expect("the record");

    assert!(description.is_none(), "and nothing was written");
}

/// **One change at a time**, held by the partial unique index rather than by a
/// read the second submission could race.
#[tokio::test]
async fn a_second_change_over_one_record_is_refused() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let reviewer = approver(&app, "mdm-second-reviewer").await;

    let workflow = publish_workflow(
        &app,
        &token,
        approval_workflow("wf_mdm_second", reviewer.id),
    )
    .await;
    let type_id = governed_type(&app, &token, "MDM_SECOND", workflow, Some("PARTY")).await;
    let supplier = party(&app, &token, "MDM-SECOND-1").await;

    let first = change_document(
        &app,
        &token,
        type_id,
        supplier,
        json!({ "externalId": "SUP-1" }),
    )
    .await;
    assert_eq!(submit(&app, &token, first).await.status, StatusCode::OK);

    let second = change_document(
        &app,
        &token,
        type_id,
        supplier,
        json!({ "externalId": "SUP-2" }),
    )
    .await;

    let refused = submit(&app, &token, second).await;

    assert_eq!(refused.status, StatusCode::CONFLICT, "{}", refused.body);

    let open: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM mdm_change_requests WHERE entity_id = $1 AND resolved_at IS NULL",
    )
    .bind(supplier)
    .fetch_one(&app.pool)
    .await
    .expect("a count");

    assert_eq!(open, 1);
}

// ---------------------------------------------------------------------------
// AC5 — approving applies the change, in the transaction that closes it
// ---------------------------------------------------------------------------

#[tokio::test]
async fn an_approval_applies_the_change_and_activates_the_record() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let reviewer = approver(&app, "mdm-apply-reviewer").await;

    let workflow =
        publish_workflow(&app, &token, approval_workflow("wf_mdm_apply", reviewer.id)).await;
    let type_id = governed_type(&app, &token, "MDM_APPLY", workflow, Some("PARTY")).await;
    let supplier = party(&app, &token, "MDM-APPLY-1").await;

    let document = change_document(
        &app,
        &token,
        type_id,
        supplier,
        json!({ "externalId": "SUP-9002", "description": "approved by the manager" }),
    )
    .await;
    let submitted = submit(&app, &token, document).await;
    assert_eq!(submitted.status, StatusCode::OK, "{}", submitted.body);

    decide(&app, &reviewer, document, "APPROVE").await;

    let (external, description, status): (Option<String>, Option<String>, String) = sqlx::query_as(
        "SELECT external_id, description, record_status FROM mdm_parties WHERE id = $1",
    )
    .bind(supplier)
    .fetch_one(&app.pool)
    .await
    .expect("the record");

    assert_eq!(external.as_deref(), Some("SUP-9002"));
    assert_eq!(description.as_deref(), Some("approved by the manager"));
    assert_eq!(
        status, "ACTIVE",
        "approving a change is what makes a record active"
    );

    let (outcome, resolved): (Option<String>, bool) = sqlx::query_as(
        "SELECT outcome, resolved_at IS NOT NULL FROM mdm_change_requests WHERE document_id = $1",
    )
    .bind(document)
    .fetch_one(&app.pool)
    .await
    .expect("the change");

    assert_eq!(outcome.as_deref(), Some("APPLIED"));
    assert!(resolved);

    // And the record is editable again.
    let edited = app
        .put(
            &format!("/api/v1/master-data/parties/{supplier}"),
            Some(&token),
            json!({ "description": "corrected afterwards" }),
        )
        .await;
    assert_eq!(edited.status, StatusCode::OK, "{}", edited.body);
}

// ---------------------------------------------------------------------------
// AC4 — a rejected change leaves the record untouched, and is on its history
// ---------------------------------------------------------------------------

/// **Back is not always `DRAFT`.** An active supplier whose change is refused is
/// still an active supplier, which is what `previous_record_status` is for.
#[tokio::test]
async fn a_rejected_change_leaves_an_active_record_active_and_unchanged() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let reviewer = approver(&app, "mdm-reject-reviewer").await;

    let workflow = publish_workflow(
        &app,
        &token,
        approval_workflow("wf_mdm_reject", reviewer.id),
    )
    .await;
    let type_id = governed_type(&app, &token, "MDM_REJECT", workflow, Some("PARTY")).await;
    let supplier = party(&app, &token, "MDM-REJECT-1").await;
    activate(&app, &token, supplier).await;

    let document = change_document(
        &app,
        &token,
        type_id,
        supplier,
        json!({ "externalId": "SUP-REFUSED" }),
    )
    .await;
    let submitted = submit(&app, &token, document).await;
    assert_eq!(submitted.status, StatusCode::OK, "{}", submitted.body);

    decide(&app, &reviewer, document, "REJECT").await;

    let (external, status): (Option<String>, String) =
        sqlx::query_as("SELECT external_id, record_status FROM mdm_parties WHERE id = $1")
            .bind(supplier)
            .fetch_one(&app.pool)
            .await
            .expect("the record");

    assert!(external.is_none(), "a refused change writes nothing");
    assert_eq!(
        status, "ACTIVE",
        "and the record goes back where it was, not to DRAFT"
    );

    // AC4: the attempt is on the record's own history, refusal included.
    let attempts = app
        .get(
            &format!("/api/v1/master-data/parties/{supplier}/change-requests"),
            Some(&token),
        )
        .await;

    assert_eq!(attempts.body["data"][0]["outcome"], "REFUSED");
    assert_eq!(attempts.body["data"][0]["previousRecordStatus"], "ACTIVE");
}

/// A rejected change over a `DRAFT` record puts it back to `DRAFT`, which is the
/// other half of the same rule.
#[tokio::test]
async fn a_rejected_change_over_a_draft_record_leaves_it_draft() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let reviewer = approver(&app, "mdm-draft-reviewer").await;

    let workflow =
        publish_workflow(&app, &token, approval_workflow("wf_mdm_draft", reviewer.id)).await;
    let type_id = governed_type(&app, &token, "MDM_DRAFT", workflow, Some("PARTY")).await;
    let supplier = party(&app, &token, "MDM-DRAFT-1").await;

    let document = change_document(
        &app,
        &token,
        type_id,
        supplier,
        json!({ "externalId": "SUP-DRAFT" }),
    )
    .await;
    let submitted = submit(&app, &token, document).await;
    assert_eq!(submitted.status, StatusCode::OK, "{}", submitted.body);

    decide(&app, &reviewer, document, "REJECT").await;

    assert_eq!(record_status(&app, supplier).await, "DRAFT");
}

// ---------------------------------------------------------------------------
// AC3, AC6 — configuration, and what an ordinary document does
// ---------------------------------------------------------------------------

/// **An ordinary document is untouched by any of this** (AC6's observable half):
/// a type that governs nothing raises no change, and the same engine path runs.
#[tokio::test]
async fn an_ordinary_document_raises_no_change() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let reviewer = approver(&app, "mdm-plain-reviewer").await;

    let workflow =
        publish_workflow(&app, &token, approval_workflow("wf_mdm_plain", reviewer.id)).await;
    let type_id = governed_type(&app, &token, "MDM_PLAIN", workflow, None).await;
    let supplier = party(&app, &token, "MDM-PLAIN-1").await;

    let document = change_document(
        &app,
        &token,
        type_id,
        supplier,
        json!({ "externalId": "SUP-IGNORED" }),
    )
    .await;
    let submitted = submit(&app, &token, document).await;
    assert_eq!(submitted.status, StatusCode::OK, "{}", submitted.body);

    assert_eq!(
        record_status(&app, supplier).await,
        "DRAFT",
        "a document that governs nothing parks nothing"
    );

    decide(&app, &reviewer, document, "APPROVE").await;

    let external: Option<String> =
        sqlx::query_scalar("SELECT external_id FROM mdm_parties WHERE id = $1")
            .bind(supplier)
            .fetch_one(&app.pool)
            .await
            .expect("the record");

    assert!(
        external.is_none(),
        "approving an ordinary document writes nothing to any record"
    );

    let changes: i64 = sqlx::query_scalar("SELECT count(*) FROM mdm_change_requests")
        .fetch_one(&app.pool)
        .await
        .expect("a count");

    assert_eq!(changes, 0);
}

/// **A type that governs an entity nothing can route to is refused at save**
/// (AC3), rather than producing documents that quietly govern nothing.
#[tokio::test]
async fn a_type_naming_an_ungovernable_entity_is_refused() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let refused = app
        .post(
            "/api/v1/document-types",
            Some(&token),
            json!({
                "typeCode": "MDM_BAD_ENTITY",
                "name": "MDM_BAD_ENTITY",
                "targetEntityType": "SUPPLIER",
            }),
        )
        .await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(
        refused.body["error"]["details"][0]["code"],
        "ENTITY_NOT_GOVERNABLE"
    );
}

/// A governed type whose document names no record is refused **at submit**: a
/// change with nothing to change would be approved by somebody and apply to
/// nothing.
#[tokio::test]
async fn a_governed_document_that_names_no_record_is_refused() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let reviewer = approver(&app, "mdm-unlinked-reviewer").await;

    let workflow = publish_workflow(
        &app,
        &token,
        approval_workflow("wf_mdm_unlinked", reviewer.id),
    )
    .await;
    let type_id = governed_type(&app, &token, "MDM_UNLINKED", workflow, Some("PARTY")).await;

    let created = app
        .post(
            "/api/v1/documents",
            Some(&token),
            json!({
                "documentTypeId": type_id,
                "title": "A change to nothing",
                "formData": { "externalId": "SUP-NOWHERE" },
            }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    let refused = submit(&app, &token, id_of(&created.body["data"])).await;

    assert_eq!(refused.status, StatusCode::CONFLICT, "{}", refused.body);
}
