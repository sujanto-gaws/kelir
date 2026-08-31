//! The events the collaboration surfaces write, and the audit column that was
//! always null (FR-ACT-002, FR-ACT-003, FR-ATT-008, FR-CMT-007, FR-AUD-005;
//! [#248]).
//!
//! [#248]: https://github.com/sujanto-gaws/kelir/issues/248

mod common;

use axum::http::{Method, StatusCode};
use serde_json::{json, Value};
use uuid::Uuid;

use common::TestApp;

const PDF: &[u8] = b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog >>\nendobj\ntrailer\n<<>>\n%%EOF\n";

fn id_of(value: &Value) -> Uuid {
    value["id"]
        .as_str()
        .expect("an id")
        .parse()
        .expect("a uuid")
}

async fn document_type(app: &TestApp, token: &str, code: &str) -> Uuid {
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

async fn draft(app: &TestApp, token: &str, type_id: Uuid) -> Uuid {
    let created = app
        .send(
            Method::POST,
            "/api/v1/documents",
            Some(token),
            Some(json!({ "documentTypeId": type_id, "title": "Two standing desks" })),
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    id_of(&created.body["data"])
}

async fn events_of(app: &TestApp, document: Uuid) -> Vec<(String, Option<Uuid>, Option<Uuid>)> {
    sqlx::query_as(
        "SELECT event_type, attachment_id, comment_id FROM activity_events \
         WHERE document_id = $1 ORDER BY created_at, id",
    )
    .bind(document)
    .fetch_all(&app.pool)
    .await
    .expect("the timeline")
}

// ---------------------------------------------------------------------------
// AC1, AC2 — the events, and the ids that let a timeline link to them
// ---------------------------------------------------------------------------

#[tokio::test]
async fn attaching_downloading_and_commenting_all_reach_the_timeline() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_248_ALL").await;
    let document = draft(&app, &token, type_id).await;

    let uploaded = app
        .post_multipart(
            &format!("/api/v1/documents/{document}/attachments"),
            Some(&token),
            "quotation.pdf",
            "application/pdf",
            PDF,
            None,
        )
        .await;
    assert_eq!(uploaded.status, StatusCode::OK, "{}", uploaded.body);
    let attachment = id_of(&uploaded.body["data"]);

    // The scanner clears it, so the download can happen.
    sqlx::query("UPDATE attachments SET virus_scan_status = 'CLEAN' WHERE id = $1")
        .bind(attachment)
        .execute(&app.pool)
        .await
        .expect("the scan result");

    let served = app
        .get_raw(
            &format!("/api/v1/documents/{document}/attachments/{attachment}"),
            Some(&token),
        )
        .await;
    assert_eq!(served.status, StatusCode::OK);

    let commented = app
        .post(
            &format!("/api/v1/documents/{document}/comments"),
            Some(&token),
            json!({ "body": "is this the right supplier?" }),
        )
        .await;
    assert_eq!(commented.status, StatusCode::OK, "{}", commented.body);
    let comment = id_of(&commented.body["data"]);

    let events = events_of(&app, document).await;
    let types: Vec<&str> = events.iter().map(|(kind, _, _)| kind.as_str()).collect();

    assert_eq!(
        types,
        vec![
            "Document.Created",
            "Attachment.Added",
            "Attachment.Downloaded",
            "Comment.Added",
        ]
    );

    // **AC2 — each event names what it is about**, so a timeline can offer the
    // file or the comment rather than only mentioning that one exists.
    //
    // **Seen red, 2026-08-31** twice: once with the download's event removed,
    // and once with `attachment_id` dropped from the upload's.
    let added = events
        .iter()
        .find(|(k, _, _)| k == "Attachment.Added")
        .expect("the upload");
    let taken = events
        .iter()
        .find(|(k, _, _)| k == "Attachment.Downloaded")
        .expect("the download");
    let said = events
        .iter()
        .find(|(k, _, _)| k == "Comment.Added")
        .expect("the comment");

    assert_eq!(added.1, Some(attachment));
    assert_eq!(taken.1, Some(attachment));
    assert_eq!(said.2, Some(comment));
}

// ---------------------------------------------------------------------------
// AC3 — the event and the thing it describes are one transaction
// ---------------------------------------------------------------------------

/// **An upload that could not be stored records neither a row nor an event.**
///
/// [#244](https://github.com/sujanto-gaws/kelir/issues/244) asserted the first
/// half; this adds the second, because a timeline that mentions a file nobody
/// can find is exactly the confusion the object-first ordering exists to avoid.
#[tokio::test]
async fn an_upload_that_fails_leaves_no_row_and_no_event() {
    let app = TestApp::spawn_with(|config| {
        config.storage_bucket = "a-bucket-nobody-provisioned".to_owned();
    })
    .await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_248_FAIL").await;
    let document = draft(&app, &token, type_id).await;

    let refused = app
        .post_multipart(
            &format!("/api/v1/documents/{document}/attachments"),
            Some(&token),
            "q.pdf",
            "application/pdf",
            PDF,
            None,
        )
        .await;

    assert_eq!(refused.status, StatusCode::INTERNAL_SERVER_ERROR);

    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM attachments")
        .fetch_one(&app.pool)
        .await
        .expect("a count");
    assert_eq!(rows, 0);

    let events = events_of(&app, document).await;
    let types: Vec<&str> = events.iter().map(|(kind, _, _)| kind.as_str()).collect();

    assert_eq!(
        types,
        vec!["Document.Created"],
        "an attachment nobody stored reached the timeline"
    );
}

// ---------------------------------------------------------------------------
// AC4, AC6 — the address, on a written row
// ---------------------------------------------------------------------------

/// **The column `middleware::client_address` promised and never filled.**
///
/// Its own documentation has said since Phase 2 that *two things key off this
/// value: the authentication rate limiter and the `ip_address` column on every
/// audit row* — while all 53 audit call sites passed `None`, so the column was
/// always null and the sentence read as though it were not. **D-44** found it.
///
/// #248 AC6 asks for the assertion to be **on a written audit row** rather than
/// on the extractor, because the extractor already worked: everything
/// downstream of it was the gap.
///
/// **Seen red, 2026-08-31**, with `Authenticated::ip_address` returning `None`
/// — which is what the product did before this item, and the test could not
/// tell the difference until the column was filled.
#[tokio::test]
async fn an_audited_action_records_where_it_came_from() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_248_ADDR").await;
    let document = draft(&app, &token, type_id).await;

    let recorded: Option<String> = sqlx::query_scalar(
        "SELECT ip_address FROM audit_events WHERE object_id = $1 AND event_type = 'Document.Created'",
    )
    .bind(document)
    .fetch_one(&app.pool)
    .await
    .expect("the audit row");

    assert_eq!(
        recorded.as_deref(),
        Some(common::TEST_PEER.ip().to_string()).as_deref(),
        "the audit row does not say where the request came from"
    );
}

/// **And it is the address the middleware resolved, not one a caller sent.**
///
/// The deployment is configured to trust no proxies, so `X-Forwarded-For` must
/// be ignored entirely — which is the property that makes the column evidence
/// rather than decoration.
#[tokio::test]
async fn a_forwarded_header_does_not_reach_the_audit_row() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_248_SPOOF").await;

    let created = app
        .send_with_headers(
            Method::POST,
            "/api/v1/documents",
            Some(&token),
            Some(json!({ "documentTypeId": type_id, "title": "Spoofed" })),
            &[("x-forwarded-for", "203.0.113.9")],
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    let recorded: Option<String> = sqlx::query_scalar(
        "SELECT ip_address FROM audit_events WHERE object_id = $1 AND event_type = 'Document.Created'",
    )
    .bind(id_of(&created.body["data"]))
    .fetch_one(&app.pool)
    .await
    .expect("the audit row");

    assert_eq!(
        recorded.as_deref(),
        Some(common::TEST_PEER.ip().to_string()).as_deref(),
        "a caller chose their own audit address"
    );
}
