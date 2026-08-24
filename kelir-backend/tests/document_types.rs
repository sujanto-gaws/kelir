//! Document types and their bindings, through the API (#157).
//!
//! The binding checks are what this file mostly exists for. A type that names a
//! form which does not exist, or one that is still a draft, is a type whose
//! documents cannot be rendered — and the second is the subtler of the two,
//! because the form is right there and merely unfinished.

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

#[tokio::test]
async fn a_type_is_created_with_its_bindings_and_read_back() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let form = published_form(&app, &token, "pr-type-form").await;
    let workflow = Uuid::now_v7();

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

/// A workflow binding names a table that does not exist yet, and is stored as
/// given.
///
/// Worth asserting rather than assuming: it is the one reference on this
/// aggregate that nothing can check, and a future reader should find that
/// recorded rather than discover it.
#[tokio::test]
async fn a_workflow_binding_is_stored_unverified_until_phase_5() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let invented = Uuid::now_v7();

    let created = app
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
        created.status,
        StatusCode::CREATED,
        "workflow_definitions does not exist until 0016; the binding is stored \
         as given and the foreign key arrives with that migration. Body: {}",
        created.body
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
                    { "workflowDefinitionId": Uuid::now_v7(), "priority": 1 },
                    { "workflowDefinitionId": Uuid::now_v7(), "priority": 2 }
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
                "workflows": [{ "workflowDefinitionId": Uuid::now_v7(), "priority": 5 }]
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
