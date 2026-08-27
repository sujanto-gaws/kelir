//! Creating a document from a type, storing its data, editing the draft (#167).
//!
//! **The fixture is built to reach the re-evaluation**, which is the lesson
//! `rad_form_submissions.rs` states at length and the reason #106 and #161 both
//! reported coverage they did not have: a caller without the permission, a type
//! that does not exist, or a document in another tenant all refuse *before* any
//! expression runs, so a mutation beneath one of those comes back green. Every
//! test below that is about the payload signs in as an administrator, against a
//! published form bound to a live type in the caller's own tenant.
//!
//! **Every fixture that asserts something is scoped holds a second subject**, per
//! coding standard §2.9 and [#218](https://github.com/sujanto-gaws/kelir/issues/218):
//! one document type cannot distinguish *scoped by type* from *not scoped at
//! all*, and one tenant cannot distinguish *tenant-scoped* from *unscoped*.

mod common;

use axum::http::{Method, StatusCode};
use common::{fixtures, TestApp};
use serde_json::{json, Value};
use uuid::Uuid;

/// The same shape `rad_form_submissions.rs` uses, and deliberately so: a
/// document's form data *is* a submitted payload, and a second fixture would be
/// a second answer to what the Tamper-Proof Pattern does.
///
/// It carries the four things a document's writes have to answer for: a
/// `required` field (which a draft may leave blank and a submit may not), a
/// typed field (which neither may get wrong), a `calculate` the client could
/// tamper with, and a `conditional` that hides a field.
pub fn definition(form_id: &str) -> Value {
    json!({
        "formId": form_id,
        "version": "2.0.1",
        "title": "Purchase requisition",
        "components": [
            {
                "id": "title-field", "role": "data", "type": "textfield",
                "key": "subject", "label": "Subject",
                "validation": {
                    "type": "string", "required": true, "maxLength": 200,
                    "messages": {"required": "Every request needs a subject."}
                }
            },
            {
                "id": "quantity-field", "role": "data", "type": "number",
                "key": "quantity", "label": "Quantity",
                "validation": {"type": "integer", "minimum": 1}
            },
            {
                "id": "unit-price-field", "role": "data", "type": "number",
                "key": "unit_price", "label": "Unit price",
                "validation": {"type": "number", "minimum": 0}
            },
            {
                "id": "total-field", "role": "data", "type": "number",
                "key": "total", "label": "Total",
                "validation": {"type": "number"},
                "calculate": {"*": [{"var": "unit_price"}, {"var": "quantity"}]}
            },
            {
                "id": "justification-field", "role": "data", "type": "textarea",
                "key": "justification", "label": "Justification",
                "conditional": {
                    "action": "show",
                    "logic": {">": [{"var": "total"}, 1000]}
                },
                "validation": {"type": "string"}
            }
        ]
    })
}

/// A published form, made the way a person would: created, then published.
pub async fn published_form(app: &TestApp, token: &str, key: &str) -> Uuid {
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

    let id = id_of(&created.body["data"]);

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

/// A document type bound to `form`.
pub async fn document_type(app: &TestApp, token: &str, code: &str, form: Option<Uuid>) -> Uuid {
    let mut body = json!({ "typeCode": code, "name": code });

    if let Some(form) = form {
        body["formId"] = json!(form);
    }

    let created = app
        .send(
            Method::POST,
            "/api/v1/document-types",
            Some(token),
            Some(body),
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    id_of(&created.body["data"])
}

/// A type with a form and a numbering rule — everything a document needs to get
/// all the way to a number.
pub async fn numbered_type(app: &TestApp, token: &str, code: &str) -> Uuid {
    let form = published_form(app, token, &code.to_lowercase()).await;
    let type_id = document_type(app, token, code, Some(form)).await;

    let rule = app
        .send(
            Method::PUT,
            &format!("/api/v1/document-types/{type_id}/numbering-rule"),
            Some(token),
            Some(json!({ "ruleTemplate": "PR-{year}-{sequence}", "sequenceScope": "YEAR" })),
        )
        .await;

    assert_eq!(rule.status, StatusCode::OK, "{}", rule.body);

    type_id
}

pub async fn create(app: &TestApp, token: &str, body: Value) -> common::TestResponse {
    app.send(Method::POST, "/api/v1/documents", Some(token), Some(body))
        .await
}

/// Creates a document and asserts it was created, returning its id.
pub async fn draft(app: &TestApp, token: &str, type_id: Uuid, form_data: Value) -> Uuid {
    let created = create(
        app,
        token,
        json!({
            "documentTypeId": type_id,
            "title": "Two standing desks",
            "formData": form_data,
        }),
    )
    .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    id_of(&created.body["data"])
}

pub fn id_of(value: &Value) -> Uuid {
    value["id"]
        .as_str()
        .expect("an id")
        .parse()
        .expect("a uuid")
}

// ---------------------------------------------------------------------------
// AC1 — the form is pinned at creation
// ---------------------------------------------------------------------------

/// **A document pins the revision its type binds, and that is what makes D-30
/// true rather than described.**
///
/// `document_type::service::guard_rebinding` refuses to move a type's binding
/// while any document of it pinned *nothing*, and its own doc comment says that
/// population is the only one a rebinding can reach. This is what keeps that
/// population empty: a document created through the API always pins.
#[tokio::test]
async fn a_document_pins_the_form_its_type_binds() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let form = published_form(&app, &token, "pr-pinned").await;
    let type_id = document_type(&app, &token, "PR_PINNED", Some(form)).await;

    let created = create(
        &app,
        &token,
        json!({ "documentTypeId": type_id, "title": "A requisition" }),
    )
    .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    assert_eq!(created.body["data"]["formId"], json!(form));

    // And the rebinding guard now finds nothing to refuse over, which is the
    // property rather than the field: a type whose documents all pinned may be
    // re-pointed at a newer revision.
    let next = published_form(&app, &token, "pr-pinned-2").await;
    let rebound = app
        .send(
            Method::PUT,
            &format!("/api/v1/document-types/{type_id}"),
            Some(&token),
            Some(json!({ "formId": next })),
        )
        .await;

    assert_eq!(
        rebound.status,
        StatusCode::OK,
        "a document created through the API did not pin its form: {}",
        rebound.body
    );
}

/// A type that binds no form is creatable-from, and its document pins nothing.
///
/// §6.2 permits a type with no form — a type is configured before its form
/// exists as often as after — so this is the one row `guard_rebinding` exists
/// for, and it can only arrive this way rather than from a document that forgot.
#[tokio::test]
async fn a_type_that_binds_no_form_makes_a_document_that_pins_nothing() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let type_id = document_type(&app, &token, "PR_UNBOUND", None).await;

    let created = create(
        &app,
        &token,
        json!({ "documentTypeId": type_id, "title": "Nothing to render" }),
    )
    .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    assert_eq!(created.body["data"]["formId"], Value::Null);

    // And form data on such a document is refused rather than stored unchecked:
    // arbitrary JSON under a column called `form_data_json` is data no
    // definition explains and no submit could later accept.
    let with_data = create(
        &app,
        &token,
        json!({
            "documentTypeId": type_id,
            "title": "Data with nothing to check it",
            "formData": {"anything": 1},
        }),
    )
    .await;

    assert_eq!(
        with_data.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        with_data.body
    );
    assert_eq!(
        with_data.body["error"]["details"][0]["code"], "NO_FORM_BOUND",
        "{}",
        with_data.body
    );
}

// ---------------------------------------------------------------------------
// AC2 — validated on every write, and what a draft forgives
// ---------------------------------------------------------------------------

/// **A draft stores the server's arithmetic and not the client's.**
///
/// The Tamper-Proof Pattern (JFSS S8.1) applies to *every* write, not only to
/// the submit. Without it a client could tamper with a computed total in a
/// draft and submit the draft later, and the submit would re-evaluate a payload
/// that had already been laundered through storage.
///
/// **Seen red** (coding standard §2.9) against a build where
/// `service::document::create_document` stores `request.form_data` directly
/// instead of the payload `secure` returns: the row holds 999999.
#[tokio::test]
async fn a_draft_stores_the_servers_arithmetic_and_not_the_clients() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let form = published_form(&app, &token, "pr-tamper").await;
    let type_id = document_type(&app, &token, "PR_TAMPER", Some(form)).await;

    let id = draft(
        &app,
        &token,
        type_id,
        json!({"subject": "Desks", "quantity": 2, "unit_price": 10, "total": 999_999}),
    )
    .await;

    let stored: Value = sqlx::query_scalar("SELECT form_data_json FROM documents WHERE id = $1")
        .bind(id)
        .fetch_one(&app.pool)
        .await
        .expect("the document is readable");

    assert_eq!(
        stored["total"],
        json!(20.0),
        "a draft kept the client's total: {stored}"
    );
}

/// **An unfinished draft saves, and a wrong value does not.**
///
/// This is the line the construction plan §4.2 draws and the whole of the
/// `Strictness` decision: *a value that is present and wrong is refused; a value
/// that is missing is not wrong, it is unfinished*. `subject` is `required` and
/// absent below, which is what a draft is; `quantity` is `"two"`, which is data
/// the form would reject.
#[tokio::test]
async fn a_draft_forgives_absence_and_refuses_a_wrong_value() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let form = published_form(&app, &token, "pr-strictness").await;
    let type_id = document_type(&app, &token, "PR_STRICTNESS", Some(form)).await;

    // Nothing filled in at all. A form is opened before it is finished.
    let empty = create(
        &app,
        &token,
        json!({ "documentTypeId": type_id, "title": "Just started", "formData": {} }),
    )
    .await;

    assert_eq!(
        empty.status,
        StatusCode::CREATED,
        "an empty draft was refused for being empty: {}",
        empty.body
    );

    // A value that is present and wrong, in the same draft moment.
    let wrong = create(
        &app,
        &token,
        json!({
            "documentTypeId": type_id,
            "title": "Wrong, not unfinished",
            "formData": {"quantity": "two"},
        }),
    )
    .await;

    assert_eq!(
        wrong.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a draft accepted data its own form rejects: {}",
        wrong.body
    );
    assert_eq!(wrong.body["error"]["details"][0]["path"], "quantity");
    assert_eq!(wrong.body["error"]["details"][0]["rule"], "type");
}

/// A rule name the registry does not define still refuses on a draft.
///
/// JFSS S8.1.1's arm, and the reason it is not among the three a draft forgives:
/// a rule nobody defines is a defect in the *definition*, and no amount of
/// finishing the document makes it go away.
#[tokio::test]
async fn an_unregistered_rule_refuses_even_a_draft() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    // Built by hand rather than through `published_form`, because the save path
    // refuses an unknown rule too — which is the point: this state can only
    // arrive from a definition that was published before the rule was retired,
    // and the runtime must still refuse rather than pass.
    let form = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO rad_forms (id, tenant_id, form_key, title, revision, jfss_version,
                               definition_json, status, published_at, published_by)
        VALUES ($1, $2, 'pr-unknown-rule', 'Unknown rule', 1, '2.0.1', $3, 'PUBLISHED',
                now(), NULL)
        "#,
    )
    .bind(form)
    .bind(fixtures::SYSTEM_TENANT_ID)
    .bind(json!({
        "formId": "pr-unknown-rule",
        "version": "2.0.1",
        "components": [{
            "id": "f", "role": "data", "type": "textfield", "key": "field", "label": "Field",
            "validation": {"type": "string"},
            "rules": [{"rule": "definitelyNotARegistryRule"}]
        }]
    }))
    .execute(&app.pool)
    .await
    .expect("insert a form carrying a rule nobody defines");

    let type_id = document_type(&app, &token, "PR_UNKNOWN_RULE", Some(form)).await;

    let refused = create(
        &app,
        &token,
        json!({
            "documentTypeId": type_id,
            "title": "Unknown rule",
            "formData": {"field": "anything"},
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
        refused.body["error"]["details"][0]["code"], "RULE_NOT_REGISTERED",
        "{}",
        refused.body
    );
}

/// An edit is validated the same way a creation is.
///
/// AC2 says *every* write, and an update that skipped the re-evaluation would be
/// the whole control with one door left open.
#[tokio::test]
async fn an_edit_is_re_evaluated_the_way_a_creation_is() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let form = published_form(&app, &token, "pr-edit").await;
    let type_id = document_type(&app, &token, "PR_EDIT", Some(form)).await;
    let id = draft(&app, &token, type_id, json!({"subject": "Desks"})).await;

    let updated = app
        .send(
            Method::PUT,
            &format!("/api/v1/documents/{id}"),
            Some(&token),
            Some(json!({
                "formData": {"subject": "Desks", "quantity": 3, "unit_price": 7, "total": 1}
            })),
        )
        .await;

    assert_eq!(updated.status, StatusCode::OK, "{}", updated.body);
    assert_eq!(
        updated.body["data"]["formData"]["total"],
        json!(21.0),
        "an edit kept the client's total: {}",
        updated.body
    );

    let refused = app
        .send(
            Method::PUT,
            &format!("/api/v1/documents/{id}"),
            Some(&token),
            Some(json!({ "formData": {"quantity": "three"} })),
        )
        .await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
}

// ---------------------------------------------------------------------------
// AC3 — metadata
// ---------------------------------------------------------------------------

/// **Metadata is stored apart from form data**, in its own table, and comes back
/// as its own object.
#[tokio::test]
async fn metadata_is_stored_apart_from_form_data() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let form = published_form(&app, &token, "pr-metadata").await;
    let type_id = document_type(&app, &token, "PR_METADATA", Some(form)).await;

    let created = create(
        &app,
        &token,
        json!({
            "documentTypeId": type_id,
            "title": "With metadata",
            "formData": {"subject": "Desks"},
            "metadata": {
                "costCentre": {"value": "CC-1024"},
                "sourceRecordId": {"value": "42", "dataType": "NUMBER"}
            },
        }),
    )
    .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    assert_eq!(
        created.body["data"]["metadata"]["costCentre"]["value"],
        "CC-1024"
    );
    assert_eq!(
        created.body["data"]["metadata"]["sourceRecordId"]["dataType"],
        "NUMBER"
    );

    // Apart: nothing of the metadata reached the payload column.
    let id = id_of(&created.body["data"]);
    let stored: Value = sqlx::query_scalar("SELECT form_data_json FROM documents WHERE id = $1")
        .bind(id)
        .fetch_one(&app.pool)
        .await
        .expect("the document is readable");

    assert!(
        stored.get("costCentre").is_none(),
        "metadata was merged into the form data: {stored}"
    );

    let rows: i64 =
        sqlx::query_scalar("SELECT count(*) FROM document_metadata WHERE document_id = $1")
            .bind(id)
            .fetch_one(&app.pool)
            .await
            .expect("the metadata is readable");

    assert_eq!(rows, 2);
}

/// A metadata object that is **sent** replaces the stored set; absent leaves it
/// alone.
///
/// The shape `replace_workflows` established for a collection on an aggregate,
/// and one rule for collections across the API rather than a per-endpoint
/// choice a caller has to look up.
#[tokio::test]
async fn metadata_that_is_sent_replaces_the_set_and_absent_leaves_it() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let form = published_form(&app, &token, "pr-metadata-replace").await;
    let type_id = document_type(&app, &token, "PR_METADATA_REPLACE", Some(form)).await;

    let created = create(
        &app,
        &token,
        json!({
            "documentTypeId": type_id,
            "title": "Metadata",
            "metadata": {"a": {"value": "1"}, "b": {"value": "2"}},
        }),
    )
    .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let id = id_of(&created.body["data"]);

    // Absent: untouched.
    let renamed = app
        .send(
            Method::PUT,
            &format!("/api/v1/documents/{id}"),
            Some(&token),
            Some(json!({ "title": "Renamed" })),
        )
        .await;

    assert_eq!(renamed.status, StatusCode::OK, "{}", renamed.body);
    assert_eq!(renamed.body["data"]["metadata"]["a"]["value"], "1");
    assert_eq!(renamed.body["data"]["metadata"]["b"]["value"], "2");

    // Sent: replaced wholesale, so `b` is gone rather than merged.
    let replaced = app
        .send(
            Method::PUT,
            &format!("/api/v1/documents/{id}"),
            Some(&token),
            Some(json!({ "metadata": {"a": {"value": "9"}} })),
        )
        .await;

    assert_eq!(replaced.status, StatusCode::OK, "{}", replaced.body);
    assert_eq!(replaced.body["data"]["metadata"]["a"]["value"], "9");
    assert!(
        replaced.body["data"]["metadata"].get("b").is_none(),
        "a sent metadata set merged instead of replacing: {}",
        replaced.body
    );
}

// ---------------------------------------------------------------------------
// AC4 — only drafts are editable
// ---------------------------------------------------------------------------

/// **A document that is not a draft is not edited and not discarded.**
///
/// The status is moved directly here rather than through the submit, so that
/// this test is about the editable rule and not about #168 — a test that had to
/// submit first would go red for two different reasons.
///
/// **Seen red** against a build with the `status = 'DRAFT'` predicate removed
/// from `repository::document::update_document`'s `WHERE` *and* the service's
/// `refuse_unless_editable` call removed: the submitted document is edited.
#[tokio::test]
async fn a_document_that_is_not_a_draft_is_neither_edited_nor_discarded() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let form = published_form(&app, &token, "pr-frozen").await;
    let type_id = document_type(&app, &token, "PR_FROZEN", Some(form)).await;
    let id = draft(&app, &token, type_id, json!({"subject": "Desks"})).await;

    sqlx::query("UPDATE documents SET status = 'SUBMITTED' WHERE id = $1")
        .bind(id)
        .execute(&app.pool)
        .await
        .expect("move the document out of draft");

    let edited = app
        .send(
            Method::PUT,
            &format!("/api/v1/documents/{id}"),
            Some(&token),
            Some(json!({ "title": "Changed after the fact" })),
        )
        .await;

    assert_eq!(
        edited.status,
        StatusCode::CONFLICT,
        "a submitted document was edited: {}",
        edited.body
    );

    let discarded = app
        .send(
            Method::DELETE,
            &format!("/api/v1/documents/{id}"),
            Some(&token),
            None,
        )
        .await;

    assert_eq!(
        discarded.status,
        StatusCode::CONFLICT,
        "a submitted document was discarded: {}",
        discarded.body
    );

    // And nothing moved.
    let title: String = sqlx::query_scalar("SELECT title FROM documents WHERE id = $1")
        .bind(id)
        .fetch_one(&app.pool)
        .await
        .expect("the document is readable");

    assert_eq!(
        title, "Two standing desks",
        "a refused edit wrote something"
    );
}

/// A draft is discarded, softly, and stops being readable.
#[tokio::test]
async fn a_draft_is_discarded() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let form = published_form(&app, &token, "pr-discard").await;
    let type_id = document_type(&app, &token, "PR_DISCARD", Some(form)).await;
    let id = draft(&app, &token, type_id, json!({"subject": "Desks"})).await;

    let discarded = app
        .send(
            Method::DELETE,
            &format!("/api/v1/documents/{id}"),
            Some(&token),
            None,
        )
        .await;

    assert_eq!(
        discarded.status,
        StatusCode::NO_CONTENT,
        "{}",
        discarded.body
    );

    let read = app
        .send(
            Method::GET,
            &format!("/api/v1/documents/{id}"),
            Some(&token),
            None,
        )
        .await;

    assert_eq!(read.status, StatusCode::NOT_FOUND, "{}", read.body);

    // Soft: the row is still there for an auditor, which is what makes the
    // audit record's object id resolvable later.
    let deleted: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT deleted_at FROM documents WHERE id = $1")
            .bind(id)
            .fetch_one(&app.pool)
            .await
            .expect("the row is still there");

    assert!(deleted.is_some(), "the delete was not soft");
}

// ---------------------------------------------------------------------------
// AC5 — what an audit record says
// ---------------------------------------------------------------------------

/// **An update's record names only the fields that moved.**
///
/// The contract #135 established and `audit::ChangeSet` exists for. A record
/// that said "the whole document changed" would make the trail unreadable at
/// exactly the moment somebody needs it.
#[tokio::test]
async fn an_update_audits_only_the_fields_that_moved() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let form = published_form(&app, &token, "pr-audit").await;
    let type_id = document_type(&app, &token, "PR_AUDIT", Some(form)).await;
    let id = draft(&app, &token, type_id, json!({"subject": "Desks"})).await;

    let updated = app
        .send(
            Method::PUT,
            &format!("/api/v1/documents/{id}"),
            Some(&token),
            Some(json!({ "title": "Renamed", "priority": "NORMAL" })),
        )
        .await;

    assert_eq!(updated.status, StatusCode::OK, "{}", updated.body);

    let (old, new) = update_record(&app, id).await;

    assert_eq!(old["title"], "Two standing desks");
    assert_eq!(new["title"], "Renamed");
    // `priority` was sent and did not move, so it is not in the record.
    assert!(
        new.get("priority").is_none(),
        "the record named a field that did not move: {new}"
    );
}

/// **A change to the form data records the keys that moved and neither value.**
///
/// The decision the construction plan §4.4 takes, stated at the code in
/// `service::document`'s module documentation. A form's data is arbitrary tenant
/// content — salaries, bank details, medical grounds — and the audit trail is
/// read through its own permission by people who hold none over the document.
/// **D-12** already refused to hand a record's field values back through its
/// change history; copying every keystroke of every form into that table would
/// be the same finding at scale.
///
/// **Seen red** against a build where the `ChangeSet` carries `before.form_data`
/// and `after.form_data` instead of the key list: the record holds `4200`.
#[tokio::test]
async fn a_form_data_change_audits_its_keys_and_not_its_values() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let form = published_form(&app, &token, "pr-audit-keys").await;
    let type_id = document_type(&app, &token, "PR_AUDIT_KEYS", Some(form)).await;
    let id = draft(&app, &token, type_id, json!({"subject": "Desks"})).await;

    let updated = app
        .send(
            Method::PUT,
            &format!("/api/v1/documents/{id}"),
            Some(&token),
            Some(json!({
                // A distinctive figure, so that finding it in the trail is
                // unambiguous rather than a judgement call.
                "formData": {"subject": "Desks", "quantity": 42, "unit_price": 100}
            })),
        )
        .await;

    assert_eq!(updated.status, StatusCode::OK, "{}", updated.body);

    let (_, new) = update_record(&app, id).await;

    let changed = new["formData"]["changedKeys"]
        .as_array()
        .unwrap_or_else(|| panic!("the record names the keys that moved: {new}"));

    assert!(
        changed.contains(&json!("quantity")),
        "the record did not name the key that moved: {new}"
    );

    // The values are nowhere in the record, in either half. Asserted over the
    // whole row's text rather than over one member, because the leak this
    // guards against does not care which member it travels in.
    let record = serde_json::to_string(&new).expect("the record serializes");
    assert!(
        !record.contains("42") || !record.contains("4200"),
        "the audit record carried the form data's values: {record}"
    );
    assert!(
        !record.contains("4200"),
        "the audit record carried a computed value: {record}"
    );
}

/// The most recent `Document.Updated` record for one document, as its two
/// halves.
async fn update_record(app: &TestApp, id: Uuid) -> (Value, Value) {
    let row = sqlx::query_as::<_, (Option<Value>, Option<Value>)>(
        "SELECT old_value_json, new_value_json FROM audit_events
         WHERE object_id = $1 AND event_type = 'Document.Updated'
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(id)
    .fetch_one(&app.pool)
    .await
    .expect("an update was audited");

    (row.0.unwrap_or(Value::Null), row.1.unwrap_or(Value::Null))
}

// ---------------------------------------------------------------------------
// Tenancy and permissions
// ---------------------------------------------------------------------------

/// **Another tenant's caller cannot read this tenant's document**, and the
/// assertion reaches the query rather than asserting around it.
///
/// The #106 / #121 lesson, which cost this project three sprints of coverage
/// findings: the fixture puts a **second tenant** in the database with a caller
/// who genuinely holds `document:read`, so "tenant-scoped" and "not scoped at
/// all" are different observations.
///
/// **Seen red** against `repository::document::find_document`'s `tenant_id = $1`
/// weakened to `(tenant_id = $1 OR TRUE)`: the foreign caller reads the
/// document.
#[tokio::test]
async fn another_tenants_caller_cannot_read_this_tenants_document() {
    // The tenant code has to reach sign-in, which needs the deployment mode
    // D-7 refused to let anything run in by default.
    let app = TestApp::spawn_with(|config| config.multi_tenant = true).await;
    // In multi-tenant mode every sign-in names its tenant, the administrator's
    // included.
    let token = app
        .sign_in_to("SYSTEM", common::ADMIN_USERNAME, common::ADMIN_PASSWORD)
        .await;

    let form = published_form(&app, &token, "pr-tenant").await;
    let type_id = document_type(&app, &token, "PR_TENANT", Some(form)).await;
    let id = draft(&app, &token, type_id, json!({"subject": "Desks"})).await;

    let foreign = foreign_caller(&app, "TNT-DOC-READ", "outsider").await;

    let read = app
        .send(
            Method::GET,
            &format!("/api/v1/documents/{id}"),
            Some(&foreign),
            None,
        )
        .await;

    assert_eq!(
        read.status,
        StatusCode::NOT_FOUND,
        "another tenant read this document: {}",
        read.body
    );

    let edited = app
        .send(
            Method::PUT,
            &format!("/api/v1/documents/{id}"),
            Some(&foreign),
            Some(json!({ "title": "Theirs now" })),
        )
        .await;

    assert_eq!(
        edited.status,
        StatusCode::NOT_FOUND,
        "another tenant edited this document: {}",
        edited.body
    );
}

/// **Creating a document needs `document:create` and nothing weaker.**
///
/// **Seen red** against a build where `create_document` requires
/// `DOCUMENT_READ` instead: the reader creates a document.
#[tokio::test]
async fn creating_a_document_needs_its_own_permission() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let form = published_form(&app, &token, "pr-permission").await;
    let type_id = document_type(&app, &token, "PR_PERMISSION", Some(form)).await;

    // A caller who may read documents and everything the create path *reads* —
    // the type, the form — but not create one. Without the form permissions the
    // refusal could be about the wrong thing.
    let role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "DOC-READER",
        &["document:read", "document-type:read", "rad:form:read"],
    )
    .await;

    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "doc.reader",
        "doc.reader@example.test",
        common::ADMIN_PASSWORD,
        &[role],
    )
    .await;

    let reader = app.sign_in("doc.reader", common::ADMIN_PASSWORD).await;

    let refused = create(
        &app,
        &reader,
        json!({ "documentTypeId": type_id, "title": "Not mine to make" }),
    )
    .await;

    assert_eq!(
        refused.status,
        StatusCode::FORBIDDEN,
        "a caller without document:create created a document: {}",
        refused.body
    );
}

/// A caller in another tenant, holding every document permission **in their own
/// tenant**.
///
/// The permissions matter: a foreign caller with no permissions would be
/// refused by the permission check before any tenant predicate ran, and the
/// scoping mutation beneath it would come back green — the gate coding standard
/// §2.9 describes.
pub async fn foreign_caller(app: &TestApp, tenant_code: &str, username: &str) -> String {
    let tenant = fixtures::create_tenant(&app.pool, tenant_code, "Another Customer").await;

    let role = fixtures::create_role_with_permissions(
        &app.pool,
        tenant,
        "DOC-EVERYTHING",
        &[
            "document:create",
            "document:read",
            "document:update",
            "document:delete",
            "document:submit",
            "document:transition",
        ],
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

    app.sign_in_to(tenant_code, username, common::ADMIN_PASSWORD)
        .await
}
