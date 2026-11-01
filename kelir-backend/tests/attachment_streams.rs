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
//! | `upload` does not ask for the document again | `a_document_deleted_while_its_file_is_read_…` |
//! | The abort not held to the bound | `storage::tests::a_store_that_hangs_…` |
//! | The bound is a minute | `storage::tests::a_store_that_hangs_…` |
//! | A body read held to the client's timeout | `storage::tests::a_body_that_stops_…` |
//! | The scan timed whole, as it was | `scanner::tests::a_slow_store_is_not_counted_…` |
//! | The store's read inside the scanner's limit | `scanner::tests::a_slow_store_is_not_counted_…` |
//! | A write with no limit | `scanner::tests::a_scanner_that_stops_reading_…` |
//! | The reply with no limit | `scanner::tests::a_scanner_that_takes_the_file_…`, which then never ends |
//!
//! The closing gate's own, made the same way and on the same day:
//!
//! | Mutation | Reddened at |
//! |---|---|
//! | `answered` off any one of `get`, a read, `put`, opening, a part, completing | `storage::tests`, by hanging: each is the only bound on its call |
//! | No abort after a part or a completion that failed | `a_part_the_store_refuses_…`, `storage::tests::a_store_that_hangs_…` |
//! | No abort before the first part landed | `a_request_dropped_at_each_await_…`, `storage::tests::a_store_that_hangs_…` |
//! | No abort on a refused category, the second check or a chunk that failed | The test named for each |
//! | No abort on a last part that failed | `a_store_that_refuses_any_call_…` |
//! | No abort on a limit met past the file | `a_body_over_the_cap_after_its_file_…` |
//! | The earlier `file` forgotten, or held to the end of the body | `the_last_file_part_wins_…`, `an_earlier_file_is_aborted_…` |
//! | A part sent at `PART_SIZE - 1` | `a_read_ending_a_byte_short_…` |
//! | A part's room one byte over | `each_size_takes_its_road_…` |
//! | A short last part dropped, or judged as a head | `each_size_takes_its_road_…` |
//! | One pass over a chunk | `each_size_takes_its_road_…` |
//! | The checksum or the count skipping parts | `each_size_takes_its_road_…` |
//! | The second check after the object | `a_document_deleted_while_its_file_is_read_…` |
//! | A refusal that does not stop later writes | `a_head_that_was_refused_…` |
//! | A refusal judged again at the end | `a_refusal_read_from_the_head_…` |
//! | A failed read ending the body cleanly | `a_download_that_fails_…` |
//! | The opening not spawned | `a_request_dropped_as_the_store_opens_…` |
//! | `connect` with no limit | `scanner::tests::a_scanner_that_never_takes_the_connection_…` |
//!
//! **Three of the scanner's timers survive**: the `zINSTREAM` write, a length
//! prefix and the final frame. Each is ten bytes or fewer beside a chunk write that
//! is timed, and blocks only with the socket's buffer full at that exact byte,
//! which no test here can arrange.
//!
//! [#834]: https://github.com/sujanto-gaws/kelir/issues/834

mod common;

use std::future::Future;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use axum::body::{to_bytes, Body, Bytes};
use axum::extract::ConnectInfo;
use axum::http::{header, Method, Request, Response, StatusCode};
use futures_util::stream::{self, BoxStream, StreamExt, TryStreamExt};
use object_store::aws::{AwsAuthorizer, AwsCredential};
use object_store::client::HttpRequestBody;
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
    /// Each upload's key, which is what MinIO's listing of them asks for.
    locations: Mutex<Vec<ObjectPath>>,
    /// The call a [`Stop`] named has been reached.
    stopped: AtomicBool,
    /// Lets that call go on, if anything is still waiting for it.
    released: tokio::sync::Notify,
}

impl Ledger {
    /// Stops here, so a test can drop the request at this `await`.
    async fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        self.released.notified().await;
    }

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

/// One `await` on the store, which a request can be dropped at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stop {
    /// The request that opens the upload has not left.
    Opening,
    /// The store has opened the upload, and the answer naming it never arrives.
    Opened,
    /// The part with this index has not left.
    Sending(usize),
    /// The store has this part, and its answer never arrives.
    Sent(usize),
    /// The request that completes the upload has not left.
    Completing,
    /// The store has made the object, and its answer never arrives.
    Completed,
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
    /// The read stream ends, with no error, once this many bytes have left it.
    end_read_after: Option<u64>,
    /// The store refuses the part with this index.
    refuse_part: Option<usize>,
    refuse_put: bool,
    refuse_open: bool,
    refuse_complete: bool,
    stop: Option<Stop>,
}

/// What the store says when a test has it refuse.
fn refused(what: &'static str) -> object_store::Error {
    object_store::Error::Generic {
        store: "Watched",
        source: format!("the store refused {what}").into(),
    }
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

        if self.refuse_put {
            return Err(refused("the write"));
        }

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
        self.ledger
            .locations
            .lock()
            .expect("the ledger")
            .push(location.clone());

        if self.refuse_open {
            return Err(refused("the upload"));
        }

        if self.stop == Some(Stop::Opening) {
            self.ledger.stop().await;
        }

        let inner = self.inner.put_multipart_opts(location, options).await?;

        if self.stop == Some(Stop::Opened) {
            self.ledger.stop().await;
        }

        Ok(Box::new(WatchedUpload {
            inner,
            ledger: Arc::clone(&self.ledger),
            flow: Arc::clone(&self.flow),
            part_takes: self.part_takes,
            refuse_part: self.refuse_part,
            refuse_complete: self.refuse_complete,
            stop: self.stop,
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
        let end_after = self.end_read_after;
        let mut served = 0_u64;
        let mut left = 0_u64;

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
            .take_while(move |chunk| {
                let ended = end_after.is_some_and(|limit| left >= limit);

                left += chunk.as_ref().map_or(0, |chunk| chunk.len() as u64);

                std::future::ready(!ended)
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
    refuse_complete: bool,
    stop: Option<Stop>,
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
            return Box::pin(async { Err(refused("the part")) });
        }

        let sent = self.inner.put_part(data);
        let flow = Arc::clone(&self.flow);
        let takes = self.part_takes;
        let ledger = Arc::clone(&self.ledger);
        let stop = self.stop;

        Box::pin(async move {
            if stop == Some(Stop::Sending(index)) {
                ledger.stop().await;
            }

            tokio::time::sleep(takes).await;
            sent.await?;
            flow.accepted(size);

            if stop == Some(Stop::Sent(index)) {
                ledger.stop().await;
            }

            Ok(())
        })
    }

    async fn complete(&mut self) -> object_store::Result<PutResult> {
        if self.refuse_complete {
            return Err(refused("the completion"));
        }

        if self.stop == Some(Stop::Completing) {
            self.ledger.stop().await;
        }

        let completed = self.inner.complete().await?;

        self.ledger.completed.fetch_add(1, Ordering::SeqCst);

        if self.stop == Some(Stop::Completed) {
            self.ledger.stop().await;
        }

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
        end_read_after: None,
        refuse_part: None,
        refuse_put: false,
        refuse_open: false,
        refuse_complete: false,
        stop: None,
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

    /// A request to the store itself, signed as `object_store` signs its own.
    async fn signed(&self, method: Method, rest: &str) -> reqwest::Response {
        let config = &self.app.state.config;
        let credential = AwsCredential {
            key_id: config.storage_access_key.clone(),
            secret_key: config.storage_secret_key.clone(),
            token: None,
        };
        let mut signed = Request::builder()
            .method(method)
            .uri(format!(
                "{}/{}{rest}",
                config.storage_endpoint, config.storage_bucket
            ))
            .body(HttpRequestBody::empty())
            .expect("a request");

        AwsAuthorizer::new(&credential, "s3", &config.storage_region).authorize(&mut signed, None);

        let mut sending =
            reqwest::Client::new().request(signed.method().clone(), signed.uri().to_string());

        for (name, value) in signed.headers() {
            sending = sending.header(name, value);
        }

        sending.send().await.expect("the store answers")
    }

    /// **Every upload the store still holds open for this request's files**, by
    /// asking the store and not the ledger: an abort that was called is not an
    /// upload that went. MinIO lists the uploads of one exact key, which is why
    /// the ledger keeps them.
    async fn incomplete(&self) -> usize {
        let locations = self.ledger.locations.lock().expect("the ledger").clone();
        let mut open = 0;

        for location in locations {
            let key = location.as_ref().replace('/', "%2F");
            let listing = self
                .signed(Method::GET, &format!("?prefix={key}&uploads="))
                .await;

            assert!(listing.status().is_success(), "{listing:?}");

            open += listing
                .text()
                .await
                .expect("a listing")
                .matches("<UploadId>")
                .count();
        }

        open
    }

    async fn added_events(&self, document: Uuid) -> i64 {
        sqlx::query_scalar(
            "SELECT count(*) FROM activity_events \
             WHERE event_type = 'Attachment.Added' AND document_id = $1",
        )
        .bind(document)
        .fetch_one(&self.app.pool)
        .await
        .expect("a count")
    }

    fn sending(
        &self,
        document: Uuid,
        body: Body,
    ) -> impl Future<Output = Result<Response<Body>, std::convert::Infallible>> + Send + 'static
    {
        self.app.router().oneshot(request(
            Method::POST,
            &format!("/api/v1/documents/{document}/attachments"),
            &self.token,
            multipart_type(),
            body,
        ))
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

/// **A read that ends one byte short of a part is not a part.** Every other test
/// here reads in frames that straddle the boundary, so the buffer is never seen at
/// exactly this length, and a store may refuse a short part that is not the last.
#[tokio::test]
async fn a_read_ending_a_byte_short_of_a_part_sends_no_part() {
    let watching = watching(ROOMY, |_| {}).await;
    let document = watching.document("PR_STREAM_SHORT_READ").await;
    let file = pdf_of(2 * PART + 7);
    let body = multipart(&[Part::File("q.pdf", &file)]);
    let file_starts = body
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("a part head")
        + 4;
    let cut = file_starts + PART - 1;

    let response = watching
        .sending(
            document,
            framed(
                vec![
                    Bytes::copy_from_slice(&body[..cut]),
                    Bytes::copy_from_slice(&body[cut..]),
                ],
                Arc::clone(&watching.flow),
            ),
        )
        .await
        .expect("the router answers");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(watching.ledger.parts(), vec![PART, PART, 7]);
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

/// **Over the cap past the end of the file**: the file's part has been read whole
/// and its upload is open, and the limit trips in the next field, which is the
/// other of the two reads `multipart_failure` is called from.
#[tokio::test]
async fn a_body_over_the_cap_after_its_file_is_refused_and_aborted() {
    let watching = watching(7 * MIB, |_| {}).await;
    let document = watching.document("PR_STREAM_CAP_AFTER").await;
    let file = pdf_of(6 * MIB);
    let filler = "x".repeat(2 * MIB);

    let (status, refused) = watching
        .post(
            document,
            multipart(&[
                Part::File("q.pdf", &file),
                Part::Text("somethingABrowserAdded", &filler),
            ]),
        )
        .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
    assert_eq!(refused["error"]["details"][0]["code"], "FILE_TOO_LARGE");

    watching.ledger.aborts_settled(1).await;

    assert_eq!(watching.incomplete().await, 0);
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

/// Polls the request until `reached`, and no further: what the caller does next
/// is what a disconnect does.
async fn until<T>(
    answering: &mut (impl Future<Output = T> + Unpin),
    reached: impl Future<Output = ()>,
) {
    tokio::select! {
        _ = answering => panic!("answered before it could be dropped"),
        () = reached => {}
        () = tokio::time::sleep(Duration::from_secs(20)) => panic!("it never got there"),
    }
}

/// One `await` on the upload's road.
struct Await {
    at: &'static str,
    /// The store call it is, or `None` for a read of the body that stopped.
    stop: Option<Stop>,
    /// How much of the body arrives, if not all of it.
    cut: Option<usize>,
    /// Parts in the store by then.
    landed: usize,
    /// Whether the store has an upload open by then.
    open: bool,
}

/// **Every `await` on the road, each dropped alone**, with the store asked what
/// it holds rather than the ledger what it was told. The listing is seen at one
/// first, wherever an upload is open, so its zero afterwards is not a listing
/// that sees nothing.
#[tokio::test]
async fn a_request_dropped_at_each_await_of_the_upload_leaves_nothing_in_the_store() {
    let file = pdf_of(2 * PART + MIB);
    let category = Uuid::now_v7().to_string();
    let whole = multipart(&[
        Part::File("q.pdf", &file),
        Part::Text("categoryId", &category),
    ]);
    let past_the_file = whole
        .windows(10)
        .rposition(|window| window == b"categoryId")
        .expect("the second part");

    for case in [
        Await {
            at: "the first read",
            stop: None,
            cut: Some(MIB),
            landed: 0,
            open: false,
        },
        Await {
            at: "opening, before the request left",
            stop: Some(Stop::Opening),
            cut: None,
            landed: 0,
            open: false,
        },
        Await {
            at: "the first part, unsent",
            stop: Some(Stop::Sending(0)),
            cut: None,
            landed: 0,
            open: true,
        },
        Await {
            at: "the first part, its answer lost",
            stop: Some(Stop::Sent(0)),
            cut: None,
            landed: 1,
            open: true,
        },
        Await {
            at: "a read between parts",
            stop: None,
            cut: Some(PART + 2 * MIB),
            landed: 1,
            open: true,
        },
        Await {
            at: "the second part, unsent",
            stop: Some(Stop::Sending(1)),
            cut: None,
            landed: 1,
            open: true,
        },
        Await {
            at: "the second part, its answer lost",
            stop: Some(Stop::Sent(1)),
            cut: None,
            landed: 2,
            open: true,
        },
        Await {
            at: "the next field, past the file",
            stop: None,
            cut: Some(past_the_file),
            landed: 2,
            open: true,
        },
        Await {
            at: "the last part, unsent",
            stop: Some(Stop::Sending(2)),
            cut: None,
            landed: 2,
            open: true,
        },
        Await {
            at: "the last part, its answer lost",
            stop: Some(Stop::Sent(2)),
            cut: None,
            landed: 3,
            open: true,
        },
        Await {
            at: "completing, before the request left",
            stop: Some(Stop::Completing),
            cut: None,
            landed: 3,
            open: true,
        },
    ] {
        let at = case.at;
        let stop = case.stop;
        let watching = watching(ROOMY, |store| store.stop = stop).await;
        let document = watching.document("PR_STREAM_AWAITS").await;

        // On this road the category has to be a real one, or the request is
        // refused before the last of the store calls is reached.
        let categories = watching
            .app
            .get("/api/v1/attachment-categories", Some(&watching.token))
            .await;
        let real = categories.body["data"][0]["id"]
            .as_str()
            .expect("a seeded category");
        let mut body = multipart(&[Part::File("q.pdf", &file), Part::Text("categoryId", real)]);
        let flow = Arc::clone(&watching.flow);

        let stream = match case.cut {
            Some(cut) => {
                body.truncate(cut);

                stream_of(body, Arc::clone(&flow))
                    .chain(stream::pending())
                    .boxed()
            }
            None => stream_of(body, Arc::clone(&flow)).boxed(),
        };

        let mut answering = Box::pin(watching.sending(document, Body::from_stream(stream)));

        until(&mut answering, async {
            loop {
                let there = match (case.stop, case.cut) {
                    (Some(_), _) => watching.ledger.stopped.load(Ordering::SeqCst),
                    (None, Some(cut)) => {
                        flow.total_read() >= cut as u64
                            && flow.total_accepted() >= (case.landed * PART) as u64
                    }
                    (None, None) => unreachable!(),
                };

                if there {
                    break;
                }

                tokio::time::sleep(Duration::from_millis(5)).await;
            }

            // Past the read that was counted, to the wait for the next one.
            tokio::time::sleep(Duration::from_millis(200)).await;
        })
        .await;

        assert_eq!(
            watching.incomplete().await,
            usize::from(case.open),
            "{at}: what the store holds before the drop"
        );

        drop(answering);

        if case.open {
            watching.ledger.aborts_settled(1).await;
        } else {
            tokio::time::sleep(Duration::from_millis(100)).await;
            assert_eq!(watching.ledger.aborted.load(Ordering::SeqCst), 0, "{at}");
        }

        assert_eq!(watching.incomplete().await, 0, "{at}: an upload was left");
        assert_eq!(
            watching.objects(document).await,
            Vec::<ObjectPath>::new(),
            "{at}"
        );
        assert_eq!(watching.rows(document).await, 0, "{at}");
        assert_eq!(watching.added_events(document).await, 0, "{at}");
    }
}

/// **The one `await` with no guard yet.** The store has opened the upload and
/// the answer that names it has not arrived, so the upload exists and nothing
/// here knows its id. Dropped there, only the store's expiry could ever clear it,
/// unless the call outlives the request.
#[tokio::test]
async fn a_request_dropped_as_the_store_opens_the_upload_leaves_nothing_in_the_store() {
    let watching = watching(ROOMY, |store| store.stop = Some(Stop::Opened)).await;
    let document = watching.document("PR_STREAM_OPENED").await;
    let file = pdf_of(2 * PART);
    let body = multipart(&[Part::File("q.pdf", &file)]);
    let stream = stream_of(body, Arc::clone(&watching.flow));

    let mut answering = Box::pin(watching.sending(document, Body::from_stream(stream)));

    until(&mut answering, async {
        while !watching.ledger.stopped.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await;

    assert_eq!(watching.incomplete().await, 1, "the store opened nothing");

    drop(answering);

    // The answer arrives after the request has gone, to whatever is left to hear.
    watching.ledger.released.notify_one();

    tokio::time::sleep(Duration::from_secs(1)).await;

    assert_eq!(watching.incomplete().await, 0, "the upload was left open");
}

/// **The last `await` an upload has, and the object wins it.** The store made the
/// object and the answer did not arrive, which is `put`'s old road by another
/// name: no upload is left, the object has no row, and that is the state
/// `service::upload`'s documentation accepts. This pins that it is that state
/// and no worse.
#[tokio::test]
async fn a_request_dropped_as_the_store_completes_leaves_the_object_and_no_upload() {
    let watching = watching(ROOMY, |store| store.stop = Some(Stop::Completed)).await;
    let document = watching.document("PR_STREAM_COMPLETED").await;
    let file = pdf_of(2 * PART);
    let body = multipart(&[Part::File("q.pdf", &file)]);
    let stream = stream_of(body, Arc::clone(&watching.flow));

    let mut answering = Box::pin(watching.sending(document, Body::from_stream(stream)));

    until(&mut answering, async {
        while !watching.ledger.stopped.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await;

    drop(answering);

    tokio::time::sleep(Duration::from_millis(500)).await;

    assert_eq!(watching.incomplete().await, 0);
    assert_eq!(watching.rows(document).await, 0);
    assert_eq!(watching.added_events(document).await, 0);

    let stored = watching.objects(document).await;

    assert_eq!(stored.len(), 1, "{stored:?}");

    watching.store.delete(&stored[0]).await.expect("cleaned up");
}

/// **The last read is over and the database is what is being waited for**, at
/// the second `may_upload`: every connection is held, so the request stops there.
#[tokio::test]
async fn a_request_dropped_waiting_for_the_database_leaves_nothing_in_the_store() {
    let watching = watching(ROOMY, |_| {}).await;
    let document = watching.document("PR_STREAM_DB_WAIT").await;
    let file = pdf_of(2 * PART);
    let mut head = multipart(&[Part::File("q.pdf", &file)]);
    let tail = head.split_off(PART + 2 * MIB);
    let flow = Arc::clone(&watching.flow);
    let (release, released) = tokio::sync::oneshot::channel::<()>();
    let total = (head.len() + tail.len()) as u64;

    let rest = {
        let flow = Arc::clone(&flow);

        stream::once(async move {
            released.await.ok();

            stream_of(tail, flow)
        })
        .flatten()
    };
    let stream = stream_of(head, Arc::clone(&flow)).chain(rest);

    let mut answering = Box::pin(watching.sending(document, Body::from_stream(stream)));

    until(&mut answering, async {
        while flow.total_accepted() < PART as u64 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await;

    let mut held = Vec::new();

    for _ in 0..common::TEST_POOL_MAX_CONNECTIONS {
        held.push(watching.app.pool.acquire().await.expect("a connection"));
    }

    release.send(()).expect("the body is waiting");

    until(&mut answering, async {
        while flow.total_read() < total {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }

        tokio::time::sleep(Duration::from_millis(300)).await;
    })
    .await;

    assert_eq!(watching.incomplete().await, 1);
    assert_eq!(watching.ledger.parts(), vec![PART], "the last part stays");

    drop(answering);
    drop(held);

    watching.ledger.aborts_settled(1).await;

    assert_eq!(watching.incomplete().await, 0);
    assert_eq!(watching.objects(document).await, Vec::<ObjectPath>::new());
    assert_eq!(watching.rows(document).await, 0);
}

/// **On a real socket, with no timer anywhere above it.** A client that stops
/// sending costs an open upload for as long as it stays: nothing in this process
/// ends the request. It costs no store call in flight, since a part goes only
/// when one is full, and what ends it is the client leaving, which is seen.
#[tokio::test]
async fn a_client_that_stalls_keeps_its_upload_open_until_it_leaves() {
    let watching = watching(ROOMY, |_| {}).await;
    let document = watching.document("PR_STREAM_STALL").await;
    let file = pdf_of(3 * PART);
    let mut body = multipart(&[Part::File("q.pdf", &file)]);
    let length = body.len();
    let flow = Arc::clone(&watching.flow);

    body.truncate(PART + 2 * MIB);

    let address = served(&watching.app, Arc::clone(&flow)).await;
    let mut socket = TcpStream::connect(address).await.expect("connect");

    socket
        .write_all(
            format!(
                "POST /api/v1/documents/{document}/attachments HTTP/1.1\r\nHost: kelir.test\r\n\
                 Authorization: Bearer {}\r\n\
                 Content-Type: multipart/form-data; boundary={BOUNDARY}\r\n\
                 Content-Length: {length}\r\n\r\n",
                watching.token
            )
            .as_bytes(),
        )
        .await
        .expect("the head");
    socket.write_all(&body).await.expect("the body");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);

    while flow.total_accepted() < PART as u64 {
        assert!(tokio::time::Instant::now() < deadline, "no part landed");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    tokio::time::sleep(Duration::from_secs(3)).await;

    assert_eq!(watching.ledger.parts(), vec![PART], "a part is still going");
    assert_eq!(watching.ledger.aborted.load(Ordering::SeqCst), 0);
    assert_eq!(watching.incomplete().await, 1, "the stall ended the upload");

    drop(socket);

    watching.ledger.aborts_settled(1).await;

    assert_eq!(watching.incomplete().await, 0);
    assert_eq!(watching.objects(document).await, Vec::<ObjectPath>::new());
    assert_eq!(watching.rows(document).await, 0);
}

/// **Each call the store can refuse**, and what each leaves: a 500, no object, no
/// upload, no row and no event. An upload that was opened is aborted; the two
/// roads that never opened one have nothing to abort.
#[tokio::test]
async fn a_store_that_refuses_any_call_leaves_nothing_and_records_nothing() {
    let refuse_open: fn(&mut Watched) = |store| store.refuse_open = true;
    let refuse_first: fn(&mut Watched) = |store| store.refuse_part = Some(0);
    let refuse_last: fn(&mut Watched) = |store| store.refuse_part = Some(2);
    let refuse_complete: fn(&mut Watched) = |store| store.refuse_complete = true;
    let refuse_put: fn(&mut Watched) = |store| store.refuse_put = true;

    for (call, tune, size, aborts) in [
        ("opening", refuse_open, 2 * PART + MIB, 0),
        ("the first part", refuse_first, 2 * PART + MIB, 1),
        ("the last part", refuse_last, 2 * PART + MIB, 1),
        ("completing", refuse_complete, 2 * PART + MIB, 1),
        ("the put", refuse_put, 1_000, 0),
    ] {
        let watching = watching(ROOMY, tune).await;
        let document = watching.document("PR_STREAM_REFUSALS").await;
        let file = pdf_of(size);

        let (status, body) = watching
            .post(document, multipart(&[Part::File("q.pdf", &file)]))
            .await;

        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{call}: {body}");

        watching.ledger.aborts_settled(aborts).await;

        assert_eq!(
            watching.ledger.aborted.load(Ordering::SeqCst),
            aborts,
            "{call}"
        );
        assert_eq!(watching.incomplete().await, 0, "{call}: an upload was left");
        assert_eq!(
            watching.objects(document).await,
            Vec::<ObjectPath>::new(),
            "{call}"
        );
        assert_eq!(watching.rows(document).await, 0, "{call}");
        assert_eq!(watching.added_events(document).await, 0, "{call}");
    }
}

/// **A refused head stays refused.** The read after the one that refused this
/// file begins like a PDF, which is what a file judged once a buffer would pass
/// on: the allow-list reads the first bytes of the file, and no later ones get a
/// turn. The frames are cut there because a refusal drops the rest of its read.
#[tokio::test]
async fn a_head_that_was_refused_is_not_judged_again_at_a_later_read() {
    let watching = watching(ROOMY, |_| {}).await;
    let document = watching.document("PR_STREAM_HEAD_ONCE").await;
    let mut file = vec![b'x'; PART + 1];

    file.extend_from_slice(&pdf_of(2 * PART));

    let body = multipart(&[Part::File("q.pdf", &file)]);
    let file_starts = body
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("a part head")
        + 4;
    let cut = file_starts + PART + 1;

    let response = watching
        .sending(
            document,
            framed(
                vec![
                    Bytes::copy_from_slice(&body[..cut]),
                    Bytes::copy_from_slice(&body[cut..]),
                ],
                Arc::clone(&watching.flow),
            ),
        )
        .await
        .expect("the router answers");

    let status = response.status();
    let refused: Value = serde_json::from_slice(
        &to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("the answer"),
    )
    .expect("an envelope");

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
    assert_eq!(
        refused["error"]["details"][0]["code"],
        "FILE_TYPE_NOT_ALLOWED"
    );
    assert_eq!(watching.ledger.calls(), 0);
}

/// **The earlier `file` is let go when the later one starts, and not when the body
/// ends**: its upload is aborted with the later file still arriving, so a request
/// does not hold two parts' worth of buffers and two uploads to read one file.
#[tokio::test]
async fn an_earlier_file_is_aborted_as_the_later_one_begins() {
    let watching = watching(ROOMY, |_| {}).await;
    let document = watching.document("PR_STREAM_TWO_LONG").await;
    let long = pdf_of(PART + MIB);
    let second = pdf_of(2 * MIB);
    let mut body = multipart(&[
        Part::File("first.pdf", &long),
        Part::File("second.pdf", &second),
    ]);

    body.truncate(PART + MIB + MIB);

    let stream = stream_of(body, Arc::clone(&watching.flow)).chain(stream::pending());
    let mut answering = Box::pin(watching.sending(document, Body::from_stream(stream)));

    until(&mut answering, async {
        while watching.ledger.aborted.load(Ordering::SeqCst) < 1 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await;

    assert_eq!(watching.incomplete().await, 0);
}

/// The multipart road's object is as encrypted as the `put` road's, by the store's
/// own report of what it holds (NFR-SEC-009, AC3).
#[tokio::test]
async fn an_object_from_either_road_is_encrypted_at_rest() {
    let watching = watching(ROOMY, |_| {}).await;
    let document = watching.document("PR_STREAM_SSE").await;

    for (size, puts, opened) in [(1_000, 1, 0), (PART + 1, 1, 1)] {
        let file = pdf_of(size);
        let attachment = watching.upload(document, "q.pdf", &file).await;

        assert_eq!(watching.ledger.puts.load(Ordering::SeqCst), puts, "{size}");
        assert_eq!(
            watching.ledger.opened.load(Ordering::SeqCst),
            opened,
            "{size}"
        );

        let stored = watching.objects(document).await;
        let object = stored
            .iter()
            .find(|path| path.as_ref().contains(&attachment.to_string()))
            .expect("the object");
        let head = watching.signed(Method::HEAD, &format!("/{object}")).await;

        assert_eq!(
            head.headers()
                .get("x-amz-server-side-encryption")
                .map(|value| value.to_str().expect("a header")),
            Some("AES256"),
            "{size}: {:?}",
            head.headers()
        );
    }
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

/// What can be wrong with a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fault {
    OverCap,
    MalformedCategory,
    NoPermission,
    DeletedDocument,
    Empty,
    LongName,
    Disallowed,
    UnknownCategory,
    StoreDown,
}

/// **The order the faults answered in at `4683c3d`**, read off that commit: what
/// the body's read fails on, then `service::upload` from its top.
const ORDER_BEFORE_STREAMING: [Fault; 9] = [
    Fault::OverCap,
    Fault::MalformedCategory,
    Fault::NoPermission,
    Fault::DeletedDocument,
    Fault::Empty,
    Fault::LongName,
    Fault::Disallowed,
    Fault::UnknownCategory,
    Fault::StoreDown,
];

impl Fault {
    fn answer(self) -> (StatusCode, Option<&'static str>) {
        match self {
            Self::OverCap => (StatusCode::UNPROCESSABLE_ENTITY, Some("FILE_TOO_LARGE")),
            Self::MalformedCategory => (StatusCode::BAD_REQUEST, None),
            Self::NoPermission => (StatusCode::FORBIDDEN, None),
            Self::DeletedDocument => (StatusCode::NOT_FOUND, None),
            Self::Empty => (StatusCode::UNPROCESSABLE_ENTITY, Some("FILE_EMPTY")),
            Self::LongName => (StatusCode::UNPROCESSABLE_ENTITY, Some("FILE_NAME_TOO_LONG")),
            Self::Disallowed => (
                StatusCode::UNPROCESSABLE_ENTITY,
                Some("FILE_TYPE_NOT_ALLOWED"),
            ),
            Self::UnknownCategory => (StatusCode::UNPROCESSABLE_ENTITY, Some("CATEGORY_NOT_FOUND")),
            Self::StoreDown => (StatusCode::INTERNAL_SERVER_ERROR, None),
        }
    }
}

/// **Every pair whose answer moved, and which fault now answers** (AC8). The first
/// two rows are the caller being judged before the body. The third is new with the
/// stream: a file past a part meets the store while the body is still arriving, so
/// a store that is down answers before anything that comes later in the body.
///
/// A disallowed type before a malformed `categoryId`, which AC8 expected to move,
/// did not: a refusal of the file is answered after the body.
fn moved(pair: &[Fault], past_a_part: bool) -> Option<Fault> {
    use Fault::{DeletedDocument, MalformedCategory, NoPermission, OverCap, StoreDown};

    let has = |fault| pair.contains(&fault);
    let body_fault = has(OverCap) || has(MalformedCategory);

    if body_fault && has(NoPermission) {
        return Some(NoPermission);
    }

    if body_fault && has(DeletedDocument) {
        return Some(DeletedDocument);
    }

    let met_later = body_fault || has(Fault::UnknownCategory);

    (has(StoreDown) && met_later && (past_a_part || has(OverCap))).then_some(StoreDown)
}

/// The body that has these faults, the shipped client's way: `file` first.
fn body_with(faults: &[Fault], past_a_part: bool) -> Vec<u8> {
    let has = |fault| faults.contains(&fault);
    let size = match (has(Fault::OverCap), past_a_part) {
        (true, _) => 8 * MIB,
        (false, true) => 6 * MIB,
        (false, false) => 1_000,
    };
    let file = if has(Fault::Empty) {
        Vec::new()
    } else if has(Fault::Disallowed) {
        vec![b'x'; size]
    } else {
        pdf_of(size)
    };
    let long_name = format!("{}.pdf", "n".repeat(300));
    let name = if has(Fault::LongName) {
        long_name.as_str()
    } else {
        "q.pdf"
    };
    let unknown = Uuid::now_v7().to_string();
    let filler = "x".repeat(8 * MIB);
    let mut parts = vec![Part::File(name, &file)];

    if has(Fault::UnknownCategory) {
        parts.push(Part::Text("categoryId", &unknown));
    }

    if has(Fault::MalformedCategory) {
        parts.push(Part::Text("categoryId", "not a uuid"));
    }

    // An empty file cannot be over the cap, so a part beside it is.
    if has(Fault::Empty) && has(Fault::OverCap) {
        parts.push(Part::Text("somethingABrowserAdded", &filler));
    }

    multipart(&parts)
}

/// **Every single fault and every pair of them**, on both roads where the road can
/// matter, against the order `4683c3d` answered in and [`moved`]. A single fault
/// answers as it did, which is AC8's other half. All of them are collected, so a
/// failure is the whole list and not its first line.
#[tokio::test]
async fn every_pair_of_faults_answers_as_it_did_or_as_recorded() {
    use Fault::{DeletedDocument, Empty, NoPermission, OverCap, StoreDown};

    let healthy = watching(7 * MIB, |_| {}).await;
    let down = watching(7 * MIB, |store| {
        store.refuse_open = true;
        store.refuse_put = true;
    })
    .await;

    let mut instances = Vec::new();

    for instance in [&healthy, &down] {
        let document = instance.document("PR_STREAM_FAULTS").await;
        let deleted = instance.document("PR_STREAM_FAULTS_GONE").await;
        let gone = instance
            .app
            .delete(
                &format!("/api/v1/documents/{deleted}"),
                Some(&instance.token),
            )
            .await;

        assert_eq!(gone.status, StatusCode::NO_CONTENT, "{}", gone.body);

        fixtures::create_user(
            &instance.app.pool,
            fixtures::SYSTEM_TENANT_ID,
            "stream-faults",
            "stream-faults@example.test",
            common::ADMIN_PASSWORD,
            &[],
        )
        .await;

        let outsider = instance
            .app
            .sign_in("stream-faults", common::ADMIN_PASSWORD)
            .await;

        instances.push((document, deleted, outsider));
    }

    let mut requests: Vec<Vec<Fault>> = ORDER_BEFORE_STREAMING.iter().map(|f| vec![*f]).collect();

    for (at, first) in ORDER_BEFORE_STREAMING.iter().enumerate() {
        for second in &ORDER_BEFORE_STREAMING[at + 1..] {
            requests.push(vec![*first, *second]);
        }
    }

    let mut wrong = Vec::new();

    for faults in &requests {
        let has = |fault| faults.contains(&fault);

        for past_a_part in [false, true] {
            if past_a_part && (has(Empty) || has(OverCap)) {
                continue;
            }

            let (instance, (document, deleted, outsider)) = if has(StoreDown) {
                (&down, &instances[1])
            } else {
                (&healthy, &instances[0])
            };
            let target = if has(DeletedDocument) {
                *deleted
            } else {
                *document
            };
            let token = if has(NoPermission) {
                outsider.as_str()
            } else {
                instance.token.as_str()
            };

            let (status, body) = instance
                .post_as(token, target, body_with(faults, past_a_part))
                .await;
            let code = body["error"]["details"][0]["code"].as_str();

            let before = ORDER_BEFORE_STREAMING
                .iter()
                .find(|fault| faults.contains(fault))
                .expect("a fault");
            let expected = match faults.len() {
                1 => *before,
                _ => moved(faults, past_a_part).unwrap_or(*before),
            };
            let (expected_status, expected_code) = expected.answer();

            if status != expected_status || code != expected_code {
                wrong.push(format!(
                    "{faults:?} past_a_part={past_a_part}: {status} {code:?}, \
                     expected {expected:?} ({expected_status} {expected_code:?})"
                ));
            }
        }
    }

    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// **The other fault that stops a body, beside the faults above**: no `file` part
/// at all, and a body that is not multipart underneath. Both are read after the
/// caller is judged now, so a caller who may not attach hears that instead.
#[tokio::test]
async fn a_body_with_no_file_answers_after_the_caller_is_judged() {
    let watching = watching(ROOMY, |_| {}).await;
    let document = watching.document("PR_STREAM_NO_FILE").await;

    fixtures::create_user(
        &watching.app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "stream-nofile",
        "stream-nofile@example.test",
        common::ADMIN_PASSWORD,
        &[],
    )
    .await;

    let outsider = watching
        .app
        .sign_in("stream-nofile", common::ADMIN_PASSWORD)
        .await;

    for body in [
        multipart(&[Part::Text("description", "no file here")]),
        b"--kelirstreamsboundary\r\nnot a part at all".to_vec(),
    ] {
        let (status, refused) = watching.post(document, body.clone()).await;

        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
        assert_eq!(refused["error"]["details"][0]["code"], "FILE_REQUIRED");

        let (status, refused) = watching.post_as(&outsider, document, body.clone()).await;

        assert_eq!(status, StatusCode::FORBIDDEN, "{refused}");

        let (status, refused) = watching.post(Uuid::now_v7(), body).await;

        assert_eq!(status, StatusCode::NOT_FOUND, "{refused}");
    }
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

/// **Through hyper and a real client**, which is the only place the length is
/// enforced: `oneshot` reads whatever the body yields. A store's read that fails
/// and one that simply ends early must both reach the client as a transfer that
/// broke, never as a shorter file that finished.
#[tokio::test]
async fn a_body_that_stops_short_of_its_length_is_a_broken_transfer_to_a_real_client() {
    let fail: fn(&mut Watched) = |store| store.fail_read_after = Some(MIB as u64);
    let end: fn(&mut Watched) = |store| store.end_read_after = Some(MIB as u64);

    for (how, tune) in [("failed", fail), ("ended", end)] {
        let watching = watching(ROOMY, tune).await;
        let document = watching.document("PR_STREAM_SHORT").await;
        let file = pdf_of(2 * PART);
        let attachment = watching.upload(document, "q.pdf", &file).await;

        watching.clear(attachment).await;

        let address = served(&watching.app, Arc::clone(&watching.flow)).await;
        let mut response = reqwest::Client::new()
            .get(format!(
                "http://{address}/api/v1/documents/{document}/attachments/{attachment}"
            ))
            .bearer_auth(&watching.token)
            .send()
            .await
            .expect("the headers");

        assert_eq!(response.status(), StatusCode::OK, "{how}");
        assert_eq!(response.content_length(), Some(2 * PART as u64), "{how}");

        let mut taken = 0;

        let ending = loop {
            match response.chunk().await {
                Ok(Some(chunk)) => taken += chunk.len(),
                Ok(None) => break Ok(()),
                Err(error) => break Err(error),
            }
        };

        assert!(taken < 2 * PART, "{how}: {taken} bytes arrived");
        assert!(
            ending.is_err(),
            "{how}: {taken} of {} bytes ended as a complete body",
            2 * PART
        );
    }
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
