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
//! **The upload's is measured on a real socket.** `multer` reads its stream until
//! the stream says *pending*, so a body that is always ready, as an in-memory one
//! is, is read whole into `multer`'s buffer whatever this product does. Over
//! hyper the body is pending between frames, because the connection and the
//! handler share a task, and that is what has to be shown. So [`served`] counts
//! what hyper hands up. The other tests here want a body in pieces, and
//! [`frames_of`] gives them one.
//!
//! # Seen to fail
//!
//! The three memory tests were red against the buffered code: 26,214,552 bytes
//! held for the upload, and 26,214,400 for the download and for the scan, against
//! a bound of 10,485,760. Streaming, the upload holds 5,624,946.
//!
//! Each mutation below was made alone and reverted. **Seen red, 2026-11-01**, in
//! `kelir-rust-tools:1.89`.
//!
//! | Mutation | Reddened at |
//! |---|---|
//! | The guard spawns no abort | Six: the cap past a part, a dropped request, a refused part, a refused category, a broken body, the second `file` |
//! | `complete` leaves the guard armed | `each_size_takes_its_road_…`, at the abort that follows the answer |
//! | A part sent as soon as the buffer is full | `each_size_takes_its_road_…`, at exactly one part |
//! | `PART_SIZE` one byte short | `each_size_takes_its_road_…` |
//! | The head not judged before an upload opens | `a_refusal_read_from_the_head_…` |
//! | A refusal answered from inside the body | `the_last_file_part_wins_…` |
//! | The body read before the caller is judged | `a_caller_who_may_not_attach_…` |
//! | A failed read ends the scan as a whole file | `scanner::tests::a_file_that_stops_mid_read_…` |
//!
//! [#834]: https://github.com/sujanto-gaws/kelir/issues/834

mod common;

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use axum::body::{to_bytes, Body, Bytes};
use axum::extract::ConnectInfo;
use axum::http::{header, Method, Request, Response, StatusCode};
use futures_util::stream::{self, BoxStream, StreamExt, TryStreamExt};
use object_store::path::Path as ObjectPath;
use object_store::{
    GetOptions, GetResult, GetResultPayload, ListResult, MultipartUpload, ObjectMeta, ObjectStore,
    PutMultipartOptions, PutOptions, PutPayload, PutResult, UploadPart,
};
use serde_json::{json, Value};
use sha2::Digest;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpSocket, TcpStream};
use tower::ServiceExt;
use uuid::Uuid;

use common::{fixtures, TestApp, TEST_PEER};
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
    /// The store refuses the part with this index.
    refuse_part: Option<usize>,
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
            refuse_part: self.refuse_part,
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
    refuse_part: Option<usize>,
}

#[async_trait]
impl MultipartUpload for WatchedUpload {
    fn put_part(&mut self, data: PutPayload) -> UploadPart {
        let size = data.content_length();
        let index = {
            let mut parts = self.ledger.parts.lock().expect("the ledger");

            parts.push(size);
            parts.len() - 1
        };

        if self.refuse_part == Some(index) {
            return Box::pin(async {
                Err(object_store::Error::Generic {
                    store: "Watched",
                    source: "the store refused the part".into(),
                })
            });
        }

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
        refuse_part: None,
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

/// Frames that arrive the way hyper delivers them: **through a channel from
/// another task**, so that between frames the body is pending until the runtime
/// has turned, and a second poll straight after the first finds nothing. A
/// `yield_now` does not do that. `multer` polls twice on its way to the first
/// boundary, and a yield resolved on the second poll read the whole of a short
/// body and its end at once, which `multer` calls an incomplete stream.
fn frames_of(
    frames: Vec<Bytes>,
    flow: Arc<Flow>,
) -> impl futures_util::Stream<Item = std::io::Result<Bytes>> + Send + 'static {
    let (sender, receiver) = tokio::sync::mpsc::channel(1);

    tokio::spawn(async move {
        for frame in frames {
            if sender.send(frame).await.is_err() {
                return;
            }
        }
    });

    stream::unfold(receiver, move |mut receiver| {
        let flow = Arc::clone(&flow);

        async move {
            let frame: Bytes = receiver.recv().await?;

            flow.read(frame.len());

            Some((Ok(frame), receiver))
        }
    })
}

fn framed(frames: Vec<Bytes>, flow: Arc<Flow>) -> Body {
    Body::from_stream(frames_of(frames, flow))
}

fn stream_of(
    body: Vec<u8>,
    flow: Arc<Flow>,
) -> impl futures_util::Stream<Item = std::io::Result<Bytes>> + Send + 'static {
    frames_of(
        body.chunks(FRAME).map(Bytes::copy_from_slice).collect(),
        flow,
    )
}

/// The application on a real socket, counting the request body as hyper hands
/// it up. See this file's header.
async fn served(app: &TestApp, flow: Arc<Flow>) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a port");
    let address = listener.local_addr().expect("an address");

    let service = app
        .router()
        .layer(axum::middleware::map_request(
            move |request: Request<Body>| {
                let flow = Arc::clone(&flow);

                async move {
                    let (parts, body) = request.into_parts();
                    let counted = body.into_data_stream().inspect_ok(move |frame| {
                        flow.read(frame.len());
                    });

                    Request::from_parts(parts, Body::from_stream(counted))
                }
            },
        ))
        .into_make_service_with_connect_info::<SocketAddr>();

    tokio::spawn(async move {
        let _ = axum::serve(listener, service).await;
    });

    address
}

/// Posts `body` down a socket as fast as the socket takes it, and hands back the
/// response's first line.
async fn post_over_a_socket(address: SocketAddr, uri: &str, token: &str, body: Vec<u8>) -> String {
    let head = format!(
        "POST {uri} HTTP/1.1\r\nHost: kelir.test\r\nAuthorization: Bearer {token}\r\n\
         Content-Type: multipart/form-data; boundary={BOUNDARY}\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );

    // `split`, not `into_split`: an owned write half shuts the socket's write side
    // when it is dropped, and hyper reads that as a client gone mid-request.
    let mut socket = TcpStream::connect(address).await.expect("connect");
    let (mut reading, mut writing) = socket.split();
    let mut answer = Vec::new();

    let write = async {
        writing.write_all(head.as_bytes()).await.expect("the head");
        writing.write_all(&body).await.expect("the body");
    };
    let read = async {
        reading.read_to_end(&mut answer).await.expect("the answer");
    };

    tokio::join!(write, read);

    let answer = String::from_utf8_lossy(&answer);

    answer.lines().next().unwrap_or_default().to_owned()
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
                Body::from_stream(stream_of(body, Arc::clone(&self.flow))),
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

    /// Every object the store holds for this document.
    async fn objects(&self, document: Uuid) -> Vec<ObjectPath> {
        let prefix = ObjectPath::from(format!(
            "tenants/{}/documents/{document}",
            fixtures::SYSTEM_TENANT_ID
        ));

        self.store
            .list(Some(&prefix))
            .map(|object| object.expect("a listing").location)
            .collect()
            .await
    }

    async fn rows(&self, document: Uuid) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM attachments WHERE document_id = $1")
            .bind(document)
            .fetch_one(&self.app.pool)
            .await
            .expect("a count")
    }

    async fn status_of(&self, attachment: Uuid) -> String {
        sqlx::query_scalar("SELECT virus_scan_status FROM attachments WHERE id = $1")
            .bind(attachment)
            .fetch_one(&self.app.pool)
            .await
            .expect("the status")
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
/// against: 100 ms a part, with the client writing as fast as the socket takes
/// it, so nothing but waiting for the store keeps the body from being read ahead
/// of it. On a real socket; this file's header is why.
#[tokio::test]
async fn an_upload_never_holds_more_than_two_parts_of_a_25_mib_file() {
    let watching = watching(ROOMY, |store| {
        store.part_takes = Duration::from_millis(100);
    })
    .await;
    let document = watching.document("PR_STREAM_MEM_UP").await;
    let file = pdf_of(LARGE);
    let address = served(&watching.app, Arc::clone(&watching.flow)).await;

    let answer = post_over_a_socket(
        address,
        &format!("/api/v1/documents/{document}/attachments"),
        &watching.token,
        multipart(&[Part::File("large.pdf", &file)]),
    )
    .await;

    assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
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

    assert_eq!(watching.status_of(attachment).await, "CLEAN");
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

        // A stream that stops is the far side closing: nothing to answer.
        loop {
            let mut length = [0_u8; 4];

            if socket.read_exact(&mut length).await.is_err() {
                return;
            }

            let length = u32::from_be_bytes(length) as usize;

            if length == 0 {
                break;
            }

            if socket.read_exact(&mut frame[..length]).await.is_err() {
                return;
            }

            flow.accepted(length);
            tokio::time::sleep(Duration::from_millis(1)).await;
        }

        socket.write_all(b"stream: OK\0").await.expect("the answer");
        socket.shutdown().await.ok();
    });

    port
}

// ---------------------------------------------------------------------------
// AC1, AC3, AC6 — which road a file takes, and that it arrives whole
// ---------------------------------------------------------------------------

/// **The boundary, from both sides.** A file of exactly one part is a `put`, and
/// one byte more is an upload; a file of exactly two parts has no empty third.
/// Each checksum and size is compared with the bytes read back from the store.
#[tokio::test]
async fn each_size_takes_its_road_and_is_stored_exactly() {
    for (size, puts, parts) in [
        (1_000, 1, vec![]),
        (PART, 1, vec![]),
        (PART + 1, 0, vec![PART, 1]),
        (2 * PART, 0, vec![PART, PART]),
        (2 * PART + 7, 0, vec![PART, PART, 7]),
    ] {
        let watching = watching(ROOMY, |_| {}).await;
        let document = watching.document("PR_STREAM_ROADS").await;
        let file = pdf_of(size);
        let (status, body) = watching
            .post(document, multipart(&[Part::File("q.pdf", &file)]))
            .await;

        assert_eq!(status, StatusCode::OK, "{size}: {body}");

        let ledger = &watching.ledger;

        assert_eq!(ledger.puts.load(Ordering::SeqCst), puts, "{size}: puts");
        assert_eq!(ledger.parts(), parts, "{size}: parts");
        assert_eq!(
            ledger.completed.load(Ordering::SeqCst),
            usize::from(puts == 0),
            "{size}: completed"
        );

        // A guard that fired on a completed upload would land after the answer.
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(ledger.aborted.load(Ordering::SeqCst), 0, "{size}: aborted");

        let stored = watching.objects(document).await;

        assert_eq!(stored.len(), 1, "{size}: {stored:?}");

        let read_back = watching
            .store
            .get(&stored[0])
            .await
            .expect("the object")
            .bytes()
            .await
            .expect("its bytes");

        assert_eq!(read_back.as_ref(), file.as_slice(), "{size}: the bytes");
        assert_eq!(body["data"]["fileSize"], read_back.len() as i64, "{size}");
        assert_eq!(
            body["data"]["checksum"],
            format!("sha256:{:x}", sha2::Sha256::digest(&read_back)),
            "{size}"
        );
    }
}

/// The body arrives a few bytes at a time at the start: the type is read from
/// the buffer, not from the first chunk, which here holds two bytes of a header
/// `infer` needs five of.
#[tokio::test]
async fn a_first_chunk_of_a_few_bytes_is_not_what_is_sniffed() {
    let watching = watching(ROOMY, |_| {}).await;
    let document = watching.document("PR_STREAM_TINY").await;
    let file = pdf_of(1_000);
    let body = multipart(&[Part::File("q.pdf", &file)]);
    let two_bytes_in = body
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("a part head")
        + 4
        + 2;

    let response = watching
        .app
        .router()
        .oneshot(request(
            Method::POST,
            &format!("/api/v1/documents/{document}/attachments"),
            &watching.token,
            multipart_type(),
            framed(
                vec![
                    Bytes::copy_from_slice(&body[..two_bytes_in]),
                    Bytes::copy_from_slice(&body[two_bytes_in..]),
                ],
                Arc::clone(&watching.flow),
            ),
        ))
        .await
        .expect("the router answers");

    assert_eq!(response.status(), StatusCode::OK);
}

/// AC1: through the route, for a file the store holds as parts.
#[tokio::test]
async fn a_download_serves_the_stored_bytes_and_says_how_many() {
    let watching = watching(ROOMY, |_| {}).await;
    let document = watching.document("PR_STREAM_DOWN").await;
    let file = pdf_of(2 * PART + 3);
    let attachment = watching.upload(document, "q.pdf", &file).await;

    watching.clear(attachment).await;

    let served = watching
        .app
        .get_raw(
            &format!("/api/v1/documents/{document}/attachments/{attachment}"),
            Some(&watching.token),
        )
        .await;

    assert_eq!(served.status, StatusCode::OK);
    assert_eq!(
        served.header(header::CONTENT_LENGTH).as_deref(),
        Some((2 * PART + 3).to_string().as_str())
    );
    assert_eq!(
        served.header(header::CONTENT_DISPOSITION).as_deref(),
        Some("attachment; filename=\"q.pdf\"")
    );
    assert_eq!(served.bytes, file);
}

// ---------------------------------------------------------------------------
// AC4 — a refusal leaves nothing
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_refusal_of_a_file_that_ends_in_the_buffer_asks_nothing_of_the_store() {
    let watching = watching(ROOMY, |_| {}).await;
    let document = watching.document("PR_STREAM_REFUSE_SHORT").await;
    let long_name = format!("{}.pdf", "n".repeat(300));
    let pdf = pdf_of(100);

    for (body, code) in [
        (multipart(&[Part::File("empty.pdf", b"")]), "FILE_EMPTY"),
        (
            multipart(&[Part::File(
                "q.pdf",
                b"only words, which no magic number claims",
            )]),
            "FILE_TYPE_NOT_ALLOWED",
        ),
        (
            multipart(&[Part::File(&long_name, &pdf)]),
            "FILE_NAME_TOO_LONG",
        ),
        (
            multipart(&[
                Part::File("q.pdf", &pdf),
                Part::Text("categoryId", &Uuid::now_v7().to_string()),
            ]),
            "CATEGORY_NOT_FOUND",
        ),
    ] {
        let (status, refused) = watching.post(document, body).await;

        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{code}: {refused}"
        );
        assert_eq!(refused["error"]["details"][0]["code"], code, "{refused}");
        assert_eq!(watching.ledger.calls(), 0, "{code}");
    }

    assert_eq!(watching.objects(document).await, Vec::<ObjectPath>::new());
    assert_eq!(watching.rows(document).await, 0);
}

/// The head decides before an upload opens, so a file that outgrows the buffer
/// and is refused by it costs no request, and the rest of it is read and dropped.
#[tokio::test]
async fn a_refusal_read_from_the_head_of_a_long_file_opens_no_upload() {
    let watching = watching(ROOMY, |_| {}).await;
    let document = watching.document("PR_STREAM_REFUSE_HEAD").await;
    let words = vec![b'x'; PART + MIB];
    let long_name = format!("{}.pdf", "n".repeat(300));
    let pdf = pdf_of(PART + MIB);

    for (body, code) in [
        (
            multipart(&[Part::File("q.pdf", &words)]),
            "FILE_TYPE_NOT_ALLOWED",
        ),
        (
            multipart(&[Part::File(&long_name, &pdf)]),
            "FILE_NAME_TOO_LONG",
        ),
    ] {
        let (status, refused) = watching.post(document, body).await;

        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{code}: {refused}"
        );
        assert_eq!(refused["error"]["details"][0]["code"], code, "{refused}");
        assert_eq!(watching.ledger.calls(), 0, "{code}");
    }
}

/// **The category comes after the file, which is what this product's own client
/// sends**, so most of the file is in the store before it can be checked. That
/// is the case the shape exists for: parts, no object, and an abort.
#[tokio::test]
async fn a_category_refused_after_the_file_leaves_no_object_and_no_upload() {
    let watching = watching(ROOMY, |_| {}).await;
    let document = watching.document("PR_STREAM_REFUSE_CAT").await;
    let file = pdf_of(2 * PART + MIB);

    for (category, status, expected) in [
        (
            Uuid::now_v7().to_string(),
            StatusCode::UNPROCESSABLE_ENTITY,
            1,
        ),
        ("not a uuid".to_owned(), StatusCode::BAD_REQUEST, 2),
    ] {
        let (answered, body) = watching
            .post(
                document,
                multipart(&[
                    Part::File("q.pdf", &file),
                    Part::Text("categoryId", &category),
                ]),
            )
            .await;

        assert_eq!(answered, status, "{body}");
        watching.ledger.aborts_settled(expected).await;
    }

    assert_eq!(watching.ledger.opened.load(Ordering::SeqCst), 2);
    assert_eq!(watching.ledger.completed.load(Ordering::SeqCst), 0);
    assert_eq!(watching.ledger.parts(), vec![PART; 4]);
    assert_eq!(watching.objects(document).await, Vec::<ObjectPath>::new());
    assert_eq!(watching.rows(document).await, 0);
}

/// **Over the cap at the first part**: the read fails inside the buffer, so
/// there is nothing to abort. The name the limit goes by is the one
/// `multipart_failure` gives, whichever read it surfaced from.
#[tokio::test]
async fn a_file_over_the_cap_inside_the_first_part_reaches_no_store() {
    let watching = watching(2 * MIB, |_| {}).await;
    let document = watching.document("PR_STREAM_CAP_FIRST").await;
    let file = pdf_of(3 * MIB);

    let (status, refused) = watching
        .post(document, multipart(&[Part::File("q.pdf", &file)]))
        .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
    assert_eq!(refused["error"]["details"][0]["code"], "FILE_TOO_LARGE");
    assert_eq!(watching.ledger.calls(), 0);
}

#[tokio::test]
async fn a_file_over_the_cap_past_the_first_part_is_refused_and_aborted() {
    let watching = watching(7 * MIB, |_| {}).await;
    let document = watching.document("PR_STREAM_CAP_PAST").await;
    let file = pdf_of(8 * MIB);

    let (status, refused) = watching
        .post(document, multipart(&[Part::File("q.pdf", &file)]))
        .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
    assert_eq!(refused["error"]["details"][0]["code"], "FILE_TOO_LARGE");

    watching.ledger.aborts_settled(1).await;

    assert_eq!(watching.ledger.parts(), vec![PART]);
    assert_eq!(watching.ledger.completed.load(Ordering::SeqCst), 0);
    assert_eq!(watching.objects(document).await, Vec::<ObjectPath>::new());
}

/// A store that stops taking parts is the caller's 500 and the upload's abort.
#[tokio::test]
async fn a_part_the_store_refuses_aborts_the_upload_and_records_nothing() {
    let watching = watching(ROOMY, |store| store.refuse_part = Some(1)).await;
    let document = watching.document("PR_STREAM_PART_FAIL").await;
    let file = pdf_of(3 * PART);

    let (status, body) = watching
        .post(document, multipart(&[Part::File("q.pdf", &file)]))
        .await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");

    watching.ledger.aborts_settled(1).await;

    assert_eq!(watching.objects(document).await, Vec::<ObjectPath>::new());
    assert_eq!(watching.rows(document).await, 0);
}

// ---------------------------------------------------------------------------
// AC5 — a dropped request aborts too
// ---------------------------------------------------------------------------

/// **The request's future is dropped mid-upload**, with a part in the store and
/// the body still open, which is what a disconnect can do: no error path runs,
/// because nothing returns.
#[tokio::test]
async fn a_request_dropped_mid_upload_aborts_it() {
    let watching = watching(ROOMY, |_| {}).await;
    let document = watching.document("PR_STREAM_DROPPED").await;
    let file = pdf_of(3 * PART);
    let mut body = multipart(&[Part::File("q.pdf", &file)]);

    body.truncate(PART + 2 * MIB);

    let flow = Arc::clone(&watching.flow);
    let never_ends = stream_of(body, Arc::clone(&flow)).chain(stream::pending());

    let mut answering = Box::pin(watching.app.router().oneshot(request(
        Method::POST,
        &format!("/api/v1/documents/{document}/attachments"),
        &watching.token,
        multipart_type(),
        Body::from_stream(never_ends),
    )));

    let a_part_landed = async {
        while watching.flow.total_accepted() < PART as u64 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    };

    tokio::select! {
        answer = &mut answering => panic!("a body with no end was answered: {answer:?}"),
        () = a_part_landed => {}
        () = tokio::time::sleep(Duration::from_secs(20)) => panic!("no part reached the store"),
    }

    drop(answering);

    watching.ledger.aborts_settled(1).await;

    assert_eq!(watching.ledger.completed.load(Ordering::SeqCst), 0);
    assert_eq!(watching.objects(document).await, Vec::<ObjectPath>::new());
    assert_eq!(watching.rows(document).await, 0);
}

/// The other thing a disconnect can be: a read that fails. It answers as any
/// malformed body does, to nobody, and the upload goes the same way.
#[tokio::test]
async fn a_body_that_breaks_mid_upload_aborts_it() {
    let watching = watching(ROOMY, |_| {}).await;
    let document = watching.document("PR_STREAM_BROKEN").await;
    let file = pdf_of(3 * PART);
    let mut body = multipart(&[Part::File("q.pdf", &file)]);

    body.truncate(PART + 2 * MIB);

    let breaks = stream_of(body, Arc::clone(&watching.flow)).chain(stream::once(async {
        Err(std::io::Error::other("the connection was reset"))
    }));

    let response = watching
        .app
        .router()
        .oneshot(request(
            Method::POST,
            &format!("/api/v1/documents/{document}/attachments"),
            &watching.token,
            multipart_type(),
            Body::from_stream(breaks),
        ))
        .await
        .expect("the router answers");

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

    watching.ledger.aborts_settled(1).await;

    assert_eq!(watching.ledger.completed.load(Ordering::SeqCst), 0);
    assert_eq!(watching.objects(document).await, Vec::<ObjectPath>::new());
}

/// **A slow body is time for the document to go.** The check before the body is
/// as old as the body is slow, so the object is written on a second one, and the
/// answer is the one a deleted document gets. Both roads: one has parts to abort,
/// and the other has only not to write.
#[tokio::test]
async fn a_document_deleted_while_its_file_is_read_is_refused_and_leaves_nothing() {
    for (size, cut) in [(1_000, 500), (2 * PART + MIB, PART + 2 * MIB)] {
        let watching = watching(ROOMY, |_| {}).await;
        let document = watching.document("PR_STREAM_GONE").await;
        let file = pdf_of(size);
        let mut head = multipart(&[Part::File("q.pdf", &file)]);
        let tail = head.split_off(cut);
        let flow = Arc::clone(&watching.flow);
        let (release, released) = tokio::sync::oneshot::channel::<()>();

        let rest = {
            let flow = Arc::clone(&flow);

            stream::once(async move {
                released.await.ok();

                stream_of(tail, flow)
            })
            .flatten()
        };

        let answering = tokio::spawn(watching.app.router().oneshot(request(
            Method::POST,
            &format!("/api/v1/documents/{document}/attachments"),
            &watching.token,
            multipart_type(),
            Body::from_stream(stream_of(head, Arc::clone(&flow)).chain(rest)),
        )));

        // Mid-read: all of the first half taken, and on the long road a part in
        // the store.
        let accepted = if size > PART { PART as u64 } else { 0 };
        let deadline = tokio::time::Instant::now() + Duration::from_secs(20);

        while flow.total_read() < cut as u64 || flow.total_accepted() < accepted {
            assert!(
                tokio::time::Instant::now() < deadline,
                "the read did not get there"
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }

        let uri = format!("/api/v1/documents/{document}");
        let deleted = watching.app.delete(&uri, Some(&watching.token)).await;

        assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);

        release.send(()).expect("the body is waiting");

        let response = answering.await.expect("the task").expect("the router");
        let status = response.status();
        let refused: Value = serde_json::from_slice(
            &to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("the answer"),
        )
        .expect("an envelope");

        let gone = watching.app.get(&uri, Some(&watching.token)).await;

        assert_eq!(status, StatusCode::NOT_FOUND, "{size}: {refused}");
        assert_eq!(gone.status, StatusCode::NOT_FOUND, "{}", gone.body);
        assert_eq!(refused["error"], gone.body["error"], "{size}");

        if size > PART {
            watching.ledger.aborts_settled(1).await;
        }

        assert_eq!(watching.ledger.puts.load(Ordering::SeqCst), 0, "{size}");
        assert_eq!(
            watching.ledger.completed.load(Ordering::SeqCst),
            0,
            "{size}"
        );
        assert_eq!(watching.objects(document).await, Vec::<ObjectPath>::new());
        assert_eq!(watching.rows(document).await, 0, "{size}");
    }
}

// ---------------------------------------------------------------------------
// AC7 — what stays
// ---------------------------------------------------------------------------

/// **The last `file` part wins, as it did**, and an earlier one is not judged:
/// today a request that sends a bad file and then a good one is a good request,
/// which is why a refusal waits for the end of the body.
#[tokio::test]
async fn the_last_file_part_wins_and_an_earlier_upload_is_aborted() {
    let watching = watching(ROOMY, |_| {}).await;
    let document = watching.document("PR_STREAM_TWO_FILES").await;
    let long = pdf_of(PART + MIB);
    let last = pdf_of(1_000);

    let (status, body) = watching
        .post(
            document,
            multipart(&[
                Part::File("first.pdf", &long),
                Part::File("last.pdf", &last),
            ]),
        )
        .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["originalFileName"], "last.pdf");
    assert_eq!(body["data"]["fileSize"], 1_000);

    watching.ledger.aborts_settled(1).await;

    assert_eq!(watching.ledger.completed.load(Ordering::SeqCst), 0);
    assert_eq!(watching.objects(document).await.len(), 1);

    let words = vec![b'x'; PART + MIB];

    for earlier in [&b""[..], b"only words", &words] {
        let (status, body) = watching
            .post(
                document,
                multipart(&[
                    Part::File("first.pdf", earlier),
                    Part::File("last.pdf", &last),
                ]),
            )
            .await;

        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["data"]["originalFileName"], "last.pdf");
    }
}

/// **Parts in any order**, and with a stranger among them. `file` first is the
/// shape this product's client sends, on a file long enough to be in parts.
#[tokio::test]
async fn the_description_and_category_may_follow_the_file_and_strangers_are_ignored() {
    let watching = watching(ROOMY, |_| {}).await;
    let document = watching.document("PR_STREAM_ORDER").await;
    let categories = watching
        .app
        .get("/api/v1/attachment-categories", Some(&watching.token))
        .await;
    let category = categories.body["data"][0]["id"]
        .as_str()
        .expect("a seeded category")
        .to_owned();
    let file = pdf_of(PART + MIB);

    let (status, body) = watching
        .post(
            document,
            multipart(&[
                Part::Text("somethingABrowserAdded", "x"),
                Part::File("q.pdf", &file),
                Part::Text("description", "  the supplier's quotation  "),
                Part::Text("categoryId", &category),
                Part::Text("somethingElse", "y"),
            ]),
        )
        .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["description"], "the supplier's quotation");
    assert_eq!(body["data"]["category"]["id"], category.as_str());
    assert_eq!(watching.ledger.completed.load(Ordering::SeqCst), 1);
}

// ---------------------------------------------------------------------------
// AC8 — what moves, and what does not
// ---------------------------------------------------------------------------

/// **403 and 404 come before any of the body is read.** The body is over the cap
/// as well, which is the two-fault request whose answer moved: it was a 422.
#[tokio::test]
async fn a_caller_who_may_not_attach_is_refused_before_the_body_or_the_store() {
    let watching = watching(2 * MIB, |_| {}).await;
    let document = watching.document("PR_STREAM_FIRST").await;
    let file = pdf_of(3 * MIB);

    fixtures::create_user(
        &watching.app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "stream-outsider",
        "stream-outsider@example.test",
        common::ADMIN_PASSWORD,
        &[],
    )
    .await;

    let outsider = watching
        .app
        .sign_in("stream-outsider", common::ADMIN_PASSWORD)
        .await;

    let (status, refused) = watching
        .post_as(
            &outsider,
            document,
            multipart(&[Part::File("q.pdf", &file)]),
        )
        .await;

    assert_eq!(status, StatusCode::FORBIDDEN, "{refused}");

    let (status, refused) = watching
        .post(Uuid::now_v7(), multipart(&[Part::File("q.pdf", &file)]))
        .await;

    assert_eq!(status, StatusCode::NOT_FOUND, "{refused}");
    assert_eq!(watching.flow.total_read(), 0, "the body was read");
    assert_eq!(watching.ledger.calls(), 0, "the store was asked");
}

/// **What did not move**, because a refusal is decided as the file is read and
/// answered after the body is: what the body was going to answer still comes
/// first.
#[tokio::test]
async fn a_body_fault_still_answers_before_what_is_wrong_with_the_file() {
    let watching = watching(2 * MIB, |_| {}).await;
    let document = watching.document("PR_STREAM_ORDER_KEPT").await;
    let words = vec![b'x'; 100];

    let (status, body) = watching
        .post(
            document,
            multipart(&[
                Part::File("q.txt", &words),
                Part::Text("categoryId", "not a uuid"),
            ]),
        )
        .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    let words = vec![b'x'; 3 * MIB];

    let (status, body) = watching
        .post(document, multipart(&[Part::File("q.txt", &words)]))
        .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["error"]["details"][0]["code"], "FILE_TOO_LARGE");
}

// ---------------------------------------------------------------------------
// AC10 — a failure mid-stream
// ---------------------------------------------------------------------------

/// A body that fails, and a length it did not reach, is how a client is told.
/// The read is logged, and the event stands: it was written when the stream
/// started, so it names a transfer that failed partway.
#[tokio::test]
async fn a_download_that_fails_after_its_headers_is_truncated_and_logged() {
    let watching = watching(ROOMY, |store| {
        store.fail_read_after = Some(MIB as u64);
    })
    .await;
    let document = watching.document("PR_STREAM_TRUNCATE").await;
    let file = pdf_of(2 * PART);
    let attachment = watching.upload(document, "q.pdf", &file).await;

    watching.clear(attachment).await;

    let captured = Captured::default();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::ERROR)
        .with_ansi(false)
        .with_writer(captured.clone())
        .finish();

    let (headers, body) = {
        let _guard = tracing::subscriber::set_default(subscriber);
        let response = watching.download(document, attachment).await;

        assert_eq!(response.status(), StatusCode::OK);

        let headers = response.headers().clone();

        (headers, to_bytes(response.into_body(), usize::MAX).await)
    };

    assert_eq!(
        headers
            .get(header::CONTENT_LENGTH)
            .and_then(|length| length.to_str().ok()),
        Some((2 * PART).to_string().as_str()),
        "the length is what makes a short body a broken one"
    );
    assert!(body.is_err(), "the body ended as though it were whole");

    let log = String::from_utf8_lossy(&captured.0.lock().expect("the capture")).into_owned();

    assert!(
        log.contains("an attachment's bytes stopped mid-read"),
        "the failure was not logged: {log}"
    );

    let recorded: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM activity_events \
         WHERE event_type = 'Attachment.Downloaded' AND document_id = $1",
    )
    .bind(document)
    .fetch_one(&watching.app.pool)
    .await
    .expect("a count");

    assert_eq!(recorded, 1, "the event names the transfer that started");
}

/// AC2's fourth outcome, through the worker: a store that fails partway says
/// nothing about the file, so the row stays as it was and the next pass asks.
#[tokio::test]
async fn a_file_the_store_stops_serving_mid_scan_stays_pending() {
    let watching = watching(ROOMY, |store| store.fail_read_after = Some(1)).await;
    let document = watching.document("PR_STREAM_SCAN_BREAK").await;
    let file = pdf_of(2 * PART);
    let attachment = watching.upload(document, "q.pdf", &file).await;
    let port = slow_clamd(Arc::clone(&watching.flow)).await;

    worker::pass(&watching.app.state, "127.0.0.1", port)
        .await
        .expect("the pass");

    assert_eq!(watching.status_of(attachment).await, "PENDING");
}

/// What a test's own subscriber writes.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Captured {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("the capture").extend_from_slice(bytes);

        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Captured {
    type Writer = Captured;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}
