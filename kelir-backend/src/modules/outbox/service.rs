//! Writing an event (EES §2; ADR-0041 §2).

use chrono::Utc;
use uuid::Uuid;

use super::domain::{envelope, NewEvent};
use super::repository;

/// Writes one event into the outbox, **in the caller's transaction**, and
/// returns its `eventId`.
///
/// The only way an event enters `outbox_events` (ADR-0041 §5). The envelope is
/// built here rather than by each producer, so every producer's events carry
/// the same fields in the same shape — EES S1 is a property of this function,
/// not a rule each caller keeps.
///
/// `occurredAt` is taken here, inside the transaction. EES §2 asks for the
/// commit time, which no statement inside the transaction can know; the
/// difference is the rest of the transaction, and a consumer ordering by it
/// is told by EES §1.1 to order by `sequence` instead.
pub async fn write(
    transaction: &mut sqlx::PgTransaction<'_>,
    event: &NewEvent<'_>,
) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::now_v7();
    let payload = envelope(id, Utc::now(), event);

    repository::insert(
        transaction,
        &repository::NewRow {
            id,
            tenant_id: event.tenant_id,
            aggregate_type: event.aggregate_type.as_db(),
            aggregate_id: event.aggregate_id,
            event_type: event.event_type,
            payload_json: &payload,
            correlation_id: &event.correlation_id,
        },
    )
    .await?;

    Ok(id)
}
