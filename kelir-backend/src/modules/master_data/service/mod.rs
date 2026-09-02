//! Party use cases. Owns transactions and permission checks (coding standard
//! §2.2/§2.6): handlers call these, never the repository.
//!
//! Split into three files by #112 — the party aggregate, the roles that hang
//! off it, and the role-filtered lists — for the same reason `domain/` and
//! `repository/` are already directories, and re-exported flat here so that
//! which file a use case lives in is a question about this module's size rather
//! than about its interface (coding standard §1.5).

// The module's own layers, named here so the three files below keep saying
// `super::domain` and `super::repository` exactly as `service.rs` did before
// #112 split it — the split is not an interface change (coding standard §1.5).
pub(super) use super::{domain, repository};

pub mod audit_record;
pub mod facility;
pub mod governance;
pub mod party;
pub mod record_status;
pub mod role;
pub mod role_view;

pub use audit_record::*;
pub use facility::*;
pub use governance::*;
pub use party::*;
pub use record_status::*;
pub use role::*;
pub use role_view::*;

/// What the audit trail calls a party (naming convention §7).
const OBJECT_TYPE: &str = "PARTY";

/// The permission that opens a party.
///
/// Named rather than repeated since #136, because it is now checked from three
/// surfaces — the party's own routes, the role views, and the change history of
/// a party — and a permission string that three surfaces have to agree on
/// should not be a literal in each of them.
const PARTY_READ: &str = "master-data:party:read";

/// The permission that opens a facility, named for the same reason.
const FACILITY_READ: &str = "master-data:facility:read";

/// The permission that makes a party's roles and profiles visible.
///
/// Separate from `master-data:party:read` because the data is: a supplier
/// profile carries a bank account number and a customer profile a credit limit,
/// so seeing that a party exists and seeing what it is worth are different
/// questions. The aggregate omits both members entirely without it.
const ROLE_READ: &str = "master-data:party-role:read";
