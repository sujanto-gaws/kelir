//! A file attached to a document (FR-ATT-001, FR-ATT-003; [#244]).
//!
//! # These tests need object storage, and use a real one
//!
//! The harness's own header says nothing is mocked, and it already requires a
//! live PostgreSQL for that reason. `KELIR_STORAGE_ENDPOINT` points at MinIO —
//! the compose stack runs one, CI starts one beside `postgres`, and the default
//! is `http://localhost:9000`. **A stand-in store would verify the stand-in**:
//! the one thing worth knowing about this path is whether the bytes reach S3
//! semantics, and an in-memory `HashMap` cannot answer that.
//!
//! The bucket is `kelir-test` and is created by whoever starts MinIO, because
//! this process holds credentials that can put and get objects rather than ones
//! that can create buckets.
//!
//! [#244]: https://github.com/sujanto-gaws/kelir/issues/244

mod common;

use axum::http::{Method, StatusCode};
use object_store::aws::AmazonS3Builder;
use object_store::path::Path as ObjectPath;
use object_store::ObjectStore;
use serde_json::{json, Value};
use uuid::Uuid;

use common::{fixtures, TestApp};
use kelir_backend::config::AppConfig;

/// **A real PDF header, because the type check reads the bytes** (#245 AC4).
///
/// Before that check existed this was a sentence of prose declared as
/// `application/pdf`, which is precisely the mismatch the check refuses: what a
/// caller says a file is, against what it is.
const FILE: &[u8] = b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog >>\nendobj\ntrailer\n<<>>\n%%EOF\n";

// ---------------------------------------------------------------------------
// Fixtures — a document to attach something to
// ---------------------------------------------------------------------------

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
            Some(json!({
                "documentTypeId": type_id,
                "title": "Two standing desks",
            })),
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    id_of(&created.body["data"])
}

/// Reads the object back out of MinIO, which is the half of AC1 the database
/// cannot answer.
async fn stored_object(reference: &str) -> Vec<u8> {
    let bucket = std::env::var("KELIR_STORAGE_BUCKET").unwrap_or_else(|_| "kelir-test".to_owned());
    let store = AmazonS3Builder::new()
        .with_endpoint(
            std::env::var("KELIR_STORAGE_ENDPOINT")
                .unwrap_or_else(|_| "http://localhost:9000".to_owned()),
        )
        .with_bucket_name(&bucket)
        .with_access_key_id(
            std::env::var("KELIR_STORAGE_ACCESS_KEY").unwrap_or_else(|_| "minioadmin".to_owned()),
        )
        .with_secret_access_key(
            std::env::var("KELIR_STORAGE_SECRET_KEY").unwrap_or_else(|_| "minioadmin".to_owned()),
        )
        .with_region(
            std::env::var("KELIR_STORAGE_REGION").unwrap_or_else(|_| "us-east-1".to_owned()),
        )
        .with_allow_http(true)
        .with_virtual_hosted_style_request(false)
        .build()
        .expect("an object store");

    store
        .get(&ObjectPath::from(reference))
        .await
        .expect("the object the upload says it wrote")
        .bytes()
        .await
        .expect("its bytes")
        .to_vec()
}

// ---------------------------------------------------------------------------
// AC1, AC3, AC6 — the row, the object, and where the object went
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_file_attached_to_a_document_is_stored_and_recorded() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_ATT_STORE").await;
    let document = draft(&app, &token, type_id).await;

    let uploaded = app
        .post_multipart(
            &format!("/api/v1/documents/{document}/attachments"),
            Some(&token),
            "quotation 2026.pdf",
            "application/pdf",
            FILE,
            Some("  the supplier's quotation  "),
        )
        .await;

    assert_eq!(uploaded.status, StatusCode::OK, "{}", uploaded.body);

    let data = &uploaded.body["data"];

    // AC1 — the name as uploaded is kept, not the name it was stored under.
    assert_eq!(data["originalFileName"], "quotation 2026.pdf");
    assert_eq!(data["mimeType"], "application/pdf");
    assert_eq!(data["fileSize"], FILE.len() as i64);
    // Trimmed, so a form that always sends the field does not store blanks.
    assert_eq!(data["description"], "the supplier's quotation");

    // AC3 — nothing has scanned it, and it says so.
    assert_eq!(data["virusScanStatus"], "PENDING");

    // **`storage_reference` is not in the payload.** Where the bytes are is this
    // process's business; a caller who knows the object path knows the shape of
    // the bucket and can do nothing with it but guess at another one.
    assert!(
        data.get("storageReference").is_none(),
        "the object path must not be serialized to a caller: {data}"
    );

    let attachment_id = id_of(data);

    let row: (String, String, String, i64, String) = sqlx::query_as(
        "SELECT file_name, original_file_name, storage_reference, file_size, virus_scan_status \
         FROM attachments WHERE id = $1",
    )
    .bind(attachment_id)
    .fetch_one(&app.pool)
    .await
    .expect("the attachment row");

    let (file_name, original, reference, size, scan) = row;

    // AC6 — generated, and it names the tenant, the document and the attachment.
    assert!(
        reference.contains(&document.to_string()) && reference.contains(&attachment_id.to_string()),
        "the storage reference is generated from the ids: {reference}"
    );
    // The stored name is derived and the uploaded one is kept beside it: a space
    // is not a character this product will put in an object key.
    assert_eq!(file_name, "quotation_2026.pdf");
    assert_eq!(original, "quotation 2026.pdf");
    assert_eq!(size, FILE.len() as i64);
    assert_eq!(scan, "PENDING");

    // AC1's other half, which the database cannot answer: the bytes are there.
    assert_eq!(stored_object(&reference).await, FILE);
}

#[tokio::test]
async fn the_checksum_is_over_the_bytes_that_were_stored() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_ATT_SUM").await;
    let document = draft(&app, &token, type_id).await;

    let uploaded = app
        .post_multipart(
            &format!("/api/v1/documents/{document}/attachments"),
            Some(&token),
            "q.pdf",
            "application/pdf",
            FILE,
            None,
        )
        .await;

    assert_eq!(uploaded.status, StatusCode::OK, "{}", uploaded.body);

    // sha256 of FILE, computed independently of the code under test.
    let expected = format!("sha256:{:x}", <sha2::Sha256 as sha2::Digest>::digest(FILE));

    assert_eq!(uploaded.body["data"]["checksum"], expected);
    assert_eq!(uploaded.body["data"]["description"], Value::Null);
}

// ---------------------------------------------------------------------------
// AC5 — a document the caller cannot see
// ---------------------------------------------------------------------------

#[tokio::test]
async fn attaching_to_a_document_that_does_not_exist_answers_404_and_stores_nothing() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let absent = Uuid::now_v7();

    let refused = app
        .post_multipart(
            &format!("/api/v1/documents/{absent}/attachments"),
            Some(&token),
            "q.pdf",
            "application/pdf",
            FILE,
            None,
        )
        .await;

    // **The same answer a read of that document gives**, which is what AC5 asks
    // for: the refusal must not tell a caller whether the document is there.
    assert_eq!(refused.status, StatusCode::NOT_FOUND, "{}", refused.body);

    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM attachments")
        .fetch_one(&app.pool)
        .await
        .expect("a count");

    assert_eq!(rows, 0, "a refused upload wrote a row");
}

#[tokio::test]
async fn a_caller_who_may_not_read_documents_cannot_attach_to_one() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_ATT_PERM").await;
    let document = draft(&app, &token, type_id).await;

    // A user with no roles at all: `attachment:create` and `document:read` are
    // both absent, and either one alone would be enough to refuse.
    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "att-outsider",
        "att-outsider@example.test",
        common::ADMIN_PASSWORD,
        &[],
    )
    .await;

    let outsider = app.sign_in("att-outsider", common::ADMIN_PASSWORD).await;

    let refused = app
        .post_multipart(
            &format!("/api/v1/documents/{document}/attachments"),
            Some(&outsider),
            "q.pdf",
            "application/pdf",
            FILE,
            None,
        )
        .await;

    assert_eq!(refused.status, StatusCode::FORBIDDEN, "{}", refused.body);

    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM attachments")
        .fetch_one(&app.pool)
        .await
        .expect("a count");

    assert_eq!(rows, 0, "a refused upload wrote a row");
}

// ---------------------------------------------------------------------------
// The refusals that need no database
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_body_with_no_file_part_is_refused_by_name() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_ATT_NOFILE").await;
    let document = draft(&app, &token, type_id).await;

    // A multipart body carrying only a `description`, which is the shape a form
    // takes when the file input was left empty.
    let refused = app
        .post_multipart_without_file(
            &format!("/api/v1/documents/{document}/attachments"),
            Some(&token),
            "no file here",
        )
        .await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(refused.body["error"]["details"][0]["code"], "FILE_REQUIRED");
}

/// **A JSON body posted to this route stays inside the envelope**, which it did
/// not before `MultipartBody` existed: `axum::extract::Multipart` rejects before
/// the handler runs, and answered 400 with a null body — the one shape a client
/// written against `error.code` cannot read (#122's finding, one content type
/// over).
#[tokio::test]
async fn a_json_body_posted_to_the_upload_route_is_refused_inside_the_envelope() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_ATT_JSON").await;
    let document = draft(&app, &token, type_id).await;

    let refused = app
        .post(
            &format!("/api/v1/documents/{document}/attachments"),
            Some(&token),
            json!({ "file": "not multipart at all" }),
        )
        .await;

    // The fix is a header rather than a payload, which is what 415 says.
    assert_eq!(
        refused.status,
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        "{}",
        refused.body
    );
    assert!(
        refused.body["error"]["code"].is_string(),
        "the refusal must carry the error envelope, not a null body: {}",
        refused.body
    );
}

#[tokio::test]
async fn an_empty_file_is_refused_and_nothing_is_stored() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_ATT_EMPTY").await;
    let document = draft(&app, &token, type_id).await;

    let refused = app
        .post_multipart(
            &format!("/api/v1/documents/{document}/attachments"),
            Some(&token),
            "empty.pdf",
            "application/pdf",
            b"",
            None,
        )
        .await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(refused.body["error"]["details"][0]["code"], "FILE_EMPTY");

    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM attachments")
        .fetch_one(&app.pool)
        .await
        .expect("a count");

    assert_eq!(rows, 0);
}

#[tokio::test]
async fn a_file_name_that_is_a_path_traversal_does_not_become_an_object_key() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_ATT_TRAVERSE").await;
    let document = draft(&app, &token, type_id).await;

    let uploaded = app
        .post_multipart(
            &format!("/api/v1/documents/{document}/attachments"),
            Some(&token),
            "../../../etc/passwd",
            "application/pdf",
            FILE,
            None,
        )
        .await;

    assert_eq!(uploaded.status, StatusCode::OK, "{}", uploaded.body);

    let reference: String =
        sqlx::query_scalar("SELECT storage_reference FROM attachments WHERE id = $1")
            .bind(id_of(&uploaded.body["data"]))
            .fetch_one(&app.pool)
            .await
            .expect("the reference");

    // **Seen red, 2026-08-31** with the basename step removed from
    // `safe_file_name`: the key became `_.._.._etc_passwd`, and the unit tests
    // beside the function went red with it.
    //
    // **The uploaded name is kept as data and never as a path.** The object key
    // stays under the prefix this tenant's document owns, and the traversal is
    // still visible to a person in `original_file_name`.
    assert!(
        !reference.contains(".."),
        "a traversal reached the object key: {reference}"
    );
    assert!(reference.ends_with("/passwd"), "{reference}");
    assert_eq!(
        uploaded.body["data"]["originalFileName"],
        "../../../etc/passwd"
    );
}

// ---------------------------------------------------------------------------
// AC2 — which of the two failures is the one that can happen
// ---------------------------------------------------------------------------

/// **A store that refuses leaves no row**, which is the half of [#244] AC2 a
/// test can reach.
///
/// The decision that criterion asks for is *object first, row second*: an object
/// with no row costs storage and reaches nobody, and a row whose
/// `storage_reference` points at nothing is a download that answers 500 to
/// somebody who did nothing wrong. Only one of those two states is reachable,
/// and this is the assertion that says which.
///
/// **Seen red, 2026-08-31**, with the `insert_attachment` moved above the
/// `storage.put` — which is the whole of the mutation, because the order is the
/// whole of the decision.
///
/// The bucket is one nobody created. `AmazonS3Builder::build` validates shape
/// and opens nothing, so the failure lands where a real outage would — on the
/// put, with the row not yet written.
#[tokio::test]
async fn an_upload_whose_bytes_cannot_be_stored_records_nothing() {
    let app = TestApp::spawn_with(|config: &mut AppConfig| {
        config.storage_bucket = "a-bucket-nobody-provisioned".to_owned();
    })
    .await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_ATT_NOBUCKET").await;
    let document = draft(&app, &token, type_id).await;

    let refused = app
        .post_multipart(
            &format!("/api/v1/documents/{document}/attachments"),
            Some(&token),
            "q.pdf",
            "application/pdf",
            FILE,
            None,
        )
        .await;

    assert_eq!(
        refused.status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "{}",
        refused.body
    );

    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM attachments")
        .fetch_one(&app.pool)
        .await
        .expect("a count");

    assert_eq!(
        rows, 0,
        "the row is written only after the object is stored; a failed store must leave nothing"
    );
}

/// **The two permissions are isolated from each other** (coding standard §2.9).
///
/// `a_caller_who_may_not_read_documents_cannot_attach_to_one` holds neither
/// permission, so removing either check leaves it passing — the gate §2.9
/// describes, and the one that let five predicates through in Sprint 8. This
/// caller holds `document:read` and **not** `attachment:create`, so it is red
/// under exactly one mutation: the `require` in `service::upload`.
#[tokio::test]
async fn reading_a_document_is_not_permission_to_attach_to_it() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_ATT_ISOLATE").await;
    let document = draft(&app, &token, type_id).await;

    let role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "ROLE-DOC-READER",
        &["document:read"],
    )
    .await;

    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "att-reader",
        "att-reader@example.test",
        common::ADMIN_PASSWORD,
        &[role],
    )
    .await;

    let reader = app.sign_in("att-reader", common::ADMIN_PASSWORD).await;

    // The document is readable to this caller, so a 404 here would mean the
    // wrong check fired.
    let readable = app
        .get(&format!("/api/v1/documents/{document}"), Some(&reader))
        .await;
    assert_eq!(readable.status, StatusCode::OK, "{}", readable.body);

    let refused = app
        .post_multipart(
            &format!("/api/v1/documents/{document}/attachments"),
            Some(&reader),
            "q.pdf",
            "application/pdf",
            FILE,
            None,
        )
        .await;

    assert_eq!(refused.status, StatusCode::FORBIDDEN, "{}", refused.body);
}
