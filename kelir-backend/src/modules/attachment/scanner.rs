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

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

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
const TIMEOUT: Duration = Duration::from_secs(30);

/// Streams one file to `clamd` and reads its answer.
pub async fn scan(host: &str, port: u16, bytes: &[u8]) -> Result<ScanOutcome, ScanError> {
    let reply = tokio::time::timeout(TIMEOUT, converse(host, port, bytes))
        .await
        .map_err(|_| ScanError(format!("clamd at {host}:{port} did not answer within 30s")))??;

    Ok(interpret(&reply))
}

/// The protocol itself.
async fn converse(host: &str, port: u16, bytes: &[u8]) -> Result<String, ScanError> {
    let mut stream = TcpStream::connect((host, port))
        .await
        .map_err(|error| ScanError(format!("cannot reach clamd at {host}:{port}: {error}")))?;

    stream
        .write_all(b"zINSTREAM\0")
        .await
        .map_err(|error| ScanError(format!("clamd closed before the stream began: {error}")))?;

    for chunk in bytes.chunks(CHUNK) {
        // The length prefix is big-endian and the protocol has no other framing,
        // so a short write here would desynchronise the stream rather than fail
        // it — which is why `write_all` is used throughout.
        let length = u32::try_from(chunk.len()).unwrap_or(u32::MAX);

        stream
            .write_all(&length.to_be_bytes())
            .await
            .map_err(|error| ScanError(format!("clamd stopped mid-stream: {error}")))?;
        stream
            .write_all(chunk)
            .await
            .map_err(|error| ScanError(format!("clamd stopped mid-stream: {error}")))?;
    }

    // A zero-length chunk ends the stream. **Its failure is not an error**: a
    // clamd that has already decided — a stream over its limit is the usual
    // case — closes the write side before this arrives, and the answer it wants
    // to give is still waiting to be read.
    let _ = stream.write_all(&0_u32.to_be_bytes()).await;

    let mut reply = Vec::new();

    stream
        .read_to_end(&mut reply)
        .await
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
