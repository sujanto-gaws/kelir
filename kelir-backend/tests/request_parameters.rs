//! A bad query or path parameter is refused inside the error envelope (#122).
//!
//! `Pagination.page` and `page_size` are `Option<u32>` deserialized by the
//! extractor, so a value that is not a `u32` was rejected before any handler
//! ran — as a bare `400` with an **empty body**. That is the one refusal shape a
//! client written against the envelope cannot read: it finds `null` where it
//! expects `error.code`. And it was on the two parameters that appear on every
//! list endpoint in the product.
//!
//! The unit tests in `src/extract.rs` pin the extractor. This file pins the
//! thing that matters: **every list endpoint actually answers that way**, across
//! all five modules that have one. A fix applied to the struct under test and
//! not to its neighbours would satisfy those tests and none of the point —
//! which is how the contradiction #122 records arose in the first place, one
//! struct carrying two refusal shapes for two of its own parameter families.

mod common;

use axum::http::StatusCode;
use common::TestApp;
use serde_json::Value;

/// Every **paginated** list endpoint in the product, by module.
///
/// Enumerated rather than discovered, and that is a known weakness: a list that
/// names what it checks has the same failure mode as the thing it is checking
/// (#138 is the same shape on the OpenAPI document). Adding a list route
/// without adding it here leaves it unswept, and nothing here will say so.
///
/// `GET /api/v1/identity/permissions` is deliberately absent: it takes no
/// pagination at all — the permission catalogue is a fixed seeded set and is
/// returned whole — so it has no `page` to get wrong. Recording that here rather
/// than leaving a gap: the sweep's boundary is the paginated endpoints, and this
/// is the one collection route outside it.
const LIST_ENDPOINTS: &[&str] = &[
    "/api/v1/identity/users",
    "/api/v1/identity/roles",
    "/api/v1/master-data/parties",
    "/api/v1/master-data/suppliers",
    "/api/v1/master-data/customers",
    "/api/v1/master-data/employees",
    "/api/v1/master-data/facilities",
    "/api/v1/rad/forms",
    "/api/v1/rad/lists",
    // One source stands for all four: the four share one handler, one
    // query struct and one extractor, so a second row would repeat the
    // same call rather than reach a second refusal path.
    "/api/v1/rad/lookups/supplier/options",
    "/api/v1/document-types",
    "/api/v1/organization/departments",
    "/api/v1/organization/tenants",
];

/// The first validation detail, or `Value::Null` if the envelope carries none.
fn first_detail(response: &common::TestResponse) -> &Value {
    &response.body["error"]["details"][0]
}

#[tokio::test]
async fn a_non_numeric_page_is_refused_in_the_envelope_on_every_list_endpoint() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    for endpoint in LIST_ENDPOINTS {
        let response = app.get(&format!("{endpoint}?page=abc"), Some(&token)).await;

        assert_eq!(
            response.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{endpoint} answered {} with {}",
            response.status,
            response.body
        );
        assert_eq!(
            response.error_code(),
            Some("VALIDATION_ERROR"),
            "{endpoint} answered outside the error envelope: {}",
            response.body
        );
        assert_eq!(
            first_detail(&response)["path"],
            "page",
            "{endpoint} did not name the parameter: {}",
            response.body
        );
    }
}

#[tokio::test]
async fn a_non_numeric_page_size_is_refused_in_the_envelope_on_every_list_endpoint() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    for endpoint in LIST_ENDPOINTS {
        let response = app
            .get(&format!("{endpoint}?pageSize=x"), Some(&token))
            .await;

        assert_eq!(
            response.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{endpoint} answered {} with {}",
            response.status,
            response.body
        );
        // The wire name, not the field name. A caller correcting their request
        // needs the spelling they sent.
        assert_eq!(
            first_detail(&response)["path"],
            "pageSize",
            "{endpoint} did not name the parameter as sent: {}",
            response.body
        );
    }
}

#[tokio::test]
async fn a_negative_page_is_refused_in_the_envelope_on_every_list_endpoint() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    for endpoint in LIST_ENDPOINTS {
        let response = app.get(&format!("{endpoint}?page=-1"), Some(&token)).await;

        assert_eq!(
            response.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{endpoint} answered {} with {}",
            response.status,
            response.body
        );
        assert_eq!(
            response.error_code(),
            Some("VALIDATION_ERROR"),
            "{endpoint} answered outside the error envelope: {}",
            response.body
        );
    }
}

#[tokio::test]
async fn the_refusal_carries_a_body_at_all() {
    // This is the assertion that is *about* #122 rather than about the status.
    // Against the pre-fix code the status was already a refusal; the body was
    // empty, and `error.code` was `null`.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let response = app
        .get("/api/v1/master-data/parties?page=abc", Some(&token))
        .await;

    assert!(
        response.body["error"]["code"].is_string(),
        "the refusal should be readable as the error envelope, got {}",
        response.body
    );
    assert_eq!(response.body["success"], false);
}

#[tokio::test]
async fn a_well_formed_page_still_lists() {
    // The other half: the extractor swap must not have broken the parameters it
    // was meant to keep working.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    for endpoint in LIST_ENDPOINTS {
        let response = app
            .get(&format!("{endpoint}?page=1&pageSize=5"), Some(&token))
            .await;

        assert_eq!(
            response.status,
            StatusCode::OK,
            "{endpoint} answered {} with {}",
            response.status,
            response.body
        );
        assert_eq!(response.body["meta"]["page"], 1, "{endpoint}");
        assert_eq!(response.body["meta"]["pageSize"], 5, "{endpoint}");
    }
}

#[tokio::test]
async fn a_role_view_filter_and_a_bad_page_are_now_the_same_shape() {
    // `RoleViewQuery` parsed its filters by hand *precisely so* a mistyped one
    // would be a 422 in the envelope, and said "like every other bad input" —
    // which was not true of the two paging parameters in the same struct. The
    // contradiction #122 was filed on is that this pair of assertions could not
    // both hold. They now do.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let filter = app
        .get("/api/v1/master-data/suppliers?statusId=NOPE", Some(&token))
        .await;
    let page = app
        .get("/api/v1/master-data/suppliers?page=abc", Some(&token))
        .await;

    assert_eq!(
        filter.status, page.status,
        "{} vs {}",
        filter.body, page.body
    );
    assert_eq!(filter.error_code(), page.error_code());
    assert_eq!(first_detail(&filter)["path"], "statusId");
    assert_eq!(first_detail(&page)["path"], "page");
}

#[tokio::test]
async fn an_unparseable_path_parameter_is_refused_in_the_envelope() {
    // The same hole one extractor over: `/parties/not-a-uuid` was a bare 400
    // with a plain-text body. The status is unchanged — the reference never
    // became one, so it is the request that is bad, not the resource that is
    // missing — and the body is now the envelope.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let response = app
        .get("/api/v1/master-data/parties/not-a-uuid", Some(&token))
        .await;

    assert_eq!(
        response.status,
        StatusCode::BAD_REQUEST,
        "got {} with {}",
        response.status,
        response.body
    );
    assert_eq!(response.error_code(), Some("BAD_REQUEST"));
    assert!(
        response.body["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("id")),
        "the message should name the parameter, got {}",
        response.body
    );
}
