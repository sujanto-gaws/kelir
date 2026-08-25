//! Form and list definition storage, through the API (#156).
//!
//! What is asserted here rather than in the domain's unit tests is everything
//! that needs a database and a request: that a refusal is a refusal *at the
//! endpoint*, that a published revision cannot be edited through any route, and
//! that an update's audit record says what changed rather than what was asked
//! for.

mod common;

use axum::http::{Method, StatusCode};
use common::TestApp;
use serde_json::{json, Value};
use uuid::Uuid;

fn definition(form_id: &str) -> Value {
    json!({
        "formId": form_id,
        "version": "2.0.1",
        "title": "Purchase requisition",
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

/// The registry §6.1 invoice, which uses every operator worth exercising.
fn definition_with_invoice_total(form_id: &str) -> Value {
    json!({
        "formId": form_id,
        "version": "2.0.1",
        "components": [{
            "id": "total",
            "role": "data",
            "type": "number",
            "key": "total",
            "label": "Total",
            "validation": { "type": "number" },
            "calculate": {
                "sum": [{"map": [
                    {"var": "items"},
                    {"*": [{"var": "unit_price"}, {"var": "quantity"}]}
                ]}]
            }
        }]
    })
}

async fn create_form(app: &TestApp, token: &str, key: &str, definition: Value) -> Value {
    let response = app
        .send(
            Method::POST,
            "/api/v1/rad/forms",
            Some(token),
            Some(json!({
                "formKey": key,
                "title": "Purchase requisition",
                "definition": definition,
            })),
        )
        .await;

    assert_eq!(
        response.status,
        StatusCode::CREATED,
        "creating {key} failed: {}",
        response.body
    );

    response.body["data"].clone()
}

fn id_of(form: &Value) -> Uuid {
    form["id"]
        .as_str()
        .expect("the response carries an id")
        .parse()
        .expect("the id is a uuid")
}

#[tokio::test]
async fn a_form_is_created_read_back_and_listed() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let created = create_form(&app, &token, "pr-basic", definition("pr-basic")).await;

    assert_eq!(created["revision"], 1, "a create is revision 1");
    assert_eq!(created["status"], "DRAFT", "a create is a draft");
    assert_eq!(
        created["jfssVersion"], "2.0.1",
        "the spec version is read out of the document, not assumed"
    );

    let id = id_of(&created);
    let read = app
        .send(
            Method::GET,
            &format!("/api/v1/rad/forms/{id}"),
            Some(&token),
            None,
        )
        .await;

    assert_eq!(read.status, StatusCode::OK);
    assert_eq!(
        read.body["data"]["definition"],
        definition("pr-basic"),
        "the document comes back as it went in"
    );

    let listed = app
        .send(Method::GET, "/api/v1/rad/forms", Some(&token), None)
        .await;

    assert_eq!(listed.status, StatusCode::OK);
    assert!(
        listed.body["data"][0]["definition"].is_null(),
        "a page of forms must not carry the documents; a page of twenty would be \
         twenty JFSS trees to render a table of titles"
    );
}

#[tokio::test]
async fn a_definition_that_is_not_jfss_is_refused_rather_than_stored() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let response = app
        .send(
            Method::POST,
            "/api/v1/rad/forms",
            Some(&token),
            Some(json!({
                "formKey": "not-jfss",
                "title": "Not JFSS",
                "definition": { "components": "this is not an array" },
            })),
        )
        .await;

    assert_eq!(
        response.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "body {}",
        response.body
    );
    assert_eq!(response.body["error"]["code"], "VALIDATION_ERROR");

    // And nothing was stored. A refusal that writes first is not a refusal.
    let stored: i64 =
        sqlx::query_scalar("SELECT count(*) FROM rad_forms WHERE form_key = 'not-jfss'")
            .fetch_one(&app.pool)
            .await
            .expect("count is queryable");

    assert_eq!(stored, 0);
}

/// The registry's "not in this registry, therefore FORBIDDEN" rule, at the
/// endpoint.
///
/// `datetime` is a real operator in the adopted engine and appears in no
/// registry, so it would evaluate identically on both sides — which is the
/// point. Parity is not governance, and without this check the engine's whole
/// proprietary surface is reachable from a stored schema.
#[tokio::test]
async fn an_operator_the_engine_supports_and_no_registry_approves_is_refused() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let mut document = definition("unregistered");
    document["components"][0]["calculate"] = json!({"datetime": ["2026-08-25"]});

    let response = app
        .send(
            Method::POST,
            "/api/v1/rad/forms",
            Some(&token),
            Some(json!({
                "formKey": "unregistered",
                "title": "Unregistered",
                "definition": document,
            })),
        )
        .await;

    assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);

    let codes: Vec<&str> = response.body["error"]["details"]
        .as_array()
        .expect("details")
        .iter()
        .map(|detail| detail["code"].as_str().unwrap_or_default())
        .collect();

    assert!(
        codes.contains(&"OPERATOR_NOT_REGISTERED"),
        "the refusal must name the reason, not merely fail; got {codes:?}"
    );
}

#[tokio::test]
async fn the_registry_invoice_calculation_is_accepted() {
    // The other half of the check above: an approved operator set, including
    // the custom `sum`, goes in unmolested.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let created = create_form(
        &app,
        &token,
        "pr-invoice",
        definition_with_invoice_total("pr-invoice"),
    )
    .await;

    assert_eq!(created["status"], "DRAFT");
}

/// A published revision is immutable, and **two layers hold that**.
///
/// The service reads the row and refuses a published one; the `UPDATE`
/// statement also carries `AND status = 'DRAFT'`, so a publish landing between
/// the read and the write affects no rows and is refused too. That is
/// deliberate — the check-then-act window is real — and it means a mutation of
/// **either layer alone leaves this test green**, which is measured rather than
/// assumed:
///
/// | Mutation | Result |
/// |---|---|
/// | service check removed | green — the statement predicate catches it |
/// | statement predicate removed | green — the service check catches it |
/// | both removed | **red**, here |
///
/// So this test asserts the *behaviour*, and neither layer is redundant. A
/// future edit that deletes one of them and sees green has not proved the
/// other is unnecessary; it has proved this test still works.
#[tokio::test]
async fn a_published_revision_cannot_be_edited() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let created = create_form(&app, &token, "pr-published", definition("pr-published")).await;
    let id = id_of(&created);

    let published = app
        .send(
            Method::POST,
            &format!("/api/v1/rad/forms/{id}/publish"),
            Some(&token),
            None,
        )
        .await;

    assert_eq!(published.status, StatusCode::OK, "body {}", published.body);
    assert_eq!(published.body["data"]["status"], "PUBLISHED");
    assert!(
        !published.body["data"]["publishedAt"].is_null(),
        "a published revision carries the stamp its immutability rule keys on"
    );

    let edit = app
        .send(
            Method::PUT,
            &format!("/api/v1/rad/forms/{id}"),
            Some(&token),
            Some(json!({ "title": "Edited after publication" })),
        )
        .await;

    assert_eq!(
        edit.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a document pins the revision it was created against; body {}",
        edit.body
    );

    // And the row did not move.
    let title: String = sqlx::query_scalar("SELECT title FROM rad_forms WHERE id = $1")
        .bind(id)
        .fetch_one(&app.pool)
        .await
        .expect("the form is queryable");

    assert_eq!(title, "Purchase requisition");
}

#[tokio::test]
async fn publishing_twice_is_refused() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let created = create_form(&app, &token, "pr-twice", definition("pr-twice")).await;
    let id = id_of(&created);

    let first = app
        .send(
            Method::POST,
            &format!("/api/v1/rad/forms/{id}/publish"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(first.status, StatusCode::OK);

    let second = app
        .send(
            Method::POST,
            &format!("/api/v1/rad/forms/{id}/publish"),
            Some(&token),
            None,
        )
        .await;

    assert_eq!(
        second.status,
        StatusCode::CONFLICT,
        "the second publish must not overwrite who published it; body {}",
        second.body
    );
}

#[tokio::test]
async fn editing_a_published_form_means_creating_the_next_revision() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let created = create_form(&app, &token, "pr-revised", definition("pr-revised")).await;
    let id = id_of(&created);

    app.send(
        Method::POST,
        &format!("/api/v1/rad/forms/{id}/publish"),
        Some(&token),
        None,
    )
    .await;

    let next = app
        .send(
            Method::POST,
            &format!("/api/v1/rad/forms/{id}/revisions"),
            Some(&token),
            Some(json!({ "title": "Purchase requisition v2" })),
        )
        .await;

    assert_eq!(next.status, StatusCode::CREATED, "body {}", next.body);
    assert_eq!(next.body["data"]["revision"], 2);
    assert_eq!(next.body["data"]["status"], "DRAFT");
    assert_eq!(
        next.body["data"]["formKey"], "pr-revised",
        "the key is the identity; the revision is what moved"
    );
    assert_eq!(
        next.body["data"]["definition"],
        definition("pr-revised"),
        "a revision that changes only the title carries the definition forward"
    );

    // The published revision is untouched — which is the whole point.
    let first = app
        .send(
            Method::GET,
            &format!("/api/v1/rad/forms/{id}"),
            Some(&token),
            None,
        )
        .await;

    assert_eq!(first.body["data"]["revision"], 1);
    assert_eq!(first.body["data"]["title"], "Purchase requisition");
}

#[tokio::test]
async fn creating_a_key_that_already_has_revisions_is_refused() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    create_form(&app, &token, "pr-duplicate", definition("pr-duplicate")).await;

    let again = app
        .send(
            Method::POST,
            "/api/v1/rad/forms",
            Some(&token),
            Some(json!({
                "formKey": "pr-duplicate",
                "title": "Again",
                "definition": definition("pr-duplicate"),
            })),
        )
        .await;

    assert_eq!(
        again.status,
        StatusCode::CONFLICT,
        "guessing whether a caller meant a second form or a second revision \
         would silently fork a form's history; body {}",
        again.body
    );
}

/// #135's contract: an update's record says what changed, not what was asked
/// for.
#[tokio::test]
async fn an_update_records_what_changed_and_not_what_was_requested() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let created = create_form(&app, &token, "pr-audited", definition("pr-audited")).await;
    let id = id_of(&created);

    // Two fields sent; one of them is the value it already holds.
    let response = app
        .send(
            Method::PUT,
            &format!("/api/v1/rad/forms/{id}"),
            Some(&token),
            Some(json!({
                "title": "Renamed",
                "definition": definition("pr-audited"),
            })),
        )
        .await;

    assert_eq!(response.status, StatusCode::OK, "body {}", response.body);

    let (old_value, new_value): (Value, Value) = sqlx::query_as(
        "SELECT old_value_json, new_value_json FROM audit_events
         WHERE object_id = $1 AND action = 'UPDATE'
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(id)
    .fetch_one(&app.pool)
    .await
    .expect("the update was audited");

    assert_eq!(
        old_value["title"], "Purchase requisition",
        "the record carries the value that moved"
    );
    assert_eq!(new_value["title"], "Renamed");
    assert!(
        new_value.get("definition").is_none(),
        "the definition did not change, so it does not appear: a record of what \
         was requested rather than what moved is what #135 rejected; got {new_value}"
    );
}

#[tokio::test]
async fn a_deleted_form_is_gone_from_reads_and_kept_in_storage() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let created = create_form(&app, &token, "pr-deleted", definition("pr-deleted")).await;
    let id = id_of(&created);

    let deleted = app
        .send(
            Method::DELETE,
            &format!("/api/v1/rad/forms/{id}"),
            Some(&token),
            None,
        )
        .await;

    assert_eq!(deleted.status, StatusCode::NO_CONTENT);

    let read = app
        .send(
            Method::GET,
            &format!("/api/v1/rad/forms/{id}"),
            Some(&token),
            None,
        )
        .await;

    assert_eq!(read.status, StatusCode::NOT_FOUND);

    let still_there: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT deleted_at FROM rad_forms WHERE id = $1")
            .bind(id)
            .fetch_one(&app.pool)
            .await
            .expect("the row is still there");

    assert!(
        still_there.is_some(),
        "a delete is a soft delete: a document may still pin this revision"
    );
}

/// A retired revision's number is not reused.
///
/// `uq_rad_forms_tenant_id_form_key_revision` is partial on `deleted_at IS
/// NULL`, so reusing the number would insert without complaint and leave two
/// rows meaning `(formKey, 1)` — one of which a document may still pin.
#[tokio::test]
async fn a_deleted_revision_number_is_not_reused() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let created = create_form(&app, &token, "pr-numbering", definition("pr-numbering")).await;
    let id = id_of(&created);

    let next = app
        .send(
            Method::POST,
            &format!("/api/v1/rad/forms/{id}/revisions"),
            Some(&token),
            Some(json!({ "title": "Second" })),
        )
        .await;
    let second = id_of(&next.body["data"]);

    app.send(
        Method::DELETE,
        &format!("/api/v1/rad/forms/{second}"),
        Some(&token),
        None,
    )
    .await;

    let third = app
        .send(
            Method::POST,
            &format!("/api/v1/rad/forms/{id}/revisions"),
            Some(&token),
            Some(json!({ "title": "Third" })),
        )
        .await;

    assert_eq!(third.status, StatusCode::CREATED, "body {}", third.body);
    assert_eq!(
        third.body["data"]["revision"], 3,
        "revision 2 was retired, not freed"
    );
}
