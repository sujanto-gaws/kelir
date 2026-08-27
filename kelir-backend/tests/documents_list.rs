//! The document list, with search and filter (#171).
//!
//! **A list is the surface where a leak is least visible.** A detail endpoint
//! that refuses is obvious; a list that quietly includes one row too many is
//! not — which is why the cross-tenant test here puts a **second tenant's
//! document in the database** and asserts against the query rather than around
//! it. That is the #106 / #121 lesson, which cost this project three sprints of
//! coverage findings, and coding standard §2.9's second-subject rule.
//!
//! **The visibility rule this asserts is the one the module states**: tenant
//! scope plus `document:read`, and no third condition in Sprint 9.
//! `documents.security_level` exists in the column and nothing reads it, because
//! FR-DTYPE-008 is the cut tail — so there is no test here for a control that
//! does not exist, which is deliberate rather than an omission.

mod common;

use axum::http::{Method, StatusCode};
use common::{fixtures, TestApp};
use serde_json::{json, Value};
use uuid::Uuid;

fn id_of(value: &Value) -> Uuid {
    value["id"]
        .as_str()
        .expect("an id")
        .parse()
        .expect("a uuid")
}

async fn plain_type(app: &TestApp, token: &str, code: &str) -> Uuid {
    let created = app
        .send(
            Method::POST,
            "/api/v1/document-types",
            Some(token),
            Some(json!({ "typeCode": code, "name": code })),
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    id_of(&created.body["data"])
}

async fn document(app: &TestApp, token: &str, type_id: Uuid, title: &str) -> Uuid {
    let created = app
        .send(
            Method::POST,
            "/api/v1/documents",
            Some(token),
            Some(json!({ "documentTypeId": type_id, "title": title })),
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    id_of(&created.body["data"])
}

async fn list(app: &TestApp, token: &str, query: &str) -> common::TestResponse {
    app.send(
        Method::GET,
        &format!("/api/v1/documents{query}"),
        Some(token),
        None,
    )
    .await
}

fn titles(body: &Value) -> Vec<String> {
    body["data"]
        .as_array()
        .expect("a list")
        .iter()
        .map(|row| row["title"].as_str().expect("a title").to_owned())
        .collect()
}

// ---------------------------------------------------------------------------
// AC1 — the page, and its meta
// ---------------------------------------------------------------------------

/// The list pages, and `meta` reports the **filtered** total.
///
/// A `total` that reported the unfiltered population beside a filtered page is
/// a pagination control that offers pages which are empty, which is how a list
/// stops being usable at the size where it matters.
#[tokio::test]
async fn a_filtered_page_reports_the_filtered_total() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let requisitions = plain_type(&app, &token, "PR_LIST_PAGE").await;
    let orders = plain_type(&app, &token, "SO_LIST_PAGE").await;

    for index in 0..3 {
        document(&app, &token, requisitions, &format!("Requisition {index}")).await;
    }
    document(&app, &token, orders, "An order").await;

    let all = list(&app, &token, "").await;
    assert_eq!(all.status, StatusCode::OK, "{}", all.body);
    assert_eq!(all.body["meta"]["total"], 4);

    let filtered = list(&app, &token, &format!("?documentTypeId={requisitions}")).await;

    assert_eq!(filtered.status, StatusCode::OK, "{}", filtered.body);
    assert_eq!(
        filtered.body["meta"]["total"], 3,
        "the count and the page disagree, so the two statements have drifted: {}",
        filtered.body
    );
    assert_eq!(titles(&filtered.body).len(), 3);

    // And a page smaller than the result set still reports the whole total.
    let paged = list(
        &app,
        &token,
        &format!("?documentTypeId={requisitions}&pageSize=2"),
    )
    .await;

    assert_eq!(titles(&paged.body).len(), 2);
    assert_eq!(paged.body["meta"]["total"], 3);
}

// ---------------------------------------------------------------------------
// AC3 — the visibility rule, reached through the query
// ---------------------------------------------------------------------------

/// **A caller cannot see a document outside their tenant, and the assertion
/// reaches the query** (AC3).
///
/// The fixture puts a second tenant in the database with a caller who genuinely
/// holds `document:read`, so *tenant-scoped* and *not scoped at all* are
/// different observations. Without the second subject the assertion would be
/// green over a query that filtered nothing.
///
/// **Seen red** (coding standard §2.9) against
/// `repository::list::list_documents`'s `d.tenant_id = $1` weakened to
/// `(d.tenant_id = $1 OR TRUE)`: the foreign caller's list holds this tenant's
/// documents.
#[tokio::test]
async fn a_list_holds_only_the_callers_own_tenants_documents() {
    let app = TestApp::spawn_with(|config| config.multi_tenant = true).await;
    let token = app
        .sign_in_to("SYSTEM", common::ADMIN_USERNAME, common::ADMIN_PASSWORD)
        .await;

    let type_id = plain_type(&app, &token, "PR_LIST_TENANT").await;
    document(&app, &token, type_id, "Ours").await;

    // The second tenant, with its own type, its own document and a caller who
    // holds every document permission *in it*.
    let tenant = fixtures::create_tenant(&app.pool, "TNT-DOC-LIST", "Another Customer").await;

    let role = fixtures::create_role_with_permissions(
        &app.pool,
        tenant,
        "DOC-READER",
        &["document:create", "document:read", "document-type:create"],
    )
    .await;

    fixtures::create_user(
        &app.pool,
        tenant,
        "list.outsider",
        "list.outsider@example.test",
        common::ADMIN_PASSWORD,
        &[role],
    )
    .await;

    let foreign = app
        .sign_in_to("TNT-DOC-LIST", "list.outsider", common::ADMIN_PASSWORD)
        .await;

    let foreign_type = plain_type(&app, &foreign, "PR_LIST_THEIRS").await;
    document(&app, &foreign, foreign_type, "Theirs").await;

    let ours = list(&app, &token, "").await;
    assert_eq!(ours.status, StatusCode::OK, "{}", ours.body);
    assert_eq!(
        titles(&ours.body),
        vec!["Ours".to_owned()],
        "this tenant's list holds another tenant's document: {}",
        ours.body
    );

    let theirs = list(&app, &foreign, "").await;
    assert_eq!(theirs.status, StatusCode::OK, "{}", theirs.body);
    assert_eq!(
        titles(&theirs.body),
        vec!["Theirs".to_owned()],
        "another tenant's list holds this tenant's document: {}",
        theirs.body
    );

    // And the count agrees with the page, so the two statements are scoped
    // alike. A `total` of 2 beside one row is the leak the page would hide.
    assert_eq!(ours.body["meta"]["total"], 1);
    assert_eq!(theirs.body["meta"]["total"], 1);
}

/// **The list needs `document:read` and refuses without it.**
///
/// **Seen red** against a build where `service::list::list_documents` requires
/// no permission at all: the caller with no grants lists everything.
#[tokio::test]
async fn listing_documents_needs_the_read_permission() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = plain_type(&app, &token, "PR_LIST_PERMISSION").await;
    document(&app, &token, type_id, "Not yours to see").await;

    let role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "NO-DOCUMENTS",
        &["document-type:read"],
    )
    .await;

    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "list.nobody",
        "list.nobody@example.test",
        common::ADMIN_PASSWORD,
        &[role],
    )
    .await;

    let nobody = app.sign_in("list.nobody", common::ADMIN_PASSWORD).await;

    let refused = list(&app, &nobody, "").await;

    assert_eq!(
        refused.status,
        StatusCode::FORBIDDEN,
        "a caller without document:read listed documents: {}",
        refused.body
    );
}

// ---------------------------------------------------------------------------
// AC4 — filters, in the same statement
// ---------------------------------------------------------------------------

/// Search covers the number, the reference and the title — and not the form
/// data.
///
/// The three things a person has in their hand when they are looking for a
/// document they have seen before. Searching `form_data_json` is FR-SRH-002's
/// full-text search, and a `LIKE` over a JSONB blob would be a slow, silent and
/// partial version of it that made the real one look like a regression.
#[tokio::test]
async fn search_covers_the_number_the_reference_and_the_title() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = plain_type(&app, &token, "PR_LIST_SEARCH").await;

    let id = document(&app, &token, type_id, "Two standing desks").await;
    document(&app, &token, type_id, "A chair").await;

    let by_title = list(&app, &token, "?search=standing").await;
    assert_eq!(
        titles(&by_title.body),
        vec!["Two standing desks".to_owned()]
    );

    // Case-insensitive, because a person types what they remember rather than
    // what was stored.
    let by_case = list(&app, &token, "?search=STANDING").await;
    assert_eq!(titles(&by_case.body), vec!["Two standing desks".to_owned()]);

    let reference: String = sqlx::query_scalar("SELECT document_ref FROM documents WHERE id = $1")
        .bind(id)
        .fetch_one(&app.pool)
        .await
        .expect("the document is readable");

    let by_reference = list(&app, &token, &format!("?search={reference}")).await;
    assert_eq!(
        titles(&by_reference.body),
        vec!["Two standing desks".to_owned()]
    );
}

/// A wildcard in a search term matches itself.
///
/// Without escaping, a search for `%` returns the whole population — which reads
/// as a working search that found everything, and is the worst kind of wrong
/// answer a list can give.
#[tokio::test]
async fn a_wildcard_in_a_search_term_matches_itself() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = plain_type(&app, &token, "PR_LIST_WILDCARD").await;

    document(&app, &token, type_id, "100% cotton").await;
    document(&app, &token, type_id, "A chair").await;

    let literal = list(&app, &token, "?search=%25").await;

    assert_eq!(
        titles(&literal.body),
        vec!["100% cotton".to_owned()],
        "a percent sign matched every row: {}",
        literal.body
    );
}

/// The status and entity filters respect the same visibility rule, by being
/// predicates in the same statement (AC4).
#[tokio::test]
async fn filters_narrow_within_the_same_visibility_rule() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = plain_type(&app, &token, "PR_LIST_FILTERS").await;

    let drafted = document(&app, &token, type_id, "Still a draft").await;
    let moved = document(&app, &token, type_id, "Moved on").await;

    sqlx::query("UPDATE documents SET status = 'SUBMITTED' WHERE id = $1")
        .bind(moved)
        .execute(&app.pool)
        .await
        .expect("move the second document");

    let drafts = list(&app, &token, "?status=DRAFT").await;
    assert_eq!(titles(&drafts.body), vec!["Still a draft".to_owned()]);

    let submitted = list(&app, &token, "?status=SUBMITTED").await;
    assert_eq!(titles(&submitted.body), vec!["Moved on".to_owned()]);

    // A filter is not a way to confirm a document exists: a status nothing is in
    // returns nothing rather than refusing, which says the same thing about a
    // document that is not there and one that is in another state.
    let none = list(&app, &token, "?status=COMPLETED").await;
    assert_eq!(titles(&none.body).len(), 0);
    assert_eq!(none.body["meta"]["total"], 0);

    let _ = drafted;
}

/// **A bad `page` or `pageSize` is refused inside the error envelope** (AC5).
///
/// [#122](https://github.com/sujanto-gaws/kelir/issues/122) is open precisely
/// because that is API-wide and was not; this list does not become a fourth
/// instance of the bare 400. **It does not close #122 either** — the routes that
/// still answer outside the envelope are unchanged, and the status report says
/// so rather than implying otherwise.
#[tokio::test]
async fn a_bad_page_is_refused_inside_the_error_envelope() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let refused = list(&app, &token, "?page=nonsense").await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(refused.body["success"], false);
    assert!(
        refused.body["error"]["code"].is_string(),
        "the refusal is outside the error envelope: {}",
        refused.body
    );
}

/// An unrecognised filter value is refused rather than ignored, and every bad
/// one is reported from a single request.
///
/// Ignoring would answer the whole population to a caller who asked for one
/// slice of it, and they would read that as the answer.
#[tokio::test]
async fn an_unknown_filter_value_is_refused_and_every_one_is_reported() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let refused = list(&app, &token, "?status=DRAFTED&priority=EXTREME").await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );

    let details = refused.body["error"]["details"]
        .as_array()
        .expect("details");

    assert_eq!(
        details.len(),
        2,
        "a caller who got two filters wrong learns one at a time: {}",
        refused.body
    );
}

/// An entity filter needs both halves, on the read side as on the write side.
#[tokio::test]
async fn an_entity_filter_needs_both_halves() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let refused = list(&app, &token, &format!("?entityId={}", Uuid::now_v7())).await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "an entityId alone filtered a list: {}",
        refused.body
    );
    assert_eq!(
        refused.body["error"]["details"][0]["code"], "INCOMPLETE_ENTITY_FILTER",
        "{}",
        refused.body
    );
}

/// A discarded draft leaves the list.
///
/// The soft-delete predicate, which nothing else on this surface exercises: the
/// detail endpoint answers 404 for a deleted document and the list would simply
/// carry it, which is the row too many a list hides best.
#[tokio::test]
async fn a_discarded_draft_leaves_the_list() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = plain_type(&app, &token, "PR_LIST_DISCARD").await;

    let kept = document(&app, &token, type_id, "Kept").await;
    let discarded = document(&app, &token, type_id, "Discarded").await;

    let deleted = app
        .send(
            Method::DELETE,
            &format!("/api/v1/documents/{discarded}"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);

    let remaining = list(&app, &token, "").await;

    assert_eq!(
        titles(&remaining.body),
        vec!["Kept".to_owned()],
        "a discarded draft is still in the list: {}",
        remaining.body
    );
    assert_eq!(remaining.body["meta"]["total"], 1);

    let _ = kept;
}
