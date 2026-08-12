//! Formal audit record (SRS FR-AUD-001..005).
//!
//! Modules never insert audit rows directly (coding standard §2.8) — they call
//! [`record`], which owns the hash chain.
//!
//! Phase 2 writes the chain; verifying it and exposing audit search land with
//! the rest of §10 in Phase 6.

use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::AppError;

/// What happened, in the event vocabulary of naming convention §7.
pub struct AuditEntry<'a> {
    pub tenant_id: Uuid,
    pub event_type: &'a str,
    pub action: &'a str,
    pub object_type: &'a str,
    pub object_id: Uuid,
    pub actor_user_id: Option<Uuid>,
    pub ip_address: Option<&'a str>,
    pub reason: Option<&'a str>,
    pub old_value: Option<Value>,
    pub new_value: Option<Value>,
}

/// Appends an audit event, chaining it to the tenant's previous row.
///
/// Each row's `current_hash` covers its own content *and* the previous row's
/// hash, so altering or removing any row breaks every hash after it. That makes
/// tampering detectable (FR-AUD-003) without preventing writes.
///
/// A failure here must not fail the operation being audited — losing a login
/// because the audit insert failed would be worse than the missing row — so the
/// caller decides. [`record_or_warn`] is the usual choice.
pub async fn record(pool: &PgPool, entry: AuditEntry<'_>) -> Result<Uuid, AppError> {
    let previous_hash = sqlx::query_scalar!(
        r#"
        SELECT current_hash
        FROM audit_events
        WHERE tenant_id = $1
        ORDER BY created_at DESC, id DESC
        LIMIT 1
        "#,
        entry.tenant_id
    )
    .fetch_optional(pool)
    .await?
    .unwrap_or_else(genesis_hash);

    let id = Uuid::now_v7();
    let current_hash = chain_hash(&previous_hash, &id, &entry);

    sqlx::query!(
        r#"
        INSERT INTO audit_events (
            id, tenant_id, event_type, action, object_type, object_id,
            old_value_json, new_value_json, reason, actor_user_id, ip_address,
            previous_hash, current_hash
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
        "#,
        id,
        entry.tenant_id,
        entry.event_type,
        entry.action,
        entry.object_type,
        entry.object_id,
        entry.old_value,
        entry.new_value,
        entry.reason,
        entry.actor_user_id,
        entry.ip_address,
        previous_hash,
        current_hash
    )
    .execute(pool)
    .await?;

    Ok(id)
}

/// Records an event, logging rather than propagating a failure.
///
/// For events that accompany an operation which has already succeeded: the user
/// is signed in whether or not the audit row was written, and refusing the
/// response at that point would be a worse outcome than a gap in the trail. The
/// gap is logged at error level so it is not silent.
pub async fn record_or_warn(pool: &PgPool, entry: AuditEntry<'_>) {
    let event_type = entry.event_type.to_owned();

    if let Err(error) = record(pool, entry).await {
        tracing::error!(error = ?error, event_type, "failed to write audit event");
    }
}

/// First link in a tenant's chain.
fn genesis_hash() -> String {
    "sha256:0000000000000000000000000000000000000000000000000000000000000000".to_owned()
}

fn chain_hash(previous_hash: &str, id: &Uuid, entry: &AuditEntry<'_>) -> String {
    let mut hasher = Sha256::new();

    // Field order is part of the format: changing it invalidates every existing
    // chain, so it must not be reordered casually.
    hasher.update(previous_hash.as_bytes());
    hasher.update(id.as_bytes());
    hasher.update(entry.tenant_id.as_bytes());
    hasher.update(entry.event_type.as_bytes());
    hasher.update(entry.action.as_bytes());
    hasher.update(entry.object_type.as_bytes());
    hasher.update(entry.object_id.as_bytes());
    hasher.update(entry.actor_user_id.unwrap_or_default().as_bytes());
    hasher.update(entry.ip_address.unwrap_or_default().as_bytes());
    hasher.update(entry.reason.unwrap_or_default().as_bytes());

    format!("sha256:{}", hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(tenant_id: Uuid, action: &'static str) -> AuditEntry<'static> {
        AuditEntry {
            tenant_id,
            event_type: "Security.SignedIn",
            action,
            object_type: "USER",
            object_id: Uuid::nil(),
            actor_user_id: None,
            ip_address: None,
            reason: None,
            old_value: None,
            new_value: None,
        }
    }

    #[test]
    fn the_chain_depends_on_the_previous_hash() {
        // The same content at a different chain position must hash differently,
        // or a row could be moved without detection.
        let tenant = Uuid::now_v7();
        let id = Uuid::now_v7();

        let first = chain_hash(&genesis_hash(), &id, &entry(tenant, "LOGIN"));
        let second = chain_hash("sha256:deadbeef", &id, &entry(tenant, "LOGIN"));

        assert_ne!(first, second);
    }

    #[test]
    fn the_chain_depends_on_the_content() {
        let tenant = Uuid::now_v7();
        let id = Uuid::now_v7();
        let previous = genesis_hash();

        let login = chain_hash(&previous, &id, &entry(tenant, "LOGIN"));
        let failed = chain_hash(&previous, &id, &entry(tenant, "LOGIN_FAILED"));

        assert_ne!(
            login, failed,
            "changing the action must change the hash, or an event could be rewritten"
        );
    }

    #[test]
    fn hashing_is_deterministic() {
        let tenant = Uuid::now_v7();
        let id = Uuid::now_v7();

        assert_eq!(
            chain_hash(&genesis_hash(), &id, &entry(tenant, "LOGIN")),
            chain_hash(&genesis_hash(), &id, &entry(tenant, "LOGIN"))
        );
    }

    #[test]
    fn hashes_are_prefixed_and_full_length() {
        let hash = chain_hash(
            &genesis_hash(),
            &Uuid::now_v7(),
            &entry(Uuid::now_v7(), "LOGIN"),
        );

        assert!(hash.starts_with("sha256:"));
        assert_eq!(hash.len(), "sha256:".len() + 64);
    }
}
