//! What a notification is (FR-NTF-001; [#251]).
//!
//! [#251]: https://github.com/sujanto-gaws/kelir/issues/251

use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

/// What a notification is about (Database Schema §11.1's vocabulary).
///
/// **A closed set here and an open `TEXT` in the column**, which is the
/// opposite way round from `EventCategory` and its `CHECK`. §11.3 declares
/// `notification_type` with no constraint because
/// [#257](https://github.com/sujanto-gaws/kelir/issues/257)'s templates key off
/// it and a plugin may add one; this enum is the set *this release writes*.
/// [`Self::from_db`] is what keeps a row somebody else wrote readable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NotificationType {
    /// A task is waiting for you.
    TaskAssigned,
    /// A document you raised was decided.
    DocumentDecided,
    /// An after-hook failed enough times in a row that its circuit breaker
    /// opened (ADR-0041 §2). Sent to the tenant's administrators.
    HookCircuitOpened,
    /// A type this build does not know, read from a row a later release wrote.
    Other,
}

impl NotificationType {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::TaskAssigned => "TASK_ASSIGNED",
            Self::DocumentDecided => "DOCUMENT_DECIDED",
            Self::HookCircuitOpened => "HOOK_CIRCUIT_OPENED",
            Self::Other => "OTHER",
        }
    }

    /// **Unknown reads as [`Self::Other`] rather than refusing**, which is
    /// `EventCategory::from_db`'s reasoning and the same trade: nothing
    /// branches on this value, so a wrong guess costs a label and not a
    /// decision. The `title` and `body` are the row's own words and carry the
    /// meaning; the type is how a client groups them.
    ///
    /// The contrast is `VirusScanStatus::from_db`, which refuses — because
    /// there a wrong guess costs the bytes.
    pub fn from_db(value: &str) -> Self {
        match value {
            "TASK_ASSIGNED" => Self::TaskAssigned,
            "DOCUMENT_DECIDED" => Self::DocumentDecided,
            "HOOK_CIRCUIT_OPENED" => Self::HookCircuitOpened,
            _ => Self::Other,
        }
    }
}

/// One notification, as its recipient reads it.
///
/// **No `recipientUserId`.** Every row this type can reach is the caller's own —
/// the statement is what guarantees it (#251 AC7) — so serializing the
/// recipient would be telling somebody their own id back. Its absence is also
/// what makes an accidentally unscoped read visible in a test: there is no
/// field to assert the wrong value in, so the assertion has to be about which
/// rows came back.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Notification {
    pub id: Uuid,

    /// Where it points. A client offers the document, the task, or neither.
    pub document_id: Option<Uuid>,
    pub workflow_instance_id: Option<Uuid>,
    pub task_id: Option<Uuid>,

    pub notification_type: NotificationType,
    pub title: String,
    pub body: String,

    /// **Null while unread**, and the only place readness lives — `status` has
    /// a `READ` value this release never writes, because two columns saying one
    /// thing invite them to disagree.
    pub read_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// What the centre reports alongside the page.
///
/// **The unread count is not `meta.total`**, and the difference is the point:
/// the page is *everything addressed to me*, and the badge is *how many of them
/// I have not read*. A client computing the badge from the page would count one
/// page of it.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UnreadCount {
    pub unread: i64,
}

/// A sender's claim on a notification: how long it has before the row is
/// claimed again ([#853]).
///
/// **Five minutes, the outbox's lease** (`outbox::domain::LEASE`), for the
/// same two reasons. It is long enough that a batch is delivered well inside
/// it: [`SEND_ALLOWANCE`] stops a pass from starting a send the lease could
/// not see finish. And it is short enough that a sender that crashed holding
/// a batch delays those emails by minutes, not by a restart somebody has to
/// notice.
///
/// [#853]: https://github.com/sujanto-gaws/kelir/issues/853
pub const LEASE: Duration = Duration::from_secs(5 * 60);

/// How much of the lease must remain for a pass to start one more send.
///
/// **A send can outlast a lease, and that is the one way two senders could
/// still both email a row**: the first's lease passes mid-send, a second
/// claims the row, and both deliver. A pass therefore starts a send only while
/// this much of its lease is left, and leaves the rest of its batch to pass
/// back to the queue when the lease does.
///
/// **Two minutes**: twice the SMTP transport's per-command timeout (lettre's
/// default, 60 s, which `Mailer::from_config` does not change), so a relay
/// that stalls on one command times out inside the lease. A relay that stalls
/// on several in one send can still outlast it; that send is the residual
/// duplicate, and `0` from `mark_delivered` is how its sender learns of it.
pub const SEND_ALLOWANCE: Duration = Duration::from_secs(2 * 60);

/// Whether a pass that claimed its batch `elapsed` ago may start another send.
///
/// `elapsed` is measured from before the claim was issued, so it is never
/// shorter than the time the database has counted against the lease.
pub fn may_begin_send(elapsed: Duration) -> bool {
    elapsed.saturating_add(SEND_ALLOWANCE) <= LEASE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_claim_may_send() {
        assert!(may_begin_send(Duration::ZERO));
    }

    #[test]
    fn a_send_may_start_while_the_allowance_still_fits_in_the_lease() {
        assert!(may_begin_send(LEASE - SEND_ALLOWANCE));
    }

    #[test]
    fn a_send_may_not_start_once_the_allowance_no_longer_fits() {
        assert!(!may_begin_send(
            LEASE - SEND_ALLOWANCE + Duration::from_millis(1)
        ));
        assert!(!may_begin_send(LEASE));
        assert!(!may_begin_send(LEASE * 2));
    }

    #[test]
    fn an_elapsed_time_past_any_lease_does_not_overflow() {
        assert!(!may_begin_send(Duration::MAX));
    }
}
