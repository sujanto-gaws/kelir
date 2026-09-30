//! Reading the integration log (FR-INT-006, #548), through the API.
//!
//! Most tests seed `integration_logs` rows directly, with the `started_at`,
//! `status` and system each needs: the order, the filters and the paging are
//! questions about the read, and a row's writer is not. **One test writes its
//! row the real way** — a test call to a mock system on loopback, through the
//! `integration_allow_loopback` seam `tests/integration_test_call.rs`
//! describes — and proves the row the API returns is the row that call stored,
//! masked payloads byte for byte, with the planted secret in neither.
//!
//! # Seen to fail (coding standard §2.9)
//!
//! Each mutation was made, this file run, the named tests observed red, and
//! the mutation reverted. **Seen red, 2026-09-29.**
//!
//! | Mutation | Reddened |
//! |---|---|
//! | the list drops `l.tenant_id = $1` | `another_tenants_rows_are_neither_listed_nor_shown` |
//! | `find_log` drops `l.tenant_id = $1` | `another_tenants_rows_are_neither_listed_nor_shown` |
//! | the tie broken by `l.id ASC` | `the_list_is_newest_first_and_a_tie_is_broken_by_id_across_pages` |
//! | `to` inclusive (`<=`) | `each_filter_narrows_the_list_and_they_combine` |
//! | `from` exclusive (`>`) | `each_filter_narrows_the_list_and_they_combine` |
//! | the count ignores `status` | `each_filter_narrows_the_list_and_they_combine` |
//! | the endpoint filter drops `entity_id` | `each_filter_narrows_the_list_and_they_combine`, `a_test_calls_rows_are_listed_and_shown_exactly_as_stored_without_the_secret` |
//! | the service ignores `endpointId` | `each_filter_narrows_the_list_and_they_combine`, `a_test_calls_rows_are_listed_and_shown_exactly_as_stored_without_the_secret` |
//! | the detail's join is `INNER` | `a_row_naming_no_system_is_shown_with_its_system_fields_null` |
//! | `get_log` requires `integration:external-system:read` instead | `both_routes_are_refused_without_the_permission_and_allowed_with_it_alone`, `another_tenants_rows_are_neither_listed_nor_shown` |
//! | `list_logs` requires nothing | `both_routes_are_refused_without_the_permission_and_allowed_with_it_alone` |
//! | `validate_query` not called | `a_filter_that_cannot_be_read_is_refused` |
//! | the detail's `requestPayload` is the response's | `the_detail_is_the_summary_and_the_payloads_as_stored`, `a_test_calls_rows_are_listed_and_shown_exactly_as_stored_without_the_secret` |
//! | `0050` grants nothing to `ROLE-ADMIN` | `the_permission_is_catalogued_at_its_id_and_held_by_the_administrator` and ten others |
//!
//! The test-engineer gate's survivors, and the tests that now redden them
//! (**seen red, 2026-09-29**):
//!
//! | Mutation | Reddened |
//! |---|---|
//! | the endpoint filter drops `entity_type` | `an_endpoint_filter_matches_endpoint_rows_only_and_a_row_is_shown_as_stored` |
//! | the detail's `documentId` is always `null` | `an_endpoint_filter_matches_endpoint_rows_only_and_a_row_is_shown_as_stored` |
//! | `direction` is always `OUTBOUND` | `an_endpoint_filter_matches_endpoint_rows_only_and_a_row_is_shown_as_stored` |
//! | `list_logs` judges the range before the permission | `a_caller_without_the_permission_is_refused_before_the_range_is_judged` |
//! | the limit is the raw `pageSize` | `the_page_size_is_clamped_and_a_page_below_one_is_the_first` |
//! | the offset is computed from the raw `page` | `the_page_size_is_clamped_and_a_page_below_one_is_the_first` |
//! | `meta.total` capped at the page size | `the_list_is_newest_first_and_a_tie_is_broken_by_id_across_pages`, `the_page_size_is_clamped_and_a_page_below_one_is_the_first` |
//!
//! **Two survive and are equivalent**: dropping `s.tenant_id = l.tenant_id`
//! from either join. The composite foreign key refuses the only row that could
//! tell them apart, and `a_row_cannot_name_another_tenants_system` asserts it.

mod common;

use axum::extract::Request;
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use chrono::{DateTime, Duration, Utc};
use common::{fixtures, TestApp, TestResponse};
use serde_json::{json, Value};
use uuid::Uuid;

const LOGS: &str = "/api/v1/integration/logs";
const SYSTEMS: &str = "/api/v1/integration/external-systems";
const PASSWORD: &str = "integration-logs-password";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn at(text: &str) -> DateTime<Utc> {
    text.parse().expect("a timestamp")
}

fn id_of(response: &TestResponse) -> Uuid {
    response.body["data"]["id"]
        .as_str()
        .and_then(|id| id.parse().ok())
        .unwrap_or_else(|| panic!("no id in {}", response.body))
}

/// A system registered through the API; returns its id.
async fn system(app: &TestApp, token: &str, code: &str, base_url: &str) -> Uuid {
    let response = app
        .post(
            SYSTEMS,
            Some(token),
            json!({
                "systemCode": code,
                "systemName": format!("{code} system"),
                "baseUrl": base_url,
                "timeoutSeconds": 2,
            }),
        )
        .await;
    assert_eq!(response.status, StatusCode::CREATED, "{}", response.body);
    id_of(&response)
}

/// One seeded row. Everything the list shows is set, so an assertion can name
/// any of it.
struct Seed {
    tenant_id: Uuid,
    system: Option<Uuid>,
    endpoint: Option<Uuid>,
    status: &'static str,
    started_at: DateTime<Utc>,
}

impl Seed {
    fn new(system: Uuid, status: &'static str, started_at: &str) -> Self {
        Self {
            tenant_id: fixtures::SYSTEM_TENANT_ID,
            system: Some(system),
            endpoint: None,
            status,
            started_at: at(started_at),
        }
    }
}

/// Inserts a row; returns its id. The id is a v7 made now, so ids sort by
/// insertion order, which the tie-break test relies on.
async fn seed(app: &TestApp, seed: Seed) -> Uuid {
    seed_with_id(app, Uuid::now_v7(), seed).await
}

async fn seed_with_id(app: &TestApp, id: Uuid, seed: Seed) -> Uuid {
    let failed = seed.status != "SUCCESS";
    sqlx::query(
        "INSERT INTO integration_logs
             (id, tenant_id, external_system_id, direction, integration_type, endpoint, method,
              correlation_id, entity_type, entity_id, request_payload_json,
              response_payload_json, status_code, status, error_message, started_at,
              completed_at, duration_ms)
         VALUES ($1, $2, $3, 'OUTBOUND', 'REST', 'https://erp.example/ping', 'GET',
                 $4, $5, $6, $7, $8, $9, $10, $11, $12, $12 + interval '15 milliseconds', 15)",
    )
    .bind(id)
    .bind(seed.tenant_id)
    .bind(seed.system)
    .bind(format!("corr-{id}"))
    .bind(seed.endpoint.map(|_| "IntegrationEndpoint"))
    .bind(seed.endpoint)
    .bind(json!({ "method": "GET", "headers": { "Authorization": "[REDACTED]" } }))
    .bind(if failed {
        None
    } else {
        Some(json!({ "statusCode": 200, "bodyPreview": "ok", "bodyTruncated": false }))
    })
    .bind(if failed { None } else { Some(200) })
    .bind(seed.status)
    .bind(if failed {
        Some("UPSTREAM_UNREACHABLE: the system could not be reached")
    } else {
        None
    })
    .bind(seed.started_at)
    .execute(&app.pool)
    .await
    .expect("seed an integration_logs row");
    id
}

fn ids(response: &TestResponse) -> Vec<String> {
    response.body["data"]
        .as_array()
        .unwrap_or_else(|| panic!("a list: {}", response.body))
        .iter()
        .map(|row| row["id"].as_str().expect("an id").to_owned())
        .collect()
}

fn strings(ids: &[Uuid]) -> Vec<String> {
    ids.iter().map(Uuid::to_string).collect()
}

/// A caller in the system tenant holding exactly `permissions`.
async fn caller_holding(app: &TestApp, label: &str, permissions: &[&str]) -> String {
    let role_id = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        &format!("ROLE-LOG-{label}"),
        permissions,
    )
    .await;

    let username = format!("user.log.{}", label.to_lowercase());
    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        &username,
        &format!("log.{}@kelir.test", label.to_lowercase()),
        PASSWORD,
        &[role_id],
    )
    .await;

    app.sign_in(&username, PASSWORD).await
}

// ---------------------------------------------------------------------------
// The list: order, paging, filters
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_list_is_newest_first_and_a_tie_is_broken_by_id_across_pages() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let erp = system(&app, &token, "ERP", "https://erp.example").await;

    // Seeded out of order; two share a start, and the later id must come first.
    let oldest = seed(&app, Seed::new(erp, "SUCCESS", "2026-09-01T08:00:00Z")).await;
    let newest = seed(&app, Seed::new(erp, "FAILED", "2026-09-03T08:00:00Z")).await;
    let tied_first = seed(&app, Seed::new(erp, "SUCCESS", "2026-09-02T08:00:00Z")).await;
    let tied_second = seed(&app, Seed::new(erp, "SUCCESS", "2026-09-02T08:00:00Z")).await;
    let middle = seed(&app, Seed::new(erp, "SUCCESS", "2026-09-02T09:00:00Z")).await;
    assert!(tied_second > tied_first, "v7 ids sort by creation");

    let expected = strings(&[newest, middle, tied_second, tied_first, oldest]);

    let whole = app.get(LOGS, Some(&token)).await;
    assert_eq!(whole.status, StatusCode::OK, "{}", whole.body);
    assert_eq!(ids(&whole), expected);
    assert_eq!(whole.body["meta"]["total"], 5);

    // Page by two: the pages are the whole list cut, with nothing repeated or
    // skipped at a boundary that falls inside the tie.
    let mut paged = Vec::new();
    for page in 1..=3 {
        let response = app
            .get(&format!("{LOGS}?page={page}&pageSize=2"), Some(&token))
            .await;
        assert_eq!(response.status, StatusCode::OK, "{}", response.body);
        assert_eq!(response.body["meta"]["total"], 5);
        assert_eq!(response.body["meta"]["page"], page);
        assert_eq!(response.body["meta"]["pageSize"], 2);
        paged.extend(ids(&response));
    }
    assert_eq!(paged, expected);
}

#[tokio::test]
async fn a_list_item_carries_the_summary_and_no_payload() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let erp = system(&app, &token, "ERP", "https://erp.example").await;
    let endpoint = Uuid::now_v7();

    let id = seed(
        &app,
        Seed {
            endpoint: Some(endpoint),
            ..Seed::new(erp, "SUCCESS", "2026-09-02T08:00:00Z")
        },
    )
    .await;

    let response = app.get(LOGS, Some(&token)).await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    let item = &response.body["data"][0];

    assert_eq!(
        item,
        &json!({
            "id": id,
            "externalSystemId": erp,
            "externalSystemCode": "ERP",
            "externalSystemName": "ERP system",
            "direction": "OUTBOUND",
            "integrationType": "REST",
            "method": "GET",
            "endpoint": "https://erp.example/ping",
            "entityType": "IntegrationEndpoint",
            "entityId": endpoint,
            "status": "SUCCESS",
            "statusCode": 200,
            "errorMessage": null,
            "correlationId": format!("corr-{id}"),
            "startedAt": "2026-09-02T08:00:00Z",
            "completedAt": "2026-09-02T08:00:00.015Z",
            "durationMs": 15,
        }),
        "the list item is the contract the page is built on"
    );
}

#[tokio::test]
async fn each_filter_narrows_the_list_and_they_combine() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let erp = system(&app, &token, "ERP", "https://erp.example").await;
    let crm = system(&app, &token, "CRM", "https://crm.example").await;
    let probe = Uuid::now_v7();
    let other_probe = Uuid::now_v7();

    let erp_ok_early = seed(
        &app,
        Seed {
            endpoint: Some(probe),
            ..Seed::new(erp, "SUCCESS", "2026-09-01T00:00:00Z")
        },
    )
    .await;
    let erp_failed_mid = seed(
        &app,
        Seed {
            endpoint: Some(probe),
            ..Seed::new(erp, "FAILED", "2026-09-02T00:00:00Z")
        },
    )
    .await;
    let erp_ok_late = seed(
        &app,
        Seed {
            endpoint: Some(other_probe),
            ..Seed::new(erp, "SUCCESS", "2026-09-03T00:00:00Z")
        },
    )
    .await;
    let crm_failed_mid = seed(&app, Seed::new(crm, "FAILED", "2026-09-02T12:00:00Z")).await;

    let cases: Vec<(String, Vec<Uuid>)> = vec![
        (
            format!("externalSystemId={erp}"),
            vec![erp_ok_late, erp_failed_mid, erp_ok_early],
        ),
        (format!("externalSystemId={crm}"), vec![crm_failed_mid]),
        (
            "status=FAILED".to_owned(),
            vec![crm_failed_mid, erp_failed_mid],
        ),
        (
            format!("endpointId={probe}"),
            vec![erp_failed_mid, erp_ok_early],
        ),
        (format!("endpointId={other_probe}"), vec![erp_ok_late]),
        // `from` is inclusive: a row starting exactly then is in.
        (
            "from=2026-09-02T00:00:00Z".to_owned(),
            vec![erp_ok_late, crm_failed_mid, erp_failed_mid],
        ),
        // `to` is exclusive: a row starting exactly then is out.
        (
            "to=2026-09-02T12:00:00Z".to_owned(),
            vec![erp_failed_mid, erp_ok_early],
        ),
        (
            "from=2026-09-02T00:00:00Z&to=2026-09-03T00:00:00Z".to_owned(),
            vec![crm_failed_mid, erp_failed_mid],
        ),
        // An empty range is allowed and selects nothing.
        (
            "from=2026-09-02T00:00:00Z&to=2026-09-02T00:00:00Z".to_owned(),
            vec![],
        ),
        (
            format!("externalSystemId={erp}&status=FAILED&from=2026-09-01T12:00:00Z"),
            vec![erp_failed_mid],
        ),
        (format!("externalSystemId={crm}&endpointId={probe}"), vec![]),
        // An id that is no system's matches nothing; it is not an error.
        (format!("externalSystemId={}", Uuid::now_v7()), vec![]),
    ];

    for (query, expected) in cases {
        let response = app.get(&format!("{LOGS}?{query}"), Some(&token)).await;
        assert_eq!(
            response.status,
            StatusCode::OK,
            "{query}: {}",
            response.body
        );
        assert_eq!(ids(&response), strings(&expected), "{query}");
        assert_eq!(
            response.body["meta"]["total"],
            expected.len(),
            "{query}: the total counts what the filters match"
        );
    }
}

#[tokio::test]
async fn a_filter_that_cannot_be_read_is_refused() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let inverted = app
        .get(
            &format!("{LOGS}?from=2026-09-02T00:00:00Z&to=2026-09-01T00:00:00Z"),
            Some(&token),
        )
        .await;
    assert_eq!(
        inverted.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        inverted.body
    );
    assert!(
        inverted.body.to_string().contains("RANGE_INVERTED"),
        "{}",
        inverted.body
    );

    for query in [
        "status=UNKNOWN",
        "externalSystemId=not-a-uuid",
        "from=yesterday",
    ] {
        let response = app.get(&format!("{LOGS}?{query}"), Some(&token)).await;
        assert_eq!(
            response.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{query}: {}",
            response.body
        );
    }
}

// ---------------------------------------------------------------------------
// The detail
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_detail_is_the_summary_and_the_payloads_as_stored() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let erp = system(&app, &token, "ERP", "https://erp.example").await;

    let id = seed(&app, Seed::new(erp, "SUCCESS", "2026-09-02T08:00:00Z")).await;

    let response = app.get(&format!("{LOGS}/{id}"), Some(&token)).await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    let data = response.data();

    // The summary's fields, as the list gives them.
    let listed = app.get(LOGS, Some(&token)).await;
    let summary = listed.body["data"][0].as_object().expect("an item");
    for (key, value) in summary {
        assert_eq!(&data[key], value, "{key} differs between list and detail");
    }

    assert_eq!(data["documentId"], Value::Null);
    assert_eq!(
        data["requestPayload"],
        json!({ "method": "GET", "headers": { "Authorization": "[REDACTED]" } })
    );
    assert_eq!(
        data["responsePayload"],
        json!({ "statusCode": 200, "bodyPreview": "ok", "bodyTruncated": false })
    );
    assert_eq!(
        data.as_object().expect("an object").len(),
        summary.len() + 3,
        "the detail is the summary plus documentId, requestPayload and responsePayload"
    );
}

#[tokio::test]
async fn a_row_naming_no_system_is_shown_with_its_system_fields_null() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let id = seed(
        &app,
        Seed {
            system: None,
            ..Seed::new(Uuid::nil(), "FAILED", "2026-09-02T08:00:00Z")
        },
    )
    .await;

    let response = app.get(&format!("{LOGS}/{id}"), Some(&token)).await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    let data = response.data();
    assert_eq!(data["externalSystemId"], Value::Null);
    assert_eq!(data["externalSystemCode"], Value::Null);
    assert_eq!(data["externalSystemName"], Value::Null);
    assert_eq!(data["status"], "FAILED");
    assert_eq!(data["responsePayload"], Value::Null);
    assert_eq!(
        data["errorMessage"],
        "UPSTREAM_UNREACHABLE: the system could not be reached"
    );

    // And it is in the list, which a join that dropped it would lose.
    let listed = app.get(LOGS, Some(&token)).await;
    assert_eq!(ids(&listed), strings(&[id]));
}

#[tokio::test]
async fn an_id_that_is_no_row_is_a_404() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let response = app
        .get(&format!("{LOGS}/{}", Uuid::now_v7()), Some(&token))
        .await;
    assert_eq!(response.status, StatusCode::NOT_FOUND, "{}", response.body);
}

// ---------------------------------------------------------------------------
// Tenant isolation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn another_tenants_rows_are_neither_listed_nor_shown() {
    let app = TestApp::spawn_with(|config| config.multi_tenant = true).await;
    let system_admin = app
        .sign_in_to("SYSTEM", common::ADMIN_USERNAME, common::ADMIN_PASSWORD)
        .await;

    let other = fixtures::create_tenant(&app.pool, "TNT-LOG-B", "Tenant B").await;
    let role = fixtures::create_role_with_permissions(
        &app.pool,
        other,
        "ROLE-LOG-B",
        &["integration:external-system:create", "integration:log:read"],
    )
    .await;
    fixtures::create_user(
        &app.pool,
        other,
        "user.log.b",
        "log.b@kelir.test",
        PASSWORD,
        &[role],
    )
    .await;
    let theirs_token = app.sign_in_to("TNT-LOG-B", "user.log.b", PASSWORD).await;

    let ours_system = system(&app, &system_admin, "ERP", "https://erp.example").await;
    let theirs_system = system(&app, &theirs_token, "ERP", "https://erp.example").await;

    let ours = seed(
        &app,
        Seed::new(ours_system, "SUCCESS", "2026-09-02T08:00:00Z"),
    )
    .await;
    let theirs = seed(
        &app,
        Seed {
            tenant_id: other,
            ..Seed::new(theirs_system, "SUCCESS", "2026-09-02T09:00:00Z")
        },
    )
    .await;

    for (token, own, foreign, foreign_system) in [
        (&system_admin, ours, theirs, theirs_system),
        (&theirs_token, theirs, ours, ours_system),
    ] {
        let listed = app.get(LOGS, Some(token)).await;
        assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);
        assert_eq!(ids(&listed), strings(&[own]));
        assert_eq!(listed.body["meta"]["total"], 1);

        // Naming the other tenant's system does not reach its rows.
        let filtered = app
            .get(
                &format!("{LOGS}?externalSystemId={foreign_system}"),
                Some(token),
            )
            .await;
        assert_eq!(ids(&filtered), Vec::<String>::new());
        assert_eq!(filtered.body["meta"]["total"], 0);

        let shown = app.get(&format!("{LOGS}/{foreign}"), Some(token)).await;
        assert_eq!(shown.status, StatusCode::NOT_FOUND, "{}", shown.body);

        let own_shown = app.get(&format!("{LOGS}/{own}"), Some(token)).await;
        assert_eq!(own_shown.status, StatusCode::OK, "{}", own_shown.body);
    }
}

// ---------------------------------------------------------------------------
// The permission
// ---------------------------------------------------------------------------

#[tokio::test]
async fn both_routes_are_refused_without_the_permission_and_allowed_with_it_alone() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let erp = system(&app, &token, "ERP", "https://erp.example").await;
    let id = seed(&app, Seed::new(erp, "SUCCESS", "2026-09-02T08:00:00Z")).await;

    // Every other integration permission, and not this one.
    let without = caller_holding(
        &app,
        "WITHOUT",
        &[
            "integration:external-system:read",
            "integration:external-system:update",
            "integration:credential:read",
            "integration:endpoint:call",
        ],
    )
    .await;
    for uri in [
        LOGS.to_owned(),
        format!("{LOGS}/{id}"),
        // 403 before existence: a missing id tells this caller nothing.
        format!("{LOGS}/{}", Uuid::now_v7()),
    ] {
        let response = app.get(&uri, Some(&without)).await;
        assert_eq!(
            response.status,
            StatusCode::FORBIDDEN,
            "{uri}: {}",
            response.body
        );
    }

    // This one alone is enough; nothing about systems is needed.
    let only = caller_holding(&app, "ONLY", &["integration:log:read"]).await;
    let listed = app.get(LOGS, Some(&only)).await;
    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);
    assert_eq!(ids(&listed), strings(&[id]));
    let shown = app.get(&format!("{LOGS}/{id}"), Some(&only)).await;
    assert_eq!(shown.status, StatusCode::OK, "{}", shown.body);

    // Without a token at all, 401.
    let anonymous = app.get(LOGS, None).await;
    assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_permission_is_catalogued_at_its_id_and_held_by_the_administrator() {
    let app = TestApp::spawn().await;

    let id: Uuid = sqlx::query_scalar(
        "SELECT id FROM permissions WHERE permission_code = 'integration:log:read'",
    )
    .fetch_one(&app.pool)
    .await
    .expect("the catalogue row");
    assert_eq!(id, uuid::uuid!("00000000-0000-0000-0001-000000000077"));

    let granted: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM role_permissions
                        WHERE role_id = $1 AND permission_id = $2 AND deleted_at IS NULL)",
    )
    .bind(fixtures::ADMIN_ROLE_ID)
    .bind(id)
    .fetch_one(&app.pool)
    .await
    .expect("the grant");
    assert!(granted);
}

// ---------------------------------------------------------------------------
// OpenAPI: two routes, and no field for anything unmasked
// ---------------------------------------------------------------------------

/// A schema's property names, following `allOf` and `$ref` within the
/// document.
fn property_names(document: &Value, schema: &Value) -> Vec<String> {
    if let Some(reference) = schema["$ref"].as_str() {
        let name = reference.rsplit('/').next().expect("a schema name");
        return property_names(document, &document["components"]["schemas"][name]);
    }

    let mut names: Vec<String> = schema["properties"]
        .as_object()
        .map(|properties| properties.keys().cloned().collect())
        .unwrap_or_default();
    for part in schema["allOf"].as_array().into_iter().flatten() {
        names.extend(property_names(document, part));
    }
    names.sort();
    names
}

#[tokio::test]
async fn both_routes_are_in_the_openapi_document_and_nothing_in_them_is_unmasked() {
    let app = TestApp::spawn().await;
    let document = app
        .send(Method::GET, "/api/docs/openapi.json", None, None)
        .await
        .body;

    assert!(
        document["paths"][LOGS]["get"].is_object(),
        "the list is missing"
    );
    assert!(
        document["paths"][&format!("{LOGS}/{{id}}")]["get"].is_object(),
        "the detail is missing"
    );

    let parameters: Vec<&str> = document["paths"][LOGS]["get"]["parameters"]
        .as_array()
        .expect("the list's parameters")
        .iter()
        .filter_map(|parameter| parameter["name"].as_str())
        .collect();
    for name in [
        "page",
        "pageSize",
        "externalSystemId",
        "endpointId",
        "status",
        "from",
        "to",
    ] {
        assert!(parameters.contains(&name), "{name} is not documented");
    }

    let summary = [
        "completedAt",
        "correlationId",
        "direction",
        "durationMs",
        "endpoint",
        "entityId",
        "entityType",
        "errorMessage",
        "externalSystemCode",
        "externalSystemId",
        "externalSystemName",
        "id",
        "integrationType",
        "method",
        "startedAt",
        "status",
        "statusCode",
    ];
    assert_eq!(
        property_names(
            &document,
            &document["components"]["schemas"]["IntegrationLogSummary"]
        ),
        summary,
        "a field added to the list item is a decision about what it may carry"
    );

    let mut detail: Vec<&str> = summary.to_vec();
    detail.extend(["documentId", "requestPayload", "responsePayload"]);
    detail.sort_unstable();
    assert_eq!(
        property_names(
            &document,
            &document["components"]["schemas"]["IntegrationLog"]
        ),
        detail,
        "the detail has no field for an unmasked payload, or anything else new"
    );
}

// ---------------------------------------------------------------------------
// A row a real test call wrote
// ---------------------------------------------------------------------------

/// A mock system on loopback that echoes the `Authorization` header it got —
/// the worst case, a system that repeats the secret — or fails.
async fn echoing_system() -> String {
    let router = Router::new().fallback(|request: Request| async move {
        let echoed = request
            .headers()
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        let response: Response = match request.uri().path() {
            "/fail" => {
                (StatusCode::INTERNAL_SERVER_ERROR, format!("broke {echoed}")).into_response()
            }
            _ => axum::Json(json!({ "youSent": echoed, "status": "ok" })).into_response(),
        };
        response
    });

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind the mock system");
    let address = listener.local_addr().expect("its address");
    tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("the mock serves");
    });
    format!("http://{address}")
}

async fn endpoint(app: &TestApp, token: &str, system: Uuid, code: &str, path: &str) -> Uuid {
    let response = app
        .post(
            &format!("{SYSTEMS}/{system}/endpoints"),
            Some(token),
            json!({ "endpointCode": code, "name": code, "method": "GET", "path": path }),
        )
        .await;
    assert_eq!(response.status, StatusCode::CREATED, "{}", response.body);
    id_of(&response)
}

#[tokio::test]
async fn a_test_calls_rows_are_listed_and_shown_exactly_as_stored_without_the_secret() {
    let app = TestApp::spawn_with(|config| config.integration_allow_loopback = true).await;
    let token = app.administrator_token().await;
    let base_url = echoing_system().await;

    let secret = "kelir-planted-log-reader-7e41c2";
    let name = format!(
        "KELIR_INTEGRATION_SECRET_SYSTEM__LOGS_{}",
        Uuid::now_v7().simple().to_string().to_uppercase()
    );
    std::env::set_var(&name, secret);

    let system = system(&app, &token, "MOCK", &base_url).await;
    let echo = endpoint(&app, &token, system, "ECHO", "/echo").await;
    let fail = endpoint(&app, &token, system, "FAIL", "/fail").await;
    let credential = app
        .post(
            &format!("{SYSTEMS}/{system}/credentials"),
            Some(&token),
            json!({ "credentialType": "BEARER_TOKEN", "secretReference": format!("env://{name}") }),
        )
        .await;
    assert_eq!(
        credential.status,
        StatusCode::CREATED,
        "{}",
        credential.body
    );

    // A success, then a failure.
    let succeeded = app
        .post(
            &format!("{SYSTEMS}/{system}/endpoints/{echo}/test-call"),
            Some(&token),
            json!({}),
        )
        .await;
    assert_eq!(succeeded.status, StatusCode::OK, "{}", succeeded.body);
    let succeeded_log = succeeded.data()["logId"]
        .as_str()
        .expect("a log id")
        .to_owned();

    let failed = app
        .post(
            &format!("{SYSTEMS}/{system}/endpoints/{fail}/test-call"),
            Some(&token),
            json!({}),
        )
        .await;
    assert_eq!(failed.status, StatusCode::OK, "{}", failed.body);
    assert_eq!(failed.data()["status"], "FAILED");
    let failed_log = failed.data()["logId"]
        .as_str()
        .expect("a log id")
        .to_owned();

    // Both are listed under the system, the failure first; each filter finds
    // its one.
    let listed = app
        .get(&format!("{LOGS}?externalSystemId={system}"), Some(&token))
        .await;
    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);
    assert_eq!(ids(&listed), [failed_log.clone(), succeeded_log.clone()]);
    let first = &listed.body["data"][0];
    assert_eq!(first["externalSystemCode"], "MOCK");
    assert_eq!(first["entityType"], "IntegrationEndpoint");
    assert_eq!(first["entityId"], fail.to_string());
    assert_eq!(first["statusCode"], 500);
    assert_eq!(first["endpoint"], format!("{base_url}/fail"));

    let by_status = app
        .get(&format!("{LOGS}?status=SUCCESS"), Some(&token))
        .await;
    assert_eq!(ids(&by_status), std::slice::from_ref(&succeeded_log));
    let by_endpoint = app
        .get(&format!("{LOGS}?endpointId={fail}"), Some(&token))
        .await;
    assert_eq!(ids(&by_endpoint), std::slice::from_ref(&failed_log));

    for log in [&succeeded_log, &failed_log] {
        let shown = app.get(&format!("{LOGS}/{log}"), Some(&token)).await;
        assert_eq!(shown.status, StatusCode::OK, "{}", shown.body);

        // The payloads are the stored JSON, whole and unchanged.
        let (request, response): (Option<Value>, Option<Value>) = sqlx::query_as(
            "SELECT request_payload_json, response_payload_json
             FROM integration_logs WHERE id = $1::uuid",
        )
        .bind(log)
        .fetch_one(&app.pool)
        .await
        .expect("the stored row");
        assert_eq!(
            shown.data()["requestPayload"],
            request.unwrap_or(Value::Null)
        );
        assert_eq!(
            shown.data()["responsePayload"],
            response.unwrap_or(Value::Null)
        );
        assert_eq!(
            shown.data()["requestPayload"]["headers"]["Authorization"],
            "[REDACTED]"
        );

        // The secret is in neither answer — and the mock did echo it, so the
        // redaction had something to do.
        assert!(
            !shown.body.to_string().contains(secret),
            "the detail carries the secret: {}",
            shown.body
        );
        assert!(
            shown.body.to_string().contains("[REDACTED]"),
            "{}",
            shown.body
        );
    }
    assert!(
        !listed.body.to_string().contains(secret),
        "the list carries the secret: {}",
        listed.body
    );

    // The success's preview is the masked echo.
    let shown = app
        .get(&format!("{LOGS}/{succeeded_log}"), Some(&token))
        .await;
    let preview: Value = serde_json::from_str(
        shown.data()["responsePayload"]["bodyPreview"]
            .as_str()
            .expect("a preview"),
    )
    .expect("the preview of a JSON body is JSON");
    assert_eq!(preview["youSent"], "[REDACTED]");
}

#[tokio::test]
async fn a_refused_test_calls_row_carries_its_error_message() {
    // A call refused before anything was sent still writes its row, and the
    // message it answered with names that row — the link the log page closes.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let system = system(&app, &token, "BARE", "https://bare.example").await;
    let probe = endpoint(&app, &token, system, "PROBE", "/probe").await;

    let refused = app
        .post(
            &format!("{SYSTEMS}/{system}/endpoints/{probe}/test-call"),
            Some(&token),
            json!({}),
        )
        .await;
    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );

    let listed = app
        .get(&format!("{LOGS}?endpointId={probe}"), Some(&token))
        .await;
    assert_eq!(listed.body["meta"]["total"], 1, "{}", listed.body);
    let row = &listed.body["data"][0];
    let id = row["id"].as_str().expect("an id");
    assert_eq!(row["status"], "FAILED");
    assert_eq!(row["statusCode"], Value::Null);
    assert!(
        row["errorMessage"]
            .as_str()
            .expect("a message")
            .starts_with("NO_USABLE_CREDENTIAL"),
        "{row}"
    );
    assert!(
        refused.body["error"]["message"]
            .as_str()
            .expect("a message")
            .contains(&format!("(integration log {id})")),
        "the refusal names the row the log lists: {}",
        refused.body
    );

    // A window around now finds it by date alone.
    let now = Utc::now();
    let window = app
        .get(
            &format!(
                "{LOGS}?from={}&to={}",
                (now - Duration::minutes(5)).to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                (now + Duration::minutes(5)).to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
            ),
            Some(&token),
        )
        .await;
    assert_eq!(ids(&window), [id.to_owned()]);
}

// ---------------------------------------------------------------------------
// The test-engineer gate's additions (#548, plan 17 row 17)
// ---------------------------------------------------------------------------

/// **The endpoint filter matches endpoint rows only**, and a row's direction,
/// document and entity are shown as stored.
///
/// A log row names what it was about with `entityType` and `entityId`, and an
/// id is only an endpoint's under `IntegrationEndpoint`. A row about a
/// document whose id is the one filtered on is not that endpoint's call, so a
/// predicate reading `entity_id` alone would list it. Every other seeded row
/// is an outbound test call with no document, so neither `direction` nor
/// `documentId` was read from the row anywhere else.
#[tokio::test]
async fn an_endpoint_filter_matches_endpoint_rows_only_and_a_row_is_shown_as_stored() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let erp = system(&app, &token, "ERP", "https://erp.example").await;

    let type_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO document_types (id, tenant_id, type_code, name)
         VALUES ($1, $2, 'LOG_DOC', 'Logged document')",
    )
    .bind(type_id)
    .bind(fixtures::SYSTEM_TENANT_ID)
    .execute(&app.pool)
    .await
    .expect("insert a document type");
    let document = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO documents (id, tenant_id, document_ref, document_type_id, title)
         VALUES ($1, $2, 'DOC-2026-000123', $3, 'A synced purchase requisition')",
    )
    .bind(document)
    .bind(fixtures::SYSTEM_TENANT_ID)
    .bind(type_id)
    .execute(&app.pool)
    .await
    .expect("insert a document");

    // An inbound call about the document, whose entity id is the id the
    // filter below names — as an endpoint's.
    let shared_id = Uuid::now_v7();
    let inbound = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO integration_logs
             (id, tenant_id, external_system_id, direction, integration_type, endpoint, method,
              correlation_id, document_id, entity_type, entity_id, request_payload_json,
              status_code, status, started_at)
         VALUES ($1, $2, $3, 'INBOUND', 'WEBHOOK', '/hooks/erp', 'POST', 'corr-inbound', $4,
                 'Document', $5, '{\"event\": \"po.updated\"}', 202, 'PENDING',
                 '2026-09-02T08:00:00Z')",
    )
    .bind(inbound)
    .bind(fixtures::SYSTEM_TENANT_ID)
    .bind(erp)
    .bind(document)
    .bind(shared_id)
    .execute(&app.pool)
    .await
    .expect("seed an inbound row");

    let endpoint_row = seed(
        &app,
        Seed {
            endpoint: Some(shared_id),
            ..Seed::new(erp, "SUCCESS", "2026-09-02T09:00:00Z")
        },
    )
    .await;

    let filtered = app
        .get(&format!("{LOGS}?endpointId={shared_id}"), Some(&token))
        .await;
    assert_eq!(filtered.status, StatusCode::OK, "{}", filtered.body);
    assert_eq!(
        ids(&filtered),
        strings(&[endpoint_row]),
        "a document's row is not an endpoint's call"
    );
    assert_eq!(filtered.body["meta"]["total"], 1);

    let shown = app.get(&format!("{LOGS}/{inbound}"), Some(&token)).await;
    assert_eq!(shown.status, StatusCode::OK, "{}", shown.body);
    let data = shown.data();
    assert_eq!(data["direction"], "INBOUND");
    assert_eq!(data["integrationType"], "WEBHOOK");
    assert_eq!(data["documentId"], document.to_string());
    assert_eq!(data["entityType"], "Document");
    assert_eq!(data["entityId"], shared_id.to_string());
    assert_eq!(data["status"], "PENDING");
    assert_eq!(data["statusCode"], 202);
    assert_eq!(data["requestPayload"], json!({ "event": "po.updated" }));
    assert_eq!(data["responsePayload"], Value::Null);
    assert_eq!(data["completedAt"], Value::Null);
    assert_eq!(data["durationMs"], Value::Null);

    let listed = app.get(LOGS, Some(&token)).await;
    assert_eq!(
        listed.body["data"][1]["direction"], "INBOUND",
        "{}",
        listed.body
    );
}

/// **A row cannot name another tenant's system**, which is why the joins'
/// `s.tenant_id = l.tenant_id` has nothing to leak today.
///
/// Dropping that join predicate survives every test here (the gate's M11 and
/// M12): the composite foreign key refuses the only row that could tell the
/// two apart. This asserts the key, so the join's safety does not rest on a
/// constraint nobody checks.
#[tokio::test]
async fn a_row_cannot_name_another_tenants_system() {
    let app = TestApp::spawn_with(|config| config.multi_tenant = true).await;
    let system_admin = app
        .sign_in_to("SYSTEM", common::ADMIN_USERNAME, common::ADMIN_PASSWORD)
        .await;
    let ours = system(&app, &system_admin, "ERP", "https://erp.example").await;
    let other = fixtures::create_tenant(&app.pool, "TNT-LOG-FK", "Tenant FK").await;

    let refused = sqlx::query(
        "INSERT INTO integration_logs
             (id, tenant_id, external_system_id, direction, correlation_id, status, started_at)
         VALUES ($1, $2, $3, 'OUTBOUND', 'corr-fk', 'FAILED', now())",
    )
    .bind(Uuid::now_v7())
    .bind(other)
    .bind(ours)
    .execute(&app.pool)
    .await
    .expect_err("a row in one tenant naming another tenant's system");
    assert!(
        refused
            .to_string()
            .contains("fk_integration_logs_external_system_id_tenant_id"),
        "{refused}"
    );
}

/// **The page size is clamped and a page below one is the first**, on the
/// wire and in `meta`.
///
/// Seeds one row more than a page can hold, so an unclamped limit would show
/// it, and asks for page 0, which an offset computed from the raw page would
/// turn negative.
#[tokio::test]
async fn the_page_size_is_clamped_and_a_page_below_one_is_the_first() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let erp = system(&app, &token, "ERP", "https://erp.example").await;

    sqlx::query(
        "INSERT INTO integration_logs
             (id, tenant_id, external_system_id, direction, correlation_id, status, started_at)
         SELECT gen_random_uuid(), $1, $2, 'OUTBOUND', 'corr-page-' || n, 'SUCCESS',
                timestamptz '2026-09-01T00:00:00Z' + n * interval '1 minute'
         FROM generate_series(1, 101) AS n",
    )
    .bind(fixtures::SYSTEM_TENANT_ID)
    .bind(erp)
    .execute(&app.pool)
    .await
    .expect("seed 101 rows");

    let first = app
        .get(&format!("{LOGS}?pageSize=1000"), Some(&token))
        .await;
    assert_eq!(first.status, StatusCode::OK, "{}", first.body);
    assert_eq!(ids(&first).len(), 100);
    assert_eq!(first.body["meta"]["pageSize"], 100);
    assert_eq!(first.body["meta"]["total"], 101);

    let second = app
        .get(&format!("{LOGS}?page=2&pageSize=1000"), Some(&token))
        .await;
    assert_eq!(second.status, StatusCode::OK, "{}", second.body);
    assert_eq!(ids(&second).len(), 1);
    assert!(!ids(&first).contains(&ids(&second)[0]));

    let zero = app
        .get(&format!("{LOGS}?page=0&pageSize=5"), Some(&token))
        .await;
    assert_eq!(zero.status, StatusCode::OK, "{}", zero.body);
    assert_eq!(zero.body["meta"]["page"], 1);
    assert_eq!(ids(&zero), ids(&first)[..5].to_vec());

    let one = app.get(&format!("{LOGS}?pageSize=0"), Some(&token)).await;
    assert_eq!(one.status, StatusCode::OK, "{}", one.body);
    assert_eq!(ids(&one).len(), 1);
    assert_eq!(one.body["meta"]["pageSize"], 1);
}

/// **The permission is checked before the query is judged**: a caller
/// without `integration:log:read` is told 403, not that their range is
/// inverted.
#[tokio::test]
async fn a_caller_without_the_permission_is_refused_before_the_range_is_judged() {
    let app = TestApp::spawn().await;
    let without = caller_holding(&app, "RANGE", &["integration:external-system:read"]).await;

    let response = app
        .get(
            &format!("{LOGS}?from=2026-09-02T00:00:00Z&to=2026-09-01T00:00:00Z"),
            Some(&without),
        )
        .await;
    assert_eq!(response.status, StatusCode::FORBIDDEN, "{}", response.body);
}

/// **No query string or path reaches a 500**, and none reads a row it should
/// not: hostile page sizes and pages, dates outside what PostgreSQL can hold,
/// and SQL in the enum and the ids.
#[tokio::test]
async fn hostile_queries_are_refused_or_answered_and_never_a_500() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let erp = system(&app, &token, "ERP", "https://erp.example").await;
    let id = seed(&app, Seed::new(erp, "SUCCESS", "2026-09-02T08:00:00Z")).await;

    // Refused: none of these parse as the parameter they name.
    for query in [
        "pageSize=4294967296",
        "pageSize=-1",
        "page=-1",
        "page=1.5",
        "status=success",
        "status=SUCCESS'%20OR%20'1'='1",
        "status=SUCCESS%3B%20DROP%20TABLE%20integration_logs",
        "status=SUCCESS&status=FAILED",
        "externalSystemId='%20OR%201=1--",
        "endpointId=00000000-0000-0000-0000-00000000000g",
        "from=2026-13-45T00:00:00Z",
        "from=2026-09-02",
        "from=2026-09-02T00:00:00",
        // An unencoded `+` is a space, so this offset does not parse.
        "from=2026-09-02T00:00:00+07:00",
    ] {
        let response = app.get(&format!("{LOGS}?{query}"), Some(&token)).await;
        assert_eq!(
            response.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{query}: {}",
            response.body
        );
    }

    // Answered: in range for the parser, and each a well-formed question.
    for (query, expected) in [
        ("page=4294967295&pageSize=100", vec![]),
        ("from=2026-09-02T15:00:00%2B07:00", vec![id]),
        (
            "from=0001-01-01T00:00:00Z&to=9999-12-31T23:59:59Z",
            vec![id],
        ),
        ("unknown=1&log=whatever", vec![id]),
    ] {
        let response = app.get(&format!("{LOGS}?{query}"), Some(&token)).await;
        assert_eq!(
            response.status,
            StatusCode::OK,
            "{query}: {}",
            response.body
        );
        assert_eq!(ids(&response), strings(&expected), "{query}");
    }

    // Years the parser takes and PostgreSQL holds, and years it refuses.
    for (query, status) in [
        ("from=-4713-11-24T00:00:00Z", StatusCode::OK),
        ("to=%2B200000-01-01T00:00:00Z", StatusCode::OK),
        ("from=%2B262142-12-31T23:59:59Z", StatusCode::OK),
        (
            "from=%2B294277-01-01T00:00:00Z",
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "from=10000-01-01T00:00:00Z",
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
    ] {
        let response = app.get(&format!("{LOGS}?{query}"), Some(&token)).await;
        assert_eq!(response.status, status, "{query}: {}", response.body);
    }

    // The detail's path takes an id and nothing else.
    for path in ["not-a-uuid", "'%20OR%201=1--", "%2E%2E"] {
        let response = app.get(&format!("{LOGS}/{path}"), Some(&token)).await;
        assert!(
            response.status.is_client_error(),
            "{path}: {} {}",
            response.status,
            response.body
        );
        assert_ne!(response.status, StatusCode::OK);
    }
}

/// **A date before PostgreSQL's first timestamp is a 422, not a 500** — a
/// defect found by the test-engineer gate for #548.
///
/// chrono parses years down to -262143 and `timestamptz` starts at 4714 BC,
/// so `from=-5000-01-01T00:00:00Z` parsed, reached the database, and came
/// back as `timestamp out of range`, which the API answered as
/// `INTERNAL_ERROR`. `validate_query` now refuses a bound PostgreSQL cannot
/// hold before the query runs.
#[tokio::test]
async fn a_date_before_postgresqls_range_is_refused_not_a_500() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    for query in [
        "from=-5000-01-01T00:00:00Z",
        "from=-4714-11-23T00:00:00Z",
        "to=-5000-01-01T00:00:00Z",
    ] {
        let response = app.get(&format!("{LOGS}?{query}"), Some(&token)).await;
        assert_eq!(
            response.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{query}: {}",
            response.body
        );
    }
}
