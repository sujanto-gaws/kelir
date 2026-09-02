//! Delivering notifications on the channels a tenant has turned on
//! (FR-NTF-004; [#257]).
//!
//! # Why this is a worker and not a line in `notify`
//!
//! [#251](https://github.com/sujanto-gaws/kelir/issues/251) AC3 requires a
//! notification to be written **in the transaction of the thing it announces**,
//! and `service::notify` says so in its own documentation — an approval that
//! rolled back must not have told anybody it happened. An SMTP call inside that
//! transaction would hold a database lock open across somebody else's network,
//! which is the shape **D-35** already cost this project once ([#257] AC3).
//!
//! So the row is the queue. `notify` writes it `PENDING` inside the caller's
//! transaction; this loop finds it after that transaction has committed, and
//! delivers. `service::notify`'s own comment predicted this design in as many
//! words: *an outbox row and a worker … is the right shape for email (#257),
//! where delivery is somebody else's network*.
//!
//! # What a failure costs, and what it does not
//!
//! **Never the notification** ([#257] AC2). The in-app record is the storage and
//! is already committed and already readable in the centre; email is an
//! additional delivery. A failed send marks the row `FAILED`, writes a
//! `notification_logs` row saying why, and leaves `read_at`, the title, the body
//! and the centre's view of it exactly as they were.
//!
//! **One attempt, and that is stated rather than assumed.** `notification_logs`
//! carries an `attempt` column and this always writes `1`: a retry needs a
//! backoff, a cap and somewhere for the permanently undeliverable to go, and a
//! loop that retried every `FAILED` row on every pass would send a mail server
//! that is refusing one message a copy of it every few seconds. The column is
//! where that lands when somebody builds it; until then a failed delivery is a
//! logged fact rather than a silent one.
//!
//! # Nothing here reminds or escalates
//!
//! [#257] AC6. FR-NTF-006 and FR-NTF-007 need a task to still be open at a time
//! nobody is asking about it, which is a scheduler (FR-WF-010) rather than a
//! sender. This loop delivers what happened; it does not decide that something
//! has *not* happened.
//!
//! [#257]: https://github.com/sujanto-gaws/kelir/issues/257

use std::time::Duration;

use super::repository as repo;
use super::template::{self, Context};
use crate::mail::Mail;
use crate::state::AppState;

/// How many notifications one pass delivers.
///
/// Larger than the scanner's eight because the work is a small SMTP round trip
/// rather than a hundred megabytes through a virus scanner, and small enough
/// that one pass cannot hold the loop for a minute against a slow relay.
const BATCH: i64 = 32;

/// Runs until the process ends.
///
/// Spawned once at startup beside the scanner, and shaped like it: errors are
/// logged and the loop continues, because the component whose job is to talk to
/// something that can be down must survive it being down.
pub async fn run(state: AppState) {
    let interval = Duration::from_secs(state.config.notification_poll_seconds.max(1));

    tracing::info!(
        ?interval,
        "the notification sender is watching for what to deliver"
    );

    loop {
        if let Err(error) = pass(&state).await {
            tracing::error!(%error, "the delivery pass could not read its work");
        }

        tokio::time::sleep(interval).await;
    }
}

/// One pass: everything waiting, up to [`BATCH`].
///
/// **Public because it is the unit a test can drive**, which is
/// `attachment::worker::pass`'s reason and the same seam: [`run`] is a loop with
/// a sleep in it, and this is the same work with the waiting taken out.
pub async fn pass(state: &AppState) -> Result<(), sqlx::Error> {
    let waiting = repo::pending_deliveries(&state.pool, BATCH).await?;

    for notification in waiting {
        deliver_one(state, &notification).await?;
    }

    Ok(())
}

/// One notification, on every channel its tenant has turned on.
///
/// **The channels are read, not branched on** ([#257] AC1). There is no
/// `match notification_type` here and no `if type == TaskAssigned`: a tenant
/// with an enabled `EMAIL` row and a template for this type gets an email, and
/// one without either does not.
async fn deliver_one(
    state: &AppState,
    notification: &repo::PendingDelivery,
) -> Result<(), sqlx::Error> {
    let channels = repo::enabled_channels(&state.pool, notification.tenant_id).await?;

    // **No channel is not a failure.** A deployment that has turned email off,
    // or has not turned anything on, is a deployment whose notifications are
    // in-app — they are delivered by being written, so the row moves to `SENT`
    // with nothing logged, because there was no attempt to log.
    if channels.is_empty() {
        repo::mark_delivered(&state.pool, notification.tenant_id, notification.id, "SENT").await?;

        return Ok(());
    }

    let mut every_attempt_worked = true;

    for channel in &channels {
        let outcome = match channel.as_str() {
            "EMAIL" => send_email(state, notification).await,
            // A channel a tenant has enabled and this build cannot send on —
            // `SMS`, `MOBILE_PUSH`, a plugin's. Recorded as a failed attempt
            // rather than skipped: the row said to deliver here and nothing did,
            // and a trail that stayed silent about it would make an unbuilt
            // channel indistinguishable from a working one.
            other => Err(format!("this build has no sender for the {other} channel")),
        };

        let (status, error) = match &outcome {
            Ok(()) => ("SENT", None),
            Err(error) => ("FAILED", Some(error.as_str())),
        };

        repo::record_attempt(
            &state.pool,
            notification.tenant_id,
            notification.id,
            channel,
            status,
            error,
        )
        .await?;

        if let Err(error) = outcome {
            tracing::warn!(
                notification = %notification.id,
                %channel,
                %error,
                "a notification could not be delivered on one of its channels"
            );

            every_attempt_worked = false;
        }
    }

    let status = if every_attempt_worked {
        "SENT"
    } else {
        "FAILED"
    };

    repo::mark_delivered(&state.pool, notification.tenant_id, notification.id, status).await?;

    Ok(())
}

/// The email itself, rendered from the tenant's template or from the
/// notification's own words.
///
/// # A template that fails to render sends a plain notification (#257 AC5)
///
/// Three ways it can fail — no template for this type, a template naming a
/// placeholder the sender cannot resolve, and a template with no subject — and
/// **all three fall back to the title and body the notification already
/// carries**. Silence is the failure this whole epic exists to end, and an
/// unsendable email because somebody mistyped a placeholder in a configuration
/// table would be that failure wearing a different hat.
///
/// The fallback is logged at `warn`, because a template that never renders is a
/// misconfiguration somebody should fix even though nobody is missing an email
/// over it.
async fn send_email(state: &AppState, notification: &repo::PendingDelivery) -> Result<(), String> {
    let Some(address) = notification.recipient_email.as_deref() else {
        // `users.email` is `NOT NULL`, so this is a recipient whose account was
        // removed between the notification being written and this pass. Not
        // retried and not silent: it is a fact about the account, and the log
        // row is where somebody sees it.
        return Err("the recipient has no active account with an email address".to_owned());
    };

    let context = Context {
        title: &notification.title,
        body: &notification.body,
    };

    let template = repo::template_for(
        &state.pool,
        notification.tenant_id,
        &notification.notification_type,
        "EMAIL",
    )
    .await
    .map_err(|error| format!("the template could not be read: {error}"))?;

    let (subject, body) = match template {
        Some(template) => {
            let rendered = template
                .subject_template
                .as_deref()
                .and_then(|subject| template::render(subject, &context))
                .zip(template::render(&template.body_template, &context));

            match rendered {
                Some((subject, body)) => (subject, body),
                None => {
                    tracing::warn!(
                        notification = %notification.id,
                        notification_type = %notification.notification_type,
                        "an email template did not render, so the notification was sent plain"
                    );

                    (notification.title.clone(), notification.body.clone())
                }
            }
        }
        // No template is not a failure either: the notification has a title and
        // a body of its own, which is what the centre shows.
        None => (notification.title.clone(), notification.body.clone()),
    };

    state
        .mailer
        .deliver(Mail {
            to: address.to_owned(),
            subject,
            body,
        })
        .await
}
