//! An attachment moves through this process as a stream ([#834], D-140).
//!
//! # A wrapped store, and it is still the real one
//!
//! `attachment_upload.rs`'s header is why: a stand-in store verifies the
//! stand-in. [`Watched`] holds the MinIO store and passes every call to it, so
//! each object here is a real S3 object and each multipart upload a real one. It
//! only writes down what it was asked and when each byte was accepted, which is
//! the one thing the store cannot say afterwards.
//!
//! # What the memory tests measure
//!
//! **No RSS.** A process's resident size says more about the allocator than
//! about this path. [`Flow`] counts the bytes one side has handed over and the
//! other has not yet taken, and its peak is the most the process can have been
//! holding for that transfer. Each is held under two parts for a 25 MiB file, and
//! against the buffered code each reads 25 MiB.
//!
//! [#834]: https://github.com/sujanto-gaws/kelir/issues/834

mod common;

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use axum::body::{to_bytes, Body, Bytes};
use axum::extract::ConnectInfo;
use axum::http::{header, Method, Request, Response, StatusCode};
use futures_util::stream::{self, BoxStream, StreamExt};
use object_store::path::Path as ObjectPath;
use object_store::{
    GetOptions, GetResult, GetResultPayload, ListResult, MultipartUpload, ObjectMeta, ObjectStore,
    PutMultipartOptions, PutOptions, PutPayload, PutResult, UploadPart,
};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpSocket;
use tower::ServiceExt;
use uuid::Uuid;

use common::{TestApp, TEST_PEER};
use kelir_backend::config::AppConfig;
use kelir_backend::modules::attachment::storage::ObjectStorage;
use kelir_backend::modules::attachment::worker;

const MIB: usize = 1024 * 1024;
const PART: usize = 5 * MIB;
/// **About two parts** (AC9): what the path may hold, for any file.
const BOUND: u64 = 2 * PART as u64;
const LARGE: usize = 25 * MIB;
/// The cap a 25 MiB file and its multipart framing fit under; the default is
/// exactly 25 MiB, and the cap is on the body.
const ROOMY: usize = 30 * MIB;
/// What one read of a request body is, roughly, off a socket.
const FRAME: usize = 64 * 1024;
const BOUNDARY: &str = "kelirstreamsboundary";

// ---------------------------------------------------------------------------
// The two instruments
// ---------------------------------------------------------------------------

/// One transfer's bytes: read from the source, accepted by the destination, and
/// the largest the difference ever was.
#[derive(Debug, Default)]
struct Flow {
    read: AtomicU64,
    accepted: AtomicU64,
    peak: AtomicU64,
}

impl Flow {
    fn read(&self, bytes: usize) {
        let read = self.read.fetch_add(bytes as u64, Ordering::SeqCst) + bytes as u64;
        let accepted = self.accepted.load(Ordering::SeqCst);

        self.peak
            .fetch_max(read.saturating_sub(accepted), Ordering::SeqCst);
    }

    fn accepted(&self, bytes: usize) {
        self.accepted.fetch_add(bytes as u64, Ordering::SeqCst);
    }

    /// The upload a read test starts from is not that test's transfer.
    fn reset(&self) {
        self.read.store(0, Ordering::SeqCst);
        self.accepted.store(0, Ordering::SeqCst);
        self.peak.store(0, Ordering::SeqCst);
    }

    fn peak(&self) -> u64 {
        self.peak.load(Ordering::SeqCst)
    }

    fn total_read(&self) -> u64 {
        self.read.load(Ordering::SeqCst)
    }

    fn total_accepted(&self) -> u64 {
        self.accepted.load(Ordering::SeqCst)
    }
}

/// What the store was asked to do.
#[derive(Debug, Default)]
struct Ledger {
    puts: AtomicUsize,
    opened: AtomicUsize,
    parts: Mutex<Vec<usize>>,
    completed: AtomicUsize,
    aborted: AtomicUsize,
    /// Every abort the store itself refused, which a count of aborts would hide.
    abort_failures: AtomicUsize,
}

impl Ledger {
    fn parts(&self) -> Vec<usize> {
        self.parts.lock().expect("the ledger").clone()
    }

    fn calls(&self) -> usize {
        self.puts.load(Ordering::SeqCst)
            + self.opened.load(Ordering::SeqCst)
            + self.parts().len()
            + self.completed.load(Ordering::SeqCst)
            + self.aborted.load(Ordering::SeqCst)
    }

    /// **The abort is spawned, so it can land after the answer.** A disconnect
    /// runs no code that could wait for it. This waits for the store's reply and
    /// no longer than a store takes to give one.
    async fn aborts_settled(&self, expected: usize) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);

        while self.aborted.load(Ordering::SeqCst) < expected {
            assert!(
                tokio::time::Instant::now() < deadline,
                "{expected} abort(s) expected; the store saw {}",
                self.aborted.load(Ordering::SeqCst)
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        assert_eq!(
            self.abort_failures.load(Ordering::SeqCst),
            0,
            "the store refused an abort"
        );
    }
}

/// The real store, watched. See this file's header.
#[derive(Debug)]
struct Watched {
    inner: Arc<dyn ObjectStore>,
    ledger: Arc<Ledger>,
    flow: Arc<Flow>,
    /// A store slower than a client is what fills a process that does not wait.
    part_takes: Duration,
    /// The read stream fails once this many bytes have left it.
    fail_read_after: Option<u64>,
}

impl std::fmt::Display for Watched {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "Watched({})", self.inner)
    }
}

#[async_trait]
impl ObjectStore for Watched {
    async fn put_opts(
        &self,
        location: &ObjectPath,
        payload: PutPayload,
        options: PutOptions,
    ) -> object_store::Result<PutResult> {
        let size = payload.content_length();

        self.ledger.puts.fetch_add(1, Ordering::SeqCst);

        let stored = self.inner.put_opts(location, payload, options).await?;

        self.flow.accepted(size);

        Ok(stored)
    }

    async fn put_multipart_opts(
        &self,
        location: &ObjectPath,
        options: PutMultipartOptions,
    ) -> object_store::Result<Box<dyn MultipartUpload>> {
        self.ledger.opened.fetch_add(1, Ordering::SeqCst);

        Ok(Box::new(WatchedUpload {
            inner: self.inner.put_multipart_opts(location, options).await?,
            ledger: Arc::clone(&self.ledger),
            flow: Arc::clone(&self.flow),
            part_takes: self.part_takes,
        }))
    }

    async fn get_opts(
        &self,
        location: &ObjectPath,
        options: GetOptions,
    ) -> object_store::Result<GetResult> {
        let result = self.inner.get_opts(location, options).await?;

        let (meta, range, attributes) = (
            result.meta.clone(),
            result.range.clone(),
            result.attributes.clone(),
        );
        let flow = Arc::clone(&self.flow);
        let fail_after = self.fail_read_after;
        let mut served = 0_u64;

        let stream = result
            .into_stream()
            .map(move |chunk| {
                if fail_after.is_some_and(|limit| served >= limit) {
                    return Err(object_store::Error::Generic {
                        store: "Watched",
                        source: "the store went away mid-read".into(),
                    });
                }

                let chunk = chunk?;

                served += chunk.len() as u64;
                flow.read(chunk.len());

                Ok(chunk)
            })
            .boxed();

        Ok(GetResult {
            payload: GetResultPayload::Stream(stream),
            meta,
            range,
            attributes,
        })
    }

    async fn delete(&self, location: &ObjectPath) -> object_store::Result<()> {
        self.inner.delete(location).await
    }

    fn list(
        &self,
        prefix: Option<&ObjectPath>,
    ) -> BoxStream<'static, object_store::Result<ObjectMeta>> {
        self.inner.list(prefix)
    }

    async fn list_with_delimiter(
        &self,
        prefix: Option<&ObjectPath>,
    ) -> object_store::Result<ListResult> {
        self.inner.list_with_delimiter(prefix).await
    }

    async fn copy(&self, from: &ObjectPath, to: &ObjectPath) -> object_store::Result<()> {
        self.inner.copy(from, to).await
    }

    async fn copy_if_not_exists(
        &self,
        from: &ObjectPath,
        to: &ObjectPath,
    ) -> object_store::Result<()> {
        self.inner.copy_if_not_exists(from, to).await
    }
}

#[derive(Debug)]
struct WatchedUpload {
    inner: Box<dyn MultipartUpload>,
    ledger: Arc<Ledger>,
    flow: Arc<Flow>,
    part_takes: Duration,
}

#[async_trait]
impl MultipartUpload for WatchedUpload {
    fn put_part(&mut self, data: PutPayload) -> UploadPart {
        let size = data.content_length();

        self.ledger.parts.lock().expect("the ledger").push(size);

        let sent = self.inner.put_part(data);
        let flow = Arc::clone(&self.flow);
        let takes = self.part_takes;

        Box::pin(async move {
            tokio::time::sleep(takes).await;
            sent.await?;
            flow.accepted(size);

            Ok(())
        })
    }

    async fn complete(&mut self) -> object_store::Result<PutResult> {
        let completed = self.inner.complete().await?;

        self.ledger.completed.fetch_add(1, Ordering::SeqCst);

        Ok(completed)
    }

    async fn abort(&mut self) -> object_store::Result<()> {
        let aborted = self.inner.abort().await;

        if aborted.is_err() {
            self.ledger.abort_failures.fetch_add(1, Ordering::SeqCst);
        }

        self.ledger.aborted.fetch_add(1, Ordering::SeqCst);

        aborted
    }
}

/// An app whose store is watched, with the file cap the caller chose.
struct Watching {
    app: TestApp,
    token: String,
    store: Arc<dyn ObjectStore>,
    ledger: Arc<Ledger>,
    flow: Arc<Flow>,
}

async fn watching(max_upload_bytes: usize, tune: impl FnOnce(&mut Watched)) -> Watching {
    let mut app = TestApp::spawn_with(|config: &mut AppConfig| {
        config.storage_max_upload_bytes = max_upload_bytes;
    })
    .await;

    let (inner, bucket) = match app.state.storage.clone() {
        ObjectStorage::S3 { store, bucket } => (store, bucket),
        ObjectStorage::Unavailable { reason } => panic!("no store: {reason}"),
    };

    let ledger = Arc::new(Ledger::default());
    let flow = Arc::new(Flow::default());
    let mut watched = Watched {
        inner: Arc::clone(&inner),
        ledger: Arc::clone(&ledger),
        flow: Arc::clone(&flow),
        part_takes: Duration::ZERO,
        fail_read_after: None,
    };

    tune(&mut watched);

    app.state.storage = ObjectStorage::S3 {
        store: Arc::new(watched),
        bucket,
    };

    let token = app.administrator_token().await;

    Watching {
        app,
        token,
        store: inner,
        ledger,
        flow,
    }
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// A PDF by its first bytes and no two parts alike after them, so that parts out
/// of order, or one sent twice, cannot read back as the same file.
fn pdf_of(size: usize) -> Vec<u8> {
    let mut file = b"%PDF-1.7\n".to_vec();

    file.extend(
        (file.len()..size)
            .map(|at| (at as u64).wrapping_mul(2_654_435_761) as u8 ^ (at >> 16) as u8),
    );
    file.truncate(size);

    file
}

enum Part<'a> {
    File(&'a str, &'a [u8]),
    Text(&'a str, &'a str),
}

fn multipart(parts: &[Part<'_>]) -> Vec<u8> {
    let mut body = Vec::new();

    for part in parts {
        body.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());

        match part {
            Part::File(name, bytes) => {
                body.extend_from_slice(
                    format!(
                        "Content-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\n\
                         Content-Type: application/pdf\r\n\r\n"
                    )
                    .as_bytes(),
                );
                body.extend_from_slice(bytes);
            }
            Part::Text(name, value) => {
                body.extend_from_slice(
                    format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
                );
                body.extend_from_slice(value.as_bytes());
            }
        }

        body.extend_from_slice(b"\r\n");
    }

    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());

    body
}

/// A body that arrives the way a socket delivers one, counting as it is read.
fn counted(body: Vec<u8>, flow: Arc<Flow>) -> Body {
    let frames: Vec<Bytes> = body.chunks(FRAME).map(Bytes::copy_from_slice).collect();

    Body::from_stream(stream::iter(frames).map(move |frame| {
        flow.read(frame.len());

        Ok::<_, std::convert::Infallible>(frame)
    }))
}

fn request(
    method: Method,
    uri: &str,
    token: &str,
    content_type: Option<String>,
    body: Body,
) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"));

    if let Some(content_type) = content_type {
        builder = builder.header(header::CONTENT_TYPE, content_type);
    }

    let mut request = builder.body(body).expect("a request");

    request.extensions_mut().insert(ConnectInfo(TEST_PEER));

    request
}

fn multipart_type() -> Option<String> {
    Some(format!("multipart/form-data; boundary={BOUNDARY}"))
}

impl Watching {
    async fn document(&self, code: &str) -> Uuid {
        let created = self
            .app
            .send(
                Method::POST,
                "/api/v1/document-types",
                Some(&self.token),
                Some(json!({ "typeCode": code, "name": code })),
            )
            .await;
        assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

        let drafted = self
            .app
            .send(
                Method::POST,
                "/api/v1/documents",
                Some(&self.token),
                Some(json!({
                    "documentTypeId": created.body["data"]["id"],
                    "title": "Two standing desks",
                })),
            )
            .await;
        assert_eq!(drafted.status, StatusCode::CREATED, "{}", drafted.body);

        id_of(&drafted.body["data"])
    }

    async fn post(&self, document: Uuid, body: Vec<u8>) -> (StatusCode, Value) {
        self.post_as(&self.token, document, body).await
    }

    async fn post_as(&self, token: &str, document: Uuid, body: Vec<u8>) -> (StatusCode, Value) {
        let response = self
            .app
            .router()
            .oneshot(request(
                Method::POST,
                &format!("/api/v1/documents/{document}/attachments"),
                token,
                multipart_type(),
                counted(body, Arc::clone(&self.flow)),
            ))
            .await
            .expect("the router answers");

        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("the answer");

        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn upload(&self, document: Uuid, name: &str, file: &[u8]) -> Uuid {
        let (status, body) = self
            .post(document, multipart(&[Part::File(name, file)]))
            .await;

        assert_eq!(status, StatusCode::OK, "{body}");

        id_of(&body["data"])
    }

    async fn download(&self, document: Uuid, attachment: Uuid) -> Response<Body> {
        self.app
            .router()
            .oneshot(request(
                Method::GET,
                &format!("/api/v1/documents/{document}/attachments/{attachment}"),
                &self.token,
                None,
                Body::empty(),
            ))
            .await
            .expect("the router answers")
    }

    async fn clear(&self, attachment: Uuid) {
        sqlx::query("UPDATE attachments SET virus_scan_status = 'CLEAN' WHERE id = $1")
            .bind(attachment)
            .execute(&self.app.pool)
            .await
            .expect("the scan result");
    }
}

fn id_of(value: &Value) -> Uuid {
    value["id"]
        .as_str()
        .expect("an id")
        .parse()
        .expect("a uuid")
}

// ---------------------------------------------------------------------------
// AC9 — the memory, seen red against the buffered code
// ---------------------------------------------------------------------------

/// **A store slower than the client** is the case a part in flight has to hold
/// against: 100 ms a part, so nothing but waiting for the store keeps the body
/// from being read ahead of it.
#[tokio::test]
async fn an_upload_never_holds_more_than_two_parts_of_a_25_mib_file() {
    let watching = watching(ROOMY, |store| store.part_takes = Duration::from_millis(100)).await;
    let document = watching.document("PR_STREAM_MEM_UP").await;
    let file = pdf_of(LARGE);

    watching.upload(document, "large.pdf", &file).await;

    assert!(
        watching.flow.total_read() >= LARGE as u64,
        "the instrument saw {} bytes",
        watching.flow.total_read()
    );
    assert_eq!(
        watching.flow.total_accepted(),
        LARGE as u64,
        "the store did not accept the whole file"
    );
    assert!(
        watching.flow.peak() <= BOUND,
        "{} bytes were read and not yet accepted; the bound is {BOUND}",
        watching.flow.peak()
    );
}

/// **A slow reader** takes each frame a millisecond after the last, so a stream
/// that reads ahead of its reader shows here, and one that reads the whole object
/// before its first frame reads 25 MiB.
#[tokio::test]
async fn a_download_behind_a_slow_reader_never_holds_more_than_two_parts() {
    let watching = watching(ROOMY, |_| {}).await;
    let document = watching.document("PR_STREAM_MEM_DOWN").await;
    let file = pdf_of(LARGE);
    let attachment = watching.upload(document, "large.pdf", &file).await;

    watching.clear(attachment).await;

    let flow = Arc::clone(&watching.flow);
    flow.reset();

    let response = watching.download(document, attachment).await;

    assert_eq!(response.status(), StatusCode::OK);

    let mut body = response.into_body().into_data_stream();
    let mut taken = Vec::with_capacity(LARGE);

    while let Some(frame) = body.next().await {
        let frame = frame.expect("a frame");

        tokio::time::sleep(Duration::from_millis(1)).await;
        flow.accepted(frame.len());
        taken.extend_from_slice(&frame);
    }

    assert_eq!(taken, file, "the bytes served are not the bytes stored");
    assert_eq!(flow.total_read(), LARGE as u64);
    assert!(
        flow.peak() <= BOUND,
        "{} bytes were read and not yet taken; the bound is {BOUND}",
        flow.peak()
    );
}

/// **A scanner that reads slowly**, on a socket with a small window, so that what
/// the worker has read and `clamd` has not cannot sit in a kernel buffer instead.
#[tokio::test]
async fn the_worker_never_holds_more_than_two_parts_of_the_file_it_scans() {
    let watching = watching(ROOMY, |_| {}).await;
    let document = watching.document("PR_STREAM_MEM_SCAN").await;
    let file = pdf_of(LARGE);
    let attachment = watching.upload(document, "large.pdf", &file).await;

    let flow = Arc::clone(&watching.flow);
    flow.reset();

    let port = slow_clamd(Arc::clone(&flow)).await;

    worker::pass(&watching.app.state, "127.0.0.1", port)
        .await
        .expect("the pass");

    let status: String =
        sqlx::query_scalar("SELECT virus_scan_status FROM attachments WHERE id = $1")
            .bind(attachment)
            .fetch_one(&watching.app.pool)
            .await
            .expect("the status");

    assert_eq!(status, "CLEAN");
    assert_eq!(flow.total_accepted(), LARGE as u64);
    assert!(
        flow.peak() <= BOUND,
        "{} bytes were read and not yet taken by the scanner; the bound is {BOUND}",
        flow.peak()
    );
}

/// INSTREAM, read at a pace, answering once the zero-length frame has come. The
/// frames are counted as they are taken off the socket.
async fn slow_clamd(flow: Arc<Flow>) -> u16 {
    let socket = TcpSocket::new_v4().expect("a socket");

    socket
        .set_recv_buffer_size(64 * 1024)
        .expect("a small window");
    socket
        .bind("127.0.0.1:0".parse().expect("an address"))
        .expect("a port");

    let port = socket.local_addr().expect("an address").port();
    let listener = socket.listen(8).expect("a listener");

    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("the worker");
        let mut command = [0_u8; 10];

        socket.read_exact(&mut command).await.expect("the command");
        assert_eq!(&command, b"zINSTREAM\0");

        let mut frame = vec![0_u8; 128 * 1024];

        loop {
            let mut length = [0_u8; 4];

            socket.read_exact(&mut length).await.expect("a length");

            let length = u32::from_be_bytes(length) as usize;

            if length == 0 {
                break;
            }

            socket
                .read_exact(&mut frame[..length])
                .await
                .expect("a chunk");
            flow.accepted(length);
            tokio::time::sleep(Duration::from_millis(1)).await;
        }

        socket.write_all(b"stream: OK\0").await.expect("the answer");
        socket.shutdown().await.ok();
    });

    port
}
