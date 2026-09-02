//! The attachment epic's tail: categories, soft delete, and links to things
//! this product does not hold (FR-ATT-006, FR-ATT-009, FR-ATT-010; [#254]) —
//! and the two carried findings that travel with it, [#293] and [#294].
//!
//! # Seen to fail (coding standard §2.9)
//!
//! Three mutations, run against this file on 2026-09-02:
//!
//! | Mutation | Reddened |
//! |---|---|
//! | The pre-#293 ordering restored — the event committed, then the object read | `a_download_whose_object_cannot_be_read_records_no_download` |
//! | `refuse_unless_uploader` forced to `Ok`, the authorship gate removed | `an_attachment_is_not_somebody_elses_to_delete`, `a_reference_is_not_somebody_elses_to_remove` |
//! | The scheme allow-list in `normalize_url` bypassed | `a_link_that_is_not_http_is_refused_and_nothing_is_stored` |
//!
//! The first is #293 AC4's own requirement — *a test drives a download whose
//! object is missing and asserts what the timeline says, seen to fail against
//! the current behaviour* — and it is the reason the ordering could be changed
//! with any confidence that it had been wrong.
//!
//! [#254]: https://github.com/sujanto-gaws/kelir/issues/254
//! [#293]: https://github.com/sujanto-gaws/kelir/issues/293
//! [#294]: https://github.com/sujanto-gaws/kelir/issues/294

mod common;

use axum::http::{Method, StatusCode};
use serde_json::{json, Value};
use uuid::Uuid;

use common::{fixtures, TestApp};

/// A real PDF header: the type check reads bytes, not names.
const PDF: &[u8] = b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog >>\nendobj\ntrailer\n<<>>\n%%EOF\n";

/// The `QUOTATION` row `0037_attachment_tail.sql` seeds for the system tenant.
const QUOTATION: &str = "00000000-0000-0000-0003-000000000001";

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

/// An account holding exactly the codes a test needs — the isolation rule
/// (coding standard §2.9) applied to a module that now has four permissions.
async fn user_with(app: &TestApp, name: &str, permissions: &[&str]) -> String {
    let role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        &format!("ROLE-{}", name.to_uppercase()),
        permissions,
    )
    .await;

    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        name,
        &format!("{name}@example.test"),
        common::ADMIN_PASSWORD,
        &[role],
    )
    .await;

    app.sign_in(name, common::ADMIN_PASSWORD).await
}

// ---------------------------------------------------------------------------
// AC1 — the categories have rows, and an attachment can carry one
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_seeded_categories_are_listed_system_rows_first() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let listed = app.get("/api/v1/attachment-categories", Some(&token)).await;

    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);

    let codes: Vec<&str> = listed.body["data"]
        .as_array()
        .expect("a list")
        .iter()
        .map(|row| row["code"].as_str().expect("a code"))
        .collect();

    assert_eq!(codes, vec!["APPROVAL", "CONTRACT", "EVIDENCE", "QUOTATION"]);
    assert!(
        listed.body["data"][0]["isSystem"]
            .as_bool()
            .expect("a flag"),
        "a seeded category says it is one, so a tenant surface can refuse to delete it"
    );
}

#[tokio::test]
async fn an_attachment_can_be_filed_under_a_category_and_reports_it() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_ATT_CAT").await;
    let document = draft(&app, &token, type_id).await;

    let uploaded = app
        .post_multipart_with_category(
            &format!("/api/v1/documents/{document}/attachments"),
            Some(&token),
            "quotation.pdf",
            "application/pdf",
            PDF,
            QUOTATION,
        )
        .await;

    assert_eq!(uploaded.status, StatusCode::OK, "{}", uploaded.body);
    assert_eq!(uploaded.body["data"]["category"]["code"], "QUOTATION");
    assert_eq!(uploaded.body["data"]["category"]["name"], "Quotation");

    let listed = app
        .get(
            &format!("/api/v1/documents/{document}/attachments"),
            Some(&token),
        )
        .await;

    assert_eq!(listed.body["data"][0]["category"]["code"], "QUOTATION");
}

/// **An uncategorized attachment is a normal state**, not a refusal: a category
/// is how somebody finds a quotation among eleven files, and an upload that
/// insisted on one would stop a person filing something they have not read yet.
#[tokio::test]
async fn an_attachment_without_a_category_is_stored_and_says_so() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_ATT_NOCAT").await;
    let document = draft(&app, &token, type_id).await;

    attach(&app, &token, document, "unfiled.pdf").await;

    let listed = app
        .get(
            &format!("/api/v1/documents/{document}/attachments"),
            Some(&token),
        )
        .await;

    assert!(listed.body["data"][0]["category"].is_null());
}

#[tokio::test]
async fn a_category_this_tenant_does_not_have_is_refused_and_stores_nothing() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_ATT_BADCAT").await;
    let document = draft(&app, &token, type_id).await;

    let refused = app
        .post_multipart_with_category(
            &format!("/api/v1/documents/{document}/attachments"),
            Some(&token),
            "quotation.pdf",
            "application/pdf",
            PDF,
            &Uuid::now_v7().to_string(),
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
        "CATEGORY_NOT_FOUND"
    );

    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM attachments")
        .fetch_one(&app.pool)
        .await
        .expect("a count");

    assert_eq!(rows, 0, "the row is refused before the object is written");
}

// ---------------------------------------------------------------------------
// AC2, AC3 — the delete is soft, the object stays, the download refuses it
// ---------------------------------------------------------------------------

/// **D-52 asserted rather than described.** The row leaves every list and keeps
/// everything a retention sweep would need: where the object is, what it was
/// called, and what it hashed to.
#[tokio::test]
async fn a_deleted_attachment_leaves_the_list_and_keeps_its_object() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_ATT_DEL").await;
    let document = draft(&app, &token, type_id).await;
    let attachment = attach(&app, &token, document, "quotation.pdf").await;

    let deleted = app
        .delete(
            &format!("/api/v1/documents/{document}/attachments/{attachment}"),
            Some(&token),
        )
        .await;

    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);

    let listed = app
        .get(
            &format!("/api/v1/documents/{document}/attachments"),
            Some(&token),
        )
        .await;

    assert_eq!(listed.body["data"].as_array().expect("a page").len(), 0);
    assert_eq!(
        listed.body["meta"]["total"], 0,
        "the count and the page apply the same rule"
    );

    // The row, and the object it still points at.
    let (reference, name, gone): (String, String, bool) = sqlx::query_as(
        "SELECT storage_reference, original_file_name, deleted_at IS NOT NULL \
         FROM attachments WHERE id = $1",
    )
    .bind(attachment)
    .fetch_one(&app.pool)
    .await
    .expect("the row");

    assert!(gone, "the delete is recorded on the row");
    assert!(
        reference.contains(&attachment.to_string()),
        "the storage reference survives, which is what an undo or a sweep needs"
    );
    assert_eq!(name, "quotation.pdf");
}

/// **AC3 — the gate is on the path that serves the bytes.**
///
/// A deleted attachment answers 404 on the download rather than 200, and it does
/// so because `find_stored_file` carries `deleted_at IS NULL` — not because the
/// service remembered to ask.
#[tokio::test]
async fn a_deleted_attachment_cannot_be_downloaded() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_ATT_DELDL").await;
    let document = draft(&app, &token, type_id).await;
    let attachment = attach(&app, &token, document, "quotation.pdf").await;

    // Cleared, so that the only thing left refusing the download is the delete.
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
    assert_eq!(served.status, StatusCode::OK, "cleared, and downloadable");

    app.delete(
        &format!("/api/v1/documents/{document}/attachments/{attachment}"),
        Some(&token),
    )
    .await;

    let refused = app
        .get_raw(
            &format!("/api/v1/documents/{document}/attachments/{attachment}"),
            Some(&token),
        )
        .await;

    assert_eq!(
        refused.status,
        StatusCode::NOT_FOUND,
        "a deleted attachment is not found rather than found and refused"
    );
}

#[tokio::test]
async fn an_attachment_is_not_somebody_elses_to_delete() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_ATT_NOTYOURS").await;
    let document = draft(&app, &token, type_id).await;
    let attachment = attach(&app, &token, document, "quotation.pdf").await;

    // Holds the whole delete permission, and uploaded nothing.
    let other = user_with(
        &app,
        "att-remover",
        &["document:read", "attachment:read", "attachment:delete"],
    )
    .await;

    let refused = app
        .delete(
            &format!("/api/v1/documents/{document}/attachments/{attachment}"),
            Some(&other),
        )
        .await;

    assert_eq!(refused.status, StatusCode::FORBIDDEN, "{}", refused.body);

    let live: i64 = sqlx::query_scalar("SELECT count(*) FROM attachments WHERE deleted_at IS NULL")
        .fetch_one(&app.pool)
        .await
        .expect("a count");

    assert_eq!(live, 1);
}

/// And the permission half: uploading is not permission to delete.
#[tokio::test]
async fn attaching_a_file_is_not_permission_to_delete_it() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_ATT_NODEL").await;
    let document = draft(&app, &token, type_id).await;

    let uploader = user_with(
        &app,
        "att-uploader",
        &["document:read", "attachment:read", "attachment:create"],
    )
    .await;

    let uploaded = app
        .post_multipart(
            &format!("/api/v1/documents/{document}/attachments"),
            Some(&uploader),
            "mine.pdf",
            "application/pdf",
            PDF,
            None,
        )
        .await;
    let attachment = id_of(&uploaded.body["data"]);

    let refused = app
        .delete(
            &format!("/api/v1/documents/{document}/attachments/{attachment}"),
            Some(&uploader),
        )
        .await;

    assert_eq!(refused.status, StatusCode::FORBIDDEN, "{}", refused.body);
}

// ---------------------------------------------------------------------------
// AC4, AC5 — a reference is a link, and is visibly not a file
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_reference_records_a_link_and_carries_none_of_a_files_fields() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_ATT_REF").await;
    let document = draft(&app, &token, type_id).await;

    let added = app
        .post(
            &format!("/api/v1/documents/{document}/references"),
            Some(&token),
            json!({
                "label": "  Vendor portal  ",
                "url": "  https://vendor.example.test/quotes/2026-11  ",
                "categoryId": QUOTATION,
            }),
        )
        .await;

    assert_eq!(added.status, StatusCode::OK, "{}", added.body);

    let reference = &added.body["data"];

    assert_eq!(reference["label"], "Vendor portal");
    assert_eq!(
        reference["url"],
        "https://vendor.example.test/quotes/2026-11"
    );
    assert_eq!(reference["category"]["code"], "QUOTATION");

    // **AC4 and AC5, held by the type rather than by a convention.** These keys
    // do not exist on the payload, so nothing can render a size for a link or
    // read `CLEAN` off one.
    for absent in ["fileSize", "checksum", "mimeType", "virusScanStatus"] {
        assert!(
            reference.get(absent).is_none(),
            "a reference reported {absent}, which is a file's field"
        );
    }
}

/// **A reference is not in the attachments list**, and the scanner cannot see
/// it: `pending_scans` reads `attachments`, and a reference is not one.
#[tokio::test]
async fn a_reference_is_not_an_attachment_and_is_never_scanned() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_ATT_REFSEP").await;
    let document = draft(&app, &token, type_id).await;

    app.post(
        &format!("/api/v1/documents/{document}/references"),
        Some(&token),
        json!({ "label": "Vendor portal", "url": "https://vendor.example.test/q" }),
    )
    .await;

    let attachments = app
        .get(
            &format!("/api/v1/documents/{document}/attachments"),
            Some(&token),
        )
        .await;

    assert_eq!(
        attachments.body["data"].as_array().expect("a page").len(),
        0,
        "a link is not a file and does not appear among them"
    );
    assert_eq!(attachments.body["meta"]["total"], 0);

    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM attachments")
        .fetch_one(&app.pool)
        .await
        .expect("a count");

    assert_eq!(rows, 0, "nothing reached the table the scan worker reads");

    let references = app
        .get(
            &format!("/api/v1/documents/{document}/references"),
            Some(&token),
        )
        .await;

    assert_eq!(references.body["data"].as_array().expect("a page").len(), 1);
    assert_eq!(references.body["meta"]["total"], 1);
}

/// **The scheme allow-list, which is a security control rather than tidiness.**
///
/// A stored URL is rendered as a link: `javascript:` in an `href` is somebody
/// else's script in this product's page.
#[tokio::test]
async fn a_link_that_is_not_http_is_refused_and_nothing_is_stored() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_ATT_SCHEME").await;
    let document = draft(&app, &token, type_id).await;

    for hostile in [
        "javascript:alert(document.cookie)",
        "JAVASCRIPT:alert(1)",
        "data:text/html;base64,PHNjcmlwdD5hbGVydCgxKTwvc2NyaXB0Pg==",
        "file:///etc/passwd",
        "ftp://vendor.example.test/quote.pdf",
    ] {
        let refused = app
            .post(
                &format!("/api/v1/documents/{document}/references"),
                Some(&token),
                json!({ "label": "Vendor portal", "url": hostile }),
            )
            .await;

        assert_eq!(
            refused.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{hostile} was accepted: {}",
            refused.body
        );
        assert_eq!(
            refused.body["error"]["details"][0]["code"],
            "URL_SCHEME_NOT_ALLOWED"
        );
    }

    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM document_external_references")
        .fetch_one(&app.pool)
        .await
        .expect("a count");

    assert_eq!(rows, 0);
}

#[tokio::test]
async fn recording_a_link_is_its_own_permission_and_reading_one_is_not() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_ATT_REFPERM").await;
    let document = draft(&app, &token, type_id).await;

    // Uploads files, and may not record a link.
    let uploader = user_with(
        &app,
        "ref-uploader",
        &["document:read", "attachment:read", "attachment:create"],
    )
    .await;

    let refused = app
        .post(
            &format!("/api/v1/documents/{document}/references"),
            Some(&uploader),
            json!({ "label": "Vendor portal", "url": "https://vendor.example.test/q" }),
        )
        .await;

    assert_eq!(refused.status, StatusCode::FORBIDDEN, "{}", refused.body);

    // Records links, and may not read what a document holds.
    let linker = user_with(
        &app,
        "ref-linker",
        &["document:read", "attachment:reference"],
    )
    .await;

    let added = app
        .post(
            &format!("/api/v1/documents/{document}/references"),
            Some(&linker),
            json!({ "label": "Vendor portal", "url": "https://vendor.example.test/q" }),
        )
        .await;
    assert_eq!(added.status, StatusCode::OK, "{}", added.body);

    let listed = app
        .get(
            &format!("/api/v1/documents/{document}/references"),
            Some(&linker),
        )
        .await;

    assert_eq!(
        listed.status,
        StatusCode::FORBIDDEN,
        "recording a link is not permission to read the list"
    );
}

#[tokio::test]
async fn a_reference_is_not_somebody_elses_to_remove() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_ATT_REFDEL").await;
    let document = draft(&app, &token, type_id).await;

    let added = app
        .post(
            &format!("/api/v1/documents/{document}/references"),
            Some(&token),
            json!({ "label": "Vendor portal", "url": "https://vendor.example.test/q" }),
        )
        .await;
    let reference = id_of(&added.body["data"]);

    let other = user_with(
        &app,
        "ref-remover",
        &["document:read", "attachment:read", "attachment:delete"],
    )
    .await;

    let refused = app
        .delete(
            &format!("/api/v1/documents/{document}/references/{reference}"),
            Some(&other),
        )
        .await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN, "{}", refused.body);

    let removed = app
        .delete(
            &format!("/api/v1/documents/{document}/references/{reference}"),
            Some(&token),
        )
        .await;
    assert_eq!(removed.status, StatusCode::NO_CONTENT, "{}", removed.body);

    let listed = app
        .get(
            &format!("/api/v1/documents/{document}/references"),
            Some(&token),
        )
        .await;

    assert_eq!(listed.body["data"].as_array().expect("a page").len(), 0);
}

// ---------------------------------------------------------------------------
// AC6 — each of them writes an activity event, and none of them says what
// ---------------------------------------------------------------------------

#[tokio::test]
async fn deleting_and_linking_reach_the_timeline_and_disclose_nothing() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_ATT_EVENTS").await;
    let document = draft(&app, &token, type_id).await;
    let attachment = attach(&app, &token, document, "quotation.pdf").await;

    app.delete(
        &format!("/api/v1/documents/{document}/attachments/{attachment}"),
        Some(&token),
    )
    .await;

    let added = app
        .post(
            &format!("/api/v1/documents/{document}/references"),
            Some(&token),
            json!({ "label": "Vendor portal", "url": "https://vendor.example.test/q" }),
        )
        .await;
    let reference = id_of(&added.body["data"]);

    app.delete(
        &format!("/api/v1/documents/{document}/references/{reference}"),
        Some(&token),
    )
    .await;

    for event_type in [
        "Attachment.Added",
        "Attachment.Deleted",
        "Reference.Added",
        "Reference.Deleted",
    ] {
        let (rows, empty): (i64, i64) = sqlx::query_as(
            "SELECT count(*), count(*) FILTER (WHERE details_json = '{}'::jsonb) \
             FROM activity_events WHERE event_type = $1 AND document_id = $2",
        )
        .bind(event_type)
        .bind(document)
        .fetch_one(&app.pool)
        .await
        .expect("a count");

        assert_eq!(rows, 1, "{event_type} did not write exactly one event");
        assert_eq!(empty, 1, "{event_type} carried detail D-45 does not permit");
    }

    // The timeline serves them, and still says nothing about the file or the
    // link: what happened is the document's history, what it was called is not.
    let timeline = app
        .get(
            &format!("/api/v1/documents/{document}/activity"),
            Some(&token),
        )
        .await;

    assert_eq!(timeline.status, StatusCode::OK, "{}", timeline.body);

    let summaries: Vec<&str> = timeline.body["data"]
        .as_array()
        .expect("a page")
        .iter()
        .map(|entry| entry["actionSummary"].as_str().expect("a summary"))
        .collect();

    assert!(summaries.iter().any(|line| line.contains("Deleted a file")));
    assert!(summaries
        .iter()
        .any(|line| line.contains("Recorded a link to something outside this document")));
}

// ---------------------------------------------------------------------------
// #293 — a download that failed is not recorded as one that happened
// ---------------------------------------------------------------------------

/// **Seen red before the fix**: with the event committed before `storage.get`,
/// this test found one `Attachment.Downloaded` row for a download that served no
/// bytes.
///
/// The object is made unreachable by pointing the row at a key that was never
/// written, which is the same failure as an object deleted underneath the row —
/// the case #293 describes — without needing to reach into the bucket.
#[tokio::test]
async fn a_download_whose_object_cannot_be_read_records_no_download() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_ATT_293").await;
    let document = draft(&app, &token, type_id).await;
    let attachment = attach(&app, &token, document, "quotation.pdf").await;

    sqlx::query(
        "UPDATE attachments \
         SET virus_scan_status = 'CLEAN', storage_reference = $2 \
         WHERE id = $1",
    )
    .bind(attachment)
    .bind(format!("tenants/gone/objects/{attachment}/quotation.pdf"))
    .execute(&app.pool)
    .await
    .expect("the row");

    let served = app
        .get_raw(
            &format!("/api/v1/documents/{document}/attachments/{attachment}"),
            Some(&token),
        )
        .await;

    assert_eq!(
        served.status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "the bytes could not be read, and the caller is told so"
    );

    let recorded: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM activity_events \
         WHERE event_type = 'Attachment.Downloaded' AND document_id = $1",
    )
    .bind(document)
    .fetch_one(&app.pool)
    .await
    .expect("a count");

    assert_eq!(
        recorded, 0,
        "#293: the timeline said somebody downloaded a file that never left the building"
    );
}

/// **And the ordering is not reversed** (#293 AC2): a download that works is
/// still recorded, and the record is written before the bytes are served.
#[tokio::test]
async fn a_download_that_works_is_still_recorded() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_ATT_293_OK").await;
    let document = draft(&app, &token, type_id).await;
    let attachment = attach(&app, &token, document, "quotation.pdf").await;

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

    let recorded: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM activity_events \
         WHERE event_type = 'Attachment.Downloaded' AND document_id = $1",
    )
    .bind(document)
    .fetch_one(&app.pool)
    .await
    .expect("a count");

    assert_eq!(recorded, 1, "a copy was taken, and the timeline says so");
}
