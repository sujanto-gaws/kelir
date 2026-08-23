//! Party domain types — the `PartyAggregate` of [architectures/05] in Rust.
//!
//! The aggregate is the API payload shape; Database Schema §4 is its storage.
//! Three things about the mapping are decisions rather than transcriptions, and
//! each is stated where it applies below:
//!
//! * **`id` is added to the response.** The aggregate is
//!   `additionalProperties: false` and carries no surrogate key — its `partyId`
//!   is the business code. Every route addresses a party by `{id}` (naming
//!   convention §5), so a client that had only seen `partyId` could not build a
//!   URL. Nothing else is added: `recordStatus` and the two document references
//!   exist in the schema (AC5) and stay off the wire until Phase 5 gives them a
//!   consumer, because a field nothing can change reads as a control that
//!   exists.
//! * **`roles` and `profiles` are present but permissioned.** They carry bank
//!   accounts and credit limits, so seeing that a party exists and seeing what
//!   it is worth are different permissions: both members are omitted entirely
//!   for a caller who holds `master-data:party:read` without
//!   `master-data:party-role:read`. Absent means *not visible to you*; `[]`
//!   means *this party holds no roles*.
//! * **`notes` and `tenantPartyId` are absent.** Neither has storage in §4 at
//!   all — recorded as deviation #16 there.
//! * **Request types carry only writable fields.** A `GET` body posted back
//!   into a `PUT` is refused with a 422 naming the extra field, which is what
//!   `deny_unknown_fields` is for (#62): a request struct that quietly accepted
//!   and ignored `createdStamp` is the shape of defect that change closed.
//!
//! [architectures/05]: ../../../../../docs/architectures/05.%20Core%20-%20Master%20Data%20-%20Party.md

pub mod facility;
pub mod party;
pub mod party_validation;
pub mod record_status;
pub mod role;
pub mod role_view;

// Re-exported flat, so the rest of the module keeps addressing these as
// `domain::PartyAggregate` — where the type is declared is a question about this
// file's size, not about the module's interface.
pub use facility::*;
pub use party::*;
pub use party_validation::*;
pub use record_status::*;
pub use role::*;
pub use role_view::*;

use crate::error::{AppError, ValidationDetail};

/// Longest `partyId` the aggregate allows, and the bound
/// `ck_mdm_parties_party_code_len` holds in the database.
pub const MAX_PARTY_CODE_LENGTH: usize = 60;

/// Longest value for the code columns of the child tables — `VARCHAR(64)` in
/// §4, so a longer value is a 422 rather than a database error.
pub const MAX_CODE_LENGTH: usize = 64;

/// Longest human name or title — `VARCHAR(200)` in §4.
pub const MAX_NAME_LENGTH: usize = 200;

// ---------------------------------------------------------------------------
// Validation primitives shared by the party and its roles
// ---------------------------------------------------------------------------

/// The aggregate repeats `partyId` inside `person` and `partyGroup`. A value
/// that disagrees with the party's own code is a mistake worth naming rather
/// than a field to ignore.
pub(super) fn echoed_party_id(
    echoed: Option<&str>,
    party_code: &str,
    path: &str,
    details: &mut Vec<ValidationDetail>,
) {
    if let Some(value) = non_empty(echoed) {
        if value != party_code.trim() {
            details.push(ValidationDetail::new(
                format!("{path}.partyId"),
                "consistency",
                "MISMATCH",
                "partyId must match the party it belongs to",
            ));
        }
    }
}

pub(super) fn require_code(
    value: &str,
    path: &str,
    max: usize,
    details: &mut Vec<ValidationDetail>,
) {
    let trimmed = value.trim();

    if trimmed.is_empty() {
        details.push(ValidationDetail::new(
            path,
            "required",
            "REQUIRED",
            "This field is required",
        ));
    } else if trimmed.chars().count() > max {
        details.push(ValidationDetail::new(
            path,
            "maxLength",
            "TOO_LONG",
            format!("Must be at most {max} characters"),
        ));
    }
}

pub(super) fn require_name(value: Option<&str>, path: &str, details: &mut Vec<ValidationDetail>) {
    match non_empty(value) {
        None => details.push(ValidationDetail::new(
            path,
            "required",
            "REQUIRED",
            "This field is required",
        )),
        Some(name) => bound_name(Some(name), path, details),
    }
}

pub(super) fn bound_name(value: Option<&str>, path: &str, details: &mut Vec<ValidationDetail>) {
    if let Some(name) = non_empty(value) {
        if name.chars().count() > MAX_NAME_LENGTH {
            details.push(ValidationDetail::new(
                path,
                "maxLength",
                "TOO_LONG",
                format!("Must be at most {MAX_NAME_LENGTH} characters"),
            ));
        }
    }
}

pub(super) fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

pub(super) fn finish(details: Vec<ValidationDetail>) -> Result<(), AppError> {
    if details.is_empty() {
        Ok(())
    } else {
        Err(AppError::validation(details))
    }
}
