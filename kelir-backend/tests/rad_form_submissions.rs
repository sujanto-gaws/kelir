//! Submitting a filled-in form, through the API and down to the row (#164).
//!
//! **This is the security-critical surface of Sprint 8**, so coding standard
//! §2.9 applies to it in full: every test below that names a control has been
//! seen to fail against a build with that control removed, and the pull request
//! cites the red run against the test by name.
//!
//! **The fixture is built to reach step 4 of the re-evaluation, and that is the
//! whole point of how it is written.** §2.9 calls the alternative a *gate*: a
//! caller without the permission, a form that is not published, or a definition
//! in another tenant all refuse before any expression is evaluated, and a
//! mutation beneath one of those then comes back green over coverage that does
//! not exist. It happened to #106 across six queries in Sprint 6 and to #161
//! this month. So the fixture here signs in as an administrator, against a
//! published form, in the caller's own tenant — everything the gate would have
//! stopped — and the assertions are about what the row holds.
//!
//! What is asserted here rather than in `service::evaluation`'s unit tests is
//! everything that needs a database and a request: that the *stored* row holds
//! the server's answer, that the refusals are refusals **at the endpoint**, and
//! that a failed re-evaluation writes nothing at all.

mod common;

use axum::http::{Method, StatusCode};
use common::{fixtures, TestApp};
use serde_json::{json, Value};
use uuid::Uuid;

/// The Calculation Rule Registry §6.1 invoice, plus the two things a submission
/// has to answer for beside the arithmetic: a `conditional` that hides a field,
/// and a `sequenceKey` the client could renumber.
fn definition(form_id: &str) -> Value {
    json!({
        "formId": form_id,
        "version": "2.0.1",
        "title": "Purchase requisition",
        "components": [
            {
                "id": "title-field", "role": "data", "type": "textfield",
                "key": "title", "label": "Title",
                "validation": {
                    "type": "string", "required": true, "maxLength": 200,
                    "messages": {"required": "Every request needs a title."}
                }
            },
            {
                "id": "budget-field", "role": "data", "type": "number",
                "key": "budget", "label": "Budget",
                "validation": {"type": "number", "minimum": 0}
            },
            {
                "id": "justification-field", "role": "data", "type": "textarea",
                "key": "justification", "label": "Justification",
                "conditional": {
                    "action": "show",
                    "logic": {">": [{"var": "budget"}, 1000]}
                },
                "validation": {"type": "string"}
            },
            {
                "id": "line-items", "role": "data", "type": "datagrid",
                "key": "line_items", "label": "Line items",
                "sequenceKey": "line_no",
                "validation": {"type": "array"},
                "components": [
                    {"id": "line-no", "role": "data", "type": "number", "key": "line_no",
                     "label": "Line", "readOnly": true, "validation": {"type": "integer"}},
                    {"id": "line-quantity", "role": "data", "type": "number", "key": "quantity",
                     "label": "Quantity",
                     "validation": {"type": "integer", "minimum": 1,
                                    "messages": {"minimum": "A line orders at least one."}}},
                    {"id": "line-unit-price", "role": "data", "type": "number", "key": "unit_price",
                     "label": "Unit price", "validation": {"type": "number", "minimum": 0}},
                    {"id": "line-total", "role": "data", "type": "number", "key": "line_total",
                     "label": "Line total", "validation": {"type": "number"},
                     "calculate": {"*": [{"var": "unit_price"}, {"var": "quantity"}]}}
                ]
            },
            {
                "id": "grand-total-field", "role": "data", "type": "number",
                "key": "grand_total", "label": "Grand total",
                "validation": {"type": "number"},
                "calculate": {"sum": [{"map": [
                    {"var": "line_items"},
                    {"*": [{"var": "unit_price"}, {"var": "quantity"}]}
                ]}]}
            }
        ]
    })
}

/// Two lines worth 20 and 22. **42 is the figure the whole Tamper-Proof
/// argument is built on**, and the figure the operator-parity spike watched
/// become a silent 0.
fn filled_in() -> Value {
    json!({
        "title": "Two standing desks",
        "budget": 500,
        "justification": null,
        "line_items": [
            {"line_no": 1, "quantity": 2, "unit_price": 10, "line_total": 20},
            {"line_no": 2, "quantity": 2, "unit_price": 11, "line_total": 22}
        ],
        "grand_total": 42
    })
}

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

    assert_eq!(
        created.status,
        StatusCode::CREATED,
        "creating {key} failed: {}",
        created.body
    );

    let id: Uuid = created.body["data"]["id"]
        .as_str()
        .expect("the created form has an id")
        .parse()
        .expect("the id is a uuid");

    let published = app
        .send(
            Method::POST,
            &format!("/api/v1/rad/forms/{id}/publish"),
            Some(token),
            None,
        )
        .await;

    assert_eq!(
        published.status,
        StatusCode::OK,
        "publishing {key} failed: {}",
        published.body
    );

    id
}

async fn submit(app: &TestApp, token: &str, form_id: Uuid, payload: Value) -> common::TestResponse {
    app.send(
        Method::POST,
        &format!("/api/v1/rad/forms/{form_id}/submissions"),
        Some(token),
        Some(json!({ "payload": payload })),
    )
    .await
}

/// What the database holds, read directly rather than through the API.
///
/// The API's answer and the stored row are different claims, and this endpoint's
/// whole purpose is to make good on the difference — so the assertion that
/// matters reads the column.
async fn stored_payload(app: &TestApp, submission_id: Uuid) -> Value {
    sqlx::query_scalar!(
        "SELECT payload_json FROM rad_form_submissions WHERE id = $1",
        submission_id
    )
    .fetch_one(&app.pool)
    .await
    .expect("the submission row is readable")
}

fn submission_id(body: &Value) -> Uuid {
    body["data"]["id"]
        .as_str()
        .expect("the submission has an id")
        .parse()
        .expect("the id is a uuid")
}

// ---------------------------------------------------------------------------
// The security control (#164 AC1, AC3, AC4)
// ---------------------------------------------------------------------------

/// **The test the sprint's security control rests on.**
///
/// It posts a total the rules do not produce and asserts the *stored row* holds
/// the computed one. Coding standard §2.9 makes the red run the evidence, and
/// the mutation is the one that removes S8.1's overwrite: in
/// `service::evaluation::Evaluation::calculate_pass`, the `derived` arm of
/// `let next = if derived {` returning `scope.get(key)` instead of the computed
/// value. Seen red 2026-08-27.
///
/// **The fixture reaches step 4 rather than a gate above it** — an
/// administrator, a published form, the caller's own tenant — which is what
/// §2.9's "a mutation that comes back green is a finding" paragraph is about.
#[tokio::test]
async fn a_tampered_total_is_stored_as_the_number_the_rules_produce() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let form_id = published_form(&app, &token, "tampered_total").await;

    let mut payload = filled_in();
    // The client claims the two lines are worth nothing at all.
    payload["grand_total"] = json!(0);
    payload["line_items"][0]["line_total"] = json!(0);
    payload["line_items"][1]["line_total"] = json!(0);

    let response = submit(&app, &token, form_id, payload).await;

    assert_eq!(
        response.status,
        StatusCode::CREATED,
        "the submission is accepted and corrected, not refused: {}",
        response.body
    );

    let stored = stored_payload(&app, submission_id(&response.body)).await;

    assert_eq!(stored["grand_total"], json!(42.0), "stored: {stored}");
    assert_eq!(stored["line_items"][0]["line_total"], json!(20.0));
    assert_eq!(stored["line_items"][1]["line_total"], json!(22.0));

    // And the response says so, which is how a caller learns the server's
    // number differs from theirs — a form that changes your number without
    // saying so is its own defect (#164 AC5).
    assert_eq!(response.body["data"]["payload"]["grand_total"], json!(42.0));
}

/// **AC2's hidden-field case**, and the second security control.
///
/// The mutation is removing the `hidden` removal loop in
/// `service::evaluation::secure_payload_with`. Seen red 2026-08-27.
#[tokio::test]
async fn a_value_submitted_for_a_field_the_conditional_hides_is_not_stored() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let form_id = published_form(&app, &token, "hidden_field").await;

    let mut payload = filled_in();
    // `justification` shows only above 1,000. The budget is 500, and a value
    // is submitted for it anyway — which S10.1.1 requires the client to do and
    // S10.2 requires the server to discard.
    payload["justification"] = json!("smuggled past the conditional");

    let response = submit(&app, &token, form_id, payload).await;

    assert_eq!(response.status, StatusCode::CREATED, "{}", response.body);

    let stored = stored_payload(&app, submission_id(&response.body)).await;

    assert!(
        stored.get("justification").is_none(),
        "a hidden component's value is never persisted; stored: {stored}"
    );
    assert_eq!(stored["budget"], json!(500));
}

/// The same field, on the branch that opens it — because a control that
/// discards everything is not a control, it is a bug.
#[tokio::test]
async fn the_same_value_is_stored_when_the_conditional_shows_the_field() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let form_id = published_form(&app, &token, "shown_field").await;

    let mut payload = filled_in();
    payload["budget"] = json!(5000);
    payload["justification"] = json!("The current desks are failing.");

    let response = submit(&app, &token, form_id, payload).await;

    assert_eq!(response.status, StatusCode::CREATED, "{}", response.body);

    let stored = stored_payload(&app, submission_id(&response.body)).await;

    assert_eq!(
        stored["justification"],
        json!("The current desks are failing.")
    );
}

/// JFSS §9.2's sequence overwrite, at the endpoint.
///
/// The mutation is removing the `row.insert(sequence_key, …)` in
/// `service::evaluation::Evaluation::apply_sequence`. Seen red 2026-08-27.
#[tokio::test]
async fn the_row_numbers_stored_are_the_servers_and_not_the_clients() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let form_id = published_form(&app, &token, "row_numbers").await;

    let mut payload = filled_in();
    payload["line_items"][0]["line_no"] = json!(99);
    payload["line_items"][1]["line_no"] = json!(99);

    let response = submit(&app, &token, form_id, payload).await;
    let stored = stored_payload(&app, submission_id(&response.body)).await;

    assert_eq!(stored["line_items"][0]["line_no"], json!(1), "{stored}");
    assert_eq!(stored["line_items"][1]["line_no"], json!(2));
}

// ---------------------------------------------------------------------------
// The gates (#164 AC6, and the ones §2.9 warns absorb mutations)
// ---------------------------------------------------------------------------

/// The permission is checked before anything is read.
///
/// The mutation is removing `caller.require(FORM_SUBMIT)?` from
/// `service::submission::submit_form`. Seen red 2026-08-27. **The caller holds
/// `rad:form:read` and not `rad:form:submit`**, which is what makes the
/// mutation land: a caller holding neither would be refused by the `find_form`
/// read below it and the test would pass over a control that had been removed —
/// the shape #161's green mutation took this month.
#[tokio::test]
async fn a_caller_who_may_read_a_form_may_not_submit_one_without_the_submit_permission() {
    let app = TestApp::spawn().await;
    let admin = app.administrator_token().await;
    let form_id = published_form(&app, &admin, "reader_cannot_submit").await;

    let role_id = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "ROLE-FORM-READER",
        &["rad:form:read"],
    )
    .await;

    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "reader.only",
        "reader.only@kelir.test",
        "a-long-enough-reader-password",
        &[role_id],
    )
    .await;

    let token = app
        .sign_in("reader.only", "a-long-enough-reader-password")
        .await;

    let response = submit(&app, &token, form_id, filled_in()).await;

    assert_eq!(
        response.status,
        StatusCode::FORBIDDEN,
        "reading a form is not submitting one: {}",
        response.body
    );
}

/// A draft is a form somebody is still writing, and its definition is still
/// editable — so a payload validated against it would be attached to a revision
/// that may no longer mean the same thing.
///
/// The mutation is removing the `form.status != FormStatus::Published` guard.
/// Seen red 2026-08-27.
#[tokio::test]
async fn a_draft_revision_cannot_be_filled_in() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let created = app
        .send(
            Method::POST,
            "/api/v1/rad/forms",
            Some(&token),
            Some(json!({
                "formKey": "still_a_draft",
                "title": "Purchase requisition",
                "definition": definition("still_a_draft"),
            })),
        )
        .await;

    let form_id: Uuid = created.body["data"]["id"]
        .as_str()
        .expect("the created form has an id")
        .parse()
        .expect("the id is a uuid");

    let response = submit(&app, &token, form_id, filled_in()).await;

    assert_eq!(response.status, StatusCode::CONFLICT, "{}", response.body);
}

/// A form in another tenant is not a form this caller can submit to, and the
/// answer is 404 rather than 403 — a caller learns that a form exists only from
/// a tenant they belong to.
///
/// The mutation is dropping `tenant_id = $1` from `repository::form::find_form`.
/// Seen red 2026-08-27.
#[tokio::test]
async fn a_form_in_another_tenant_cannot_be_submitted_to() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let form_id = published_form(&app, &token, "other_tenant").await;

    // Move the form to a second tenant, leaving the caller where they were.
    // Rewriting the row rather than signing in elsewhere keeps the caller's
    // permissions identical, so the only thing that changed is the tenant —
    // which is the predicate under test.
    let other_tenant = fixtures::create_tenant(&app.pool, "TNT-OTHER", "Another tenant").await;

    sqlx::query!(
        "UPDATE rad_forms SET tenant_id = $1 WHERE id = $2",
        other_tenant,
        form_id
    )
    .execute(&app.pool)
    .await
    .expect("the form moves tenant");

    let response = submit(&app, &token, form_id, filled_in()).await;

    assert_eq!(response.status, StatusCode::NOT_FOUND, "{}", response.body);
}

#[tokio::test]
async fn submitting_to_a_form_that_does_not_exist_is_a_404() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let response = submit(&app, &token, Uuid::now_v7(), filled_in()).await;

    assert_eq!(response.status, StatusCode::NOT_FOUND, "{}", response.body);
}

#[tokio::test]
async fn submitting_without_a_token_is_a_401() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let form_id = published_form(&app, &token, "needs_a_token").await;

    let response = app
        .send(
            Method::POST,
            &format!("/api/v1/rad/forms/{form_id}/submissions"),
            None,
            Some(json!({ "payload": filled_in() })),
        )
        .await;

    assert_eq!(
        response.status,
        StatusCode::UNAUTHORIZED,
        "{}",
        response.body
    );
}

// ---------------------------------------------------------------------------
// Refusals, and the rule that a refusal writes nothing (#164 AC6)
// ---------------------------------------------------------------------------

/// The S10.3 envelope, with a dot-notation `path` naming the row.
#[tokio::test]
async fn a_row_that_fails_validation_is_refused_and_named_by_its_path() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let form_id = published_form(&app, &token, "bad_row").await;

    let mut payload = filled_in();
    payload["line_items"][1]["quantity"] = json!(0);

    let response = submit(&app, &token, form_id, payload).await;

    assert_eq!(
        response.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        response.body
    );
    assert_eq!(response.error_code(), Some("VALIDATION_ERROR"));

    let details = response.body["error"]["details"]
        .as_array()
        .expect("the envelope carries details");

    assert_eq!(details.len(), 1, "got {details:?}");
    assert_eq!(details[0]["path"], json!("line_items.1.quantity"));
    assert_eq!(details[0]["rule"], json!("minimum"));
    // The definition's own words, not a sentence the server invented.
    assert_eq!(details[0]["message"], json!("A line orders at least one."));
}

/// **Never a partial write** (AC6). A refused submission leaves no row at all —
/// not a row with the client's numbers in it, and not a row missing the field
/// that failed.
#[tokio::test]
async fn a_refused_submission_stores_nothing() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let form_id = published_form(&app, &token, "nothing_stored").await;

    let mut payload = filled_in();
    payload["title"] = json!(null);

    let response = submit(&app, &token, form_id, payload).await;

    assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);

    let rows = sqlx::query_scalar!(
        "SELECT count(*) FROM rad_form_submissions WHERE form_id = $1",
        form_id
    )
    .fetch_one(&app.pool)
    .await
    .expect("the count is readable");

    assert_eq!(rows, Some(0), "a refusal writes nothing");
}

/// **Decision D-24 at the submission** (construction plan §6.3 step 6). The
/// browser renders a field whose calculation failed blank and does not block
/// typing; here the same failure is a refusal that names the field.
#[tokio::test]
async fn a_division_by_zero_refuses_the_submission_and_names_the_field() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let mut with_an_average = definition("division_by_zero");
    with_an_average["components"]
        .as_array_mut()
        .expect("components is an array")
        .push(json!({
            "id": "average-field", "role": "data", "type": "number",
            "key": "average_line", "label": "Average line",
            "validation": {"type": "number"},
            "calculate": {"/": [{"var": "grand_total"}, {"var": "line_count"}]}
        }));
    with_an_average["components"]
        .as_array_mut()
        .expect("components is an array")
        .push(json!({
            "id": "line-count-field", "role": "data", "type": "number",
            "key": "line_count", "label": "Line count",
            "validation": {"type": "integer"}
        }));

    let created = app
        .send(
            Method::POST,
            "/api/v1/rad/forms",
            Some(&token),
            Some(json!({
                "formKey": "division_by_zero",
                "title": "Purchase requisition",
                "definition": with_an_average,
            })),
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    let form_id: Uuid = created.body["data"]["id"]
        .as_str()
        .expect("the created form has an id")
        .parse()
        .expect("the id is a uuid");

    app.send(
        Method::POST,
        &format!("/api/v1/rad/forms/{form_id}/publish"),
        Some(&token),
        None,
    )
    .await;

    let mut payload = filled_in();
    payload["line_count"] = json!(0);

    let response = submit(&app, &token, form_id, payload).await;

    assert_eq!(
        response.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        response.body
    );

    let details = response.body["error"]["details"]
        .as_array()
        .expect("the envelope carries details");

    assert_eq!(details[0]["path"], json!("average_line"));
    assert_eq!(details[0]["code"], json!("EVALUATION_FAILED"));
}

// ---------------------------------------------------------------------------
// The record it leaves
// ---------------------------------------------------------------------------

/// The row carries the revision it was filled in against, and the audit trail
/// records that a submission happened without carrying the payload twice.
#[tokio::test]
async fn a_submission_records_the_revision_and_leaves_an_audit_entry() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let form_id = published_form(&app, &token, "audited").await;

    let response = submit(&app, &token, form_id, filled_in()).await;
    let id = submission_id(&response.body);

    assert_eq!(response.body["data"]["formRevision"], json!(1));

    let record = sqlx::query!(
        r#"
        SELECT event_type, action, object_type, new_value_json
        FROM audit_events
        WHERE object_id = $1
        "#,
        id
    )
    .fetch_one(&app.pool)
    .await
    .expect("a submission is audited");

    assert_eq!(record.event_type, "RadForm.Submitted");
    assert_eq!(record.action, "CREATE");
    assert_eq!(record.object_type, "RAD_FORM_SUBMISSION");

    let new_value = record
        .new_value_json
        .expect("the record says what happened");

    assert_eq!(new_value["formKey"], json!("audited"));
    assert_eq!(new_value["formRevision"], json!(1));
    assert!(
        new_value.get("payload").is_none(),
        "the payload is in the row, not in the trail as well: {new_value}"
    );
}

/// S10.1: every data key is submitted, and a key the definition does not
/// declare is dropped rather than stored. A submission is not a way to write
/// arbitrary JSON into the database.
#[tokio::test]
async fn a_key_the_definition_does_not_declare_is_not_stored() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let form_id = published_form(&app, &token, "undeclared_key").await;

    let mut payload = filled_in();
    payload["is_approved"] = json!(true);

    let response = submit(&app, &token, form_id, payload).await;
    let stored = stored_payload(&app, submission_id(&response.body)).await;

    assert!(stored.get("is_approved").is_none(), "stored: {stored}");
}
