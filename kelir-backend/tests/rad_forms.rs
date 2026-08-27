//! Form and list definition storage, through the API (#156).
//!
//! What is asserted here rather than in the domain's unit tests is everything
//! that needs a database and a request: that a refusal is a refusal *at the
//! endpoint*, that a published revision cannot be edited through any route, and
//! that an update's audit record says what changed rather than what was asked
//! for.

mod common;

use std::time::Duration;

use axum::http::{Method, StatusCode};
use common::{fixtures, TestApp};
use kelir_backend::modules::rad::repository::form as form_repo;
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

/// A `sum` that would evaluate to `0` without meaning to is refused at save
/// ([#201](https://github.com/sujanto-gaws/kelir/issues/201), **D-22**).
///
/// The evaluation is not wrong and both engines agree on it, which is what
/// makes this invisible at runtime: JFSS S8.1's re-evaluation catches a client
/// that disagrees with the server, and here they agree on `0`.
#[tokio::test]
async fn a_sum_that_would_silently_be_zero_is_refused_rather_than_stored() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let mut document = definition("silent-zero");
    // `+` takes a list of operands; `sum` takes one argument and sums the array
    // it evaluates to. Writing one where the other was meant is the mistake.
    document["components"][0]["calculate"] =
        json!({"sum": [{"var": "quantity"}, {"var": "unit_price"}]});

    let response = app
        .send(
            Method::POST,
            "/api/v1/rad/forms",
            Some(&token),
            Some(json!({
                "formKey": "silent-zero",
                "title": "Silent zero",
                "definition": document,
            })),
        )
        .await;

    assert_eq!(
        response.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        response.body
    );

    let codes: Vec<&str> = response.body["error"]["details"]
        .as_array()
        .expect("details")
        .iter()
        .map(|detail| detail["code"].as_str().unwrap_or_default())
        .collect();

    assert!(
        codes.contains(&"SUM_TAKES_ONE_ARRAY"),
        "the refusal must name the reason, not merely fail; got {codes:?}"
    );

    // And nothing was stored: a refused definition is not a draft.
    let listed = app.get("/api/v1/rad/forms", Some(&token)).await;
    assert!(
        !listed.body["data"].to_string().contains("silent-zero"),
        "the definition must not be stored: {}",
        listed.body
    );
}

/// The shorthand evaluates correctly on both engines, so it is not refused.
#[tokio::test]
async fn the_sum_shorthand_is_accepted_because_it_works() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let mut document = definition("sum-shorthand");
    document["components"][0]["calculate"] = json!({"sum": {"var": "line_totals"}});

    let response = app
        .send(
            Method::POST,
            "/api/v1/rad/forms",
            Some(&token),
            Some(json!({
                "formKey": "sum-shorthand",
                "title": "Sum shorthand",
                "definition": document,
            })),
        )
        .await;

    assert_eq!(
        response.status,
        StatusCode::CREATED,
        "refusing this would refuse definitions that evaluate correctly; {}",
        response.body
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

/// **A form key is taken per tenant, and `highest_revision` is the only thing
/// that says so** (#206).
///
/// The conflict check above reads the highest revision of a key; if that read
/// crosses tenants, tenant A is refused a `formKey` because tenant B uses one
/// by that name — a functional block and an existence disclosure in the same
/// refusal. `uq_rad_forms_tenant_id_form_key_revision` does not backstop it:
/// the index is per-tenant, so it permits exactly the row this refusal would
/// have prevented.
///
/// Seen red (coding standard §2.9) against `highest_revision`'s
/// `tenant_id = $1` weakened to `(tenant_id = $1 OR TRUE)`: the create below
/// answers 409 naming a key this tenant has never used.
#[tokio::test]
async fn a_form_key_another_tenant_uses_is_free_here() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    // Written directly rather than through the API: the API is tenant-scoped by
    // the caller's token, which is the thing being probed, so the other
    // tenant's row has to arrive some other way.
    let other = fixtures::create_tenant(&app.pool, "TNT-FORMS", "Another Customer").await;
    sqlx::query(
        "INSERT INTO rad_forms (id, tenant_id, form_key, title, revision, jfss_version, definition_json)
         VALUES ($1, $2, 'pr-shared-key', 'Theirs', 1, '2.0.1', $3)",
    )
    .bind(Uuid::now_v7())
    .bind(other)
    .bind(definition("pr-shared-key"))
    .execute(&app.pool)
    .await
    .expect("the other tenant's form is stored");

    let created = app
        .send(
            Method::POST,
            "/api/v1/rad/forms",
            Some(&token),
            Some(json!({
                "formKey": "pr-shared-key",
                "title": "Ours",
                "definition": definition("pr-shared-key"),
            })),
        )
        .await;

    assert_eq!(
        created.status,
        StatusCode::CREATED,
        "another tenant's key blocked this one: {}",
        created.body
    );
    assert_eq!(created.body["data"]["revision"], 1, "and it starts at 1");
}

/// **The publish that lands between a service's read and its write** (#206).
///
/// `update_draft` and `publish` both carry `AND status = 'DRAFT'` in the
/// statement, and the module comment says why: the service reads the row,
/// refuses a published one, and then writes — and a concurrent publish can land
/// in between. Every existing test exercises the *service* check, which fires
/// first, so both predicates were unexercised while
/// `a_published_revision_cannot_be_edited` and `publishing_twice_is_refused`
/// passed.
///
/// The race is arranged rather than raced for: the publishing transaction takes
/// the row's lock and holds it, the edit blocks on that lock, and the commit
/// releases it. Under `READ COMMITTED` the blocked `UPDATE` re-evaluates its
/// `WHERE` against the committed row — which is exactly the moment the
/// predicate exists for, and the only moment it can be observed in.
///
/// Seen red (coding standard §2.9) against `AND status = 'DRAFT'` removed from
/// each statement in turn: the edit applies to a published revision, and the
/// second publish overwrites the first publisher's stamp.
#[tokio::test]
async fn an_edit_blocked_by_a_publish_applies_to_nothing() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let created = create_form(&app, &token, "pr-race", definition("pr-race")).await;
    let id = id_of(&created);
    let tenant = fixtures::SYSTEM_TENANT_ID;

    // The publisher, holding the row lock and not yet committed.
    let mut publishing = app.pool.begin().await.expect("a transaction");
    let published = form_repo::publish(&mut *publishing, tenant, id, None)
        .await
        .expect("the publish runs");
    assert_eq!(published, 1, "the publish is the one that wins the row");

    // The editor, blocking on that lock. It reached the statement before the
    // publish committed, which is the interleaving the service check cannot
    // see.
    let pool = app.pool.clone();
    let editing = tokio::spawn(async move {
        form_repo::update_draft(
            &pool,
            tenant,
            id,
            &form_repo::FormFields {
                title: Some("Edited after the publish"),
                definition_json: None,
                entity_id: None,
            },
            None,
        )
        .await
    });

    tokio::time::sleep(Duration::from_millis(200)).await;
    publishing.commit().await.expect("the publish commits");

    let edited = editing
        .await
        .expect("the edit task finishes")
        .expect("the edit runs");

    assert_eq!(
        edited, 0,
        "the edit applied to a revision that had just been published"
    );

    let after = app
        .send(
            Method::GET,
            &format!("/api/v1/rad/forms/{id}"),
            Some(&token),
            None,
        )
        .await;

    assert_eq!(after.body["data"]["status"], "PUBLISHED");
    assert_eq!(
        after.body["data"]["title"], "Purchase requisition",
        "the published definition kept the title it was published with"
    );
}

/// The same interleaving for the other statement: two publishes, one row.
///
/// Without `AND status = 'DRAFT'` the second publish rewrites `published_at`
/// and `published_by`, so the record of who published a revision becomes
/// whoever published it last.
#[tokio::test]
async fn a_publish_blocked_by_a_publish_writes_nothing() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let created = create_form(
        &app,
        &token,
        "pr-race-publish",
        definition("pr-race-publish"),
    )
    .await;
    let id = id_of(&created);
    let tenant = fixtures::SYSTEM_TENANT_ID;

    let mut first = app.pool.begin().await.expect("a transaction");
    assert_eq!(
        form_repo::publish(&mut *first, tenant, id, None)
            .await
            .expect("the first publish runs"),
        1
    );

    let pool = app.pool.clone();
    let second = tokio::spawn(async move { form_repo::publish(&pool, tenant, id, None).await });

    tokio::time::sleep(Duration::from_millis(200)).await;
    first.commit().await.expect("the first publish commits");

    assert_eq!(
        second
            .await
            .expect("the second publish task finishes")
            .expect("the second publish runs"),
        0,
        "the second publish rewrote a revision that was already published"
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
