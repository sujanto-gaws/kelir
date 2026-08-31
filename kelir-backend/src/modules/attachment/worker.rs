//! The thing that asks the scanner (FR-ATT-001, FR-ATT-002; [#246]).
//!
//! # Asynchronous, and a poll rather than a queue
//!
//! Coding standard §2.4 and [system design](../../../../docs/design/01.%20System%20Design%20Document.md)
//! §13.1 both say scanning is never inline in a request handler: an upload
//! returns as soon as the bytes are stored, with `virus_scan_status = PENDING`
//! (#246 AC1), and this loop answers the question afterwards.
//!
//! **A poll, because the queue this wants is the outbox and the outbox is Phase
//! 8.** What the choice costs is latency bounded by `KELIR_CLAMAV_POLL_SECONDS`.
//! What it buys is that a scan interrupted by a restart is simply picked up
//! again — the row is still `PENDING`, which is the only state a lost scan can
//! leave behind. A task spawned at upload time would lose it silently, and the
//! attachment would sit undownloadable with nothing looking at it.
//!
//! # Nothing here can produce a false `CLEAN`
//!
//! Three separate things have to hold, and they are in three different places on
//! purpose:
//!
//! 1. [`super::scanner::scan`] returns `Result<ScanOutcome, ScanError>`, so *the
//!    scanner did not answer* is a different type from *the scanner answered*.
//! 2. This loop writes a status **only** for the `Ok` arm. A `ScanError` leaves
//!    the row `PENDING` and logs — #246 AC7's *the failure mode is
//!    unavailability, never a silent pass*.
//! 3. [`super::repository::record_scan_result`] writes only over `PENDING`, so
//!    no result can overwrite a decided one.
//!
//! **There is no configuration that turns scanning off.** A deployment with no
//! reachable scanner has every attachment `PENDING` and every attachment
//! undownloadable, which is loud and safe. A switch saying *skip the scan* would
//! be a switch saying *serve unscanned bytes*.
//!
//! [#246]: https://github.com/sujanto-gaws/kelir/issues/246

use std::time::Duration;

use super::repository as repo;
use super::scanner::{self, ScanOutcome};
use crate::state::AppState;

/// How many attachments one pass looks at.
///
/// Small, because the measurement says one `clamd` is throughput-bound at about
/// 156 MiB/s and gains nothing from parallelism: sixteen concurrent 25 MiB scans
/// finish no faster in aggregate than one at a time. A large batch would only
/// make one pass long.
const BATCH: i64 = 8;

/// Runs until the process ends.
///
/// Spawned once at startup. Errors are logged and the loop continues: this is
/// the component whose job is to keep working while the thing it talks to is
/// down.
pub async fn run(state: AppState) {
    let interval = Duration::from_secs(state.config.clamav_poll_seconds.max(1));
    let host = state.config.clamav_host.clone();
    let port = state.config.clamav_port;

    tracing::info!(%host, %port, ?interval, "the attachment scanner is watching for new files");

    loop {
        if let Err(error) = pass(&state, &host, port).await {
            // A database that cannot be read is not this loop's to fix, and it
            // must not end the loop: the next pass may find it back.
            tracing::error!(%error, "the scan pass could not read its work");
        }

        tokio::time::sleep(interval).await;
    }
}

/// One pass: everything waiting, up to [`BATCH`].
///
/// **Public because it is the unit a test can drive.** [`run`] is a loop with a
/// sleep in it, which a test can only wait on; this is the same work with the
/// waiting taken out, so a test points it at a scanner it controls and asserts
/// what the row became. The seam exists for that reason and for no other — the
/// server calls [`run`].
pub async fn pass(state: &AppState, host: &str, port: u16) -> Result<(), sqlx::Error> {
    let waiting = repo::pending_scans(&state.pool, BATCH).await?;

    for attachment in waiting {
        scan_one(state, host, port, &attachment).await;
    }

    Ok(())
}

/// One attachment, and every way this can go.
async fn scan_one(state: &AppState, host: &str, port: u16, attachment: &repo::PendingScan) {
    let bytes = match state.storage.get(&attachment.storage_reference).await {
        Ok(bytes) => bytes,
        Err(error) => {
            // The row says there are bytes and storage says there are not. That
            // is a defect somewhere else and this loop cannot fix it — but it
            // must not mark the file clean, and it must not spin silently.
            tracing::error!(
                %error,
                attachment = %attachment.id,
                reference = %attachment.storage_reference,
                "an attachment waiting to be scanned could not be read from storage"
            );

            return;
        }
    };

    match scanner::scan(host, port, &bytes).await {
        Ok(outcome) => {
            let status = match &outcome {
                ScanOutcome::Clean => "CLEAN",
                ScanOutcome::Infected(_) => "INFECTED",
                ScanOutcome::Failed(_) => "FAILED",
            };

            match repo::record_scan_result(&state.pool, attachment.id, status).await {
                Ok(0) => tracing::debug!(
                    attachment = %attachment.id,
                    %status,
                    "a scan finished on an attachment somebody else had already decided"
                ),
                Ok(_) => match &outcome {
                    ScanOutcome::Clean => tracing::info!(
                        attachment = %attachment.id, "an attachment scanned clean"
                    ),
                    // **A signature name is worth a warning and not a secret.**
                    // Somebody has uploaded an infected file to this tenant and
                    // the operator is the person who needs to know.
                    ScanOutcome::Infected(signature) => tracing::warn!(
                        attachment = %attachment.id,
                        tenant = %attachment.tenant_id,
                        %signature,
                        "an attachment is infected and will not be served"
                    ),
                    ScanOutcome::Failed(reply) => tracing::warn!(
                        attachment = %attachment.id,
                        %reply,
                        "the scanner refused to clear an attachment; it will not be served"
                    ),
                },
                Err(error) => tracing::error!(
                    %error,
                    attachment = %attachment.id,
                    "a scan result could not be written; the attachment stays pending"
                ),
            }
        }
        // **The scanner did not answer, so nothing is known.** The row stays
        // `PENDING`, the file stays undownloadable, and the next pass asks
        // again. This is the arm that must never write a status.
        Err(error) => tracing::warn!(
            %error,
            attachment = %attachment.id,
            "the scanner could not be reached; the attachment stays pending and will be retried"
        ),
    }
}
