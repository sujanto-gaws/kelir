//! Delivering what the outbox holds ([#519]; ADR-0041 §2).
//!
//! # A claim, then one transaction per event
//!
//! A pass **claims** what is due in one short statement — each row goes
//! `PROCESSING` under a five-minute lease — and then delivers each event in a
//! transaction of its own. The consumer's writes (the execution log, a
//! breaker's notification) and the row's settlement commit together, so a
//! worker that dies mid-delivery leaves the row `PROCESSING` with nothing of
//! its attempt recorded, and the lease is what hands it to the next pass.
//!
//! # Retries are the worker's, not the handler's
//!
//! **A delivery that fails is retried whole**, on ADR-0041's schedule: 30 s,
//! 1 min, 2 min, 4 min, 8 min, and the sixth failed attempt dead-letters the
//! row. That is architectures/01 §12.5 putting the retry schedule, the dead
//! letter and the circuit breaker on the worker rather than on each handler.
//! `DEAD_LETTER` keeps the envelope — the dispatch is exhausted, the event is
//! not — and is visible by SQL alone until a screen reads it.
//!
//! # An event type this build does not consume is settled, not held
//!
//! EES §3: *consumers MUST ignore event types they do not know.* This build has
//! one consumer, so an unknown type has nothing to be delivered to, and it is
//! marked `PROCESSED` with a warning — the same settlement as a transition
//! whose after-chain is empty. Leaving it `PENDING` would have every pass claim
//! it again and deliver it to nobody, for ever; failing it would dead-letter an
//! event nothing was wrong with. **The row is kept either way**, so a consumer
//! a later release adds is a question of replaying rows, not of recovering
//! them — ADR-0041 §6's revisit trigger, where a row's status starts to mean the
//! fan-out.
//!
//! # A poll, on a fixed interval
//!
//! The scanner and the sender each read their interval from configuration.
//! This one does not: a delivery is in-process work with no external party to
//! be gentle with, and a variable is a row in the deployment reference that
//! nobody has a reason to change. [`POLL_INTERVAL`] is the latency between a
//! transition committing and its after-hooks running.
//!
//! [#519]: https://github.com/sujanto-gaws/kelir/issues/519

use std::time::Duration;

use super::domain::{Delivery, Settlement, LEASE, WORKFLOW_TRANSITIONED};
use super::repository::{self as repo, Claimed};
use crate::error::AppError;
use crate::modules::workflow;
use crate::state::AppState;

/// How many events one pass claims.
///
/// Small enough that a claimed batch is delivered well inside its lease — the
/// chain budget is five seconds an event, and thirty-two of those is under
/// three minutes of five.
const BATCH: i64 = 32;

/// How long the loop sleeps between passes that found nothing to do.
pub const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Runs until the process ends.
///
/// Spawned once at startup beside the scanner and the sender, and shaped like
/// them: errors are logged and the loop continues. **A pass that found a full
/// batch goes again at once**, so a backlog drains at the speed of delivery
/// rather than at one batch per interval.
pub async fn run(state: AppState) {
    tracing::info!(
        interval = ?POLL_INTERVAL,
        "the outbox worker is watching for events to deliver"
    );

    loop {
        let claimed = match pass(&state).await {
            Ok(claimed) => claimed,
            Err(error) => {
                tracing::error!(%error, "the outbox pass could not claim its work");
                0
            }
        };

        if claimed < BATCH as usize {
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }
}

/// One pass: claim what is due, up to [`BATCH`], and deliver each. Returns
/// how many were claimed.
///
/// **Public because it is the unit a test can drive**, which is
/// `attachment::worker::pass`'s reason and the same seam.
pub async fn pass(state: &AppState) -> Result<usize, sqlx::Error> {
    let claimed = repo::claim(&state.pool, BATCH, LEASE.as_secs_f64()).await?;

    for event in &claimed {
        // One event's database failure is that event's: it is recorded on its
        // own row below and the rest of the batch is still delivered.
        if let Err(error) = deliver_one(state, event).await {
            tracing::error!(
                event = %event.id,
                %error,
                "an outbox event could not be settled; its lease will return it"
            );
        }
    }

    Ok(claimed.len())
}

async fn deliver_one(state: &AppState, event: &Claimed) -> Result<(), sqlx::Error> {
    let attempt = event.attempt_count + 1;
    let mut transaction = state.pool.begin().await?;

    match dispatch(&mut transaction, event).await {
        Ok(delivery) => {
            let settlement = Settlement::after(attempt, &delivery);
            let reason = match &delivery {
                Delivery::Delivered => None,
                Delivery::Retry(reason) => Some(reason.as_str()),
            };

            let held = repo::settle(
                &mut *transaction,
                event.tenant_id,
                event,
                attempt,
                settlement,
                reason,
            )
            .await?;

            if !held {
                // Overtaken: this pass outlived its lease and another claimed
                // the row. Its runs are discarded with the transaction, because
                // the newer claimant is delivering the same event and will log
                // its own.
                overtaken(event);

                return transaction.rollback().await;
            }

            log_settlement(event, attempt, settlement, reason);

            transaction.commit().await
        }
        Err(error) => {
            // The consumer's own writes are discarded with its transaction —
            // an attempt that failed half-way records nothing of the half — and
            // the attempt is counted on the row outside it, so a delivery that
            // errors on every attempt reaches the dead letter rather than
            // cycling on its lease for ever.
            transaction.rollback().await?;

            let reason = cause_of(&error);
            let settlement = Settlement::after(attempt, &Delivery::Retry(reason.clone()));

            let held = repo::settle(
                &state.pool,
                event.tenant_id,
                event,
                attempt,
                settlement,
                Some(&reason),
            )
            .await?;

            if held {
                log_settlement(event, attempt, settlement, Some(&reason));
            } else {
                overtaken(event);
            }

            Ok(())
        }
    }
}

/// What went wrong, in words an operator can act on.
///
/// **Not `AppError`'s `Display`**, which is the response a *caller* would see:
/// for `Internal` that is `INTERNAL_ERROR: An unexpected error occurred`, by
/// design, and on this row it would say nothing about why the attempt failed.
/// The worker has no caller to shield, so it records the source — the chain
/// `anyhow` carries, which for a database failure is the driver's message.
///
/// **Nothing here is a secret.** The source is an `sqlx` or application error:
/// a statement's failure, never a credential or a payload. Form data does not
/// reach an error message on this path — the consumer builds none from it.
fn cause_of(error: &AppError) -> String {
    match error {
        AppError::Internal { source } => format!("{source:#}"),
        other => other.to_string(),
    }
}

fn overtaken(event: &Claimed) {
    tracing::warn!(
        event = %event.id,
        "an outbox delivery outlived its lease and another worker has claimed the event; \
         this attempt's result is dropped"
    );
}

/// The dispatch table: an event type to its consumer (ADR-0041 §4).
async fn dispatch(
    transaction: &mut sqlx::PgTransaction<'_>,
    event: &Claimed,
) -> Result<Delivery, AppError> {
    match event.event_type.as_str() {
        WORKFLOW_TRANSITIONED => {
            workflow::service::after_hooks::deliver(
                transaction,
                event.tenant_id,
                &event.payload_json,
            )
            .await
        }
        other => {
            tracing::warn!(
                event = %event.id,
                event_type = %other,
                "no consumer in this build reads this event type; it is settled unread"
            );

            Ok(Delivery::Delivered)
        }
    }
}

fn log_settlement(event: &Claimed, attempt: i32, settlement: Settlement, reason: Option<&str>) {
    match settlement {
        Settlement::Processed => {}
        Settlement::Failed(delay) => tracing::warn!(
            event = %event.id,
            attempt,
            retry_in = ?delay,
            reason = reason.unwrap_or_default(),
            "an outbox delivery failed and will be retried"
        ),
        Settlement::DeadLetter => tracing::error!(
            event = %event.id,
            attempt,
            reason = reason.unwrap_or_default(),
            "an outbox delivery failed for the last time and is dead-lettered"
        ),
    }
}
