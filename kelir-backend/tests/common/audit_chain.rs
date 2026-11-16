//! The audit chain as a verifier reads it (FR-AUD-003, ADR-0012).
//!
//! Moved here from `tests/audit_hash_chain.rs` for `tests/audit_walk.rs`
//! (#960), which verifies the chain after walking every mutating route. A
//! verifier that each file reimplemented would be checking its own copy of the
//! rule, which is the reason `chain_hash` is public in the first place.
//!
//! The read is a runtime query rather than `sqlx::query!`: this module is
//! compiled into every test binary, and the walk is what reads it most.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::Row;
use uuid::Uuid;

use kelir_backend::modules::audit::domain::ObjectType;
use kelir_backend::modules::audit::{chain_hash, AuditEntry};

use super::TestApp;

/// The first link of every tenant's chain.
pub const GENESIS: &str = "sha256:0000000000000000000000000000000000000000000000000000000000000000";

/// One `audit_events` row, as a verifier would read it.
pub struct StoredRow {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub event_type: String,
    pub action: String,
    pub object_type: String,
    pub object_id: Uuid,
    pub actor_user_id: Option<Uuid>,
    pub ip_address: Option<String>,
    pub reason: Option<String>,
    pub old_value: Option<Value>,
    pub new_value: Option<Value>,
    pub previous_hash: String,
    pub current_hash: String,
    pub created_at: DateTime<Utc>,
}

impl StoredRow {
    /// The hash this row should carry, recomputed from the row itself.
    pub fn recomputed(&self) -> String {
        chain_hash(
            &self.previous_hash,
            &self.id,
            self.created_at,
            &AuditEntry {
                tenant_id: self.tenant_id,
                event_type: &self.event_type,
                action: &self.action,
                // **The column's text, read back through the vocabulary**
                // ([#323](https://github.com/sujanto-gaws/kelir/issues/323)).
                // `AuditEntry.object_type` is an `ObjectType` now, and `as_db`
                // returns the same bytes the chain was taken over — so this
                // recomputation is unchanged for every row and the chain does
                // not break at the release that made the field a type.
                //
                // **A row this build cannot place is a row it cannot verify**,
                // and that is stated rather than smoothed over: it panics here.
                // Only this crate writes rows into a test database, so reaching
                // it means the writer and the vocabulary disagree, which is
                // worth a loud failure. A plugin's row in a real deployment is a
                // different question, and #252 AC5 keeps chain verification off
                // the caller-facing surfaces entirely.
                object_type: ObjectType::from_db(&self.object_type).unwrap_or_else(|| {
                    panic!(
                        "audit row {} names object type {}, which this build cannot place",
                        self.id, self.object_type
                    )
                }),
                object_id: self.object_id,
                actor_user_id: self.actor_user_id,
                ip_address: self.ip_address.as_deref(),
                reason: self.reason.as_deref(),
                old_value: self.old_value.clone(),
                new_value: self.new_value.clone(),
            },
        )
    }

    /// `event_type / action on OBJECT_TYPE object_id`, for a failure listing.
    pub fn label(&self) -> String {
        format!(
            "{} / {} on {} {}",
            self.event_type, self.action, self.object_type, self.object_id
        )
    }
}

/// Every audit row of every tenant, oldest first — the order a verifier walks.
pub async fn stored_rows(app: &TestApp) -> Vec<StoredRow> {
    let rows = sqlx::query(
        "SELECT id, tenant_id, event_type, action, object_type, object_id, \
                actor_user_id, ip_address, reason, old_value_json, new_value_json, \
                previous_hash, current_hash, created_at \
         FROM audit_events \
         ORDER BY created_at, id",
    )
    .fetch_all(&app.pool)
    .await
    .expect("the audit rows read back");

    rows.into_iter()
        .map(|row| StoredRow {
            id: row.get("id"),
            tenant_id: row.get("tenant_id"),
            event_type: row.get("event_type"),
            action: row.get("action"),
            object_type: row.get("object_type"),
            object_id: row.get("object_id"),
            actor_user_id: row.get("actor_user_id"),
            ip_address: row.get("ip_address"),
            reason: row.get("reason"),
            old_value: row.get("old_value_json"),
            new_value: row.get("new_value_json"),
            previous_hash: row.get("previous_hash"),
            current_hash: row.get("current_hash"),
            created_at: row.get("created_at"),
        })
        .collect()
}

/// Every place the chain does not verify, tenant by tenant: a row that does
/// not link to the one before it in its tenant, and a row that does not
/// recompute to its own hash. Empty when the chain is whole.
pub fn chain_breaks(rows: &[StoredRow]) -> Vec<String> {
    let mut last: BTreeMap<Uuid, String> = BTreeMap::new();
    let mut breaks = Vec::new();

    for row in rows {
        let expected = last
            .get(&row.tenant_id)
            .cloned()
            .unwrap_or_else(|| GENESIS.to_owned());

        if row.previous_hash != expected {
            breaks.push(format!(
                "tenant {}: {} links to {}, and the row before it is {}",
                row.tenant_id,
                row.label(),
                row.previous_hash,
                expected
            ));
        }

        if row.recomputed() != row.current_hash {
            breaks.push(format!(
                "tenant {}: {} does not recompute to its own hash",
                row.tenant_id,
                row.label()
            ));
        }

        last.insert(row.tenant_id, row.current_hash.clone());
    }

    breaks
}
