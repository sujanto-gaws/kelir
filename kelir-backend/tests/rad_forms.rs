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

/// **AC1, at the endpoint.** A rule name no registry declares is refused where
/// the definition is *written*, and the refusal names the rule ([#338]).
///
/// It was already refused — S8.1.1 has never let an unknown rule pass — but at
/// submit, against the person filling the form in, who cannot fix a definition.
/// By then the form had been published and used.
///
/// [#338]: https://github.com/sujanto-gaws/kelir/issues/338
#[tokio::test]
async fn a_rule_name_no_registry_declares_is_refused_at_save() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let mut document = definition("unregistered-rule");
    document["components"][0]["rules"] = json!([{
        "rule": "looksNice", "scope": "both", "message": "This does not look nice."
    }]);

    let response = app
        .send(
            Method::POST,
            "/api/v1/rad/forms",
            Some(&token),
            Some(json!({
                "formKey": "unregistered-rule",
                "title": "Unregistered rule",
                "definition": document,
            })),
        )
        .await;

    assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);

    let detail = response.body["error"]["details"]
        .as_array()
        .expect("details")
        .iter()
        .find(|detail| detail["code"] == "RULE_NOT_REGISTERED")
        .cloned()
        .unwrap_or_else(|| panic!("body {}", response.body));

    assert!(
        detail["message"]
            .as_str()
            .is_some_and(|message| message.contains("looksNice")),
        "the refusal must name the rule; got {detail}"
    );
}

/// **[#413] at the seam that reported a `201`.**
///
/// The issue's own table: `caf\b` matches `café` in the browser and not here;
/// `[[:digit:]]` is syntax ECMA-262 does not have, and `\p{Nd}` is syntax it reads
/// only under the `u` flag. All three were
/// stored, so the divergence reached a deployment rather than an author.
///
/// **Each refused pattern sits beside the portable spelling its message names**
/// ([coding standard] §2.9's second subject), so a blanket refusal of anything
/// containing a backslash would fail this rather than pass it.
///
/// **Seen red, 2026-09-13**: `divergent_construct` returning `None` for every
/// arm but `Class` restores the `201` for all three and reddens this.
///
/// [#413]: https://github.com/sujanto-gaws/kelir/issues/413
/// [coding standard]: ../../docs/standards/01.%20Coding%20Standard.md
#[tokio::test]
async fn the_three_constructs_that_decided_one_input_two_ways_are_refused_at_save() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    // (pattern, expected code, the portable spelling that must still store)
    let cases = [
        (
            r"caf\b",
            "PATTERN_CONSTRUCT_NOT_PORTABLE",
            "(^|[^A-Za-z0-9_])caf",
        ),
        ("^[[:digit:]]+$", "PATTERN_CLASS_NOT_PINNED", "^[0-9]+$"),
        (r"^\p{Nd}+$", "PATTERN_CLASS_NOT_PINNED", "^[0-9]+$"),
    ];

    for (index, (pattern, code, portable)) in cases.iter().enumerate() {
        let key = format!("divergent-{index}");
        let mut document = definition(&key);
        document["components"][0]["rules"] = json!([{
            "rule": "regex", "scope": "both",
            "params": {"pattern": pattern},
            "message": "No.",
        }]);

        let response = app
            .send(
                Method::POST,
                "/api/v1/rad/forms",
                Some(&token),
                Some(json!({
                    "formKey": key,
                    "title": "Divergent pattern",
                    "definition": document,
                })),
            )
            .await;

        assert_eq!(
            response.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "`{pattern}` must not be stored; body {}",
            response.body
        );
        assert!(
            response.body["error"]["details"]
                .as_array()
                .expect("details")
                .iter()
                .any(|detail| detail["code"] == *code),
            "`{pattern}` must be refused as {code}; body {}",
            response.body
        );

        // The second subject: the spelling the message names is still stored.
        let portable_key = format!("portable-{index}");
        let mut portable_document = definition(&portable_key);
        portable_document["components"][0]["rules"] = json!([{
            "rule": "regex", "scope": "both",
            "params": {"pattern": portable},
            "message": "No.",
        }]);

        let stored = app
            .send(
                Method::POST,
                "/api/v1/rad/forms",
                Some(&token),
                Some(json!({
                    "formKey": portable_key,
                    "title": "Portable pattern",
                    "definition": portable_document,
                })),
            )
            .await;

        assert_eq!(
            stored.status,
            StatusCode::CREATED,
            "`{portable}` is what the refusal tells an author to write, so it must store; body {}",
            stored.body
        );
    }
}

/// **[#466] at the endpoint that sent the false reasons.**
///
/// Record 16 finding 3 posted five patterns to this route on 2026-09-16 and
/// read a message that was false for each: `^\D+$`, `^\W+$`, `^\S$`, `caf\B`,
/// and `^\p{Nd}+$` under `params.flags` `u`. **This sends the same five** and
/// checks each detail for a phrase only its own reason carries, beside the
/// remedy that reason names, which must store ([coding standard] §2.9's
/// second subject).
///
/// [#466]: https://github.com/sujanto-gaws/kelir/issues/466
/// [coding standard]: ../../docs/standards/01.%20Coding%20Standard.md
#[tokio::test]
async fn a_negated_construct_and_a_flagged_property_are_refused_with_true_reasons() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    // (pattern, flags, code, a phrase only its own reason carries, the remedy)
    let cases = [
        (
            r"^\D+$",
            "",
            "PATTERN_CLASS_NOT_PINNED",
            "passes `^\\D+$` in the browser and fails here",
            "^[^0-9]+$",
        ),
        (
            r"^\W+$",
            "",
            "PATTERN_CLASS_NOT_PINNED",
            "`[^A-Za-z0-9_]`",
            "^[^A-Za-z0-9_]+$",
        ),
        (
            r"^\S$",
            "",
            "PATTERN_CLASS_NOT_PINNED",
            "should not treat as space",
            r"^[^ \t\r\n]$",
        ),
        (
            r"caf\B",
            "",
            "PATTERN_CONSTRUCT_NOT_PORTABLE",
            "`caf\\B` does not match `café` in the browser",
            "caf[A-Za-z0-9_]",
        ),
        (
            r"^\p{Nd}+$",
            "u",
            "PATTERN_CLASS_NOT_PINNED",
            "Under the `u` flag",
            "^[0-9]+$",
        ),
    ];

    for (index, (pattern, flags, code, phrase, remedy)) in cases.iter().enumerate() {
        let key = format!("negated-{index}");
        let mut document = definition(&key);
        document["components"][0]["rules"] = json!([{
            "rule": "regex", "scope": "both",
            "params": {"pattern": pattern, "flags": flags},
            "message": "No.",
        }]);

        let response = app
            .send(
                Method::POST,
                "/api/v1/rad/forms",
                Some(&token),
                Some(json!({"formKey": key, "title": "Negated pattern", "definition": document})),
            )
            .await;

        assert_eq!(
            response.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "`{pattern}` must not be stored; body {}",
            response.body
        );
        let detail = response.body["error"]["details"]
            .as_array()
            .expect("details")
            .iter()
            .find(|detail| detail["code"] == *code)
            .cloned()
            .unwrap_or_else(|| {
                panic!(
                    "`{pattern}` must be refused as {code}; body {}",
                    response.body
                )
            });
        assert!(
            detail["message"]
                .as_str()
                .is_some_and(|message| message.contains(phrase)),
            "`{pattern}`'s reason must be its own, containing {phrase:?}; got {detail}"
        );

        // The second subject: the remedy the reason names still stores.
        let remedy_key = format!("negated-remedy-{index}");
        let mut remedy_document = definition(&remedy_key);
        remedy_document["components"][0]["rules"] = json!([{
            "rule": "regex", "scope": "both",
            "params": {"pattern": remedy},
            "message": "No.",
        }]);

        let stored = app
            .send(
                Method::POST,
                "/api/v1/rad/forms",
                Some(&token),
                Some(json!({"formKey": remedy_key, "title": "Remedy", "definition": remedy_document})),
            )
            .await;

        assert_eq!(
            stored.status,
            StatusCode::CREATED,
            "`{remedy}` is what `{pattern}`'s refusal tells an author to write, so it must store; body {}",
            stored.body
        );
    }
}

/// **[#391] AC1 and AC3, at the endpoint.** A pattern this backend cannot
/// compile is refused where the definition is written.
///
/// **The defect this ends is a field nobody can fill.** Stored, the rule
/// rejected every value — `matches_pattern` maps a compile error to `false` —
/// with no error naming the pattern, because a rule that could not be applied
/// has not been satisfied. That is right at submit and wrong at save, which is
/// the reasoning [SDD] §8.2.3 gives for checking a definition when it is
/// written.
///
/// **A second subject, because one component cannot tell *refused because
/// uncompilable* from *refused at all***: the ordinary pattern beside it must
/// not be named in the refusal.
///
/// [#391]: https://github.com/sujanto-gaws/kelir/issues/391
/// [SDD]: ../../docs/design/01.%20System%20Design%20Document.md
#[tokio::test]
async fn a_pattern_this_backend_cannot_compile_is_refused_at_save() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let mut document = definition("uncompilable-pattern");
    document["components"][0]["rules"] = json!([{
        "rule": "regex", "scope": "both",
        "params": {"pattern": "(?=.*[A-Z])[A-Za-z]{8,}"},
        "message": "Needs a capital.",
    }]);

    let response = app
        .send(
            Method::POST,
            "/api/v1/rad/forms",
            Some(&token),
            Some(json!({
                "formKey": "uncompilable-pattern",
                "title": "Uncompilable pattern",
                "definition": document,
            })),
        )
        .await;

    assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);

    let detail = response.body["error"]["details"]
        .as_array()
        .expect("details")
        .iter()
        .find(|detail| detail["code"] == "PATTERN_NOT_COMPILABLE")
        .cloned()
        .unwrap_or_else(|| panic!("body {}", response.body));

    assert!(
        detail["message"]
            .as_str()
            .is_some_and(|message| message.contains("look-around")),
        "the refusal must carry the compiler's own reason; got {detail}"
    );

    // The second subject: the same definition with a pattern this crate builds
    // is stored, so the refusal is the pattern rather than the shape of the
    // request.
    let mut ordinary = definition("compilable-pattern");
    ordinary["components"][0]["rules"] = json!([{
        "rule": "regex", "scope": "both",
        "params": {"pattern": "^[A-Z]{2}-[0-9]{4}$"},
        "message": "Two letters, a dash, four digits.",
    }]);

    let accepted = app
        .send(
            Method::POST,
            "/api/v1/rad/forms",
            Some(&token),
            Some(json!({
                "formKey": "compilable-pattern",
                "title": "Compilable pattern",
                "definition": ordinary,
            })),
        )
        .await;

    assert_eq!(
        accepted.status,
        StatusCode::CREATED,
        "a pattern this backend compiles must still be storable: {}",
        accepted.body
    );
}

/// **AC3, at the endpoint.** Two fields that calculate from each other are a
/// definition JFSS S12.2 calls invalid, and it is refused where it is written.
#[tokio::test]
async fn a_cyclic_definition_is_refused_at_save_and_names_both_fields() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let mut document = definition("cyclic");
    document["components"] = json!([
        {"id": "a", "role": "data", "type": "number", "key": "a", "label": "A",
         "validation": {"type": "number"}, "calculate": {"+": [{"var": "b"}, 1]}},
        {"id": "b", "role": "data", "type": "number", "key": "b", "label": "B",
         "validation": {"type": "number"}, "calculate": {"+": [{"var": "a"}, 1]}}
    ]);

    let response = app
        .send(
            Method::POST,
            "/api/v1/rad/forms",
            Some(&token),
            Some(json!({
                "formKey": "cyclic",
                "title": "Cyclic",
                "definition": document,
            })),
        )
        .await;

    assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);

    let detail = response.body["error"]["details"]
        .as_array()
        .expect("details")
        .iter()
        .find(|detail| detail["code"] == "CALCULATION_CYCLE")
        .cloned()
        .unwrap_or_else(|| panic!("body {}", response.body));
    let message = detail["message"].as_str().unwrap_or_default();

    assert!(message.contains("`a`"), "{message}");
    assert!(message.contains("`b`"), "{message}");
}

/// **AC3's other half: the publish itself refuses**, not only the write that
/// preceded it.
///
/// The draft is written through the repository rather than the API, because the
/// API now refuses it — which is exactly the state this guards. A definition
/// stored before a check existed keeps what it was stored with, and publishing
/// is the one-way door: after it, documents pin the revision and the definition
/// can never be corrected in place. So the **stored** definition is what is
/// checked, at the moment it goes live.
#[tokio::test]
async fn publishing_a_definition_the_engine_refuses_is_refused() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let created = create_form(&app, &token, "cyclic-draft", definition("cyclic-draft")).await;
    let id = id_of(&created);

    let mut cyclic = definition("cyclic-draft");
    cyclic["components"] = json!([
        {"id": "a", "role": "data", "type": "number", "key": "a", "label": "A",
         "validation": {"type": "number"}, "calculate": {"+": [{"var": "b"}, 1]}},
        {"id": "b", "role": "data", "type": "number", "key": "b", "label": "B",
         "validation": {"type": "number"}, "calculate": {"+": [{"var": "a"}, 1]}}
    ]);

    assert_eq!(
        form_repo::update_draft(
            &app.pool,
            fixtures::SYSTEM_TENANT_ID,
            id,
            &form_repo::FormFields {
                title: None,
                definition_json: Some(&cyclic),
                entity_id: None,
            },
            None,
        )
        .await
        .expect("the draft is rewritten behind the API"),
        1
    );

    let response = app
        .send(
            Method::POST,
            &format!("/api/v1/rad/forms/{id}/publish"),
            Some(&token),
            None,
        )
        .await;

    assert_eq!(
        response.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a cyclic definition must not go live; body {}",
        response.body
    );

    let read = app
        .send(
            Method::GET,
            &format!("/api/v1/rad/forms/{id}"),
            Some(&token),
            None,
        )
        .await;

    assert_eq!(
        read.body["data"]["status"], "DRAFT",
        "the refused publish must leave the revision a draft; body {}",
        read.body
    );
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

// -- A rule with nothing to compare against (#722) -------------------------

/// The five rules whose one parameter decides the verdict, each with the
/// parameter the builder seeds empty when *Add rule* is pressed.
const EMPTY_RULES: [(&str, &str); 5] = [
    ("matchesField", "target"),
    ("notMatchesField", "target"),
    ("regex", "pattern"),
    ("oneOf", "values"),
    ("notOneOf", "values"),
];

/// `definition(form_id)` with one rule on `quantity`, its parameter empty.
fn definition_with_empty_rule(form_id: &str, rule: &str, param: &str) -> Value {
    let empty = if param == "values" {
        json!([])
    } else {
        json!("")
    };
    let mut document = definition(form_id);

    document["components"][0]["rules"] = json!([{
        "rule": rule, "scope": "both", "params": { param: empty }, "message": "No.",
    }]);
    document
}

/// The one detail a refusal carries at `path`, which must be `code` for `rule`.
fn refusal_at<'a>(body: &'a Value, path: &str, code: &str, rule: &str) -> &'a Value {
    let at_path: Vec<&Value> = body["error"]["details"]
        .as_array()
        .unwrap_or_else(|| panic!("no details: {body}"))
        .iter()
        .filter(|detail| detail["path"] == path)
        .collect();

    assert_eq!(at_path.len(), 1, "one detail at {path}; body {body}");
    assert_eq!(at_path[0]["code"], code, "body {body}");
    assert_eq!(at_path[0]["rule"], rule, "body {body}");
    at_path[0]
}

/// Writes `definition` into a draft without the checks the API runs — the
/// state a draft stored before #722 is in.
async fn store_behind_the_api(app: &TestApp, id: Uuid, definition: &Value) {
    let affected = sqlx::query("UPDATE rad_forms SET definition_json = $1 WHERE id = $2")
        .bind(definition)
        .bind(id)
        .execute(&app.pool)
        .await
        .expect("the draft is rewritten behind the API")
        .rows_affected();

    assert_eq!(affected, 1);
}

/// **[#722] AC1–AC3 and AC9, at create.** Each rule the builder seeds empty is
/// refused at its parameter's path, which is where the builder lands it.
///
/// **Seen red, 2026-10-15**: with `check_rule_params` removed from
/// `check_scope`, every rule here stores with a `201`, and the five refusal
/// tests below redden with this one — six of the seven; the filled-in second
/// subject stays green. Making the `RULE_TARGET_NOT_FOUND` guard `false`
/// reddens the scope test alone.
///
/// [#722]: https://github.com/sujanto-gaws/kelir/issues/722
#[tokio::test]
async fn an_empty_rule_parameter_is_refused_at_create() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    for (rule, param) in EMPTY_RULES {
        let key = format!("empty-create-{}", rule.to_lowercase());
        let response = app
            .send(
                Method::POST,
                "/api/v1/rad/forms",
                Some(&token),
                Some(json!({
                    "formKey": key,
                    "title": "Empty rule",
                    "definition": definition_with_empty_rule(&key, rule, param),
                })),
            )
            .await;

        assert_eq!(
            response.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{rule} with an empty `{param}` must not be stored; body {}",
            response.body
        );

        let detail = refusal_at(
            &response.body,
            &format!("definition.components.0.rules.0.params.{param}"),
            "RULE_PARAM_MISSING",
            rule,
        );
        let message = detail["message"].as_str().unwrap_or_default();
        assert!(message.contains("`quantity`"), "{message}");
        assert!(message.contains(&format!("`{param}`")), "{message}");
    }
}

/// **AC4 and AC5 at the endpoint**: a row's rule naming a top-level field, and
/// a rule naming its own key, in one answer (AC8).
#[tokio::test]
async fn a_target_outside_the_rules_scope_or_its_own_key_is_refused_at_create() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let mut document = definition("misdirected-target");
    document["components"][0]["rules"] = json!([{
        "rule": "notMatchesField", "scope": "server",
        "params": {"target": "quantity"}, "message": "No.",
    }]);
    document["components"]
        .as_array_mut()
        .expect("components")
        .push(json!({
            "id": "lines", "role": "data", "type": "datagrid", "key": "lines",
            "label": "Lines", "validation": {"type": "array"},
            "components": [{
                "id": "line_qty", "role": "data", "type": "number", "key": "line_qty",
                "label": "Line quantity", "validation": {"type": "number"},
                "rules": [{
                    "rule": "matchesField", "scope": "both",
                    "params": {"target": "quantity"}, "message": "No.",
                }]
            }]
        }));

    let response = app
        .send(
            Method::POST,
            "/api/v1/rad/forms",
            Some(&token),
            Some(json!({
                "formKey": "misdirected-target",
                "title": "Misdirected target",
                "definition": document,
            })),
        )
        .await;

    assert_eq!(
        response.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "body {}",
        response.body
    );
    refusal_at(
        &response.body,
        "definition.components.0.rules.0.params.target",
        "RULE_PARAM_INVALID",
        "notMatchesField",
    );
    refusal_at(
        &response.body,
        "definition.components.1.components.0.rules.0.params.target",
        "RULE_TARGET_NOT_FOUND",
        "matchesField",
    );
}

/// **AC9, at update.** The draft is fine; the edit that empties a rule is not.
#[tokio::test]
async fn an_empty_rule_parameter_is_refused_at_update() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    for (rule, param) in EMPTY_RULES {
        let key = format!("empty-update-{}", rule.to_lowercase());
        let id = id_of(&create_form(&app, &token, &key, definition(&key)).await);

        let response = app
            .send(
                Method::PUT,
                &format!("/api/v1/rad/forms/{id}"),
                Some(&token),
                Some(json!({ "definition": definition_with_empty_rule(&key, rule, param) })),
            )
            .await;

        assert_eq!(
            response.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{rule}: body {}",
            response.body
        );
        refusal_at(
            &response.body,
            &format!("definition.components.0.rules.0.params.{param}"),
            "RULE_PARAM_MISSING",
            rule,
        );
    }
}

/// **AC9 and AC11, at publish (D-67).** A draft stored before this check keeps
/// what it was stored with, and publishing is the one-way door, so the stored
/// definition is checked there and the revision stays a draft.
#[tokio::test]
async fn a_stored_empty_rule_parameter_is_refused_at_publish() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    for (rule, param) in EMPTY_RULES {
        let key = format!("empty-publish-{}", rule.to_lowercase());
        let id = id_of(&create_form(&app, &token, &key, definition(&key)).await);
        store_behind_the_api(&app, id, &definition_with_empty_rule(&key, rule, param)).await;

        let response = app
            .send(
                Method::POST,
                &format!("/api/v1/rad/forms/{id}/publish"),
                Some(&token),
                None,
            )
            .await;

        assert_eq!(
            response.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{rule}: body {}",
            response.body
        );
        refusal_at(
            &response.body,
            &format!("definition.components.0.rules.0.params.{param}"),
            "RULE_PARAM_MISSING",
            rule,
        );

        let status: String = sqlx::query_scalar("SELECT status FROM rad_forms WHERE id = $1")
            .bind(id)
            .fetch_one(&app.pool)
            .await
            .expect("the form is queryable");
        assert_eq!(status, "DRAFT", "{rule}: a refused publish leaves a draft");
    }
}

/// **AC9: a revision copies its source unchecked**, because a request with no
/// definition in it has nothing new to validate — so the copy is caught where
/// it would go live.
#[tokio::test]
async fn a_revision_copying_an_empty_rule_is_refused_at_publish() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let source =
        id_of(&create_form(&app, &token, "empty-copied", definition("empty-copied")).await);
    store_behind_the_api(
        &app,
        source,
        &definition_with_empty_rule("empty-copied", "oneOf", "values"),
    )
    .await;

    let next = app
        .send(
            Method::POST,
            &format!("/api/v1/rad/forms/{source}/revisions"),
            Some(&token),
            Some(json!({ "title": "Copied" })),
        )
        .await;
    assert_eq!(next.status, StatusCode::CREATED, "body {}", next.body);
    let copy = id_of(&next.body["data"]);

    let response = app
        .send(
            Method::POST,
            &format!("/api/v1/rad/forms/{copy}/publish"),
            Some(&token),
            None,
        )
        .await;

    assert_eq!(
        response.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "body {}",
        response.body
    );
    refusal_at(
        &response.body,
        "definition.components.0.rules.0.params.values",
        "RULE_PARAM_MISSING",
        "oneOf",
    );
}

/// **AC9 and AC11: a title is not a definition.** A title-only update of a
/// stored draft in this state still saves; the draft is refused at publish
/// until its definition is corrected, and publishes after.
#[tokio::test]
async fn a_stored_empty_rule_keeps_its_title_edits_and_publishes_once_corrected() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let id =
        id_of(&create_form(&app, &token, "empty-retitled", definition("empty-retitled")).await);
    store_behind_the_api(
        &app,
        id,
        &definition_with_empty_rule("empty-retitled", "regex", "pattern"),
    )
    .await;

    let retitled = app
        .send(
            Method::PUT,
            &format!("/api/v1/rad/forms/{id}"),
            Some(&token),
            Some(json!({ "title": "Retitled" })),
        )
        .await;
    assert_eq!(retitled.status, StatusCode::OK, "body {}", retitled.body);
    assert_eq!(retitled.body["data"]["title"], "Retitled");

    // Retitled, and still not publishable.
    let refused = app
        .send(
            Method::POST,
            &format!("/api/v1/rad/forms/{id}/publish"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "body {}",
        refused.body
    );
    refusal_at(
        &refused.body,
        "definition.components.0.rules.0.params.pattern",
        "RULE_PARAM_MISSING",
        "regex",
    );

    let mut corrected = definition_with_empty_rule("empty-retitled", "regex", "pattern");
    corrected["components"][0]["rules"][0]["params"]["pattern"] = json!("^[0-9]+$");

    let saved = app
        .send(
            Method::PUT,
            &format!("/api/v1/rad/forms/{id}"),
            Some(&token),
            Some(json!({ "definition": corrected })),
        )
        .await;
    assert_eq!(saved.status, StatusCode::OK, "body {}", saved.body);

    let published = app
        .send(
            Method::POST,
            &format!("/api/v1/rad/forms/{id}/publish"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(published.status, StatusCode::OK, "body {}", published.body);
}

/// **The second subject** ([coding standard] §2.9): all five rules filled in,
/// a row's rule naming its sibling among them, still create, update and
/// publish.
///
/// [coding standard]: ../../docs/standards/01.%20Coding%20Standard.md
#[tokio::test]
async fn all_five_rules_filled_in_create_update_and_publish() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let rule = |rule: &str, params: Value| json!({"rule": rule, "scope": "both", "params": params, "message": "No."});
    let mut document = definition("all-five");
    document["components"][0]["rules"] = json!([
        rule("oneOf", json!({"values": [1, 2, 3]})),
        rule("notOneOf", json!({"values": [0]})),
    ]);
    document["components"]
        .as_array_mut()
        .expect("components")
        .extend([
            json!({
                "id": "reference", "role": "data", "type": "textfield", "key": "reference",
                "label": "Reference", "validation": {"type": "string"},
                "rules": [rule("regex", json!({"pattern": "^[A-Z]{2}-[0-9]{4}$"}))]
            }),
            json!({
                "id": "confirm", "role": "data", "type": "textfield", "key": "confirm",
                "label": "Confirm", "validation": {"type": "string"},
                "rules": [rule("matchesField", json!({"target": "reference"}))]
            }),
            json!({
                "id": "lines", "role": "data", "type": "datagrid", "key": "lines",
                "label": "Lines", "validation": {"type": "array"},
                "components": [
                    {"id": "from", "role": "data", "type": "textfield", "key": "from",
                     "label": "From", "validation": {"type": "string"}},
                    {"id": "to", "role": "data", "type": "textfield", "key": "to",
                     "label": "To", "validation": {"type": "string"},
                     "rules": [rule("notMatchesField", json!({"target": "from"}))]}
                ]
            }),
        ]);

    let id = id_of(&create_form(&app, &token, "all-five", document.clone()).await);

    let updated = app
        .send(
            Method::PUT,
            &format!("/api/v1/rad/forms/{id}"),
            Some(&token),
            Some(json!({ "definition": document })),
        )
        .await;
    assert_eq!(updated.status, StatusCode::OK, "body {}", updated.body);

    let published = app
        .send(
            Method::POST,
            &format!("/api/v1/rad/forms/{id}/publish"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(published.status, StatusCode::OK, "body {}", published.body);
}

// -- A shape a browser can take exponential time on (#604) ------------------

/// The phrase each shape's refusal names it by, and the limit every refusal
/// carries ([#604], **D-93**).
///
/// [#604]: https://github.com/sujanto-gaws/kelir/issues/604
const REPEATED_QUANTIFIER: &str = "is itself repeated without one";
const REPEATED_ALTERNATION: &str = "alternatives are separated by `|`";
const SHAPE_LIMIT: &str = "says nothing about any other pattern";

/// One text field, `code`, holding `pattern` as a `regex` rule of `scope`
/// (`rule` is `regex`) or as §5's keyword (`rule` is `pattern`).
fn text_field(pattern: &str, rule: &str, scope: &str) -> Value {
    let mut field = json!({
        "id": "code", "role": "data", "type": "textfield", "key": "code",
        "label": "Code", "validation": {"type": "string"},
    });

    if rule == "regex" {
        field["rules"] = json!([{
            "rule": "regex", "scope": scope,
            "params": {"pattern": pattern}, "message": "No.",
        }]);
    } else {
        field["validation"]["pattern"] = json!(pattern);
    }
    field
}

fn definition_holding(form_id: &str, component: Value) -> Value {
    json!({
        "formId": form_id,
        "version": "2.0.1",
        "title": "Code",
        "components": [component],
    })
}

/// `component` as the one field of a datagrid's row template.
fn in_a_row(component: Value) -> Value {
    json!({
        "id": "lines", "role": "data", "type": "datagrid", "key": "lines",
        "label": "Lines", "validation": {"type": "array"},
        "components": [component],
    })
}

async fn post_form(
    app: &TestApp,
    token: &str,
    key: &str,
    definition: Value,
) -> common::TestResponse {
    app.send(
        Method::POST,
        "/api/v1/rad/forms",
        Some(token),
        Some(json!({"formKey": key, "title": "Code", "definition": definition})),
    )
    .await
}

/// The refusal at `path` is the shape's, naming `shape` and carrying the limit.
fn backtracking_refusal_at(body: &Value, path: &str, rule: &str, shape: &str) {
    let detail = refusal_at(body, path, "PATTERN_BACKTRACKING_SHAPE", rule);
    let message = detail["message"].as_str().unwrap_or_default();

    assert!(
        message.contains(shape),
        "the refusal names the shape: {message}"
    );
    assert!(
        message.contains("exponential time"),
        "the refusal says what a browser may do: {message}"
    );
    assert!(message.contains(SHAPE_LIMIT), "{message}");
}

/// **[#604] AC1: a group repeated without an upper bound, around one repeated
/// item or an alternation, is refused where the definition is written.**
///
/// The server's engine never backtracks, and ADR-0038 §1's argument rested on
/// that; the browser evaluates the same rule with an engine that does. Record
/// 17 finding 9 measured `(?:[a-z]|[a-z0-9])*$` splitting in Firefox and
/// WebKit, and every refused case below stored with a `201`.
///
/// **Each refused pattern sits beside one that stores** ([coding standard]
/// §2.9), and the stored ones are the edges the narrow rule leaves open on
/// purpose: a group of more than one item, a bounded repeat, an alternation
/// that is not repeated.
///
/// **`(\w*)*`, which the row's criteria list among the refused, is refused as
/// the divergence it also is**: a pattern both divergent and backtracking
/// reports the divergence alone, so the shape's case is spelled `([a-z]*)*`
/// here and `(\w*)*` is asserted for that precedence below.
///
/// [#604]: https://github.com/sujanto-gaws/kelir/issues/604
/// [coding standard]: ../../docs/standards/01.%20Coding%20Standard.md
#[tokio::test]
async fn a_shape_a_browser_backtracks_on_is_refused_at_save() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    // (refused, the shape its message names, a pattern beside it that stores)
    let cases = [
        (
            "^(a+)+$",
            REPEATED_QUANTIFIER,
            r"^([a-z0-9-]+\.)+[a-z]{2,}$",
        ),
        ("([a-z]*)*", REPEATED_QUANTIFIER, r"[a-z]+(\.[a-z]+)*"),
        ("((a+))+", REPEATED_QUANTIFIER, "^(?:[a-z][0-9])*$"),
        ("(?:[a-z]|[a-z0-9])*$", REPEATED_ALTERNATION, "^(png|jpg)$"),
        ("^(cat|dog)+$", REPEATED_ALTERNATION, "^(a+){2}$"),
        // The cost the narrow rule states, beside the list rewrite ADR-0038
        // and the Validation Rule Registry cite for it.
        (
            "^(png|jpg)+$",
            REPEATED_ALTERNATION,
            "^(png|jpg)(,(png|jpg))*$",
        ),
        // A class opening with `]` ends there in the browser, so `(a+)+` is
        // outside it; with a member first, the group is not repeated.
        ("[^](a+)+]", REPEATED_QUANTIFIER, "[^a](a+)"),
    ];
    let places = [
        ("regex", "definition.components.0.rules.0.params.pattern"),
        ("pattern", "definition.components.0.validation.pattern"),
    ];

    for (index, (pattern, shape, accepted)) in cases.iter().enumerate() {
        for (rule, path) in places {
            let key = format!("backtracking-{rule}-{index}");
            let response = post_form(
                &app,
                &token,
                &key,
                definition_holding(&key, text_field(pattern, rule, "both")),
            )
            .await;

            assert_eq!(
                response.status,
                StatusCode::UNPROCESSABLE_ENTITY,
                "`{pattern}` as `{rule}` must not be stored; body {}",
                response.body
            );
            backtracking_refusal_at(&response.body, path, rule, shape);

            let accepted_key = format!("not-named-{rule}-{index}");
            let stored = post_form(
                &app,
                &token,
                &accepted_key,
                definition_holding(&accepted_key, text_field(accepted, rule, "both")),
            )
            .await;

            assert_eq!(
                stored.status,
                StatusCode::CREATED,
                "`{accepted}` as `{rule}` is not a shape the check names, so it stores; body {}",
                stored.body
            );
        }
    }

    // A row template is checked at its own path.
    let response = post_form(
        &app,
        &token,
        "backtracking-row",
        definition_holding(
            "backtracking-row",
            in_a_row(text_field("^(cat|dog)+$", "regex", "both")),
        ),
    )
    .await;

    assert_eq!(
        response.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "body {}",
        response.body
    );
    backtracking_refusal_at(
        &response.body,
        "definition.components.0.components.0.rules.0.params.pattern",
        "regex",
        REPEATED_ALTERNATION,
    );

    let stored = post_form(
        &app,
        &token,
        "not-named-row",
        definition_holding(
            "not-named-row",
            in_a_row(text_field(r"^([a-z0-9-]+\.)+[a-z]{2,}$", "regex", "both")),
        ),
    )
    .await;
    assert_eq!(stored.status, StatusCode::CREATED, "body {}", stored.body);

    // Divergent and backtracking: the divergence alone.
    let response = post_form(
        &app,
        &token,
        "backtracking-divergent",
        definition_holding(
            "backtracking-divergent",
            text_field(r"(\w*)*", "regex", "both"),
        ),
    )
    .await;

    assert_eq!(
        response.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "body {}",
        response.body
    );
    refusal_at(
        &response.body,
        "definition.components.0.rules.0.params.pattern",
        "PATTERN_CLASS_NOT_PINNED",
        "regex",
    );
}

/// **[#604] AC6: a `client`-scoped rule is refused too.** The server skips
/// it at submit, and the browser, which runs it, is the engine at risk.
///
/// [#604]: https://github.com/sujanto-gaws/kelir/issues/604
#[tokio::test]
async fn a_client_scoped_regex_with_the_shape_is_refused_at_save() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let response = post_form(
        &app,
        &token,
        "backtracking-client",
        definition_holding(
            "backtracking-client",
            text_field("^(a+)+$", "regex", "client"),
        ),
    )
    .await;

    assert_eq!(
        response.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a client-scoped rule runs in the browser, so it must not be stored; body {}",
        response.body
    );
    backtracking_refusal_at(
        &response.body,
        "definition.components.0.rules.0.params.pattern",
        "regex",
        REPEATED_QUANTIFIER,
    );

    // The second subject: the same scope, a pattern the check does not name.
    let stored = post_form(
        &app,
        &token,
        "not-named-client",
        definition_holding("not-named-client", text_field("^a+$", "regex", "client")),
    )
    .await;
    assert_eq!(stored.status, StatusCode::CREATED, "body {}", stored.body);
}

/// **[#604] AC4: a draft stored with the shape reads back, and is refused at
/// the next write and at publish.**
///
/// It is written behind the API, as
/// `publishing_a_definition_the_engine_refuses_is_refused` writes its own,
/// because the API now refuses it: this is a draft saved before the check.
///
/// **Green on `main` by design**: the `GET`. The `PUT` and the publish are
/// the halves seen red.
///
/// [#604]: https://github.com/sujanto-gaws/kelir/issues/604
#[tokio::test]
async fn a_draft_stored_with_the_shape_reads_back_and_is_refused_at_publish() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let path = "definition.components.0.rules.0.params.pattern";

    let id = id_of(
        &create_form(
            &app,
            &token,
            "backtracking-draft",
            definition_holding("backtracking-draft", text_field("^a+$", "regex", "both")),
        )
        .await,
    );
    let stored = definition_holding("backtracking-draft", text_field("^(a+)+$", "regex", "both"));

    assert_eq!(
        form_repo::update_draft(
            &app.pool,
            fixtures::SYSTEM_TENANT_ID,
            id,
            &form_repo::FormFields {
                title: None,
                definition_json: Some(&stored),
                entity_id: None,
            },
            None,
        )
        .await
        .expect("the draft is rewritten behind the API"),
        1
    );

    let read = app
        .send(
            Method::GET,
            &format!("/api/v1/rad/forms/{id}"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(read.status, StatusCode::OK, "body {}", read.body);
    assert_eq!(
        read.body["data"]["definition"]["components"][0]["rules"][0]["params"]["pattern"],
        "^(a+)+$",
        "the stored pattern reads back unchanged"
    );

    let rewritten = app
        .send(
            Method::PUT,
            &format!("/api/v1/rad/forms/{id}"),
            Some(&token),
            Some(json!({ "definition": stored })),
        )
        .await;
    assert_eq!(
        rewritten.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "body {}",
        rewritten.body
    );
    backtracking_refusal_at(&rewritten.body, path, "regex", REPEATED_QUANTIFIER);

    let published = app
        .send(
            Method::POST,
            &format!("/api/v1/rad/forms/{id}/publish"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(
        published.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "the stored shape must not go live; body {}",
        published.body
    );
    backtracking_refusal_at(&published.body, path, "regex", REPEATED_QUANTIFIER);

    let status: String = sqlx::query_scalar("SELECT status FROM rad_forms WHERE id = $1")
        .bind(id)
        .fetch_one(&app.pool)
        .await
        .expect("the form is queryable");
    assert_eq!(status, "DRAFT", "a refused publish leaves a draft");
}

/// **[#604] AC5: a revision already published with the shape keeps working,
/// and the revision after it is refused.**
///
/// The server's evaluator is unchanged, so a document is created and
/// submitted against the published revision as before, and a value that does
/// not match is answered rather than left running. A new revision carrying
/// the shape is refused; one with no definition copies it unchecked
/// (`create_revision` in `service/form.rs`), and meets the refusal at publish.
///
/// **Green on `main` by design**: the reads, the document and the two `201`s.
/// The revision carrying the shape and the copy's publish are the halves seen
/// red.
///
/// [#604]: https://github.com/sujanto-gaws/kelir/issues/604
#[tokio::test]
async fn a_published_revision_with_the_shape_works_and_its_next_revision_is_refused() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let path = "definition.components.0.rules.0.params.pattern";
    let corrected = definition_holding("backtracking-live", text_field("^a+$", "regex", "both"));
    let shape = definition_holding("backtracking-live", text_field("^(a+)+$", "regex", "both"));

    let form = id_of(&create_form(&app, &token, "backtracking-live", corrected.clone()).await);
    let published = app
        .send(
            Method::POST,
            &format!("/api/v1/rad/forms/{form}/publish"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(published.status, StatusCode::OK, "body {}", published.body);
    store_behind_the_api(&app, form, &shape).await;

    let read = app
        .send(
            Method::GET,
            &format!("/api/v1/rad/forms/{form}"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(read.status, StatusCode::OK, "body {}", read.body);
    assert_eq!(read.body["data"]["definition"], shape);

    // A document against it: created, submitted, and a value that does not
    // match answered.
    let created = app
        .send(
            Method::POST,
            "/api/v1/document-types",
            Some(&token),
            Some(json!({"typeCode": "BACKTRACKING", "name": "Backtracking", "formId": form})),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "body {}", created.body);
    let type_id = id_of(&created.body["data"]);

    let rule = app
        .send(
            Method::PUT,
            &format!("/api/v1/document-types/{type_id}/numbering-rule"),
            Some(&token),
            Some(json!({
                "ruleTemplate": "BT-{year}-{sequence}",
                "sequenceScope": "YEAR",
                "gapPolicy": "GAPLESS",
            })),
        )
        .await;
    assert_eq!(rule.status, StatusCode::OK, "body {}", rule.body);

    let draft = app
        .send(
            Method::POST,
            "/api/v1/documents",
            Some(&token),
            Some(json!({"documentTypeId": type_id, "title": "Matches", "formData": {"code": "aaaa"}})),
        )
        .await;
    assert_eq!(draft.status, StatusCode::CREATED, "body {}", draft.body);
    let document = id_of(&draft.body["data"]);

    let submitted = app
        .send(
            Method::POST,
            &format!("/api/v1/documents/{document}/submission"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(submitted.status, StatusCode::OK, "body {}", submitted.body);
    assert_eq!(submitted.body["data"]["status"], "SUBMITTED");

    let not_matching = format!("{}!", "a".repeat(29));
    let refused = tokio::time::timeout(
        Duration::from_secs(10),
        app.send(
            Method::POST,
            "/api/v1/documents",
            Some(&token),
            Some(json!({
                "documentTypeId": type_id,
                "title": "Does not match",
                "formData": {"code": not_matching},
            })),
        ),
    )
    .await
    .expect("a 30-character value that does not match is answered");
    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "the rule still refuses what it does not match; body {}",
        refused.body
    );

    // The next revision: carrying the shape, corrected, and copied.
    let revisions = format!("/api/v1/rad/forms/{form}/revisions");
    let revise = |body: Value| app.send(Method::POST, &revisions, Some(&token), Some(body));

    let carrying = revise(json!({ "definition": shape })).await;
    assert_eq!(
        carrying.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "body {}",
        carrying.body
    );
    backtracking_refusal_at(&carrying.body, path, "regex", REPEATED_QUANTIFIER);

    let fixed = revise(json!({ "definition": corrected })).await;
    assert_eq!(fixed.status, StatusCode::CREATED, "body {}", fixed.body);

    let copied = revise(json!({ "title": "Copied" })).await;
    assert_eq!(copied.status, StatusCode::CREATED, "body {}", copied.body);
    let copy = id_of(&copied.body["data"]);

    let published = app
        .send(
            Method::POST,
            &format!("/api/v1/rad/forms/{copy}/publish"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(
        published.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "the copy is checked where it would go live; body {}",
        published.body
    );
    backtracking_refusal_at(&published.body, path, "regex", REPEATED_QUANTIFIER);
}
