use uuid::Uuid;

/// Lifecycle state of a tenant (`tenants.status`, database schema §1.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TenantStatus {
    Active,
    Suspended,
    Inactive,
}

impl TenantStatus {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Active => "ACTIVE",
            Self::Suspended => "SUSPENDED",
            Self::Inactive => "INACTIVE",
        }
    }

    /// Anything unrecognised is read as `Inactive`.
    ///
    /// The column is `CHECK`-constrained, so an unknown value means the schema
    /// moved ahead of this code. Defaulting a *permission-bearing* state to the
    /// closed one keeps that skew from admitting sign-ins it should refuse.
    pub fn from_db(value: &str) -> Self {
        match value {
            "ACTIVE" => Self::Active,
            "SUSPENDED" => Self::Suspended,
            _ => Self::Inactive,
        }
    }

    /// Only an active tenant admits sign-in (FR-IDM-009).
    ///
    /// Suspension and deactivation are how an operator takes a tenant offline;
    /// if its users could still authenticate, neither would mean anything.
    pub fn admits_sign_in(self) -> bool {
        matches!(self, Self::Active)
    }
}

/// A tenant, as the resolver needs it. Not an API type — nothing here is
/// serialised to a caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tenant {
    pub id: Uuid,
    pub tenant_code: String,
    pub name: String,
    pub status: TenantStatus,
}

/// Canonical form of a tenant code.
///
/// Codes are uppercase by convention (`SYSTEM`, `TNT-001`), and a caller typing
/// `acme` means the same tenant as `ACME`. Normalising in one place is what lets
/// the lookup keep using the unique index on `tenant_code` — a case-insensitive
/// comparison in SQL would not — and is also what keeps the rate-limiter bucket
/// for a tenant from being split by capitalisation.
pub fn normalize_tenant_code(raw: &str) -> String {
    raw.trim().to_ascii_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_active_tenants_admit_sign_in() {
        assert!(TenantStatus::Active.admits_sign_in());
        assert!(!TenantStatus::Suspended.admits_sign_in());
        assert!(!TenantStatus::Inactive.admits_sign_in());
    }

    #[test]
    fn status_round_trips_through_the_database_vocabulary() {
        for status in [
            TenantStatus::Active,
            TenantStatus::Suspended,
            TenantStatus::Inactive,
        ] {
            assert_eq!(TenantStatus::from_db(status.as_db()), status);
        }
    }

    #[test]
    fn an_unrecognised_status_does_not_admit_sign_in() {
        // Schema drift must fail closed, never open.
        assert!(!TenantStatus::from_db("PROVISIONING").admits_sign_in());
        assert!(!TenantStatus::from_db("").admits_sign_in());
    }

    #[test]
    fn normalises_case_and_surrounding_space() {
        for raw in ["acme", "ACME", "  Acme  ", "\tacme\n"] {
            assert_eq!(normalize_tenant_code(raw), "ACME", "for {raw:?}");
        }
    }

    #[test]
    fn normalisation_is_idempotent() {
        // Config normalises on load and requests normalise on arrival; applying
        // it twice must not change the answer.
        let once = normalize_tenant_code(" system ");
        assert_eq!(normalize_tenant_code(&once), once);
    }
}
