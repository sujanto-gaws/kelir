//! The scan, and the things it must never do (FR-ATT-001, FR-ATT-002; [#246]).
//!
//! # Two kinds of scanner, and both are real in the sense that matters
//!
//! **A real `clamd`** answers what only it can: whether an actual signature
//! matches, and whether an actual clean file comes back clean.
//! `KELIR_CLAMAV_HOST` and `KELIR_CLAMAV_PORT` point at it; CI runs one beside
//! `postgres` and `minio`, and `deploy/docker` runs one for a developer.
//!
//! **A scripted listener** answers what `clamd` will not produce on demand: the
//! `ERROR` reply, a reply nothing recognises, and a socket that accepts and says
//! nothing. Those are the arms where a mistake is a silent pass, and waiting for
//! a real scanner to be in a bad mood is not a test strategy. It speaks the same
//! protocol the worker does, which the measurement on [#246] pinned against the
//! real thing.
//!
//! [#246]: https://github.com/sujanto-gaws/kelir/issues/246

mod common;

use axum::http::{Method, StatusCode};
use kelir_backend::modules::attachment::scanner::{self, ScanOutcome};
use kelir_backend::modules::attachment::{repository, worker};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use uuid::Uuid;

use common::TestApp;

const PDF: &[u8] = b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog >>\nendobj\ntrailer\n<<>>\n%%EOF\n";

/// The EICAR test file, **exactly** and nothing more.
///
/// **It cannot be wrapped in anything, which is a fact about the signature
/// rather than about this test.** Measured 2026-08-31: `clamd` answers
/// `Eicar-Test-Signature FOUND` for these bytes alone and `OK` for the same
/// bytes with a PDF header in front, a PDF trailer after, or a newline either
/// side. The signature is anchored to the whole file.
///
/// That is why real detection is proven at [`scanner::scan`] — where no type
/// check stands between the bytes and the scanner — and the *product's*
/// behaviour on an infected file is driven by a scripted `FOUND`. A fixture that
/// tried to be both a valid PDF and a detectable virus would be neither, and the
/// first version of this file was exactly that: it scanned clean and the two
/// tests failed for a reason that had nothing to do with the code.
const EICAR: &[u8] = br#"X5O!P%@AP[4\PZX54(P^)7CC)7}$EICAR-STANDARD-ANTIVIRUS-TEST-FILE!$H+H*"#;

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

async fn attach(app: &TestApp, token: &str, document: Uuid, name: &str, bytes: &[u8]) -> Uuid {
    let uploaded = app
        .post_multipart(
            &format!("/api/v1/documents/{document}/attachments"),
            Some(token),
            name,
            "application/pdf",
            bytes,
            None,
        )
        .await;

    assert_eq!(uploaded.status, StatusCode::OK, "{}", uploaded.body);
    // #246 AC1: the upload returned without waiting for any scanner.
    assert_eq!(uploaded.body["data"]["virusScanStatus"], "PENDING");

    id_of(&uploaded.body["data"])
}

async fn status_of(app: &TestApp, id: Uuid) -> String {
    sqlx::query_scalar("SELECT virus_scan_status FROM attachments WHERE id = $1")
        .bind(id)
        .fetch_one(&app.pool)
        .await
        .expect("the scan status")
}

/// A listener that speaks INSTREAM and answers with whatever it was told to.
///
/// It drains the stream first: a worker that could not finish writing would fail
/// for the wrong reason. `None` means answer nothing at all.
async fn scripted_clamd(reply: Option<&'static str>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a port");
    let port = listener.local_addr().expect("an address").port();

    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut sink = [0_u8; 8192];

                loop {
                    match socket.read(&mut sink).await {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            if let Some(reply) = reply {
                                let _ = socket.write_all(reply.as_bytes()).await;
                                let _ = socket.write_all(b"\0").await;
                                let _ = socket.shutdown().await;
                                return;
                            }
                        }
                    }
                }

                let _ = socket.shutdown().await;
            });
        }
    });

    port
}

/// The environment's real scanner.
fn real_clamd() -> (String, u16) {
    (
        std::env::var("KELIR_CLAMAV_HOST").unwrap_or_else(|_| "127.0.0.1".to_owned()),
        std::env::var("KELIR_CLAMAV_PORT")
            .ok()
            .and_then(|raw| raw.parse().ok())
            .unwrap_or(3310),
    )
}

// ---------------------------------------------------------------------------
// Against a real scanner
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_clean_file_is_cleared_and_then_served() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_SCAN_CLEAN").await;
    let document = draft(&app, &token, type_id).await;
    let attachment = attach(&app, &token, document, "q.pdf", PDF).await;

    // Refused before the scan — #245's gate, and the state every attachment is
    // in until this pass runs.
    let before = app
        .get(
            &format!("/api/v1/documents/{document}/attachments/{attachment}"),
            Some(&token),
        )
        .await;
    assert_eq!(before.status, StatusCode::CONFLICT, "{}", before.body);

    let (host, port) = real_clamd();
    worker::pass(&app.state, &host, port).await.expect("a pass");

    assert_eq!(status_of(&app, attachment).await, "CLEAN");

    let after = app
        .get_raw(
            &format!("/api/v1/documents/{document}/attachments/{attachment}"),
            Some(&token),
        )
        .await;

    assert_eq!(after.status, StatusCode::OK);
    assert_eq!(after.bytes, PDF);
}

/// **Real detection, proven where nothing stands between the bytes and the
/// scanner.**
///
/// This is the half of an infected file only a real `clamd` can answer, and it
/// is asserted at the client rather than through an upload because the upload
/// would refuse these bytes: they are not a type this deployment stores, and
/// they cannot be made into one without ceasing to be detectable.
#[tokio::test]
async fn a_real_scanner_finds_a_real_signature() {
    let (host, port) = real_clamd();

    let outcome = scanner::scan(&host, port, EICAR)
        .await
        .expect("the scanner to answer");

    match outcome {
        ScanOutcome::Infected(signature) => {
            assert!(
                signature.contains("Eicar"),
                "the signature should name what it found: {signature}"
            );
        }
        other => panic!("a real signature was not found: {other:?}"),
    }
}

/// **And the product's behaviour once something is found**, driven by a scripted
/// `FOUND` for the reason the fixture's own documentation gives.
#[tokio::test]
async fn an_infected_file_is_recorded_and_never_served() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_SCAN_EICAR").await;
    let document = draft(&app, &token, type_id).await;
    let attachment = attach(&app, &token, document, "invoice.pdf", PDF).await;

    let port = scripted_clamd(Some("stream: Eicar-Test-Signature FOUND")).await;
    worker::pass(&app.state, "127.0.0.1", port)
        .await
        .expect("a pass");

    assert_eq!(status_of(&app, attachment).await, "INFECTED");

    let refused = app
        .get(
            &format!("/api/v1/documents/{document}/attachments/{attachment}"),
            Some(&token),
        )
        .await;

    assert_eq!(refused.status, StatusCode::CONFLICT, "{}", refused.body);
}

// ---------------------------------------------------------------------------
// The arms a real scanner will not produce on demand
// ---------------------------------------------------------------------------

/// **#246 AC7.** A scanner that is not there says nothing about the file, so the
/// row stays `PENDING` — the download stays refused and the next pass tries
/// again. Recording `FAILED` would be worse than useless: the statement writes
/// only over `PENDING`, so a transport failure stored as a result would make the
/// attachment permanently undownloadable.
///
/// **Seen red, 2026-08-31**, with the `Err` arm changed to record `FAILED` —
/// together with the silent-scanner test below it.
#[tokio::test]
async fn a_scanner_that_cannot_be_reached_leaves_the_file_pending() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_SCAN_DOWN").await;
    let document = draft(&app, &token, type_id).await;
    let attachment = attach(&app, &token, document, "q.pdf", PDF).await;

    // A port nothing is listening on: bound, then dropped.
    let closed = TcpListener::bind("127.0.0.1:0").await.expect("a port");
    let port = closed.local_addr().expect("an address").port();
    drop(closed);

    worker::pass(&app.state, "127.0.0.1", port)
        .await
        .expect("a pass that survives an unreachable scanner");

    assert_eq!(
        status_of(&app, attachment).await,
        "PENDING",
        "an unreachable scanner must leave the file unknown, not decided"
    );
}

#[tokio::test]
async fn a_scanner_that_accepts_and_says_nothing_leaves_the_file_pending() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_SCAN_SILENT").await;
    let document = draft(&app, &token, type_id).await;
    let attachment = attach(&app, &token, document, "q.pdf", PDF).await;

    let port = scripted_clamd(None).await;

    worker::pass(&app.state, "127.0.0.1", port)
        .await
        .expect("a pass that survives a silent scanner");

    assert_eq!(status_of(&app, attachment).await, "PENDING");
}

/// The reply the measurement found and `clamdscan` hides: a stream `clamd`
/// refuses. **It ran and it refused**, which is a result — `FAILED` — and not
/// the same thing as not answering.
#[tokio::test]
async fn a_stream_the_scanner_refuses_is_recorded_failed_and_not_clean() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_SCAN_ERROR").await;
    let document = draft(&app, &token, type_id).await;
    let attachment = attach(&app, &token, document, "q.pdf", PDF).await;

    let port = scripted_clamd(Some("INSTREAM size limit exceeded. ERROR")).await;

    worker::pass(&app.state, "127.0.0.1", port)
        .await
        .expect("a pass");

    assert_eq!(status_of(&app, attachment).await, "FAILED");
}

/// **A reply this binary does not understand is not a pass.**
///
/// **Seen red, 2026-08-31**, with `interpret`'s fallthrough changed from
/// `Failed` to `Clean` — which reddens the two unit tests beside that function
/// first, so the campaign sees it there.
#[tokio::test]
async fn a_reply_nothing_recognises_is_recorded_failed() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_SCAN_ODD").await;
    let document = draft(&app, &token, type_id).await;
    let attachment = attach(&app, &token, document, "q.pdf", PDF).await;

    let port = scripted_clamd(Some("stream: SOMETHING NEW IN A LATER CLAMAV")).await;

    worker::pass(&app.state, "127.0.0.1", port)
        .await
        .expect("a pass");

    assert_eq!(status_of(&app, attachment).await, "FAILED");
}

// ---------------------------------------------------------------------------
// AC5 — a decided row is decided
// ---------------------------------------------------------------------------

/// **The `PENDING` predicate on the write, reached directly.**
///
/// The test below drives two worker passes and *does not reach this
/// statement's guard at all*: `pending_scans` selects only `PENDING` rows, so a
/// decided attachment is never handed to `record_scan_result` in the first
/// place. Dropping `AND virus_scan_status = 'PENDING'` from the `UPDATE` left
/// every test in this file green — found by mutation on 2026-08-31, and it is
/// the shape [record 07](../../projects/verifications/07.%20Sprint%2010%20Surface%20Verification.md)
/// named: a guard behind another guard is a guard nothing exercises.
///
/// So this calls the statement itself. **Seen red, 2026-08-31**, with the
/// predicate removed.
#[tokio::test]
async fn the_write_itself_refuses_to_move_a_decided_row() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_SCAN_WRITE").await;
    let document = draft(&app, &token, type_id).await;
    let attachment = attach(&app, &token, document, "q.pdf", PDF).await;

    // Decided, by whatever route.
    // **The tenant travels with the id** (#294 AC1). It is the same tenant the
    // worker reads off the row, so this call proves the predicate does not
    // refuse a legitimate write rather than proving it fires.
    let tenant = common::fixtures::SYSTEM_TENANT_ID;

    let moved = repository::record_scan_result(&app.pool, tenant, attachment, "INFECTED")
        .await
        .expect("the first result");
    assert_eq!(moved, 1, "the first result moves the row");

    // Everything a later writer might try, including the same value again.
    for later in ["CLEAN", "PENDING", "FAILED", "INFECTED"] {
        let moved = repository::record_scan_result(&app.pool, tenant, attachment, later)
            .await
            .expect("a later result");

        assert_eq!(moved, 0, "`{later}` moved a row that was already decided");
        assert_eq!(status_of(&app, attachment).await, "INFECTED");
    }
}

/// **A scan result moves the row exactly once, and never out of `INFECTED`.**
///
/// The second pass is given a scripted scanner that says `OK` about everything.
/// If the statement wrote over anything but `PENDING`, this is where an infected
/// file would become downloadable.
#[tokio::test]
async fn a_decided_attachment_is_not_moved_by_a_later_scan() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_SCAN_ONCE").await;
    let document = draft(&app, &token, type_id).await;
    let attachment = attach(&app, &token, document, "invoice.pdf", PDF).await;

    let infected = scripted_clamd(Some("stream: Eicar-Test-Signature FOUND")).await;
    worker::pass(&app.state, "127.0.0.1", infected)
        .await
        .expect("the first pass");

    assert_eq!(status_of(&app, attachment).await, "INFECTED");

    // A scanner that clears everything, run over the same attachment again.
    let forgiving = scripted_clamd(Some("stream: OK")).await;
    worker::pass(&app.state, "127.0.0.1", forgiving)
        .await
        .expect("the second pass");

    assert_eq!(
        status_of(&app, attachment).await,
        "INFECTED",
        "a decided row was moved by a later scan"
    );

    let refused = app
        .get(
            &format!("/api/v1/documents/{document}/attachments/{attachment}"),
            Some(&token),
        )
        .await;

    assert_eq!(refused.status, StatusCode::CONFLICT, "{}", refused.body);
}
