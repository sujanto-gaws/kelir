//! The envelope, the statuses a row moves through, and the retry schedule
//! (EES 1.0.0 §2; Database Schema §12.8; ADR-0041 §2).

use std::time::Duration;

use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{json, Value};
use uuid::Uuid;

/// The EES version every envelope this build writes carries (EES §2).
pub const ENVELOPE_VERSION: &str = "1.0.0";

/// A workflow instance moved from one state to another (ADR-0041 §2).
///
/// **One event type for every transition**, decided or automatic, with the
/// action in the payload. `Workflow.Approved`, `Workflow.Rejected` and the rest
/// would be ten names for one fact, and a consumer wanting every transition
/// would have to subscribe to all of them and to whichever action a later JWSS
/// adds.
pub const WORKFLOW_TRANSITIONED: &str = "Workflow.Transitioned";

/// The aggregate an event is about (EES §2's enum).
///
/// Only the ones this build writes, for `hook::Stage`'s reason: an enum is a
/// claim about what the code can produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AggregateType {
    WorkflowInstance,
}

impl AggregateType {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::WorkflowInstance => "WORKFLOW_INSTANCE",
        }
    }
}

/// Who performed what an event records (EES §2's `actor`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Actor {
    /// A person. A decided transition is theirs.
    User(Uuid),
    /// The engine, taking an `AUTO` edge nobody decided.
    WorkflowEngine,
    /// Nobody identifiable — a transition reached with no actor at all.
    System,
}

impl Actor {
    /// A decided transition's actor: the person, where there is one.
    pub fn user_or_system(user: Option<Uuid>) -> Self {
        user.map_or(Self::System, Self::User)
    }

    fn as_json(self) -> Value {
        match self {
            Self::User(id) => json!({ "actorType": "USER", "actorId": id.to_string() }),
            Self::WorkflowEngine => json!({ "actorType": "WORKFLOW_ENGINE", "actorId": null }),
            Self::System => json!({ "actorType": "SYSTEM", "actorId": null }),
        }
    }
}

/// An event as its producer hands it over.
pub struct NewEvent<'a> {
    pub tenant_id: Uuid,
    pub aggregate_type: AggregateType,
    pub aggregate_id: Uuid,
    pub event_type: &'a str,
    /// 1-based and monotonic per aggregate (EES §2), or `None` where the
    /// producer cannot sequence.
    pub sequence: Option<i64>,
    pub correlation_id: String,
    /// The `eventId` of the event whose processing produced this one; `None`
    /// at the origin, which is every event this build writes.
    pub causation_id: Option<Uuid>,
    pub actor: Actor,
    /// The profile fields, and no form data (EES §4.2).
    pub payload: Value,
}

/// The envelope as EES §2 shapes it, and as `payload_json` stores it.
///
/// **`eventId` is the row's own id**, so the deduplication key a consumer is
/// told to trust and the key the table is indexed by are one value, and a
/// redelivery carries the same one by construction.
pub fn envelope(event_id: Uuid, occurred_at: DateTime<Utc>, event: &NewEvent<'_>) -> Value {
    json!({
        "eventId": event_id.to_string(),
        "version": ENVELOPE_VERSION,
        "eventType": event.event_type,
        "occurredAt": occurred_at.to_rfc3339_opts(SecondsFormat::Micros, true),
        "tenantId": event.tenant_id.to_string(),
        "aggregateType": event.aggregate_type.as_db(),
        "aggregateId": event.aggregate_id.to_string(),
        "sequence": event.sequence,
        "correlationId": event.correlation_id,
        "causationId": event.causation_id.map(|id| id.to_string()),
        "actor": event.actor.as_json(),
        "payload": event.payload,
    })
}

/// A claimed row's lease: how long a worker has before the row is claimed
/// again (ADR-0041 §2).
///
/// **Held in `next_attempt_at`**, so a crashed worker's row needs no sweeper:
/// the claim that finds `FAILED` rows whose time has come finds expired
/// `PROCESSING` rows the same way.
pub const LEASE: Duration = Duration::from_secs(5 * 60);

/// The attempt that dead-letters a row rather than scheduling another.
pub const MAX_ATTEMPTS: i32 = 6;

/// How long after its `attempt`th failure a row is retried: 30 s × 2^(attempt − 1)
/// — 30 s, 1 min, 2 min, 4 min, 8 min (ADR-0041 §2).
pub fn retry_delay(attempt: i32) -> Duration {
    let exponent = attempt.clamp(1, MAX_ATTEMPTS) - 1;

    Duration::from_secs(30 * 2u64.pow(exponent as u32))
}

/// What a consumer made of one delivery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delivery {
    /// Done — including *nothing to do*, which is most transitions.
    Delivered,
    /// Try again later, and this is why.
    Retry(String),
}

/// Where a row goes after an attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Settlement {
    Processed,
    /// Retried after this long.
    Failed(Duration),
    /// The dispatch is exhausted. The envelope stays.
    DeadLetter,
}

impl Settlement {
    /// `attempt` is 1-based: the attempt that just finished.
    pub fn after(attempt: i32, delivery: &Delivery) -> Self {
        match delivery {
            Delivery::Delivered => Self::Processed,
            Delivery::Retry(_) if attempt >= MAX_ATTEMPTS => Self::DeadLetter,
            Delivery::Retry(_) => Self::Failed(retry_delay(attempt)),
        }
    }

    pub fn as_db(self) -> &'static str {
        match self {
            Self::Processed => "PROCESSED",
            Self::Failed(_) => "FAILED",
            Self::DeadLetter => "DEAD_LETTER",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ADR-0041 §2's schedule, number by number.
    #[test]
    fn a_failure_is_retried_on_the_doubling_schedule() {
        let seconds: Vec<u64> = (1..=5).map(|n| retry_delay(n).as_secs()).collect();

        assert_eq!(seconds, [30, 60, 120, 240, 480]);
    }

    /// **The sixth failed attempt dead-letters the row**, and the fifth does
    /// not: five retries after the first attempt, then none.
    #[test]
    fn the_sixth_failure_dead_letters_and_the_fifth_does_not() {
        let retry = Delivery::Retry("down".to_owned());

        assert_eq!(
            Settlement::after(5, &retry),
            Settlement::Failed(Duration::from_secs(480))
        );
        assert_eq!(Settlement::after(6, &retry), Settlement::DeadLetter);
        assert_eq!(
            Settlement::after(6, &Delivery::Delivered),
            Settlement::Processed
        );
    }

    #[test]
    fn the_envelope_carries_every_field_ees_requires() {
        let id = Uuid::now_v7();
        let envelope = envelope(
            id,
            Utc::now(),
            &NewEvent {
                tenant_id: Uuid::nil(),
                aggregate_type: AggregateType::WorkflowInstance,
                aggregate_id: Uuid::nil(),
                event_type: WORKFLOW_TRANSITIONED,
                sequence: Some(2),
                correlation_id: "c".to_owned(),
                causation_id: None,
                actor: Actor::WorkflowEngine,
                payload: json!({}),
            },
        );

        for field in [
            "eventId",
            "version",
            "eventType",
            "occurredAt",
            "tenantId",
            "aggregateType",
            "aggregateId",
            "sequence",
            "correlationId",
            "causationId",
            "actor",
            "payload",
        ] {
            assert!(envelope.get(field).is_some(), "`{field}` is missing");
        }

        assert_eq!(envelope["eventId"], id.to_string());
        // Present and null, not absent: the meta-schema requires the key.
        assert!(envelope["causationId"].is_null());
        assert_eq!(envelope["actor"]["actorType"], "WORKFLOW_ENGINE");
        assert!(envelope["actor"]["actorId"].is_null());
    }

    #[test]
    fn a_decided_transition_without_a_person_is_the_system_s() {
        let person = Uuid::now_v7();

        assert_eq!(Actor::user_or_system(Some(person)), Actor::User(person));
        assert_eq!(Actor::user_or_system(None), Actor::System);
    }
}
