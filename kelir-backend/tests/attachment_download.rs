//! Retrieving a file, and the two doors it has to get past (FR-ATT-002,
//! FR-ATT-004, FR-ATT-005; [#245]).
//!
//! # These tests set `virus_scan_status` by hand, and that is the honest shape
//!
//! Nothing in this release scans: [#246](https://github.com/sujanto-gaws/kelir/issues/246)
//! is the scanner, and it moves the column. The download gate is here — refusing
//! anything but `CLEAN` — so a test of the happy path has to say *given a row a
//! scanner has cleared*, and it says it with an `UPDATE` rather than by
//! pretending an upload produced one.
//!
//! [#245]: https://github.com/sujanto-gaws/kelir/issues/245

mod common;

use axum::http::{header, Method, StatusCode};
use serde_json::{json, Value};
use uuid::Uuid;

use common::{fixtures, TestApp};

/// A real PDF header: the type check reads bytes, not names.
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

async fn attach(app: &TestApp, token: &str, document: Uuid, name: &str) -> Uuid {
    let uploaded = app
        .post_multipart(
            &format!("/api/v1/documents/{document}/attachments"),
            Some(token),
            name,
            "application/pdf",
            PDF,
            None,
        )
        .await;

    assert_eq!(uploaded.status, StatusCode::OK, "{}", uploaded.body);

    id_of(&uploaded.body["data"])
}

/// What the scanner will do in [#246](https://github.com/sujanto-gaws/kelir/issues/246).
async fn scanned(app: &TestApp, id: Uuid, status: &str) {
    sqlx::query("UPDATE attachments SET virus_scan_status = $2 WHERE id = $1")
        .bind(id)
        .bind(status)
        .execute(&app.pool)
        .await
        .expect("the scan result");
}

// ---------------------------------------------------------------------------
// The bytes, once something has cleared them
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_cleared_attachment_is_served_with_its_name_and_never_inline() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_DL_OK").await;
    let document = draft(&app, &token, type_id).await;
    let attachment = attach(&app, &token, document, "quotation 2026.pdf").await;

    scanned(&app, attachment, "CLEAN").await;

    let served = app
        .get_raw(
            &format!("/api/v1/documents/{document}/attachments/{attachment}"),
            Some(&token),
        )
        .await;

    assert_eq!(served.status, StatusCode::OK);
    assert_eq!(served.bytes, PDF, "the bytes that were stored");
    assert_eq!(
        served.header(header::CONTENT_TYPE).as_deref(),
        Some("application/pdf")
    );

    // **Never inline.** Serving caller-supplied bytes inline is a stored
    // cross-site scripting hole with this product's own session behind it.
    let disposition = served
        .header(header::CONTENT_DISPOSITION)
        .expect("a disposition");

    assert!(
        disposition.starts_with("attachment;"),
        "attachments are never served inline: {disposition}"
    );
    assert!(disposition.contains("quotation 2026.pdf"), "{disposition}");
}

// ---------------------------------------------------------------------------
// The scan gate (#246 AC2, AC3, taken early — see the service documentation)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn an_unscanned_attachment_is_listed_but_its_bytes_are_refused() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_DL_PENDING").await;
    let document = draft(&app, &token, type_id).await;
    let attachment = attach(&app, &token, document, "q.pdf").await;

    // **Seen red, 2026-08-31** with the `virus_scan_status != Clean` gate
    // deleted, together with the three-refusals test below it.
    //
    // Listed, with its status: a file that vanished until a scanner cleared it
    // would look like a lost upload.
    let listed = app
        .get(
            &format!("/api/v1/documents/{document}/attachments"),
            Some(&token),
        )
        .await;

    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);
    assert_eq!(listed.body["data"][0]["virusScanStatus"], "PENDING");

    let refused = app
        .get(
            &format!("/api/v1/documents/{document}/attachments/{attachment}"),
            Some(&token),
        )
        .await;

    assert_eq!(refused.status, StatusCode::CONFLICT, "{}", refused.body);
}

#[tokio::test]
async fn the_three_refusals_are_told_apart_because_they_need_different_things() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_DL_STATES").await;
    let document = draft(&app, &token, type_id).await;

    let mut messages = Vec::new();

    for status in ["PENDING", "INFECTED", "FAILED"] {
        let attachment = attach(&app, &token, document, &format!("{status}.pdf")).await;
        scanned(&app, attachment, status).await;

        let refused = app
            .get(
                &format!("/api/v1/documents/{document}/attachments/{attachment}"),
                Some(&token),
            )
            .await;

        assert_eq!(
            refused.status,
            StatusCode::CONFLICT,
            "{status}: {}",
            refused.body
        );

        messages.push(
            refused.body["error"]["message"]
                .as_str()
                .expect("a message")
                .to_owned(),
        );
    }

    // **Not yet, never, and could not be told apart.** `FAILED` is a refusal
    // rather than a pass: a scan that did not run has cleared nothing.
    messages.sort();
    messages.dedup();

    assert_eq!(messages.len(), 3, "the three refusals read the same");
}

// ---------------------------------------------------------------------------
// AC1, AC2 — the document decides, and the answer does not vary
// ---------------------------------------------------------------------------

#[tokio::test]
async fn an_attachment_id_from_another_document_is_not_found() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_DL_CROSS").await;
    let mine = draft(&app, &token, type_id).await;
    let theirs = draft(&app, &token, type_id).await;
    let attachment = attach(&app, &token, theirs, "theirs.pdf").await;

    scanned(&app, attachment, "CLEAN").await;

    // **Seen red, 2026-08-31** with `document_id = $2` dropped from
    // `find_stored_file`: the attachment was served from the wrong document.
    //
    // The id is real and this caller may read both documents; what is wrong is
    // the pairing, and the statement is what refuses it.
    let refused = app
        .get(
            &format!("/api/v1/documents/{mine}/attachments/{attachment}"),
            Some(&token),
        )
        .await;

    assert_eq!(refused.status, StatusCode::NOT_FOUND, "{}", refused.body);
}

/// **AC2 stated as an equality**, which is the only way to assert it: a caller
/// who may not read the document gets the same answer whether or not the
/// attachment exists.
#[tokio::test]
async fn the_refusal_does_not_say_whether_the_attachment_exists() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_DL_OPAQUE").await;
    let document = draft(&app, &token, type_id).await;
    let real = attach(&app, &token, document, "real.pdf").await;
    scanned(&app, real, "CLEAN").await;

    let role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "ROLE-ATT-READER",
        &["attachment:read"],
    )
    .await;

    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "dl-outsider",
        "dl-outsider@example.test",
        common::ADMIN_PASSWORD,
        &[role],
    )
    .await;

    let outsider = app.sign_in("dl-outsider", common::ADMIN_PASSWORD).await;
    let invented = Uuid::now_v7();

    let for_a_real_one = app
        .get(
            &format!("/api/v1/documents/{document}/attachments/{real}"),
            Some(&outsider),
        )
        .await;

    let for_an_invented_one = app
        .get(
            &format!("/api/v1/documents/{document}/attachments/{invented}"),
            Some(&outsider),
        )
        .await;

    assert_eq!(for_a_real_one.status, for_an_invented_one.status);
    assert_eq!(
        for_a_real_one.body["error"]["code"], for_an_invented_one.body["error"]["code"],
        "the two refusals differ, so one of them says the attachment is there"
    );
}

/// **`attachment:read` is not `document:read`** (coding standard §2.9).
///
/// A caller with neither is refused by whichever check runs first, so the two
/// are separated: this one holds `document:read` and not `attachment:read`.
///
/// **Seen red, 2026-08-31**, with `caller.require(ATTACHMENT_READ)?` deleted.
#[tokio::test]
async fn reading_a_document_is_not_permission_to_download_its_files() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_DL_ISOLATE").await;
    let document = draft(&app, &token, type_id).await;
    let attachment = attach(&app, &token, document, "q.pdf").await;
    scanned(&app, attachment, "CLEAN").await;

    let role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "ROLE-DOC-ONLY",
        &["document:read"],
    )
    .await;

    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "dl-reader",
        "dl-reader@example.test",
        common::ADMIN_PASSWORD,
        &[role],
    )
    .await;

    let reader = app.sign_in("dl-reader", common::ADMIN_PASSWORD).await;

    let readable = app
        .get(&format!("/api/v1/documents/{document}"), Some(&reader))
        .await;
    assert_eq!(readable.status, StatusCode::OK, "{}", readable.body);

    let refused = app
        .get(
            &format!("/api/v1/documents/{document}/attachments/{attachment}"),
            Some(&reader),
        )
        .await;

    assert_eq!(refused.status, StatusCode::FORBIDDEN, "{}", refused.body);
}

// ---------------------------------------------------------------------------
// AC3, AC4, AC6 — the door, from the other side
// ---------------------------------------------------------------------------

/// **The limit is enforced on the body, not on what was accepted** (AC3).
///
/// The harness configures 4096 bytes so this costs no wall clock; the mechanism
/// is the same one 25 MB uses in production — a layer that refuses while the
/// body is being read.
///
/// **Seen red, 2026-08-31**, with `DefaultBodyLimit::max(...)` replaced by
/// `disable()` — and red once before that, against the code as first written,
/// with `FILE_REQUIRED` where `FILE_TOO_LARGE` belonged.
#[tokio::test]
async fn a_file_over_the_limit_is_refused_and_the_refusal_names_the_limit() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_DL_BIG").await;
    let document = draft(&app, &token, type_id).await;

    let mut oversized = PDF.to_vec();
    oversized.resize(8192, b'0');

    let refused = app
        .post_multipart(
            &format!("/api/v1/documents/{document}/attachments"),
            Some(&token),
            "big.pdf",
            "application/pdf",
            &oversized,
            None,
        )
        .await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(
        refused.body["error"]["details"][0]["code"], "FILE_TOO_LARGE",
        "an over-size upload must not read as a missing file: {}",
        refused.body
    );

    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM attachments")
        .fetch_one(&app.pool)
        .await
        .expect("a count");

    assert_eq!(rows, 0);
}

/// **Content decides, not the extension** (AC4).
///
/// **Seen red, 2026-08-31**, with the `type_is_allowed` check deleted: the
/// script was stored, named `invoice.pdf`.
#[tokio::test]
async fn a_payload_named_pdf_is_refused_when_its_bytes_are_not() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_DL_TYPE").await;
    let document = draft(&app, &token, type_id).await;

    let refused = app
        .post_multipart(
            &format!("/api/v1/documents/{document}/attachments"),
            Some(&token),
            // Named `.pdf`, declared `application/pdf`, and neither is evidence.
            "invoice.pdf",
            "application/pdf",
            b"<html><script>alert(1)</script></html>",
            None,
        )
        .await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(
        refused.body["error"]["details"][0]["code"],
        "FILE_TYPE_NOT_ALLOWED"
    );

    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM attachments")
        .fetch_one(&app.pool)
        .await
        .expect("a count");

    assert_eq!(rows, 0, "a refused type reached storage");
}
