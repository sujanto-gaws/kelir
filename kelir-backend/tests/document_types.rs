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
