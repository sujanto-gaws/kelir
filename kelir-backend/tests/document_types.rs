//! Document types and their bindings, through the API (#157, #165).
//!
//! The binding checks are what this file mostly exists for. A type that names a
//! form which does not exist, or one that is still a draft, is a type whose
//! documents cannot be rendered — and the second is the subtler of the two,
//! because the form is right there and merely unfinished.
//!
//! **#165 added the third check and the two that were missing.** Re-pointing a
//! type at a new form revision is allowed and existing documents keep the
//! revision they pinned, which is the decision AC3 asks for — enforced by
//! refusing the rebinding while any document exists that pinned *nothing*, since
//! those are the only ones a rebinding can reach. And the create-path binding
//! refusals #157 wrote were asserted on create alone: removing `check_bindings`
//! from `update_type` left every test in this file green.

mod common;

use axum::http::{Method, StatusCode};
use common::{fixtures, TestApp};
use serde_json::{json, Value};
use uuid::Uuid;

fn definition(form_id: &str) -> Value {
    json!({
        "formId": form_id,
        "version": "2.0.1",
        "components": [{
            "id": "quantity",
            "role": "data",
            "type": "number",
            "key": "quantity",
            "label": "Quantity",
            "validation": { "type": "number" }
        }]
    })
}

/// A published form, made the way a person would: created, then published.
async fn published_form(app: &TestApp, token: &str, key: &str) -> Uuid {
    let created = app
        .send(
            Method::POST,
            "/api/v1/rad/forms",
            Some(token),
            Some(json!({
                "formKey": key,
                "title": "Purchase requisition",
                "definition": definition(key),
            })),
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    let id: Uuid = created.body["data"]["id"]
        .as_str()
        .expect("an id")
        .parse()
        .expect("a uuid");

    let published = app
        .send(
            Method::POST,
            &format!("/api/v1/rad/forms/{id}/publish"),
            Some(token),
            None,
        )
        .await;

    assert_eq!(published.status, StatusCode::OK, "{}", published.body);

    id
}

/// A form left in draft.
async fn draft_form(app: &TestApp, token: &str, key: &str) -> Uuid {
    let created = app
        .send(
            Method::POST,
            "/api/v1/rad/forms",
            Some(token),
            Some(json!({
                "formKey": key,
                "title": "Draft",
                "definition": definition(key),
            })),
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    created.body["data"]["id"]
        .as_str()
        .expect("an id")
        .parse()
        .expect("a uuid")
}

fn id_of(value: &Value) -> Uuid {
    value["id"]
        .as_str()
        .expect("an id")
        .parse()
        .expect("a uuid")
}

/// A published workflow definition, which a type may bind.
///
/// **New in Sprint 10.** Before [#187](https://github.com/sujanto-gaws/kelir/issues/187)
/// a workflow binding named a table that did not exist and was stored as given;
/// now it must name a definition that exists in this tenant and is `ACTIVE`,
/// checked in the write's transaction under a share lock. The tests below were
/// written against the old behaviour and are updated rather than deleted,
/// because what they assert — that a binding round-trips, and that a sent
/// collection replaces the stored set — is still the thing worth asserting.
async fn published_workflow(app: &TestApp, token: &str, key: &str) -> Uuid {
    let created = app
        .send(
            Method::POST,
            "/api/v1/workflow/definitions",
            Some(token),
            Some(json!({
                "workflowKey": key,
                "name": "Standard approval",
                "definition": {
                    "workflowKey": key,
                    "version": "1.0.0",
                    "name": "Standard approval",
                    "initialState": "APPROVAL",
                    "states": [
                        { "code": "APPROVAL", "name": "Approval",
                          "mapsToDocumentStatus": "PENDING_APPROVAL",
                          "task": { "taskDefinitionKey": "approval", "taskName": "Approve",
                                    "assignment": { "assigneeType": "OWNER" } } },
                        { "code": "COMPLETED", "name": "Completed",
                          "mapsToDocumentStatus": "COMPLETED", "isFinal": true }
                    ],
                    "transitions": [
                        { "from": "APPROVAL", "to": "COMPLETED", "action": "APPROVE",
                          "allowedBy": "OWNER" }
                    ]
                },
            })),
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let id = id_of(&created.body["data"]);

    let publication = app
        .send(
            Method::POST,
            &format!("/api/v1/workflow/definitions/{id}/publication"),
            Some(token),
            Some(json!({})),
        )
        .await;
    assert_eq!(publication.status, StatusCode::OK, "{}", publication.body);

    id
}

#[tokio::test]
async fn a_type_is_created_with_its_bindings_and_read_back() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let form = published_form(&app, &token, "pr-type-form").await;
    let workflow = published_workflow(&app, &token, "wf_type_binding").await;

    let created = app
        .send(
            Method::POST,
            "/api/v1/document-types",
            Some(&token),
            Some(json!({
                "typeCode": "PURCHASE_REQUISITION",
                "name": "Purchase requisition",
                "category": "PROCUREMENT",
                "formId": form,
                "workflows": [
                    { "workflowDefinitionId": workflow, "priority": 1 }
                ]
            })),
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    assert_eq!(created.body["data"]["formId"], form.to_string());
    assert_eq!(
        created.body["data"]["defaultSecurityLevel"], "INTERNAL",
        "the default when none is sent"
    );
    assert_eq!(created.body["data"]["status"], "ACTIVE");

    let id = id_of(&created.body["data"]);
    let read = app
        .send(
            Method::GET,
            &format!("/api/v1/document-types/{id}"),
            Some(&token),
            None,
        )
        .await;

    assert_eq!(read.status, StatusCode::OK);

    let workflows = read.body["data"]["workflows"]
        .as_array()
        .expect("workflows");
    assert_eq!(workflows.len(), 1);
    assert_eq!(workflows[0]["workflowDefinitionId"], workflow.to_string());
}

/// A workflow binding that names nothing is refused and not stored.
///
/// **This test used to assert the opposite**, and the change is the point.
/// Until Sprint 10 `workflow_definitions` did not exist, so this was the one
/// reference on the aggregate that nothing could check and the old test recorded
/// that fact rather than letting a reader discover it. `0025_workflow.sql` added
/// the table and the foreign key `0015_document.sql` deferred, and
/// [#187](https://github.com/sujanto-gaws/kelir/issues/187) AC2 made the check
/// real — so the assertion inverts, in place, where the history is visible.
///
/// The full behaviour, including the draft arm and the accepted case, is in
/// `workflow_engine.rs`; what is here is that this surface refuses.
#[tokio::test]
async fn a_workflow_binding_that_names_nothing_is_refused_and_not_stored() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let invented = Uuid::now_v7();

    let refused = app
        .send(
            Method::POST,
            "/api/v1/document-types",
            Some(&token),
            Some(json!({
                "typeCode": "UNVERIFIED_WORKFLOW",
                "name": "Unverified",
                "workflows": [{ "workflowDefinitionId": invented, "priority": 1 }]
            })),
        )
        .await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a document type was bound to a workflow that does not exist: {}",
        refused.body
    );
    assert_eq!(
        refused.body["error"]["details"][0]["path"], "workflows.0.workflowDefinitionId",
        "{}",
        refused.body
    );

    // And nothing was stored, so the refusal is not a message over a write that
    // happened anyway.
    let listed = app
        .send(Method::GET, "/api/v1/document-types", Some(&token), None)
        .await;
    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);
    assert!(
        !listed.body.to_string().contains("UNVERIFIED_WORKFLOW"),
        "the type was created despite the refusal: {}",
        listed.body
    );
}

#[tokio::test]
async fn a_form_binding_that_names_nothing_is_refused_and_not_stored() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let invented = Uuid::now_v7();

    let response = app
        .send(
            Method::POST,
            "/api/v1/document-types",
            Some(&token),
            Some(json!({
                "typeCode": "NO_SUCH_FORM",
                "name": "No such form",
                "formId": invented,
            })),
        )
        .await;

    assert_eq!(
        response.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        response.body
    );
    assert_eq!(response.body["error"]["details"][0]["path"], "formId");
    assert_eq!(response.body["error"]["details"][0]["code"], "NOT_FOUND");

    let stored: i64 =
        sqlx::query_scalar("SELECT count(*) FROM document_types WHERE type_code = 'NO_SUCH_FORM'")
            .fetch_one(&app.pool)
            .await
            .expect("count is queryable");

    assert_eq!(stored, 0, "a refusal that writes first is not a refusal");
}

/// A draft form cannot be bound, and the refusal says which of the two mistakes
/// it is.
#[tokio::test]
async fn a_draft_form_cannot_be_bound() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let draft = draft_form(&app, &token, "pr-still-draft").await;

    let response = app
        .send(
            Method::POST,
            "/api/v1/document-types",
            Some(&token),
            Some(json!({
                "typeCode": "DRAFT_FORM",
                "name": "Draft form",
                "formId": draft,
            })),
        )
        .await;

    assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        response.body["error"]["details"][0]["code"], "NOT_PUBLISHED",
        "a document pins the revision it was created against, so binding a \
         draft pins a definition that can still change underneath it. Body: {}",
        response.body
    );
}

/// A form in another tenant is not bindable, and reads as absent rather than
/// forbidden.
#[tokio::test]
async fn a_form_in_another_tenant_cannot_be_bound() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let other = fixtures::create_tenant(&app.pool, "TNT-DT-OTHER", "Other tenant").await;

    let hidden = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO rad_forms (id, tenant_id, form_key, title, jfss_version,
                                definition_json, status, published_at)
         VALUES ($1, $2, 'hidden', 'Hidden', '2.0.1', $3, 'PUBLISHED', now())",
    )
    .bind(hidden)
    .bind(other)
    .bind(definition("hidden"))
    .execute(&app.pool)
    .await
    .expect("insert the other tenant's form");

    let response = app
        .send(
            Method::POST,
            "/api/v1/document-types",
            Some(&token),
            Some(json!({
                "typeCode": "CROSS_TENANT",
                "name": "Cross tenant",
                "formId": hidden,
            })),
        )
        .await;

    assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        response.body["error"]["details"][0]["code"], "NOT_FOUND",
        "another tenant's form must read as absent, not as unpublished — the \
         second would confirm it exists. Body: {}",
        response.body
    );
}

#[tokio::test]
async fn a_duplicate_type_code_is_a_conflict() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let body = json!({ "typeCode": "DUPLICATE", "name": "First" });

    let first = app
        .send(
            Method::POST,
            "/api/v1/document-types",
            Some(&token),
            Some(body.clone()),
        )
        .await;
    assert_eq!(first.status, StatusCode::CREATED, "{}", first.body);

    let again = app
        .send(
            Method::POST,
            "/api/v1/document-types",
            Some(&token),
            Some(body),
        )
        .await;

    assert_eq!(again.status, StatusCode::CONFLICT, "{}", again.body);
}

#[tokio::test]
async fn an_update_records_what_changed_and_replaces_the_bindings_it_is_sent() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    // Three published workflows, because the assertion is that a *sent*
    // collection replaces the stored set: two bindings become one, and the one
    // has to be a definition that exists (#187).
    let first_workflow = published_workflow(&app, &token, "wf_audited_one").await;
    let second_workflow = published_workflow(&app, &token, "wf_audited_two").await;
    let third_workflow = published_workflow(&app, &token, "wf_audited_three").await;

    let created = app
        .send(
            Method::POST,
            "/api/v1/document-types",
            Some(&token),
            Some(json!({
                "typeCode": "AUDITED",
                "name": "Audited",
                "category": "PROCUREMENT",
                "workflows": [
                    { "workflowDefinitionId": first_workflow, "priority": 1 },
                    { "workflowDefinitionId": second_workflow, "priority": 2 }
                ]
            })),
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let id = id_of(&created.body["data"]);

    // The name moves; the category is re-sent at its current value.
    let updated = app
        .send(
            Method::PUT,
            &format!("/api/v1/document-types/{id}"),
            Some(&token),
            Some(json!({
                "name": "Renamed",
                "category": "PROCUREMENT",
                "workflows": [{ "workflowDefinitionId": third_workflow, "priority": 5 }]
            })),
        )
        .await;

    assert_eq!(updated.status, StatusCode::OK, "{}", updated.body);
    assert_eq!(
        updated.body["data"]["workflows"]
            .as_array()
            .expect("workflows")
            .len(),
        1,
        "a collection that is sent replaces the stored set wholesale"
    );

    let (old_value, new_value): (Value, Value) = sqlx::query_as(
        "SELECT old_value_json, new_value_json FROM audit_events
         WHERE object_id = $1 AND action = 'UPDATE'
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(id)
    .fetch_one(&app.pool)
    .await
    .expect("the update was audited");

    assert_eq!(old_value["name"], "Audited");
    assert_eq!(new_value["name"], "Renamed");
    assert!(
        new_value.get("category").is_none(),
        "the category was re-sent at its current value, so it did not move: a \
         record of what was requested rather than what changed is what #135 \
         rejected; got {new_value}"
    );

    // And no dead rows: the bindings are replaced, not accumulated.
    let rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM document_type_workflows WHERE document_type_id = $1",
    )
    .bind(id)
    .fetch_one(&app.pool)
    .await
    .expect("count is queryable");

    assert_eq!(rows, 1);
}

/// The binding rows of a type as storage holds them, in a stable order.
async fn stored_binding_rows(app: &TestApp, type_id: Uuid) -> Vec<(Uuid, String)> {
    sqlx::query_as(
        "SELECT id, status FROM document_type_workflows
         WHERE document_type_id = $1 AND deleted_at IS NULL
         ORDER BY priority",
    )
    .bind(type_id)
    .fetch_all(&app.pool)
    .await
    .expect("bindings are queryable")
}

/// An edit that does not send `workflows`, or sends it as `null`, leaves every
/// stored binding exactly as it was (#718).
///
/// The test above pins the other half: a collection that **is** sent replaces
/// the stored set. Nothing pinned this half, and the admin dialog depends on
/// it. The dialog sees one binding at most and cannot send back fields it
/// never read, so it leaves `workflows` out to keep the rest.
///
/// Both bindings carry the fields the dialog does not author: a condition on
/// one and a validity window on the other. The rows are checked by id and
/// status as well as on the wire. A server that rewrote the same set would
/// keep the wire shape but insert new rows, with new ids and `status` back at
/// its default. That is not leaving the bindings alone.
#[tokio::test]
async fn an_update_that_does_not_send_workflows_keeps_every_stored_binding() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let conditional = published_workflow(&app, &token, "wf_kept_conditional").await;
    let windowed = published_workflow(&app, &token, "wf_kept_windowed").await;
    let condition = r#"{"<=":[{"var":"amount"},10000000]}"#;

    let created = app
        .send(
            Method::POST,
            "/api/v1/document-types",
            Some(&token),
            Some(json!({
                "typeCode": "KEPT_BINDINGS",
                "name": "Kept bindings",
                "workflows": [
                    {
                        "workflowDefinitionId": conditional,
                        "conditionExpression": condition,
                        "priority": 1
                    },
                    {
                        "workflowDefinitionId": windowed,
                        "priority": 2,
                        "validFrom": "2026-01-01",
                        "validTo": "2026-12-31"
                    }
                ]
            })),
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let id = id_of(&created.body["data"]);

    let expected = json!([
        {
            "workflowDefinitionId": conditional,
            "conditionExpression": condition,
            "priority": 1,
            "validFrom": null,
            "validTo": null
        },
        {
            "workflowDefinitionId": windowed,
            "conditionExpression": null,
            "priority": 2,
            "validFrom": "2026-01-01",
            "validTo": "2026-12-31"
        }
    ]);
    assert_eq!(
        created.body["data"]["workflows"], expected,
        "the bindings were not stored as sent, so the assertions below would \
         compare against the wrong baseline"
    );

    let rows_before = stored_binding_rows(&app, id).await;
    assert_eq!(rows_before.len(), 2, "{rows_before:?}");

    let edits = [
        (
            "an absent key",
            json!({ "name": "Renamed without workflows" }),
        ),
        (
            "an explicit null",
            json!({ "name": "Renamed with null workflows", "workflows": null }),
        ),
    ];

    for (label, body) in edits {
        let updated = app
            .send(
                Method::PUT,
                &format!("/api/v1/document-types/{id}"),
                Some(&token),
                Some(body.clone()),
            )
            .await;

        assert_eq!(
            updated.status,
            StatusCode::OK,
            "{label}: an edit that leaves workflows alone was refused: {}",
            updated.body
        );
        assert_eq!(updated.body["data"]["name"], body["name"], "{label}");

        let read = app
            .send(
                Method::GET,
                &format!("/api/v1/document-types/{id}"),
                Some(&token),
                None,
            )
            .await;

        assert_eq!(read.status, StatusCode::OK, "{}", read.body);
        assert_eq!(
            read.body["data"]["workflows"], expected,
            "{label}: an edit that did not send workflows changed the stored \
             bindings"
        );

        assert_eq!(
            stored_binding_rows(&app, id).await,
            rows_before,
            "{label}: the binding rows were rewritten rather than left alone"
        );

        let (old_value, new_value): (Value, Value) = sqlx::query_as(
            "SELECT old_value_json, new_value_json FROM audit_events
             WHERE object_id = $1 AND event_type = 'DocumentType.Updated'
             ORDER BY created_at DESC LIMIT 1",
        )
        .bind(id)
        .fetch_one(&app.pool)
        .await
        .expect("the update was audited");

        assert_eq!(new_value["name"], body["name"], "{label}: {new_value}");
        assert!(
            old_value.get("workflows").is_none() && new_value.get("workflows").is_none(),
            "{label}: the audit records a workflows change that did not happen; \
             old {old_value}, new {new_value}"
        );
    }
}

/// A rename of a type whose bound revision was deprecated since succeeds, and
/// the binding stays as it was (#718's verification campaign).
///
/// Only an `ACTIVE` revision may be bound. Until #716 the server checked every
/// binding it was sent, so the dialog's old body, which sent the stored binding
/// back on every save, made a type bound to a revision deprecated after binding
/// impossible even to rename. Leaving `workflows` out binds nothing, so nothing
/// is checked.
///
/// **The last half asserted the opposite until #716**, and inverts in place: a
/// binding sent back exactly as stored is not re-checked either, so the rename
/// saves and the binding is still the deprecated revision. Changing it in one
/// field (here its priority) is binding something, and is still refused.
///
/// # Seen to fail
///
/// Red against an `update_type` that re-checks the stored bindings when
/// `workflows` is absent: the rename was refused `NOT_PUBLISHED`. The re-sent
/// half was red against #716's parent (5781320): `NOT_PUBLISHED`.
#[tokio::test]
async fn a_rename_that_does_not_send_workflows_keeps_a_binding_deprecated_since() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let workflow = published_workflow(&app, &token, "wf_deprecated_since").await;

    let created = app
        .send(
            Method::POST,
            "/api/v1/document-types",
            Some(&token),
            Some(json!({
                "typeCode": "DEPRECATED_SINCE",
                "name": "Deprecated since",
                "workflows": [{ "workflowDefinitionId": workflow, "priority": 4 }]
            })),
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let id = id_of(&created.body["data"]);

    let deprecated = app
        .send(
            Method::POST,
            &format!("/api/v1/workflow/definitions/{workflow}/deprecation"),
            Some(&token),
            Some(json!({})),
        )
        .await;

    assert_eq!(deprecated.status, StatusCode::OK, "{}", deprecated.body);

    let expected = json!([{
        "workflowDefinitionId": workflow,
        "conditionExpression": null,
        "priority": 4,
        "validFrom": null,
        "validTo": null
    }]);
    let rows_before = stored_binding_rows(&app, id).await;

    let renamed = app
        .send(
            Method::PUT,
            &format!("/api/v1/document-types/{id}"),
            Some(&token),
            Some(json!({ "name": "Renamed after deprecation" })),
        )
        .await;

    assert_eq!(
        renamed.status,
        StatusCode::OK,
        "a rename binds nothing, so a binding deprecated since must not refuse \
         it: {}",
        renamed.body
    );
    assert_eq!(renamed.body["data"]["name"], "Renamed after deprecation");
    assert_eq!(
        renamed.body["data"]["workflows"], expected,
        "{}",
        renamed.body
    );
    assert_eq!(stored_binding_rows(&app, id).await, rows_before);

    let resent = app
        .send(
            Method::PUT,
            &format!("/api/v1/document-types/{id}"),
            Some(&token),
            Some(json!({
                "name": "Renamed and re-sent",
                "workflows": [{ "workflowDefinitionId": workflow, "priority": 4 }]
            })),
        )
        .await;

    assert_eq!(
        resent.status,
        StatusCode::OK,
        "a binding sent back unchanged binds nothing new, so it must not be \
         re-checked (#716): {}",
        resent.body
    );
    assert_eq!(resent.body["data"]["name"], "Renamed and re-sent");
    assert_eq!(
        resent.body["data"]["workflows"], expected,
        "{}",
        resent.body
    );

    let edited = app
        .send(
            Method::PUT,
            &format!("/api/v1/document-types/{id}"),
            Some(&token),
            Some(json!({
                "name": "Renamed and rebound",
                "workflows": [{ "workflowDefinitionId": workflow, "priority": 5 }]
            })),
        )
        .await;

    assert_eq!(
        edited.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        edited.body
    );
    assert_eq!(
        edited.body["error"]["details"][0]["code"], "NOT_PUBLISHED",
        "{}",
        edited.body
    );

    let read = app
        .send(
            Method::GET,
            &format!("/api/v1/document-types/{id}"),
            Some(&token),
            None,
        )
        .await;

    assert_eq!(read.status, StatusCode::OK, "{}", read.body);
    assert_eq!(read.body["data"]["name"], "Renamed and re-sent");
    assert_eq!(read.body["data"]["workflows"], expected);
}

// ---------------------------------------------------------------------------
// A binding sent back unchanged is not re-checked (#716)
// ---------------------------------------------------------------------------
//
// Option (a) of the criteria on #716: `update_type` checks only the workflow
// bindings that differ from a stored row in one of their five fields. (The
// form and list bindings are skipped only when their key is left out; one sent
// at its stored value is still checked, #739.) So an API
// client that PUTs the whole type back can rename a type whose revision was
// deprecated since, and nothing becomes routable that was not: a submission
// still refuses the deprecated binding (#187, #713; `workflow_deprecation.rs`).

/// Publishes a workflow and deprecates it, which is how a binding to a revision
/// that is no longer `ACTIVE` comes to exist: storage refuses binding one.
async fn deprecate(app: &TestApp, token: &str, workflow: Uuid) {
    let deprecated = app
        .send(
            Method::POST,
            &format!("/api/v1/workflow/definitions/{workflow}/deprecation"),
            Some(token),
            Some(json!({})),
        )
        .await;

    assert_eq!(deprecated.status, StatusCode::OK, "{}", deprecated.body);
}

async fn deprecated_workflow(app: &TestApp, token: &str, key: &str) -> Uuid {
    let workflow = published_workflow(app, token, key).await;
    deprecate(app, token, workflow).await;
    workflow
}

async fn read_type(app: &TestApp, token: &str, id: Uuid) -> Value {
    let read = app
        .send(
            Method::GET,
            &format!("/api/v1/document-types/{id}"),
            Some(token),
            None,
        )
        .await;

    assert_eq!(read.status, StatusCode::OK, "{}", read.body);
    read.body["data"].clone()
}

async fn put_type(app: &TestApp, token: &str, id: Uuid, body: Value) -> common::TestResponse {
    app.send(
        Method::PUT,
        &format!("/api/v1/document-types/{id}"),
        Some(token),
        Some(body),
    )
    .await
}

fn assert_refused_at(response: &common::TestResponse, path: &str, code: &str, label: &str) {
    assert_eq!(
        response.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{label}: {}",
        response.body
    );
    assert_eq!(
        response.body["error"]["details"][0]["path"], path,
        "{label}: {}",
        response.body
    );
    assert_eq!(
        response.body["error"]["details"][0]["code"], code,
        "{label}: {}",
        response.body
    );
}

/// **Unchanged means all five fields** (#716 AC1, AC6). A type is bound to a
/// deprecated revision through a binding that sets every field: a condition, a
/// priority and a window. Sent back exactly, the binding saves with a rename.
/// Each field edited alone is a binding the caller is setting, so it is checked
/// and refused at `workflows.0.workflowDefinitionId` — `NOT_FOUND` for an id
/// that names nothing, `NOT_PUBLISHED` for everything else — and every refusal
/// leaves the type as it was.
///
/// # Seen to fail
///
/// Red against #716's parent (5781320): the unchanged re-send was refused
/// `NOT_PUBLISHED`. The edit arms are the two mutations AC6 names: skipping the
/// check for every sent binding accepts all seven edits, and matching on
/// `workflowDefinitionId` alone accepts the four that keep the revision.
#[tokio::test]
async fn a_deprecated_binding_sent_back_unchanged_saves_and_any_edit_to_it_is_checked() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let bound = published_workflow(&app, &token, "wf_unchanged_bound").await;
    let other_deprecated = deprecated_workflow(&app, &token, "wf_unchanged_other").await;
    let condition = r#"{"<=":[{"var":"amount"},10000000]}"#;

    let stored = json!({
        "workflowDefinitionId": bound,
        "conditionExpression": condition,
        "priority": 3,
        "validFrom": "2026-01-01",
        "validTo": "2026-12-31"
    });

    let created = app
        .send(
            Method::POST,
            "/api/v1/document-types",
            Some(&token),
            Some(json!({
                "typeCode": "UNCHANGED_FIVE",
                "name": "Unchanged five",
                "workflows": [stored.clone()]
            })),
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    assert_eq!(created.body["data"]["workflows"], json!([stored.clone()]));
    let id = id_of(&created.body["data"]);

    deprecate(&app, &token, bound).await;

    let renamed = put_type(
        &app,
        &token,
        id,
        json!({ "name": "Renamed, binding re-sent", "workflows": [stored.clone()] }),
    )
    .await;

    assert_eq!(
        renamed.status,
        StatusCode::OK,
        "the stored binding was sent back unchanged in all five fields: {}",
        renamed.body
    );
    assert_eq!(renamed.body["data"]["name"], "Renamed, binding re-sent");
    assert_eq!(renamed.body["data"]["workflows"], json!([stored.clone()]));

    let edit = |field: &str, value: Value| {
        let mut binding = stored.clone();
        binding[field] = value;
        binding
    };

    let edits = [
        (
            "another deprecated revision",
            edit("workflowDefinitionId", json!(other_deprecated)),
            "NOT_PUBLISHED",
        ),
        (
            "a revision that names nothing",
            edit("workflowDefinitionId", json!(Uuid::now_v7())),
            "NOT_FOUND",
        ),
        (
            "another condition",
            edit(
                "conditionExpression",
                json!(r#"{"<=":[{"var":"amount"},5]}"#),
            ),
            "NOT_PUBLISHED",
        ),
        (
            "the condition cleared",
            edit("conditionExpression", Value::Null),
            "NOT_PUBLISHED",
        ),
        (
            "another priority",
            edit("priority", json!(4)),
            "NOT_PUBLISHED",
        ),
        (
            "another start",
            edit("validFrom", json!("2026-02-01")),
            "NOT_PUBLISHED",
        ),
        ("an open end", edit("validTo", Value::Null), "NOT_PUBLISHED"),
    ];

    for (label, binding, code) in edits {
        let refused = put_type(
            &app,
            &token,
            id,
            json!({ "name": format!("Edited: {label}"), "workflows": [binding] }),
        )
        .await;

        assert_refused_at(&refused, "workflows.0.workflowDefinitionId", code, label);

        let read = read_type(&app, &token, id).await;
        assert_eq!(read["name"], "Renamed, binding re-sent", "{label}");
        assert_eq!(read["workflows"], json!([stored.clone()]), "{label}");
    }
}

/// **Two stored bindings, one on a deprecated revision** (#716 AC2). Both sent
/// back unchanged save, in either order. An `ACTIVE` binding edited beside the
/// unchanged deprecated one saves too, because only the edit is checked. And a
/// deprecated revision put in place of the deprecated binding is refused at
/// the index it was sent at — `0` or `1` — with the stored set left alone.
///
/// # Seen to fail
///
/// Red against #716's parent (5781320): the unchanged pair was refused
/// `NOT_PUBLISHED` at `workflows.0`. The index arms are what a check that
/// reported the position of the matching *stored* row, or of the first
/// binding, would get wrong.
#[tokio::test]
async fn of_two_bindings_only_the_one_that_changed_is_checked_at_its_index() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let stale = published_workflow(&app, &token, "wf_pair_stale").await;
    let live = published_workflow(&app, &token, "wf_pair_live").await;
    let replacement = deprecated_workflow(&app, &token, "wf_pair_replacement").await;

    let stale_binding = json!({
        "workflowDefinitionId": stale,
        "conditionExpression": r#"{">":[{"var":"amount"},1000]}"#,
        "priority": 1,
        "validFrom": null,
        "validTo": null
    });
    let live_binding = json!({
        "workflowDefinitionId": live,
        "conditionExpression": null,
        "priority": 2,
        "validFrom": null,
        "validTo": null
    });

    let created = app
        .send(
            Method::POST,
            "/api/v1/document-types",
            Some(&token),
            Some(json!({
                "typeCode": "PAIR_ONE_STALE",
                "name": "Pair, one stale",
                "workflows": [stale_binding.clone(), live_binding.clone()]
            })),
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let id = id_of(&created.body["data"]);

    deprecate(&app, &token, stale).await;

    let stored = json!([stale_binding.clone(), live_binding.clone()]);

    for (label, workflows) in [
        (
            "as read",
            json!([stale_binding.clone(), live_binding.clone()]),
        ),
        (
            "reversed",
            json!([live_binding.clone(), stale_binding.clone()]),
        ),
    ] {
        let saved = put_type(
            &app,
            &token,
            id,
            json!({ "name": format!("Both re-sent, {label}"), "workflows": workflows }),
        )
        .await;

        assert_eq!(saved.status, StatusCode::OK, "{label}: {}", saved.body);
        assert_eq!(saved.body["data"]["workflows"], stored, "{label}");
    }

    // The ACTIVE binding is edited beside the unchanged deprecated one.
    let mut widened = live_binding.clone();
    widened["validFrom"] = json!("2026-01-01");

    let saved = put_type(
        &app,
        &token,
        id,
        json!({
            "name": "Live binding edited",
            "workflows": [stale_binding.clone(), widened.clone()]
        }),
    )
    .await;

    assert_eq!(saved.status, StatusCode::OK, "{}", saved.body);
    let stored = json!([stale_binding.clone(), widened.clone()]);
    assert_eq!(saved.body["data"]["workflows"], stored);

    // The deprecated binding replaced by another deprecated revision, at
    // either index.
    let mut replaced = stale_binding.clone();
    replaced["workflowDefinitionId"] = json!(replacement);

    for (path, workflows) in [
        (
            "workflows.0.workflowDefinitionId",
            json!([replaced.clone(), widened.clone()]),
        ),
        (
            "workflows.1.workflowDefinitionId",
            json!([widened.clone(), replaced.clone()]),
        ),
    ] {
        let refused = put_type(
            &app,
            &token,
            id,
            json!({ "name": "Replaced", "workflows": workflows }),
        )
        .await;

        assert_refused_at(&refused, path, "NOT_PUBLISHED", path);

        let read = read_type(&app, &token, id).await;
        assert_eq!(read["name"], "Live binding edited", "{path}");
        assert_eq!(read["workflows"], stored, "{path}");
    }
}

/// **A type bound to a deprecated revision is renamed in both request shapes**
/// (#716 AC5). The admin dialog's: every field it authors and no `workflows`,
/// which it leaves out while the chooser has not moved (#718,
/// `DocumentTypeFormDialog.vue`'s `save()`). And an API client's: the same
/// body with the stored bindings sent back as it read them. Both save, and the
/// binding is still the deprecated revision.
///
/// # Seen to fail
///
/// The client's half was red against #716's parent (5781320): `NOT_PUBLISHED`
/// at `workflows.0.workflowDefinitionId`. The dialog's half was green there,
/// since #718 already made an absent `workflows` bind nothing.
#[tokio::test]
async fn a_type_bound_to_a_deprecated_revision_is_renamed_by_the_dialog_and_by_a_client() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let form = published_form(&app, &token, "pr-renamed-deprecated").await;
    let workflow = published_workflow(&app, &token, "wf_renamed_deprecated").await;

    let created = app
        .send(
            Method::POST,
            "/api/v1/document-types",
            Some(&token),
            Some(json!({
                "typeCode": "RENAMED_DEPRECATED",
                "name": "Before the rename",
                "category": "PROCUREMENT",
                "formId": form,
                "workflows": [{ "workflowDefinitionId": workflow, "priority": 1 }]
            })),
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let id = id_of(&created.body["data"]);

    deprecate(&app, &token, workflow).await;

    let before = read_type(&app, &token, id).await;
    let stored = before["workflows"].clone();
    assert_eq!(stored.as_array().map(Vec::len), Some(1), "{before}");

    // The dialog's body, field for field: `typeCode` dropped on an edit, the
    // blank optionals as `null`, and no `workflows` key.
    let dialog = json!({
        "name": "Renamed by the dialog",
        "description": null,
        "category": before["category"],
        "formId": before["formId"],
        "listId": before["listId"],
        "targetEntityType": before["targetEntityType"],
        "defaultSecurityLevel": before["defaultSecurityLevel"],
        "status": before["status"],
    });

    let renamed = put_type(&app, &token, id, dialog.clone()).await;
    assert_eq!(renamed.status, StatusCode::OK, "{}", renamed.body);
    assert_eq!(renamed.body["data"]["name"], "Renamed by the dialog");
    assert_eq!(renamed.body["data"]["workflows"], stored);

    let mut client = dialog;
    client["name"] = json!("Renamed by a client");
    client["workflows"] = stored.clone();

    let renamed = put_type(&app, &token, id, client).await;
    assert_eq!(
        renamed.status,
        StatusCode::OK,
        "a full-body PUT that sends the stored binding back must be idempotent \
         for it: {}",
        renamed.body
    );
    assert_eq!(renamed.body["data"]["name"], "Renamed by a client");
    assert_eq!(renamed.body["data"]["workflows"], stored);

    let read = read_type(&app, &token, id).await;
    assert_eq!(read["name"], "Renamed by a client");
    assert_eq!(read["workflows"], stored);
    assert_eq!(read["formId"], form.to_string());
}

/// **One unchanged binding sent twice is refused as a duplicate priority**
/// (#716's verification campaign). `changed_bindings` skips every sent binding
/// equal to a stored one, so both copies of a deprecated binding are skipped
/// and nothing below the request's validation would stop the replace storing
/// the binding twice. What does stop it is `validate_update`'s duplicate
/// priority check, which no update-path test pinned: the unit test reaches it
/// through `validate_create` only.
///
/// # Seen to fail
///
/// Red against `validate_update` without its `check_workflows` call: the save
/// was accepted and the type read back with the deprecated binding twice.
#[tokio::test]
async fn a_deprecated_binding_sent_back_twice_is_refused_as_a_duplicate_priority() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let workflow = published_workflow(&app, &token, "wf_twice_deprecated").await;

    let created = app
        .send(
            Method::POST,
            "/api/v1/document-types",
            Some(&token),
            Some(json!({
                "typeCode": "TWICE_DEPRECATED",
                "name": "Sent twice",
                "workflows": [{ "workflowDefinitionId": workflow, "priority": 3 }]
            })),
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let id = id_of(&created.body["data"]);

    deprecate(&app, &token, workflow).await;

    let stored = read_type(&app, &token, id).await["workflows"].clone();
    assert_eq!(stored.as_array().map(Vec::len), Some(1), "{stored}");

    let twice = put_type(
        &app,
        &token,
        id,
        json!({ "name": "Twice", "workflows": [stored[0].clone(), stored[0].clone()] }),
    )
    .await;

    assert_refused_at(&twice, "workflows.1.priority", "DUPLICATE", "sent twice");

    let read = read_type(&app, &token, id).await;
    assert_eq!(read["name"], "Sent twice");
    assert_eq!(read["workflows"], stored);
}

#[tokio::test]
async fn an_update_may_leave_a_binding_alone_even_when_its_form_was_retired() {
    // Re-checking an unchanged binding on every edit would make a type
    // unrenameable because a form was retired weeks ago. The fix for a stale
    // binding is to change it, not to be unable to touch the type.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let form = published_form(&app, &token, "pr-later-retired").await;

    let created = app
        .send(
            Method::POST,
            "/api/v1/document-types",
            Some(&token),
            Some(json!({
                "typeCode": "STALE_BINDING",
                "name": "Stale binding",
                "formId": form,
            })),
        )
        .await;
    let id = id_of(&created.body["data"]);

    app.send(
        Method::DELETE,
        &format!("/api/v1/rad/forms/{form}"),
        Some(&token),
        None,
    )
    .await;

    let renamed = app
        .send(
            Method::PUT,
            &format!("/api/v1/document-types/{id}"),
            Some(&token),
            Some(json!({ "name": "Renamed anyway" })),
        )
        .await;

    assert_eq!(renamed.status, StatusCode::OK, "{}", renamed.body);
    assert_eq!(renamed.body["data"]["name"], "Renamed anyway");
}

/// A type with documents cannot be retired.
#[tokio::test]
async fn a_type_with_documents_is_refused_rather_than_retired() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let created = app
        .send(
            Method::POST,
            "/api/v1/document-types",
            Some(&token),
            Some(json!({ "typeCode": "IN_USE", "name": "In use" })),
        )
        .await;
    let id = id_of(&created.body["data"]);

    // Inserted directly: documents have no endpoint until Sprint 9, and the
    // refusal is written now precisely so it does not have to be remembered
    // then.
    sqlx::query(
        "INSERT INTO documents (id, tenant_id, document_ref, document_type_id, title)
         VALUES ($1, $2, 'DOC-2026-000001', $3, 'A document')",
    )
    .bind(Uuid::now_v7())
    .bind(fixtures::SYSTEM_TENANT_ID)
    .bind(id)
    .execute(&app.pool)
    .await
    .expect("insert a document of this type");

    let deleted = app
        .send(
            Method::DELETE,
            &format!("/api/v1/document-types/{id}"),
            Some(&token),
            None,
        )
        .await;

    assert_eq!(
        deleted.status,
        StatusCode::CONFLICT,
        "retiring a type under live documents leaves them pointing at something \
         no read returns. Body: {}",
        deleted.body
    );
}

#[tokio::test]
async fn a_type_with_no_documents_is_retired() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let created = app
        .send(
            Method::POST,
            "/api/v1/document-types",
            Some(&token),
            Some(json!({ "typeCode": "UNUSED", "name": "Unused" })),
        )
        .await;
    let id = id_of(&created.body["data"]);

    let deleted = app
        .send(
            Method::DELETE,
            &format!("/api/v1/document-types/{id}"),
            Some(&token),
            None,
        )
        .await;

    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);

    let read = app
        .send(
            Method::GET,
            &format!("/api/v1/document-types/{id}"),
            Some(&token),
            None,
        )
        .await;

    assert_eq!(read.status, StatusCode::NOT_FOUND);
}

/// A form retired **while** the binding is being written is refused.
///
/// This is the test the `FOR SHARE` in `lock_bindable_form` exists for, and it
/// is here because removing that lock left every other test in this file green:
/// they are all single-threaded, so none of them reaches a concurrent write.
/// The coding standard calls a surviving mutation a finding, and the usual
/// cause is a fixture that never reaches the guard — which was exactly the case.
///
/// **It is deterministic, not a race.** The lock is what makes it so: a
/// transaction soft-deletes the form and holds it open, the binding request
/// then blocks on the same row rather than reading a stale answer, and the
/// commit releases it. Without the lock the request does not block at all — it
/// reads the form as live under READ COMMITTED, binds it, and commits before
/// the delete lands, leaving a document type pointing at a retired definition.
///
/// The timeout is a guard against the opposite failure: if the request blocked
/// on something that never released, the test would hang rather than fail.
#[tokio::test]
async fn a_form_retired_during_the_write_is_refused() {
    use std::time::Duration;

    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let form = published_form(&app, &token, "pr-retired-mid-write").await;

    // Hold the form's row in a transaction that soft-deletes it.
    let mut holding = app.pool.begin().await.expect("a transaction opens");
    sqlx::query("UPDATE rad_forms SET deleted_at = now() WHERE id = $1")
        .bind(form)
        .execute(&mut *holding)
        .await
        .expect("the soft delete applies inside the transaction");

    let binding = app.send(
        Method::POST,
        "/api/v1/document-types",
        Some(&token),
        Some(json!({
            "typeCode": "RETIRED_MID_WRITE",
            "name": "Retired mid-write",
            "formId": form,
        })),
    );

    // Let the request reach the locked read before the delete commits. Without
    // `FOR SHARE` it will already have finished by now, with a 201.
    let committed = async {
        tokio::time::sleep(Duration::from_millis(250)).await;
        holding.commit().await.expect("the soft delete commits");
    };

    let (response, ()) = tokio::time::timeout(
        Duration::from_secs(20),
        futures_lite_join(binding, committed),
    )
    .await
    .expect("the binding request must not block forever");

    assert_eq!(
        response.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "the form was retired while this binding was being written, so the \
         binding must be refused rather than stored against a definition no \
         read returns. Body: {}",
        response.body
    );

    let stored: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM document_types WHERE type_code = 'RETIRED_MID_WRITE'",
    )
    .fetch_one(&app.pool)
    .await
    .expect("count is queryable");

    assert_eq!(stored, 0);
}

/// `tokio::join!` as a function, so the two futures above read as values.
async fn futures_lite_join<A: std::future::Future, B: std::future::Future>(
    a: A,
    b: B,
) -> (A::Output, B::Output) {
    tokio::join!(a, b)
}

#[tokio::test]
async fn a_type_in_another_tenant_is_not_found() {
    let app = TestApp::spawn().await;
    let other = fixtures::create_tenant(&app.pool, "TNT-DT-HIDDEN", "Hidden tenant").await;

    let hidden = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO document_types (id, tenant_id, type_code, name)
         VALUES ($1, $2, 'HIDDEN', 'Hidden')",
    )
    .bind(hidden)
    .bind(other)
    .execute(&app.pool)
    .await
    .expect("insert the other tenant's type");

    let token = app.administrator_token().await;
    let response = app
        .send(
            Method::GET,
            &format!("/api/v1/document-types/{hidden}"),
            Some(&token),
            None,
        )
        .await;

    assert_eq!(
        response.status,
        StatusCode::NOT_FOUND,
        "an administrator holding every permission must still not see another \
         tenant's type; body {}",
        response.body
    );
}

// ---------------------------------------------------------------------------
// Rebinding the form on a type that already has documents (#165 AC3)
// ---------------------------------------------------------------------------

/// Creates a type bound to `form`, and returns its id.
async fn bound_type(app: &TestApp, token: &str, code: &str, form: Uuid) -> Uuid {
    let created = app
        .send(
            Method::POST,
            "/api/v1/document-types",
            Some(token),
            Some(json!({ "typeCode": code, "name": code, "formId": form })),
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    id_of(&created.body["data"])
}

/// Inserts a document of `type_id`, pinning `form` where one is given.
///
/// Inserted directly, as `a_type_with_documents_is_refused_rather_than_retired`
/// does and for the same reason: documents have no endpoint until Sprint 9, and
/// the rule is written now precisely so that it does not have to be remembered
/// then.
async fn seed_document(app: &TestApp, type_id: Uuid, reference: &str, form: Option<Uuid>) {
    sqlx::query(
        "INSERT INTO documents (id, tenant_id, document_ref, document_type_id, form_id, title)
         VALUES ($1, $2, $3, $4, $5, 'A document')",
    )
    .bind(Uuid::now_v7())
    .bind(fixtures::SYSTEM_TENANT_ID)
    .bind(reference)
    .bind(type_id)
    .bind(form)
    .execute(&app.pool)
    .await
    .expect("insert a document of this type");
}

/// **The decision #165 AC3 asks for, in the direction that keeps form revisions
/// usable.**
///
/// A form is revised by publishing the next revision, so a type that could never
/// be re-pointed would be stuck on revision 1 the moment one document existed.
/// The rebinding is therefore allowed — and the documents that already exist
/// keep the revision they pinned, which is what makes it safe.
#[tokio::test]
async fn a_type_whose_documents_pinned_their_form_can_be_rebound() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let first = published_form(&app, &token, "pr-revision-1").await;
    let next = published_form(&app, &token, "pr-revision-2").await;
    let id = bound_type(&app, &token, "REBINDABLE", first).await;

    seed_document(&app, id, "DOC-2026-000001", Some(first)).await;
    seed_document(&app, id, "DOC-2026-000002", Some(first)).await;

    let updated = app
        .send(
            Method::PUT,
            &format!("/api/v1/document-types/{id}"),
            Some(&token),
            Some(json!({ "formId": next })),
        )
        .await;

    assert_eq!(
        updated.status,
        StatusCode::OK,
        "a type whose documents each pinned a revision may be re-pointed: the \
         pinned ones are unreachable from the type's binding. Body: {}",
        updated.body
    );
    assert_eq!(updated.body["data"]["formId"], json!(next.to_string()));

    // And the documents did not move with it. This is the half of the decision
    // that matters: "existing documents keep the definition they were filled
    // against" is a claim about these two rows.
    let pinned: Vec<Uuid> =
        sqlx::query_scalar("SELECT form_id FROM documents WHERE document_type_id = $1")
            .bind(id)
            .fetch_all(&app.pool)
            .await
            .expect("the documents are readable");

    assert_eq!(
        pinned,
        vec![first, first],
        "a pinned revision is never moved"
    );
}

/// **The other half, and the reason the decision is enforced rather than
/// described.**
///
/// `documents.form_id` is nullable (Database Schema §6.6), so a document may
/// exist having pinned nothing — and such a document has only its type's
/// *current* binding to render against. Moving that binding re-renders it
/// against a definition nobody filled in, which is the data-integrity problem
/// AC3 names and which looks like a UI bug from every direction.
///
/// **Seen red** against a build with the `guard_rebinding` call removed from
/// `service::update_type`.
#[tokio::test]
async fn a_type_with_a_document_that_pinned_no_form_cannot_be_rebound() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let first = published_form(&app, &token, "pr-unpinned-1").await;
    let next = published_form(&app, &token, "pr-unpinned-2").await;
    let id = bound_type(&app, &token, "UNPINNED", first).await;

    seed_document(&app, id, "DOC-2026-000010", None).await;

    let updated = app
        .send(
            Method::PUT,
            &format!("/api/v1/document-types/{id}"),
            Some(&token),
            Some(json!({ "formId": next })),
        )
        .await;

    assert_eq!(
        updated.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        updated.body
    );
    assert_eq!(
        updated.body["error"]["details"][0]["code"], "DOCUMENTS_WITHOUT_A_PINNED_FORM",
        "{}",
        updated.body
    );
    assert_eq!(updated.body["error"]["details"][0]["path"], "formId");
    // The count is in the message rather than the word "some": an administrator
    // told "some documents" cannot tell one stray row from a year of them.
    assert!(
        updated.body["error"]["details"][0]["message"]
            .as_str()
            .expect("a message")
            .contains('1'),
        "{}",
        updated.body
    );

    // And nothing moved.
    let bound: Uuid = sqlx::query_scalar("SELECT form_id FROM document_types WHERE id = $1")
        .bind(id)
        .fetch_one(&app.pool)
        .await
        .expect("the type is readable");

    assert_eq!(bound, first, "a refused rebinding writes nothing");
}

/// **Clearing the binding is a change like any other.**
///
/// `formId: null` leaves an unpinned document with nothing at all to render
/// against, which is strictly worse than pointing it at the wrong definition —
/// so the guard is on the *change*, not on the new value being present.
#[tokio::test]
async fn the_binding_cannot_be_cleared_out_from_under_an_unpinned_document() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let form = published_form(&app, &token, "pr-cleared").await;
    let id = bound_type(&app, &token, "CLEARED", form).await;

    seed_document(&app, id, "DOC-2026-000020", None).await;

    let updated = app
        .send(
            Method::PUT,
            &format!("/api/v1/document-types/{id}"),
            Some(&token),
            Some(json!({ "formId": Value::Null })),
        )
        .await;

    assert_eq!(
        updated.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        updated.body
    );
    assert_eq!(
        updated.body["error"]["details"][0]["code"], "DOCUMENTS_WITHOUT_A_PINNED_FORM",
        "{}",
        updated.body
    );
}

/// An unrelated edit is not a rebinding, and must not be refused as one.
///
/// The complement of the two above: a guard that fired whenever documents
/// existed would make a type with one unpinned document unrenameable, which is
/// the failure `an_update_may_leave_a_binding_alone_even_when_its_form_was_retired`
/// exists to prevent one step earlier.
#[tokio::test]
async fn an_unpinned_document_does_not_stop_the_type_being_renamed() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let form = published_form(&app, &token, "pr-renameable").await;
    let id = bound_type(&app, &token, "RENAMEABLE", form).await;

    seed_document(&app, id, "DOC-2026-000030", None).await;

    let updated = app
        .send(
            Method::PUT,
            &format!("/api/v1/document-types/{id}"),
            Some(&token),
            Some(json!({ "name": "Renamed anyway" })),
        )
        .await;

    assert_eq!(updated.status, StatusCode::OK, "{}", updated.body);
    assert_eq!(updated.body["data"]["name"], "Renamed anyway");
}

/// Re-sending the binding at its current value is not a change either.
///
/// A client that PUTs the whole resource back sends `formId` on every edit, so
/// a guard keyed on the property being *present* rather than on the value
/// *moving* would refuse every such edit — and the update payload's other
/// fields are already written to be re-sendable (`an_update_records_what_changed…`).
#[tokio::test]
async fn re_sending_the_same_binding_is_not_a_rebinding() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let form = published_form(&app, &token, "pr-resent").await;
    let id = bound_type(&app, &token, "RESENT", form).await;

    seed_document(&app, id, "DOC-2026-000040", None).await;

    let updated = app
        .send(
            Method::PUT,
            &format!("/api/v1/document-types/{id}"),
            Some(&token),
            Some(json!({ "name": "Resent", "formId": form })),
        )
        .await;

    assert_eq!(updated.status, StatusCode::OK, "{}", updated.body);
}

// ---------------------------------------------------------------------------
// The binding checks, on the update path (#165 AC2)
// ---------------------------------------------------------------------------
//
// #157 covered these on create and left the update path asserted by nothing:
// `an_update_may_leave_a_binding_alone_even_when_its_form_was_retired` asserts
// the *absence* of a refusal, so removing `check_bindings` from `update_type`
// left every test green. The two below are the same claims as the create-path
// pair, made where AC2 also makes them.

/// **Seen red** against a build with the `check_bindings` call removed from
/// `service::update_type`.
#[tokio::test]
async fn a_type_cannot_be_rebound_to_a_draft_form() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let published = published_form(&app, &token, "pr-was-published").await;
    let draft = draft_form(&app, &token, "pr-still-a-draft").await;
    let id = bound_type(&app, &token, "REBIND_DRAFT", published).await;

    let updated = app
        .send(
            Method::PUT,
            &format!("/api/v1/document-types/{id}"),
            Some(&token),
            Some(json!({ "formId": draft })),
        )
        .await;

    assert_eq!(
        updated.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        updated.body
    );
    assert_eq!(
        updated.body["error"]["details"][0]["code"], "NOT_PUBLISHED",
        "a document pins the revision it was created against, so binding a \
         draft pins a definition that can still change underneath it. Body: {}",
        updated.body
    );
}

#[tokio::test]
async fn a_type_cannot_be_rebound_to_a_form_that_does_not_exist() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let published = published_form(&app, &token, "pr-exists").await;
    let id = bound_type(&app, &token, "REBIND_MISSING", published).await;

    let updated = app
        .send(
            Method::PUT,
            &format!("/api/v1/document-types/{id}"),
            Some(&token),
            Some(json!({ "formId": Uuid::now_v7() })),
        )
        .await;

    assert_eq!(
        updated.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        updated.body
    );
    assert_eq!(
        updated.body["error"]["details"][0]["code"], "NOT_FOUND",
        "{}",
        updated.body
    );

    // Nothing was written: the binding still names the form it did.
    let bound: Uuid = sqlx::query_scalar("SELECT form_id FROM document_types WHERE id = $1")
        .bind(id)
        .fetch_one(&app.pool)
        .await
        .expect("the type is readable");

    assert_eq!(bound, published);
}

/// A type retired before the write is not updated.
///
/// Coverage of behaviour that already held rather than a new guarantee, and it
/// is written that way on purpose: `update_type` now reads the row three times
/// — on the pool for its audit `before`, under the lock, and in the `UPDATE`'s
/// own predicate — and each of the three refuses a retired row on its own. This
/// asserts the outcome, so a refactor that removes one of them cannot turn the
/// endpoint into one that writes onto a row no read returns.
///
/// The *race* it is the still-life of is `a_form_retired_during_the_write_is_refused`,
/// which holds the row open across the request.
#[tokio::test]
async fn a_type_retired_before_the_write_is_not_updated() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let form = published_form(&app, &token, "pr-retired-type").await;
    let id = bound_type(&app, &token, "RETIRED_MID", form).await;

    sqlx::query("UPDATE document_types SET deleted_at = now() WHERE id = $1")
        .bind(id)
        .execute(&app.pool)
        .await
        .expect("retire the type");

    let updated = app
        .send(
            Method::PUT,
            &format!("/api/v1/document-types/{id}"),
            Some(&token),
            Some(json!({ "name": "Too late" })),
        )
        .await;

    assert_eq!(updated.status, StatusCode::NOT_FOUND, "{}", updated.body);
}

/// **The rebinding guard reads `documents`, so the lock has to cover a document
/// arriving** (coding standard §2.5).
///
/// A count on the pool followed by a write is check-then-act: a document created
/// in the gap is one the guard never saw, and it renders against a binding that
/// moved out from under it. `lock_type_binding` takes `FOR UPDATE` on the type
/// row, and that is the row both sides go through — because inserting a document
/// takes a `FOR KEY SHARE` on the `document_types` row its foreign key names,
/// and `FOR UPDATE` conflicts with `FOR KEY SHARE`. **The lock is therefore
/// enforced by the foreign key rather than by whatever Sprint 9's [#167]
/// remembers to do**, which is the difference between a rule and a note.
///
/// The transaction below holds that key-share lock by inserting an unpinned
/// document without committing. The rebinding request must block on it rather
/// than read a stale zero, and refuse once it commits.
///
/// **Seen red** with `FOR UPDATE` weakened to `FOR NO KEY UPDATE`, which does
/// *not* conflict with `FOR KEY SHARE`: the request then does not block at all,
/// counts zero unpinned documents, and returns 200 having rebound the type out
/// from under a document that was being created.
///
/// The timeout is a guard against the opposite failure: a request blocking on
/// something that never releases would hang the suite rather than fail it.
///
/// [#167]: https://github.com/sujanto-gaws/kelir/issues/167
#[tokio::test]
async fn a_document_created_during_the_rebinding_is_not_missed() {
    use std::time::Duration;

    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let first = published_form(&app, &token, "pr-race-1").await;
    let next = published_form(&app, &token, "pr-race-2").await;
    let id = bound_type(&app, &token, "RACED", first).await;

    // Hold the type's row by inserting a document that references it. The
    // foreign key takes `FOR KEY SHARE` on `document_types` for the life of
    // this transaction.
    let mut holding = app.pool.begin().await.expect("a transaction opens");
    sqlx::query(
        "INSERT INTO documents (id, tenant_id, document_ref, document_type_id, form_id, title)
         VALUES ($1, $2, 'DOC-2026-000099', $3, NULL, 'Created mid-rebinding')",
    )
    .bind(Uuid::now_v7())
    .bind(fixtures::SYSTEM_TENANT_ID)
    .bind(id)
    .execute(&mut *holding)
    .await
    .expect("the document inserts inside the transaction");

    // Bound to a `let` so the borrow outlives the future, which `app.send`
    // holds until it is awaited.
    let route = format!("/api/v1/document-types/{id}");
    let rebinding = app.send(
        Method::PUT,
        &route,
        Some(&token),
        Some(json!({ "formId": next })),
    );

    // Let the request reach the locked read before the insert commits. Without
    // a conflicting lock it will already have finished by now, with a 200.
    let committed = async {
        tokio::time::sleep(Duration::from_millis(250)).await;
        holding.commit().await.expect("the document commits");
    };

    let (response, ()) = tokio::time::timeout(
        Duration::from_secs(20),
        futures_lite_join(rebinding, committed),
    )
    .await
    .expect("the rebinding request must not block forever");

    assert_eq!(
        response.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a document that pinned no form arrived while this rebinding was being \
         written, so the rebinding must be refused rather than moving the \
         definition that document renders against. Body: {}",
        response.body
    );

    let bound: Uuid = sqlx::query_scalar("SELECT form_id FROM document_types WHERE id = $1")
        .bind(id)
        .fetch_one(&app.pool)
        .await
        .expect("the type is readable");

    assert_eq!(bound, first, "a refused rebinding writes nothing");
}

/// **The guard is scoped to the type being rebound** ([#218](https://github.com/sujanto-gaws/kelir/issues/218), predicate 5).
///
/// `count_documents_without_a_pinned_form` counts unpinned documents, and its
/// `document_type_id = $2` is what makes it count *this* type's. Defeat it and
/// any unpinned document anywhere in the tenant blocks every rebinding in it —
/// **D-30**'s guarantee widened into a refusal nobody can clear, because the
/// document that causes it belongs to a type the administrator is not editing
/// and may not even be able to see.
///
/// **The fixture is the fix.** Every test above builds a database holding one
/// document type, so *scoped by type* and *not scoped at all* produce identical
/// observations and no assertion over that database can tell them apart
/// (coding standard §2.9, the second-subject rule). This one puts the unpinned
/// document on a **second** type.
///
/// Seen red against `count_documents_without_a_pinned_form`'s
/// `document_type_id = $2` weakened to `(document_type_id = $2 OR TRUE)`: the
/// rebinding is refused with `DOCUMENTS_WITHOUT_A_PINNED_FORM`.
#[tokio::test]
async fn an_unpinned_document_of_another_type_does_not_block_this_rebinding() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let first = published_form(&app, &token, "pr-other-type-1").await;
    let next = published_form(&app, &token, "pr-other-type-2").await;

    let rebound = bound_type(&app, &token, "REBOUND_TYPE", first).await;
    let bystander = bound_type(&app, &token, "BYSTANDER_TYPE", first).await;

    // The unpinned document is the bystander's, and the rebinding is the other
    // type's. Nothing about this document is reachable from `rebound`'s
    // binding.
    seed_document(&app, bystander, "DOC-2026-000210", None).await;

    let updated = app
        .send(
            Method::PUT,
            &format!("/api/v1/document-types/{rebound}"),
            Some(&token),
            Some(json!({ "formId": next })),
        )
        .await;

    assert_eq!(
        updated.status,
        StatusCode::OK,
        "another type's unpinned document blocked this type's rebinding: {}",
        updated.body
    );

    let bound: Uuid = sqlx::query_scalar("SELECT form_id FROM document_types WHERE id = $1")
        .bind(rebound)
        .fetch_one(&app.pool)
        .await
        .expect("the type is readable");

    assert_eq!(bound, next, "the rebinding landed");
}

// ---------------------------------------------------------------------------
// The list, filtered by a bound workflow revision (#713, row 6b)
// ---------------------------------------------------------------------------
//
// The workflow editor's *Deprecate* warning names the document types still
// bound to the revision it is about to deprecate, in one call. A type is
// listed when it has a **live** binding row naming that revision: not
// soft-deleted, and `ACTIVE` — the two conditions `repository::workflow_binding`
// applies to the row itself. The validity window is not one of them: a binding
// whose window has not opened yet will still route to the revision, which is
// exactly what the warning is for.
//
// # Seen to fail (coding standard §2.9)
//
// Every test below was first run against the unchanged list, which ignored
// `workflowDefinitionId` as it ignores any unknown parameter: the eight
// asserting rows and totals got every type, the OpenAPI one found no such
// parameter, and the malformed id was a 200. The mutations were then made one
// at a time in `document_type::repository`, the binary run, and the mutation
// reverted. **Seen red, 2026-10-10.**
//
// | Mutation | Reddened |
// |---|---|
// | `AND w.deleted_at IS NULL` dropped from both statements | `a_retired_or_inactive_binding_does_not_count` |
// | `AND w.status = 'ACTIVE'` dropped from both statements | `a_retired_or_inactive_binding_does_not_count` |
// | `w.tenant_id = $1` replaced by `TRUE` in both statements | `another_tenants_binding_row_does_not_count` |
// | the filter neutered in `count_types` alone | six, among them `the_filter_pages_and_counts_the_same_rows` and `an_unknown_revision_is_an_empty_page_not_a_404` |
// | `AND w.workflow_definition_id = $4` dropped from both statements | `a_binding_to_another_revision_of_the_same_key_does_not_count`, `only_the_types_bound_to_the_revision_are_listed`, `an_unknown_revision_is_an_empty_page_not_a_404` |

/// A type created through the API, bound to each of `workflows` in order.
async fn type_bound_to(
    app: &TestApp,
    token: &str,
    code: &str,
    status: &str,
    workflows: &[Uuid],
) -> Uuid {
    let bindings: Vec<Value> = workflows
        .iter()
        .enumerate()
        .map(|(index, id)| json!({ "workflowDefinitionId": id, "priority": index + 1 }))
        .collect();

    let created = app
        .send(
            Method::POST,
            "/api/v1/document-types",
            Some(token),
            Some(json!({
                "typeCode": code,
                "name": code,
                "status": status,
                "workflows": bindings,
            })),
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    id_of(&created.body["data"])
}

/// The type codes on one page of the list at `query`, and `meta.total`.
async fn listed(app: &TestApp, token: &str, query: &str) -> (Vec<String>, u64) {
    let response = app
        .send(
            Method::GET,
            &format!("/api/v1/document-types?{query}"),
            Some(token),
            None,
        )
        .await;

    assert_eq!(
        response.status,
        StatusCode::OK,
        "{query}: {}",
        response.body
    );

    let codes = response.body["data"]
        .as_array()
        .expect("a page")
        .iter()
        .map(|row| row["typeCode"].as_str().expect("a code").to_owned())
        .collect();
    let total = response.body["meta"]["total"].as_u64().expect("a total");

    (codes, total)
}

#[tokio::test]
async fn only_the_types_bound_to_the_revision_are_listed() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let workflow = published_workflow(&app, &token, "wf_filter_bound").await;
    let elsewhere = published_workflow(&app, &token, "wf_filter_elsewhere").await;

    type_bound_to(&app, &token, "FLT_BOUND_A", "ACTIVE", &[workflow]).await;
    // Bound second, behind another workflow: any live row counts, not only
    // the one selection would pick first.
    type_bound_to(
        &app,
        &token,
        "FLT_BOUND_B",
        "ACTIVE",
        &[elsewhere, workflow],
    )
    .await;
    type_bound_to(&app, &token, "FLT_UNBOUND", "ACTIVE", &[elsewhere]).await;
    type_bound_to(&app, &token, "FLT_NOTHING", "ACTIVE", &[]).await;

    let (codes, total) = listed(&app, &token, &format!("workflowDefinitionId={workflow}")).await;

    assert_eq!(codes, ["FLT_BOUND_A", "FLT_BOUND_B"]);
    assert_eq!(total, 2);
}

#[tokio::test]
async fn a_binding_to_another_revision_of_the_same_key_does_not_count() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let first = published_workflow(&app, &token, "wf_filter_revised").await;

    let revision = app
        .send(
            Method::POST,
            &format!("/api/v1/workflow/definitions/{first}/revisions"),
            Some(&token),
            Some(json!({})),
        )
        .await;
    assert_eq!(revision.status, StatusCode::CREATED, "{}", revision.body);
    let second = id_of(&revision.body["data"]);

    let published = app
        .send(
            Method::POST,
            &format!("/api/v1/workflow/definitions/{second}/publication"),
            Some(&token),
            Some(json!({})),
        )
        .await;
    assert_eq!(published.status, StatusCode::OK, "{}", published.body);

    type_bound_to(&app, &token, "FLT_ON_FIRST", "ACTIVE", &[first]).await;
    type_bound_to(&app, &token, "FLT_ON_SECOND", "ACTIVE", &[second]).await;

    let (on_first, _) = listed(&app, &token, &format!("workflowDefinitionId={first}")).await;
    let (on_second, _) = listed(&app, &token, &format!("workflowDefinitionId={second}")).await;

    assert_eq!(on_first, ["FLT_ON_FIRST"], "a revision, not a workflow key");
    assert_eq!(
        on_second,
        ["FLT_ON_SECOND"],
        "a revision, not a workflow key"
    );
}

#[tokio::test]
async fn a_retired_or_inactive_binding_does_not_count() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let workflow = published_workflow(&app, &token, "wf_filter_dead").await;

    type_bound_to(&app, &token, "FLT_LIVE", "ACTIVE", &[workflow]).await;
    let retired = type_bound_to(&app, &token, "FLT_RETIRED", "ACTIVE", &[workflow]).await;
    let inactive = type_bound_to(&app, &token, "FLT_INACTIVE", "ACTIVE", &[workflow]).await;

    // Neither state has a route: the API replaces a type's bindings by hard
    // delete and inserts them `ACTIVE`. Both are still states §6.4 allows and
    // `workflow_binding` skips, so the filter must skip them too.
    sqlx::query(
        "UPDATE document_type_workflows SET deleted_at = now() WHERE document_type_id = $1",
    )
    .bind(retired)
    .execute(&app.pool)
    .await
    .expect("retire the binding");
    sqlx::query(
        "UPDATE document_type_workflows SET status = 'INACTIVE' WHERE document_type_id = $1",
    )
    .bind(inactive)
    .execute(&app.pool)
    .await
    .expect("deactivate the binding");

    let (codes, total) = listed(&app, &token, &format!("workflowDefinitionId={workflow}")).await;

    assert_eq!(codes, ["FLT_LIVE"]);
    assert_eq!(total, 1);
}

#[tokio::test]
async fn another_tenants_binding_row_does_not_count() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let workflow = published_workflow(&app, &token, "wf_filter_tenant").await;
    let other = fixtures::create_tenant(&app.pool, "TNT-DT-FILTER", "Filter tenant").await;

    type_bound_to(&app, &token, "FLT_MINE", "ACTIVE", &[workflow]).await;
    let unbound_here = type_bound_to(&app, &token, "FLT_NOT_MINE", "ACTIVE", &[]).await;

    // The other tenant's own type, bound to this revision...
    let theirs = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO document_types (id, tenant_id, type_code, name)
         VALUES ($1, $2, 'FLT_THEIRS', 'Theirs')",
    )
    .bind(theirs)
    .bind(other)
    .execute(&app.pool)
    .await
    .expect("insert the other tenant's type");

    // ...and a stray row filed under the other tenant that names *this*
    // tenant's type. Neither foreign key is composite, so the row is
    // writable; only `tenant_id` on the binding keeps it out.
    for type_id in [theirs, unbound_here] {
        sqlx::query(
            "INSERT INTO document_type_workflows
                 (id, tenant_id, document_type_id, workflow_definition_id)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(Uuid::now_v7())
        .bind(other)
        .bind(type_id)
        .bind(workflow)
        .execute(&app.pool)
        .await
        .expect("insert the other tenant's binding");
    }

    let (codes, total) = listed(&app, &token, &format!("workflowDefinitionId={workflow}")).await;

    assert_eq!(codes, ["FLT_MINE"]);
    assert_eq!(total, 1);
}

#[tokio::test]
async fn the_filter_composes_with_status_and_search() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let workflow = published_workflow(&app, &token, "wf_filter_status").await;

    type_bound_to(&app, &token, "FLT_ACTIVE_BOUND", "ACTIVE", &[workflow]).await;
    type_bound_to(&app, &token, "FLT_DRAFT_BOUND", "DRAFT", &[workflow]).await;
    type_bound_to(&app, &token, "FLT_DRAFT_OTHER", "DRAFT", &[workflow]).await;
    type_bound_to(&app, &token, "FLT_DRAFT_UNBOUND", "DRAFT", &[]).await;

    let (drafts, total) = listed(
        &app,
        &token,
        &format!("workflowDefinitionId={workflow}&status=DRAFT"),
    )
    .await;
    assert_eq!(drafts, ["FLT_DRAFT_BOUND", "FLT_DRAFT_OTHER"]);
    assert_eq!(total, 2);

    // `bound` is a substring of `FLT_DRAFT_UNBOUND` too: the search alone
    // would list it, and the binding filter is what keeps it out.
    let (searched, total) = listed(
        &app,
        &token,
        &format!("workflowDefinitionId={workflow}&status=DRAFT&search=bound"),
    )
    .await;
    assert_eq!(searched, ["FLT_DRAFT_BOUND"]);
    assert_eq!(total, 1);
}

#[tokio::test]
async fn the_filter_pages_and_counts_the_same_rows() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let workflow = published_workflow(&app, &token, "wf_filter_paging").await;

    for code in ["FLT_PAGE_1", "FLT_PAGE_2", "FLT_PAGE_3"] {
        type_bound_to(&app, &token, code, "ACTIVE", &[workflow]).await;
    }
    // Unbound rows sorting ahead of and between the bound ones, so a page
    // taken before the filter would hold the wrong rows.
    for code in ["FLT_PAGE_0", "FLT_PAGE_1A", "FLT_PAGE_2A"] {
        type_bound_to(&app, &token, code, "ACTIVE", &[]).await;
    }

    let filter = format!("workflowDefinitionId={workflow}");
    let (first, total) = listed(&app, &token, &format!("{filter}&page=1&pageSize=2")).await;
    assert_eq!(first, ["FLT_PAGE_1", "FLT_PAGE_2"]);
    assert_eq!(total, 3, "meta.total counts the filtered rows");

    let (second, total) = listed(&app, &token, &format!("{filter}&page=2&pageSize=2")).await;
    assert_eq!(second, ["FLT_PAGE_3"]);
    assert_eq!(total, 3);
}

#[tokio::test]
async fn an_unknown_revision_is_an_empty_page_not_a_404() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let workflow = published_workflow(&app, &token, "wf_filter_unknown").await;

    type_bound_to(&app, &token, "FLT_SOMETHING", "ACTIVE", &[workflow]).await;

    let (codes, total) = listed(
        &app,
        &token,
        &format!("workflowDefinitionId={}", Uuid::now_v7()),
    )
    .await;

    assert!(codes.is_empty(), "{codes:?}");
    assert_eq!(total, 0);
}

#[tokio::test]
async fn the_list_documents_its_workflow_filter() {
    let app = TestApp::spawn().await;
    let document = app
        .send(Method::GET, "/api/docs/openapi.json", None, None)
        .await;

    let parameter = document.body["paths"]["/api/v1/document-types"]["get"]["parameters"]
        .as_array()
        .expect("the list has parameters")
        .iter()
        .find(|parameter| parameter["name"] == "workflowDefinitionId")
        .cloned()
        .unwrap_or_else(|| panic!("workflowDefinitionId is not documented: {}", document.body));

    assert_eq!(parameter["in"], "query");
    assert_eq!(parameter["schema"]["format"], "uuid", "{parameter}");
}

#[tokio::test]
async fn a_revision_that_is_not_a_uuid_is_refused_by_name() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let response = app
        .send(
            Method::GET,
            "/api/v1/document-types?workflowDefinitionId=not-a-uuid",
            Some(&token),
            None,
        )
        .await;

    assert_eq!(
        response.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        response.body
    );
    assert_eq!(
        response.body["error"]["details"][0]["path"], "workflowDefinitionId",
        "{}",
        response.body
    );
}

// ---------------------------------------------------------------------------
// The workflow filter, adversarially (#713, test-engineer campaign 2026-10-10)
// ---------------------------------------------------------------------------
//
// The builder's nine tests above hold the predicates the filter adds. These
// hold the seams around them: rows the filter must not double, windows it must
// not consult, a revision of another tenant, the page cap the warning's "and N
// more" counts past, and the permission the warning's no-read path assumes.
//
// # Seen to fail (coding standard §2.9)
//
// Each mutation was made in `document_type::repository` (or the service), the
// binary built online against a throwaway database migrated to head, and the
// mutation reverted. **Seen red, 2026-10-10.** The builder's nine stayed green
// under five of the six; the tenant one reddens one of theirs as well.
//
// | Mutation | Reddened |
// |---|---|
// | `EXISTS` rewritten as a `LEFT JOIN` in `list_types` | `a_type_bound_twice_to_the_revision_is_listed_once` |
// | `count_types` joined to the bindings, counting rows | `a_type_bound_twice_to_the_revision_is_listed_once` |
// | the validity window added to both statements | `a_binding_outside_its_validity_window_still_counts` |
// | the revision's `deleted_at` added to both statements | `a_revision_soft_deleted_since_binding_still_lists_its_types` |
// | the caller's tenant dropped, the binding matched to the type's | `another_tenants_revision_lists_nothing_of_theirs`, `another_tenants_binding_row_does_not_count` |
// | `document-type:read` required only without the filter | `the_filter_opens_the_list_to_no_caller_without_document_type_read` |

/// A published workflow revision filed under `tenant`, inserted directly: the
/// API cannot write into a tenant the caller is not signed in to.
async fn foreign_revision(app: &TestApp, tenant: Uuid, key: &str) -> Uuid {
    let id = Uuid::now_v7();

    sqlx::query(
        "INSERT INTO workflow_definitions (id, tenant_id, workflow_key, name, jwss_version,
                                           definition_json, initial_state, status,
                                           published_at)
         VALUES ($1, $2, $3, 'Foreign approval', '1.0.0', '{}', 'draft', 'ACTIVE', now())",
    )
    .bind(id)
    .bind(tenant)
    .bind(key)
    .execute(&app.pool)
    .await
    .expect("insert the foreign revision");

    id
}

/// A binding row written directly, for the states the API never writes.
async fn insert_binding(app: &TestApp, tenant: Uuid, type_id: Uuid, workflow: Uuid, priority: i32) {
    sqlx::query(
        "INSERT INTO document_type_workflows
             (id, tenant_id, document_type_id, workflow_definition_id, priority)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(Uuid::now_v7())
    .bind(tenant)
    .bind(type_id)
    .bind(workflow)
    .bind(priority)
    .execute(&app.pool)
    .await
    .expect("insert the binding");
}

#[tokio::test]
async fn a_type_bound_twice_to_the_revision_is_listed_once() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let workflow = published_workflow(&app, &token, "wf_filter_twice").await;

    let twice = type_bound_to(&app, &token, "FLT_TWICE", "ACTIVE", &[workflow]).await;
    // A second live row naming the same revision, as a conditional binding
    // ahead of a default one would: nothing in the DDL makes the pair unique.
    insert_binding(&app, fixtures::SYSTEM_TENANT_ID, twice, workflow, 2).await;
    type_bound_to(&app, &token, "FLT_ONCE", "ACTIVE", &[workflow]).await;

    let stored: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM document_type_workflows
         WHERE document_type_id = $1 AND workflow_definition_id = $2",
    )
    .bind(twice)
    .bind(workflow)
    .fetch_one(&app.pool)
    .await
    .expect("count the bindings");
    assert_eq!(stored, 2, "the fixture holds two live rows");

    let (codes, total) = listed(&app, &token, &format!("workflowDefinitionId={workflow}")).await;

    assert_eq!(
        codes,
        ["FLT_ONCE", "FLT_TWICE"],
        "one row per type, not per binding"
    );
    assert_eq!(total, 2, "meta.total counts types, not bindings");
}

#[tokio::test]
async fn a_binding_outside_its_validity_window_still_counts() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let workflow = published_workflow(&app, &token, "wf_filter_window").await;

    let closed = type_bound_to(&app, &token, "FLT_WINDOW_CLOSED", "ACTIVE", &[workflow]).await;
    let pending = type_bound_to(&app, &token, "FLT_WINDOW_PENDING", "ACTIVE", &[workflow]).await;
    type_bound_to(&app, &token, "FLT_WINDOW_OPEN", "ACTIVE", &[workflow]).await;

    // A window that closed last year, and one that opens next year: neither
    // routes today (`workflow_binding`), and both still name the revision.
    sqlx::query(
        "UPDATE document_type_workflows
         SET valid_from = current_date - 400, valid_to = current_date - 365
         WHERE document_type_id = $1",
    )
    .bind(closed)
    .execute(&app.pool)
    .await
    .expect("close the window");
    sqlx::query(
        "UPDATE document_type_workflows
         SET valid_from = current_date + 365, valid_to = NULL
         WHERE document_type_id = $1",
    )
    .bind(pending)
    .execute(&app.pool)
    .await
    .expect("defer the window");

    let (codes, total) = listed(&app, &token, &format!("workflowDefinitionId={workflow}")).await;

    assert_eq!(
        codes,
        ["FLT_WINDOW_CLOSED", "FLT_WINDOW_OPEN", "FLT_WINDOW_PENDING"]
    );
    assert_eq!(total, 3);
}

#[tokio::test]
async fn a_binding_status_is_not_a_type_status_and_is_refused_by_name() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let workflow = published_workflow(&app, &token, "wf_filter_inactive").await;

    type_bound_to(&app, &token, "FLT_STATUS_BOUND", "ACTIVE", &[workflow]).await;

    // `INACTIVE` is the binding row's vocabulary; `status` filters the type.
    // The filter must not make the parameter mean the other one.
    let response = app
        .send(
            Method::GET,
            &format!("/api/v1/document-types?workflowDefinitionId={workflow}&status=INACTIVE"),
            Some(&token),
            None,
        )
        .await;

    assert_eq!(
        response.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        response.body
    );
    assert_eq!(
        response.body["error"]["details"][0]["path"], "status",
        "{}",
        response.body
    );

    let (deprecated, total) = listed(
        &app,
        &token,
        &format!("workflowDefinitionId={workflow}&status=DEPRECATED"),
    )
    .await;
    assert!(deprecated.is_empty(), "{deprecated:?}");
    assert_eq!(total, 0);
}

#[tokio::test]
async fn the_filter_caps_a_page_at_a_hundred_and_counts_past_it() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let workflow = published_workflow(&app, &token, "wf_filter_hundred").await;

    // 102 types, all but one bound: the warning names the first hundred and
    // says how many more there are, which is only right if `meta.total`
    // counts past the cap.
    sqlx::query(
        "INSERT INTO document_types (id, tenant_id, type_code, name, status)
         SELECT gen_random_uuid(), $1, 'FLT_CAP_' || lpad(i::text, 3, '0'), 'Capped ' || i,
                'ACTIVE'
         FROM generate_series(1, 102) AS i",
    )
    .bind(fixtures::SYSTEM_TENANT_ID)
    .execute(&app.pool)
    .await
    .expect("insert the types");
    sqlx::query(
        "INSERT INTO document_type_workflows
             (id, tenant_id, document_type_id, workflow_definition_id)
         SELECT gen_random_uuid(), tenant_id, id, $2
         FROM document_types
         WHERE tenant_id = $1 AND type_code LIKE 'FLT_CAP_%' AND type_code <> 'FLT_CAP_050'",
    )
    .bind(fixtures::SYSTEM_TENANT_ID)
    .bind(workflow)
    .execute(&app.pool)
    .await
    .expect("bind all but one");

    let response = app
        .send(
            Method::GET,
            &format!("/api/v1/document-types?workflowDefinitionId={workflow}&pageSize=500"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.body);

    let rows = response.body["data"].as_array().expect("a page");
    assert_eq!(rows.len(), 100, "a page holds at most a hundred");
    assert_eq!(
        response.body["meta"]["pageSize"], 100,
        "the clamped size is echoed"
    );
    assert_eq!(
        response.body["meta"]["total"], 101,
        "the total counts past the cap"
    );
    assert!(
        rows.iter().all(|row| row["typeCode"] != "FLT_CAP_050"),
        "the unbound type is not on the page"
    );

    let (rest, total) = listed(
        &app,
        &token,
        &format!("workflowDefinitionId={workflow}&page=2&pageSize=500"),
    )
    .await;
    assert_eq!(rest, ["FLT_CAP_102"]);
    assert_eq!(total, 101);
}

#[tokio::test]
async fn another_tenants_revision_lists_nothing_of_theirs() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let other = fixtures::create_tenant(&app.pool, "TNT-DT-FOREIGN", "Foreign tenant").await;
    let theirs = foreign_revision(&app, other, "wf_filter_foreign").await;
    let mine = published_workflow(&app, &token, "wf_filter_home").await;

    // Their type, bound to their revision, in their tenant: what the id would
    // reveal if the filter trusted it.
    let their_type = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO document_types (id, tenant_id, type_code, name)
         VALUES ($1, $2, 'FLT_FOREIGN', 'Foreign')",
    )
    .bind(their_type)
    .bind(other)
    .execute(&app.pool)
    .await
    .expect("insert their type");
    insert_binding(&app, other, their_type, theirs, 1).await;
    // And one of mine, bound to my own revision, so the page is not empty for
    // want of data.
    type_bound_to(&app, &token, "FLT_HOME", "ACTIVE", &[mine]).await;

    let response = app
        .send(
            Method::GET,
            &format!("/api/v1/document-types?workflowDefinitionId={theirs}"),
            Some(&token),
            None,
        )
        .await;

    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    assert_eq!(response.body["data"], json!([]), "{}", response.body);
    assert_eq!(response.body["meta"]["total"], 0, "{}", response.body);
    assert!(
        !response.body.to_string().contains("FLT_FOREIGN"),
        "nothing of theirs is echoed: {}",
        response.body
    );
}

#[tokio::test]
async fn a_revision_soft_deleted_since_binding_still_lists_its_types() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let workflow = published_workflow(&app, &token, "wf_filter_retired_rev").await;

    type_bound_to(&app, &token, "FLT_ON_RETIRED", "ACTIVE", &[workflow]).await;

    // No route soft-deletes a published revision; the row can still reach the
    // state. The binding is the type's and still names the revision, and
    // `workflow_binding` does not consult the revision's `deleted_at` either,
    // so the filter answers with what the binding says (characterization).
    sqlx::query("UPDATE workflow_definitions SET deleted_at = now() WHERE id = $1")
        .bind(workflow)
        .execute(&app.pool)
        .await
        .expect("retire the revision");

    let (codes, total) = listed(&app, &token, &format!("workflowDefinitionId={workflow}")).await;

    assert_eq!(codes, ["FLT_ON_RETIRED"]);
    assert_eq!(total, 1);
}

#[tokio::test]
async fn the_filter_opens_the_list_to_no_caller_without_document_type_read() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let workflow = published_workflow(&app, &token, "wf_filter_permission").await;

    type_bound_to(&app, &token, "FLT_GUARDED", "ACTIVE", &[workflow]).await;

    // The warning's no-read path: a deprecator who cannot read types.
    let role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "ROLE-DT-DEPRECATOR",
        &["workflow:definition:read", "workflow:definition:deprecate"],
    )
    .await;
    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "user.deprecator",
        "deprecator@kelir.test",
        "deprecator-only-password",
        &[role],
    )
    .await;
    let deprecator = app
        .sign_in("user.deprecator", "deprecator-only-password")
        .await;

    let response = app
        .send(
            Method::GET,
            &format!("/api/v1/document-types?workflowDefinitionId={workflow}"),
            Some(&deprecator),
            None,
        )
        .await;

    assert_eq!(response.status, StatusCode::FORBIDDEN, "{}", response.body);
    assert!(
        !response.body.to_string().contains("FLT_GUARDED"),
        "{}",
        response.body
    );
}

// ---------------------------------------------------------------------------
// A replacement built from a stale read (#728, D-117)
// ---------------------------------------------------------------------------
//
// `workflows` replaces the stored set wholesale, and the type-row lock only
// serialises writers: it cannot see that a writer's input was read before
// another writer's commit. So a client that read one binding and saved a
// one-element set deleted every binding added in between. `expectedWorkflows`
// is the set the client read; a stored set that differs is refused with a 409
// `DOCUMENT_TYPE_WORKFLOWS_CHANGED`, under the lock and before any write.

const WORKFLOWS_CHANGED: &str = "DOCUMENT_TYPE_WORKFLOWS_CHANGED";

/// The `UPDATE` audit rows written for a type.
async fn audited_updates(app: &TestApp, type_id: Uuid) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE object_id = $1 AND action = 'UPDATE'",
    )
    .bind(type_id)
    .fetch_one(&app.pool)
    .await
    .expect("audit rows are countable")
}

/// The revisions a wire `workflows` array binds, in its order.
fn revisions(workflows: &Value) -> Vec<String> {
    workflows
        .as_array()
        .expect("workflows is an array")
        .iter()
        .map(|binding| {
            binding["workflowDefinitionId"]
                .as_str()
                .expect("a revision")
                .to_owned()
        })
        .collect()
}

fn assert_workflows_changed(response: &common::TestResponse, label: &str) {
    assert_eq!(
        response.status,
        StatusCode::CONFLICT,
        "{label}: {}",
        response.body
    );
    assert_eq!(
        response.body["error"]["code"], WORKFLOWS_CHANGED,
        "{label}: {}",
        response.body
    );
}

/// **AC7, sequential.** A and B both read a type bound to one workflow. B saves
/// two bindings. A then saves the one-element set its dialog built, with its
/// read as `expectedWorkflows`, and is refused: B's two bindings are intact.
///
/// # Seen to fail
///
/// 2026-10-16, against `update_type` with the comparison mutated out (its
/// condition `!same_bindings(expected, stored)` made `... && false`, so the
/// refusal is never taken): A's stale save was accepted, 200 where 409 was
/// asserted. A 200 here is the replace having run over B's two bindings, which
/// is #728 itself. Restored, green.
#[tokio::test]
async fn a_stale_replacement_is_refused_and_the_newer_set_survives() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let first = published_workflow(&app, &token, "wf_stale_first").await;
    let second = published_workflow(&app, &token, "wf_stale_second").await;
    let id = type_bound_to(&app, &token, "STALE_SEQUENTIAL", "ACTIVE", &[first]).await;

    let read_by_a = read_type(&app, &token, id).await["workflows"].clone();
    let read_by_b = read_type(&app, &token, id).await["workflows"].clone();

    let by_b = put_type(
        &app,
        &token,
        id,
        json!({
            "workflows": [
                { "workflowDefinitionId": first, "priority": 1 },
                { "workflowDefinitionId": second, "priority": 2 }
            ],
            "expectedWorkflows": read_by_b,
        }),
    )
    .await;
    assert_eq!(by_b.status, StatusCode::OK, "{}", by_b.body);
    let saved_by_b = by_b.body["data"]["workflows"].clone();
    assert_eq!(
        saved_by_b.as_array().map(Vec::len),
        Some(2),
        "{}",
        by_b.body
    );

    // A's dialog moved the chooser to `second` and built one element from its
    // read, which saw only `first`.
    let by_a = put_type(
        &app,
        &token,
        id,
        json!({
            "workflows": [{ "workflowDefinitionId": second, "priority": 1 }],
            "expectedWorkflows": read_by_a,
        }),
    )
    .await;

    assert_workflows_changed(&by_a, "a save built from a read B has moved past");
    let message = by_a.body["error"]["message"].as_str().expect("a message");
    assert!(
        message.contains("changed since they were read") && message.contains("Reload"),
        "the message says what happened and what to do: {message}"
    );

    let read = read_type(&app, &token, id).await;
    assert_eq!(read["workflows"], saved_by_b, "B's set is intact: {read}");
}

/// **AC7, concurrent.** Two saves from one read, under `tokio::join!`. The type
/// row's lock serialises them, and the second to take it reads the first's set,
/// which is not the one it read: exactly one is accepted, and the stored set is
/// the winner's. The order the lock grants is not asserted, only that one wins.
///
/// # Seen to fail
///
/// 2026-10-16, against the same mutation as the sequential test: both saves
/// were accepted, so the one taken as the loser answered 200 where 409 was
/// asserted, and the second to commit had replaced the first's set unseen.
/// Restored, green.
#[tokio::test]
async fn two_saves_from_one_read_race_and_exactly_one_wins() {
    use std::time::Duration;

    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let shared = published_workflow(&app, &token, "wf_race_shared").await;
    let for_a = published_workflow(&app, &token, "wf_race_for_a").await;
    let for_b = published_workflow(&app, &token, "wf_race_for_b").await;
    let id = type_bound_to(&app, &token, "STALE_CONCURRENT", "ACTIVE", &[shared]).await;

    let read = read_type(&app, &token, id).await["workflows"].clone();

    // Bound to a `let` so the borrow outlives the futures, which `app.send`
    // holds until they are awaited.
    let route = format!("/api/v1/document-types/{id}");
    let body = |added: Uuid| {
        json!({
            "workflows": [
                { "workflowDefinitionId": shared, "priority": 1 },
                { "workflowDefinitionId": added, "priority": 2 }
            ],
            "expectedWorkflows": read.clone(),
        })
    };

    let by_a = app.send(Method::PUT, &route, Some(&token), Some(body(for_a)));
    let by_b = app.send(Method::PUT, &route, Some(&token), Some(body(for_b)));

    let (by_a, by_b) = tokio::time::timeout(Duration::from_secs(20), futures_lite_join(by_a, by_b))
        .await
        .expect("neither save may block forever");

    let (winner, loser, added) = match (by_a.status, by_b.status) {
        (StatusCode::OK, _) => (by_a, by_b, for_a),
        (_, StatusCode::OK) => (by_b, by_a, for_b),
        _ => panic!(
            "one of two saves from one read must win: A {} {}, B {} {}",
            by_a.status, by_a.body, by_b.status, by_b.body
        ),
    };

    assert_workflows_changed(&loser, "the second save to take the lock");

    let stored = read_type(&app, &token, id).await["workflows"].clone();
    assert_eq!(stored, winner.body["data"]["workflows"], "{stored}");
    assert_eq!(
        revisions(&stored),
        [shared.to_string(), added.to_string()],
        "the stored set is the winner's"
    );
}

/// **AC2: a refused save writes nothing.** The `PUT` also renames the type and
/// deprecates it. The comparison runs before the scalar `UPDATE`, so a GET
/// after it shows the name, status, `updatedAt` and bindings as they were, and
/// no audit entry was written.
///
/// # Seen to fail
///
/// 2026-10-16, against the comparison mutated out: the stale rename was
/// accepted, 200 where 409 was asserted. A 409 rolls the transaction
/// back, so this test cannot tell a check placed before the scalar `UPDATE`
/// from one placed after it in the same transaction; that placement is held by
/// `update_type`'s structure and its comment, and what this test holds is that
/// a refusal leaves no trace.
#[tokio::test]
async fn a_refused_save_writes_nothing_not_even_the_name() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let first = published_workflow(&app, &token, "wf_refused_first").await;
    let second = published_workflow(&app, &token, "wf_refused_second").await;
    let id = type_bound_to(&app, &token, "STALE_REFUSED", "ACTIVE", &[first, second]).await;

    let before = read_type(&app, &token, id).await;
    let audited = audited_updates(&app, id).await;

    // A read that saw only the first binding.
    let stale = json!([before["workflows"][0].clone()]);

    let refused = put_type(
        &app,
        &token,
        id,
        json!({
            "name": "Renamed while stale",
            "status": "DEPRECATED",
            "workflows": [{ "workflowDefinitionId": second, "priority": 1 }],
            "expectedWorkflows": stale,
        }),
    )
    .await;

    assert_workflows_changed(&refused, "a stale save that also renames");

    let after = read_type(&app, &token, id).await;
    assert_eq!(after["name"], before["name"], "{after}");
    assert_eq!(after["status"], before["status"], "{after}");
    assert_eq!(after["updatedAt"], before["updatedAt"], "{after}");
    assert_eq!(after["workflows"], before["workflows"], "{after}");
    assert_eq!(after, before, "every field is as it was");
    assert_eq!(
        audited_updates(&app, id).await,
        audited,
        "a refused save is not audited"
    );
}

/// **AC4: a current save works as today.** With `expectedWorkflows` equal to
/// the stored set, the `PUT` replaces the set, answers 200 with the new one,
/// and is audited.
#[tokio::test]
async fn a_current_save_replaces_the_set_and_is_audited() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let first = published_workflow(&app, &token, "wf_current_first").await;
    let second = published_workflow(&app, &token, "wf_current_second").await;
    let id = type_bound_to(&app, &token, "STALE_CURRENT", "ACTIVE", &[first]).await;

    let read = read_type(&app, &token, id).await;
    let audited = audited_updates(&app, id).await;

    let saved = put_type(
        &app,
        &token,
        id,
        json!({
            "name": "Saved while current",
            "workflows": [{ "workflowDefinitionId": second, "priority": 1 }],
            "expectedWorkflows": read["workflows"],
        }),
    )
    .await;

    assert_eq!(saved.status, StatusCode::OK, "{}", saved.body);
    assert_eq!(
        revisions(&saved.body["data"]["workflows"]),
        [second.to_string()]
    );
    assert_eq!(saved.body["data"]["name"], "Saved while current");
    assert_eq!(
        read_type(&app, &token, id).await["workflows"],
        saved.body["data"]["workflows"]
    );
    assert_eq!(audited_updates(&app, id).await, audited + 1);
}

/// **AC3 on the wire.** Order does not matter, one field on one binding does,
/// and `[]` matches a type with no bindings.
///
/// The first binding carries a condition and a window, so the read sent back
/// has every field set; the mismatch edits only its condition.
///
/// # Seen to fail
///
/// 2026-10-16, red against both mutations. With the comparison mutated out,
/// the one-condition mismatch was accepted, 200 where 409 was asserted. With
/// it made positional (`expected == stored` in place of `same_bindings`), the
/// read sent back in reverse order was refused, 409 where 200 was asserted.
/// Restored, green.
#[tokio::test]
async fn the_comparison_ignores_order_and_sees_one_field() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let first = published_workflow(&app, &token, "wf_compare_first").await;
    let second = published_workflow(&app, &token, "wf_compare_second").await;
    let condition = r#"{"<=":[{"var":"amount"},10000000]}"#;
    let bindings = json!([
        {
            "workflowDefinitionId": first,
            "conditionExpression": condition,
            "priority": 1,
            "validFrom": "2026-01-01",
            "validTo": "2026-12-31"
        },
        { "workflowDefinitionId": second, "priority": 2 }
    ]);

    let created = app
        .send(
            Method::POST,
            "/api/v1/document-types",
            Some(&token),
            Some(json!({
                "typeCode": "STALE_COMPARED",
                "name": "Compared",
                "workflows": bindings,
            })),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let id = id_of(&created.body["data"]);

    let read = read_type(&app, &token, id).await["workflows"].clone();
    let reversed = json!([read[1].clone(), read[0].clone()]);

    // One field on one binding: the condition of the first.
    let mut edited = read.clone();
    edited[0]["conditionExpression"] = json!(r#"{"<=":[{"var":"amount"},5000000]}"#);

    let refused = put_type(
        &app,
        &token,
        id,
        json!({ "workflows": read.clone(), "expectedWorkflows": edited }),
    )
    .await;
    assert_workflows_changed(&refused, "one condition differs");

    // The same set read in another order is the set stored. Clearing it
    // leaves a type with none.
    let cleared = put_type(
        &app,
        &token,
        id,
        json!({ "workflows": [], "expectedWorkflows": reversed }),
    )
    .await;
    assert_eq!(cleared.status, StatusCode::OK, "{}", cleared.body);
    assert_eq!(cleared.body["data"]["workflows"], json!([]));

    // `[]` read is the empty set stored.
    let rebound = put_type(
        &app,
        &token,
        id,
        json!({
            "workflows": [{ "workflowDefinitionId": second, "priority": 1 }],
            "expectedWorkflows": [],
        }),
    )
    .await;
    assert_eq!(rebound.status, StatusCode::OK, "{}", rebound.body);

    // And `[]` is not a set that has a binding.
    let refused = put_type(
        &app,
        &token,
        id,
        json!({ "workflows": [], "expectedWorkflows": [] }),
    )
    .await;
    assert_workflows_changed(&refused, "an empty read against one binding");
}

/// **AC5: compatibility.** A `PUT` with `workflows` and no `expectedWorkflows`,
/// or with it `null`, replaces whatever is stored, as before #728: such a
/// client is not compared, and so is not protected.
#[tokio::test]
async fn a_save_without_expected_workflows_replaces_whatever_is_stored() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let first = published_workflow(&app, &token, "wf_compat_first").await;
    let second = published_workflow(&app, &token, "wf_compat_second").await;
    let id = type_bound_to(&app, &token, "STALE_COMPATIBLE", "ACTIVE", &[first, second]).await;

    let absent = put_type(
        &app,
        &token,
        id,
        json!({ "workflows": [{ "workflowDefinitionId": second, "priority": 1 }] }),
    )
    .await;
    assert_eq!(absent.status, StatusCode::OK, "{}", absent.body);
    assert_eq!(
        revisions(&absent.body["data"]["workflows"]),
        [second.to_string()]
    );

    let null = put_type(
        &app,
        &token,
        id,
        json!({
            "workflows": [{ "workflowDefinitionId": first, "priority": 1 }],
            "expectedWorkflows": null,
        }),
    )
    .await;
    assert_eq!(null.status, StatusCode::OK, "{}", null.body);
    assert_eq!(
        revisions(&null.body["data"]["workflows"]),
        [first.to_string()]
    );
}

/// **AC6: misuse.** `expectedWorkflows` without `workflows` guards nothing, so
/// it is a 422 at its own path, and the rename sent with it is not written.
#[tokio::test]
async fn expected_workflows_without_workflows_is_refused_and_writes_nothing() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let workflow = published_workflow(&app, &token, "wf_misused").await;
    let id = type_bound_to(&app, &token, "STALE_MISUSED", "ACTIVE", &[workflow]).await;

    let before = read_type(&app, &token, id).await;
    let audited = audited_updates(&app, id).await;

    let refused = put_type(
        &app,
        &token,
        id,
        json!({
            "name": "Renamed with a precondition and nothing to guard",
            "expectedWorkflows": before["workflows"],
        }),
    )
    .await;

    assert_refused_at(
        &refused,
        "expectedWorkflows",
        "REQUIRES_WORKFLOWS",
        "expectedWorkflows without workflows",
    );
    assert_eq!(read_type(&app, &token, id).await, before);
    assert_eq!(audited_updates(&app, id).await, audited);
}

/// **AC13: the 409 is documented** on the update, with its code.
#[tokio::test]
async fn the_update_documents_its_stale_workflows_refusal() {
    let app = TestApp::spawn().await;
    let document = app
        .send(Method::GET, "/api/docs/openapi.json", None, None)
        .await;

    let update = &document.body["paths"]["/api/v1/document-types/{id}"]["put"];
    let conflict = update["responses"]["409"]["description"]
        .as_str()
        .unwrap_or_else(|| panic!("the update documents no 409: {update}"));
    assert!(conflict.contains(WORKFLOWS_CHANGED), "{conflict}");

    let schema = &document.body["components"]["schemas"]["UpdateDocumentTypeRequest"];
    let field = &schema["properties"]["expectedWorkflows"];
    assert!(
        field["description"]
            .as_str()
            .is_some_and(|text| text.contains(WORKFLOWS_CHANGED)),
        "expectedWorkflows is described: {schema}"
    );
}

// ---------------------------------------------------------------------------
// #728's verification campaign (test-engineer, 2026-10-16): the round trip a
// client makes, and the lock under an arranged race rather than a hoped one.
//
// Seen to fail (coding standard 2.9), 2026-10-16: fifteen mutants, switched
// in one build by an environment variable and run against the `document_type`
// unit tests and this file. A clean run was 37 and 54 green.
//
// - Killed by this file and the unit tests: the comparison skipped; made
//   positional; made a set, not a multiset (unit tests only); each of the
//   five fields ignored (`workflowDefinitionId`, `priority`, `validFrom` and
//   `validTo` by the unit tests only; `conditionExpression` by both); the
//   length check dropped; the 422 for `expectedWorkflows` alone dropped; the
//   check skipped for an empty `workflows`; the 409 given a generic code.
// - **Killed only by the arranged race below**: the stored set read before
//   `lock_type_binding`, and `lock_type_binding` without `FOR UPDATE`.
//   `two_saves_from_one_read_race_and_exactly_one_wins` stayed green under
//   both, so that test is not what holds the lock.
// - Survived, and equivalent: the comparison moved after the scalar `UPDATE`.
//   A 409 rolls the whole transaction back, so no request can tell the two
//   places apart.
// ---------------------------------------------------------------------------

/// **The riskiest property of #728: a client echoing exactly what it read is
/// never refused.** The comparison holds the stored set as `update_type`
/// deserialises it; a client holds what the GET serialised from the same rows.
/// If the two ever differed, in a condition's text, a date's form, a default
/// priority or a null against an absent key, every honest save would be a
/// false 409.
///
/// So every field is set to something a normalisation would change: a
/// condition with spacing and key order no serializer would produce, both date
/// bounds, and priorities that are not the default. The read goes back as
/// `expectedWorkflows` in four forms: as read, reversed, with its nulls and
/// default priority left out, and as the previous `PUT` answered it.
///
/// It also pins that the condition is compared as the stored text, which is
/// what a GET returns: the same JSON re-serialised is a different condition,
/// as it would be if it were sent as `workflows` and stored.
#[tokio::test]
async fn a_read_sent_back_unchanged_is_the_stored_set() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let first = published_workflow(&app, &token, "wf_echo_first").await;
    let second = published_workflow(&app, &token, "wf_echo_second").await;
    let third = published_workflow(&app, &token, "wf_echo_third").await;
    let condition =
        "{ \"<=\" :  [ { \"var\" : \"formData.amount\" } , 10000000.50 ] ,\n \"_\": \"é\" }";

    let created = app
        .send(
            Method::POST,
            "/api/v1/document-types",
            Some(&token),
            Some(json!({
                "typeCode": "ECHOED",
                "name": "Echoed",
                "workflows": [
                    {
                        "workflowDefinitionId": first,
                        "conditionExpression": condition,
                        "priority": 7,
                        "validFrom": "2026-02-28",
                        "validTo": "2026-12-31"
                    },
                    { "workflowDefinitionId": second, "priority": 30, "validTo": "2027-01-01" },
                    { "workflowDefinitionId": third }
                ],
            })),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let id = id_of(&created.body["data"]);

    let read = read_type(&app, &token, id).await["workflows"].clone();
    let stored_condition = read
        .as_array()
        .expect("an array")
        .iter()
        .find(|binding| binding["priority"] == 7)
        .map(|binding| binding["conditionExpression"].clone());
    assert_eq!(
        stored_condition,
        Some(json!(condition)),
        "the condition is stored and read back as sent: {read}"
    );

    // As read, with an unrelated edit beside it.
    let echoed = put_type(
        &app,
        &token,
        id,
        json!({ "name": "Echoed once", "workflows": read.clone(), "expectedWorkflows": read.clone() }),
    )
    .await;
    assert_eq!(echoed.status, StatusCode::OK, "as read: {}", echoed.body);

    // As the `PUT` answered it, in reverse.
    let answered = echoed.body["data"]["workflows"].clone();
    let mut reversed = answered.as_array().expect("an array").clone();
    reversed.reverse();
    let echoed = put_type(
        &app,
        &token,
        id,
        json!({ "workflows": read.clone(), "expectedWorkflows": reversed }),
    )
    .await;
    assert_eq!(
        echoed.status,
        StatusCode::OK,
        "as answered, reversed: {}",
        echoed.body
    );

    // With every null and the default priority left off the wire.
    let sparse: Vec<Value> = read
        .as_array()
        .expect("an array")
        .iter()
        .map(|binding| {
            let mut sparse = binding.as_object().expect("an object").clone();
            sparse.retain(|key, value| !(value.is_null() || (key == "priority" && *value == 1)));
            Value::Object(sparse)
        })
        .collect();
    assert!(
        sparse
            .iter()
            .any(|binding| binding.get("priority").is_none()),
        "one binding goes without its default priority: {sparse:?}"
    );
    let echoed = put_type(
        &app,
        &token,
        id,
        json!({ "workflows": read.clone(), "expectedWorkflows": sparse }),
    )
    .await;
    assert_eq!(
        echoed.status,
        StatusCode::OK,
        "nulls and the default left out: {}",
        echoed.body
    );

    // The same condition re-serialised compactly is not the stored text.
    let mut reserialised = read.clone();
    for binding in reserialised.as_array_mut().expect("an array") {
        if binding["priority"] == 7 {
            let parsed: Value = serde_json::from_str(condition).expect("the condition is JSON");
            binding["conditionExpression"] = json!(parsed.to_string());
        }
    }
    let refused = put_type(
        &app,
        &token,
        id,
        json!({ "workflows": read.clone(), "expectedWorkflows": reserialised }),
    )
    .await;
    assert_workflows_changed(&refused, "the condition re-serialised");

    assert_eq!(read_type(&app, &token, id).await["workflows"], read);
}

/// **AC2 and AC7: the comparison reads the set under the lock**, held by an
/// arranged race rather than a hoped one.
///
/// `two_saves_from_one_read_race_and_exactly_one_wins` fires two requests and
/// asserts on whatever interleaving the scheduler gives. A comparison made
/// outside the lock is red there only when both requests read before either
/// commits, which the scheduler does not promise. Here a transaction holds the
/// type row and adds a binding, uncommitted, and the save is let go once it is
/// seen queued in `pg_locks`. A save that read the set before taking the lock
/// read the set without that binding, and is accepted, deleting it; one that
/// reads after the lock sees it and is refused.
///
/// # Seen to fail (coding standard 2.9)
///
/// 2026-10-16: red against the stored set read before `lock_type_binding`,
/// and against `lock_type_binding` without `FOR UPDATE`; the only test in this
/// file that failed under either. Restored, green.
#[tokio::test]
async fn a_save_queued_on_the_lock_compares_against_the_set_committed_ahead_of_it() {
    use std::time::Duration;

    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let first = published_workflow(&app, &token, "wf_queued_first").await;
    let added = published_workflow(&app, &token, "wf_queued_added").await;
    let id = type_bound_to(&app, &token, "STALE_QUEUED", "ACTIVE", &[first]).await;

    let read = read_type(&app, &token, id).await["workflows"].clone();

    // Another writer, mid-transaction: it holds the type row as `update_type`
    // does, and has added a binding it has not committed.
    let mut holder = app.pool.begin().await.expect("a transaction opens");
    sqlx::query("SELECT id FROM document_types WHERE id = $1 FOR UPDATE")
        .bind(id)
        .execute(&mut *holder)
        .await
        .expect("holds the type row");
    sqlx::query(
        "INSERT INTO document_type_workflows
            (id, tenant_id, document_type_id, workflow_definition_id, priority)
         VALUES ($1, $2, $3, $4, 2)",
    )
    .bind(Uuid::now_v7())
    .bind(fixtures::SYSTEM_TENANT_ID)
    .bind(id)
    .bind(added)
    .execute(&mut *holder)
    .await
    .expect("adds a binding, uncommitted");

    let route = format!("/api/v1/document-types/{id}");
    let save = app.send(
        Method::PUT,
        &route,
        Some(&token),
        Some(json!({
            "workflows": [{ "workflowDefinitionId": first, "priority": 3 }],
            "expectedWorkflows": read,
        })),
    );

    let release = async {
        // Commit once the save is queued on a lock. If it never queues, commit
        // after ten seconds anyway, and the assertions say what it did.
        for _ in 0..200 {
            let waiting: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM pg_locks WHERE NOT granted \
                 AND pid IN (SELECT pid FROM pg_stat_activity WHERE datname = current_database())",
            )
            .fetch_one(&app.pool)
            .await
            .expect("reads pg_locks");
            if waiting > 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        holder.commit().await.expect("the other writer commits");
    };

    let (saved, ()) =
        tokio::time::timeout(Duration::from_secs(20), futures_lite_join(save, release))
            .await
            .expect("the save must not block forever");

    assert_workflows_changed(&saved, "a save queued behind a writer that added a binding");

    let stored = read_type(&app, &token, id).await["workflows"].clone();
    assert_eq!(
        revisions(&stored),
        [first.to_string(), added.to_string()],
        "the binding committed ahead of the save survives: {stored}"
    );
}
