//! Speaking to ClamAV (FR-ATT-001, FR-ATT-002; [#246]).
//!
//! # INSTREAM, and why not `clamdscan`
//!
//! The protocol is four lines: send `zINSTREAM\0`, then each chunk as a
//! four-byte big-endian length followed by its bytes, then a zero length, then
//! read one reply. Speaking it directly is what lets the scanner live somewhere
//! this process cannot see the filesystem of, which is every deployment where
//! ClamAV is its own container.
//!
//! **`clamdscan` was measured and rejected as an implementation, and the reason
//! is a defect it hides.** Against a scanner that shares a filesystem it falls
//! back to scanning the path rather than streaming, and a 101 MiB file — over
//! `clamd`'s stream limit — came back `OK, exit 0` where the same payload over
//! the wire returned `INSTREAM size limit exceeded. ERROR`. A worker built on
//! `clamdscan` would record *clean* exactly where a stream is refused. Measured
//! 2026-08-31; the numbers are on [#246].
//!
//! # The four answers, and the two that are not the same kind of failure
//!
//! | Reply | Meaning |
//! |---|---|
//! | `stream: OK` | [`ScanOutcome::Clean`] |
//! | `stream: <signature> FOUND` | [`ScanOutcome::Infected`] |
//! | anything ending `ERROR` | [`ScanOutcome::Failed`] — the scanner ran and refused |
//! | no reply at all | [`ScanError`] — the scanner did not run |
//!
//! **The last row is not a scan result and must never be stored as one.** A
//! connection refused, a timeout, a half-written stream: none of them say
//! anything about the file, so the row stays `PENDING` and the attachment stays
//! undownloadable until a scanner exists to answer. That is #246 AC7's *the
//! failure mode is unavailability, never a silent pass*, and it is why this
//! function returns `Result<ScanOutcome, ScanError>` rather than folding the two
//! into one enum: the type makes *unknown* impossible to confuse with *bad*.
//!
//! [#246]: https://github.com/sujanto-gaws/kelir/issues/246

use std::future::Future;
use std::time::Duration;

use futures_util::StreamExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use super::storage::ObjectStream;

/// What the scanner said about a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanOutcome {
    Clean,
    /// The signature it matched, kept for the log rather than for a caller: a
    /// person holding an infected file needs to replace it, not to look it up.
    Infected(String),
    /// The scanner ran and refused to clear the file — a stream over its limit,
    /// an archive it would not open. **A refusal, not a pass.**
    Failed(String),
}

/// The scanner did not answer, so nothing is known about the file.
#[derive(Debug)]
pub struct ScanError(pub String);

impl std::fmt::Display for ScanError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

/// How large a chunk the stream is written in.
///
/// 64 KiB, which is what the measurement used. Larger buys nothing — the scan is
/// throughput-bound at about 156 MiB/s and the framing is not the cost.
const CHUNK: usize = 64 * 1024;

/// Long enough for a 25 MiB scan by two orders of magnitude.
///
/// The measured cost is ~170 ms; this is 30 seconds. It is not a performance
/// bound but a liveness one: a scanner that has stopped answering must not hold
/// a worker for ever, because the worker is what would otherwise pick the file
/// up again.
///
/// **It holds each wait on `clamd` and not the conversation** ([#834]): the
/// connect, each write and the reply. The file is read from the store as it is
/// written to the scanner, and over the whole conversation this would count the
/// store's time as the scanner's. A file that takes the store longer than this
/// to send would then time out on every pass, from its first byte each time, and
/// sit `PENDING` for ever. The store's reads have a bound of their own, in
/// [`ObjectStorage::get`](super::storage::ObjectStorage::get).
///
/// [#834]: https://github.com/sujanto-gaws/kelir/issues/834
const TIMEOUT: Duration = Duration::from_secs(30);

/// Streams one file to `clamd` and reads its answer.
pub async fn scan(host: &str, port: u16, file: ObjectStream) -> Result<ScanOutcome, ScanError> {
    Ok(interpret(&converse(host, port, file, TIMEOUT).await?))
}

/// One wait on `clamd`, held to `limit`.
async fn within<T>(limit: Duration, wait: impl Future<Output = T>) -> Result<T, ScanError> {
    tokio::time::timeout(limit, wait)
        .await
        .map_err(|_| ScanError(format!("clamd did not answer within {limit:?}")))
}

/// The protocol itself.
///
/// **A file that stops being readable ends the conversation with no final
/// frame.** That frame says *this was the whole file*, and `clamd` would answer
/// `OK` for the part it had, so sending it after a failed read is the one way
/// this function could turn a truncated read into a false `CLEAN`.
async fn converse(
    host: &str,
    port: u16,
    mut file: ObjectStream,
    limit: Duration,
) -> Result<String, ScanError> {
    let mut stream = within(limit, TcpStream::connect((host, port)))
        .await?
        .map_err(|error| ScanError(format!("cannot reach clamd at {host}:{port}: {error}")))?;

    within(limit, stream.write_all(b"zINSTREAM\0"))
        .await?
        .map_err(|error| ScanError(format!("clamd closed before the stream began: {error}")))?;

    while let Some(bytes) = file.next().await {
        let bytes =
            bytes.map_err(|error| ScanError(format!("the file stopped mid-read: {error}")))?;

        // What the store sends is any size, and a zero-length frame ends the
        // stream, so `chunks` matters twice: an empty read sends nothing.
        for chunk in bytes.chunks(CHUNK) {
            // The length prefix is big-endian and the protocol has no other
            // framing, so a short write here would desynchronise the stream
            // rather than fail it — which is why `write_all` is used throughout.
            let length = u32::try_from(chunk.len()).unwrap_or(u32::MAX);

            within(limit, stream.write_all(&length.to_be_bytes()))
                .await?
                .map_err(|error| ScanError(format!("clamd stopped mid-stream: {error}")))?;
            within(limit, stream.write_all(chunk))
                .await?
                .map_err(|error| ScanError(format!("clamd stopped mid-stream: {error}")))?;
        }
    }

    // A zero-length chunk ends the stream. **Its failure is not an error**: a
    // clamd that has already decided — a stream over its limit is the usual
    // case — closes the write side before this arrives, and the answer it wants
    // to give is still waiting to be read.
    let _ = within(limit, stream.write_all(&0_u32.to_be_bytes())).await;

    let mut reply = Vec::new();

    within(limit, stream.read_to_end(&mut reply))
        .await?
        .map_err(|error| ScanError(format!("clamd gave no answer: {error}")))?;

    if reply.is_empty() {
        return Err(ScanError("clamd closed without answering".to_owned()));
    }

    Ok(String::from_utf8_lossy(&reply)
        .trim_end_matches('\0')
        .trim()
        .to_owned())
}

/// Reads one reply line.
///
/// **`OK` is matched exactly and everything else is a refusal**, which is the
/// direction an allow-list has to fail in: a reply this function does not
/// recognise is a reply it must not read as clean.
fn interpret(reply: &str) -> ScanOutcome {
    if reply.ends_with("OK") {
        return ScanOutcome::Clean;
    }

    if let Some(rest) = reply.strip_suffix("FOUND") {
        let signature = rest
            .rsplit_once(':')
            .map(|(_, signature)| signature)
            .unwrap_or(rest)
            .trim()
            .to_owned();

        return ScanOutcome::Infected(signature);
    }

    ScanOutcome::Failed(reply.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Bytes;
    use futures_util::stream;
    use object_store::Error as StoreError;
    use tokio::net::TcpListener;

    /// Everything `clamd` is told, read until the far side closes.
    async fn listener() -> (u16, tokio::task::JoinHandle<Vec<u8>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("a port");
        let port = listener.local_addr().expect("an address").port();

        let told = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("the worker");
            let mut told = Vec::new();

            socket.read_to_end(&mut told).await.expect("the stream");

            told
        });

        (port, told)
    }

    #[tokio::test]
    async fn a_file_that_stops_mid_read_is_not_scanned_as_the_part_that_arrived() {
        let (port, told) = listener().await;
        let file = stream::iter([
            Ok(Bytes::from_static(b"the first half")),
            Err(StoreError::Generic {
                store: "test",
                source: "the store went away".into(),
            }),
        ])
        .boxed();

        let answer = scan("127.0.0.1", port, file).await;

        assert!(
            answer
                .as_ref()
                .is_err_and(|error| error.0.contains("mid-read")),
            "a read that failed reached a verdict: {answer:?}"
        );

        let told = told.await.expect("the listener");

        assert!(
            !told.ends_with(&0_u32.to_be_bytes()),
            "the final frame told clamd the file had ended: {told:?}"
        );
    }

    /// A `clamd` that reads the stream to its final frame and answers `OK`.
    async fn answering_clamd() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("a port");
        let port = listener.local_addr().expect("an address").port();

        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("the worker");
            let mut command = [0_u8; 10];

            socket.read_exact(&mut command).await.expect("the command");

            loop {
                let length = socket.read_u32().await.expect("a length") as usize;

                if length == 0 {
                    break;
                }

                let mut chunk = vec![0_u8; length];

                socket.read_exact(&mut chunk).await.expect("a chunk");
            }

            socket.write_all(b"stream: OK\0").await.expect("the answer");
            socket.shutdown().await.ok();
        });

        port
    }

    /// **A store slower than the limit is not a scanner that stopped answering.**
    /// Three reads of 150 ms against a limit of 100 ms, so neither one read nor
    /// the conversation may be what is timed: either way this times out, on
    /// every pass, and the row stays `PENDING` for ever.
    #[tokio::test]
    async fn a_slow_store_is_not_counted_against_the_scanner() {
        let port = answering_clamd().await;
        let file = stream::iter(0..3)
            .then(|_| async {
                tokio::time::sleep(Duration::from_millis(150)).await;

                Ok(Bytes::from_static(b"part"))
            })
            .boxed();

        let reply = converse("127.0.0.1", port, file, Duration::from_millis(100))
            .await
            .expect("the scanner answered");

        assert_eq!(interpret(&reply), ScanOutcome::Clean);
    }

    #[tokio::test]
    async fn a_scanner_that_takes_the_file_and_never_answers_is_still_timed() {
        let (port, _told) = listener().await;
        let file = stream::iter([Ok(Bytes::from_static(b"the file"))]).boxed();

        let answer = converse("127.0.0.1", port, file, Duration::from_millis(100)).await;

        assert!(
            answer.is_err_and(|error| error.0.contains("did not answer")),
            "a silent scanner was not given up on"
        );
    }

    /// Accepts and never reads, so the writes stop once the socket's buffers are
    /// full. The outer timeout is what a write with no limit of its own would
    /// run into.
    #[tokio::test]
    async fn a_scanner_that_stops_reading_is_still_timed() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("a port");
        let port = listener.local_addr().expect("an address").port();
        let held = tokio::spawn(async move { listener.accept().await });
        let megabyte = Bytes::from(vec![7_u8; 1024 * 1024]);
        let file = stream::repeat_with(move || Ok(megabyte.clone()))
            .take(256)
            .boxed();

        let answer = tokio::time::timeout(
            Duration::from_secs(10),
            converse("127.0.0.1", port, file, Duration::from_millis(100)),
        )
        .await
        .expect("a write with no limit waited for ever");

        assert!(
            answer.is_err_and(|error| error.0.contains("did not answer")),
            "a stalled scanner was not given up on"
        );

        held.abort();
    }

    /// A zero-length frame is the end of the stream, so an empty read in the
    /// middle of a file must not become one.
    #[tokio::test]
    async fn an_empty_read_is_not_the_end_of_the_file() {
        let (port, told) = listener().await;
        let file = stream::iter([
            Ok(Bytes::from_static(b"one")),
            Ok(Bytes::new()),
            Ok(Bytes::from_static(b"two")),
        ])
        .boxed();

        // The listener says nothing, so the wait for its answer ends here; what
        // matters is what it was told before that.
        tokio::time::timeout(Duration::from_millis(200), scan("127.0.0.1", port, file))
            .await
            .expect_err("nothing answers this scan");

        let mut framed = b"zINSTREAM\0".to_vec();

        for chunk in [&b"one"[..], b"two"] {
            framed.extend_from_slice(&(chunk.len() as u32).to_be_bytes());
            framed.extend_from_slice(chunk);
        }

        framed.extend_from_slice(&0_u32.to_be_bytes());

        assert_eq!(told.await.expect("the listener"), framed);
    }

    #[test]
    fn the_three_replies_clamd_gives_are_read_as_themselves() {
        assert_eq!(interpret("stream: OK"), ScanOutcome::Clean);
        assert_eq!(
            interpret("stream: Eicar-Test-Signature FOUND"),
            ScanOutcome::Infected("Eicar-Test-Signature".to_owned())
        );
        assert_eq!(
            interpret("INSTREAM size limit exceeded. ERROR"),
            ScanOutcome::Failed("INSTREAM size limit exceeded. ERROR".to_owned())
        );
    }

    #[test]
    fn a_reply_this_binary_does_not_know_is_a_refusal_and_not_a_pass() {
        // The whole point of the `OK` suffix being matched exactly: a future
        // clamd saying something new must not be read as clearing the file.
        assert!(matches!(
            interpret("stream: SOMETHING NEW"),
            ScanOutcome::Failed(_)
        ));
        assert!(matches!(interpret(""), ScanOutcome::Failed(_)));
    }

    #[test]
    fn a_signature_with_a_colon_in_it_keeps_its_own_name() {
        assert_eq!(
            interpret("stream: Win.Test.EICAR_HDB-1 FOUND"),
            ScanOutcome::Infected("Win.Test.EICAR_HDB-1".to_owned())
        );
    }
}
