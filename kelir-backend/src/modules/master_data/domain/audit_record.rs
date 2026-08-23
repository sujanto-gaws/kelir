//! Reading back what happened to a master-data record (FR-MDM-009).
//!
//! The *write* path has existed since #80's first endpoint: every party create,
//! update, delete, role assignment, role removal, facility change and lifecycle
//! transition is already hash-chained into `audit_events`. This is the surface
//! that reads it, and it is the one requirement in the epic that is about being
//! able to answer a question later rather than about storing something.
//!
//! # Two things this deliberately does not carry
//!
//! **`previousHash` and `currentHash` are absent.** They make tampering
//! detectable, and nothing verifies them yet — chain verification is FR-AUD-003
//! in Phase 6. Publishing the columns would let a client display "verified"
//! beside a chain that nobody checked, which is worse than not showing it: a
//! control that appears to exist is harder to notice missing than one that
//! plainly does not (#100 AC7).
//!
//! **Nothing the aggregate withholds.** #81 keeps `roles` and `profiles` from a
//! caller without `master-data:party-role:read`, and a role assignment's audit
//! record names the role type — so `master-data:audit:read` alone would put
//! *this party is a supplier* one URL away from a permission that refuses it.
//! [`AuditRecord::is_role_change`] is what the service filters on.

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

/// One thing that happened to a master-data record.
///
/// `oldValue` and `newValue` are what make the record answer a question rather
/// than announce an event: "the credit limit was raised" needs both ends.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuditRecord {
    pub id: Uuid,
    /// `Party.Created`, `Supplier.Updated`, `Facility.RecordStatusChanged` …
    /// The business subject, not the table (naming convention §7).
    pub event_type: String,
    /// `CREATE`, `UPDATE`, `DELETE`, `STATUS_CHANGE`, `ROLE_ASSIGNED`,
    /// `RECORD_STATUS_CHANGE` …
    pub action: String,
    pub occurred_at: DateTime<Utc>,
    /// Who did it. `null` for a change no user is behind — a migration, or a
    /// system process.
    pub actor_user_id: Option<Uuid>,
    /// The username at the time of reading, so a client does not have to
    /// resolve every actor itself. `null` when the user has since been removed.
    pub actor_username: Option<String>,
    pub reason: Option<String>,
    pub old_value: Option<Value>,
    pub new_value: Option<Value>,
}

impl AuditRecord {
    /// Whether this record is about a role, and so gated behind
    /// `master-data:party-role:read`.
    ///
    /// Matched on `action` rather than on `event_type`. The event type is the
    /// business subject and varies per role — `Supplier.Created`,
    /// `Customer.Updated`, and whatever a tenant's own role type produces,
    /// since role types are rows and not a migration (#81 AC4). The action
    /// vocabulary is fixed by this module and is the thing that can be
    /// enumerated.
    pub fn is_role_change(&self) -> bool {
        matches!(
            self.action.as_str(),
            "ROLE_ASSIGNED" | "ROLE_UPDATED" | "ROLE_REMOVED"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(action: &str) -> AuditRecord {
        AuditRecord {
            id: Uuid::now_v7(),
            event_type: "Supplier.Created".to_owned(),
            action: action.to_owned(),
            occurred_at: DateTime::<Utc>::from_timestamp(0, 0).expect("epoch"),
            actor_user_id: None,
            actor_username: None,
            reason: None,
            old_value: None,
            new_value: None,
        }
    }

    #[test]
    fn every_role_action_is_recognised_as_one() {
        for action in ["ROLE_ASSIGNED", "ROLE_UPDATED", "ROLE_REMOVED"] {
            assert!(
                record(action).is_role_change(),
                "{action} was not treated as a role change"
            );
        }
    }

    #[test]
    fn a_party_change_is_not_a_role_change() {
        // The gate must not swallow the records it is not for: a caller without
        // `master-data:party-role:read` still sees what happened to the party.
        for action in [
            "CREATE",
            "UPDATE",
            "DELETE",
            "STATUS_CHANGE",
            "RECORD_STATUS_CHANGE",
        ] {
            assert!(
                !record(action).is_role_change(),
                "{action} was withheld as if it were a role change"
            );
        }
    }
}
