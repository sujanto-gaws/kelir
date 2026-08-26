//! The role views — the parties holding one role, as a list (FR-MDM-002,
//! FR-MDM-008).
//!
//! `/master-data/suppliers`, `/customers` and `/employees` are projections over
//! the party surface rather than entities of their own: a supplier *is* a party
//! holding the SUPPLIER role (Database Schema §14 deviation #1), so these read
//! the same tables the aggregate does and add the one thing a list of suppliers
//! has to show that a list of parties does not — the supplier number.
//!
//! **One row shape across all three.** A caller reading `/suppliers` gets
//! `roleTypeId: "SUPPLIER"` and `roleNumber` rather than a `supplierNumber`
//! field, and `/customers` differs only in the values. The alternative — three
//! row types differing in one key — would make the client that renders all
//! three (#101) three components instead of one, to say nothing new.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use super::{PartyRoleStatus, PartyStatusCode, PartyType};
use crate::error::{AppError, ValidationDetail};
use crate::response::Pagination;

/// Longest `search` this accepts, past which the request is a 422 rather than a
/// scan. Matches the bound on the columns it searches (§4, `VARCHAR(200)`):
/// nothing longer can match anything, so accepting it would only be work.
pub const MAX_SEARCH_LENGTH: usize = 200;

/// The three role views this module serves.
///
/// An enum rather than a string taken from the path: the role type code reaches
/// SQL, and coding standard §2.5 requires anything dynamic that lands in a
/// query to resolve through an allow-list. There is no route that turns caller
/// input into a role type here — `/suppliers` is `Supplier` by construction.
///
/// CONTACT is the fourth profiled role (`PROFILED_ROLE_TYPES`) and has no view:
/// a contact profile carries no number, so a list of contacts would be a list
/// of parties with a filter on it, which is the thing this issue set out not to
/// ship.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoleView {
    Supplier,
    Customer,
    Employee,
}

impl RoleView {
    /// The `mdm_role_types.role_type_code` this view is over.
    pub fn role_type_code(self) -> &'static str {
        match self {
            Self::Supplier => "SUPPLIER",
            Self::Customer => "CUSTOMER",
            Self::Employee => "EMPLOYEE",
        }
    }
}

/// One row of a role view: the party summary, plus what the role makes it.
///
/// `roleNumber` is the supplier number, the customer number or the employee
/// number, depending on which view produced the row. It is `null` when the
/// party holds the role but carries no profile — an assignment without one is
/// legal (`AssignRoleRequest.profile` is optional), and hiding such a party
/// would make the list disagree with the role it claims to list.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RoleViewRow {
    pub id: Uuid,
    pub party_id: String,
    pub party_type_id: PartyType,
    pub status_id: PartyStatusCode,
    pub name: String,
    pub external_id: Option<String>,
    /// `SUPPLIER`, `CUSTOMER` or `EMPLOYEE` — which view this row came from, so
    /// one client component can render all three.
    pub role_type_id: String,
    /// The supplier / customer / employee number, or `null` when the party
    /// holds the role without a profile.
    pub role_number: Option<String>,
    pub role_status_id: PartyRoleStatus,
    pub from_date: DateTime<Utc>,
    pub thru_date: Option<DateTime<Utc>>,
    pub created_stamp: DateTime<Utc>,
    pub last_updated_stamp: DateTime<Utc>,
}

/// Query parameters accepted by all three role views (FR-MDM-008).
///
/// The vocabularies arrive as strings rather than as the enums they name, and
/// are parsed by [`RoleViewQuery::filters`].
///
/// **The original reason for that is gone; a smaller one remains.** Hand-parsing
/// was introduced because deserializing the enums directly would hand an
/// unrecognised value to Axum's `Query` rejection, which answered 400 in plain
/// text — outside the error envelope every other refusal in this API uses
/// (naming convention §5). That comment then claimed a mistyped filter was a 422
/// "like every other bad input", which was not true of `page` and `pageSize` in
/// this same struct: they were `Option<u32>` and got the bare 400 (#122). The
/// extractor is now [`crate::extract::QueryParams`], every parameter here lands
/// in the envelope, and the two families answer alike.
///
/// What hand-parsing still buys is the *content* of the refusal, which the
/// extractor cannot produce: `Must be one of PARTY_ENABLED, PARTY_DISABLED`
/// instead of serde's `unknown variant`, and every bad filter collected before
/// any is reported, so a caller who got two wrong learns both from one response.
///
/// Unknown parameters are ignored rather than refused, which is what
/// [`Pagination`] already does everywhere else in the API; `deny_unknown_fields`
/// here and nowhere else would be a difference between endpoints with no reason
/// behind it (coding standard §1.1).
#[derive(Debug, Clone, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct RoleViewQuery {
    /// 1-based page number; values below 1 are treated as 1.
    pub page: Option<u32>,
    /// Rows per page, clamped to `response::MAX_PAGE_SIZE`.
    pub page_size: Option<u32>,
    /// Case-insensitive substring of the party code, the party name, or the
    /// role number. `%` and `_` in it match themselves.
    pub search: Option<String>,
    /// `PARTY_ENABLED` or `PARTY_DISABLED` — the party's own status.
    pub status_id: Option<String>,
    /// `PERSON` or `PARTY_GROUP`.
    pub party_type_id: Option<String>,
    /// `ACTIVE` or `INACTIVE` — the status of the role assignment, which is not
    /// the same as its removal. A removed role leaves the view entirely.
    pub role_status_id: Option<String>,
}

impl RoleViewQuery {
    /// The paging half, so clamping and the 1-based page live in one place
    /// (`response::Pagination`) rather than being restated per endpoint.
    pub fn pagination(&self) -> Pagination {
        Pagination {
            page: self.page,
            page_size: self.page_size,
        }
    }

    /// The filtering half, parsed and bounded, or a 422 naming what was wrong.
    ///
    /// Every unrecognised value is collected before any is reported, so a
    /// caller who got two parameters wrong learns both from one response.
    pub fn filters(&self) -> Result<RoleViewFilters, AppError> {
        let mut details = Vec::new();

        let search = self
            .search
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);

        if search
            .as_deref()
            .is_some_and(|value| value.chars().count() > MAX_SEARCH_LENGTH)
        {
            details.push(ValidationDetail::new(
                "search",
                "maxLength",
                "TOO_LONG",
                format!("Must be at most {MAX_SEARCH_LENGTH} characters"),
            ));
        }

        let status = parse_choice(
            self.status_id.as_deref(),
            "statusId",
            &[
                ("PARTY_ENABLED", PartyStatusCode::PartyEnabled),
                ("PARTY_DISABLED", PartyStatusCode::PartyDisabled),
            ],
            &mut details,
        );
        let party_type = parse_choice(
            self.party_type_id.as_deref(),
            "partyTypeId",
            &[
                ("PERSON", PartyType::Person),
                ("PARTY_GROUP", PartyType::PartyGroup),
            ],
            &mut details,
        );
        let role_status = parse_choice(
            self.role_status_id.as_deref(),
            "roleStatusId",
            &[
                ("ACTIVE", PartyRoleStatus::Active),
                ("INACTIVE", PartyRoleStatus::Inactive),
            ],
            &mut details,
        );

        if details.is_empty() {
            Ok(RoleViewFilters {
                search,
                status,
                party_type,
                role_status,
            })
        } else {
            Err(AppError::validation(details))
        }
    }
}

/// The parsed filters, ready for the repository to bind.
///
/// `None` in every member means *do not filter on this*, which is the shape the
/// SQL expects: each predicate is written so that a null parameter matches
/// every row, keeping one static statement rather than a query assembled from
/// strings (coding standard §2.5).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RoleViewFilters {
    pub search: Option<String>,
    pub status: Option<PartyStatusCode>,
    pub party_type: Option<PartyType>,
    pub role_status: Option<PartyRoleStatus>,
}

/// Parses one query parameter against the values it is allowed to take.
///
/// Unrecognised values are refused rather than ignored. The leniency
/// `PartyStatusCode::from_db` uses is for values already in the database, where
/// a `CHECK` has vouched for them; applied here it would turn `?statusId=ENABLED`
/// into an unfiltered list, and the caller would read the whole population as
/// the answer to their filter.
fn parse_choice<T: Copy>(
    value: Option<&str>,
    parameter: &str,
    allowed: &[(&str, T)],
    details: &mut Vec<ValidationDetail>,
) -> Option<T> {
    let value = value.map(str::trim).filter(|value| !value.is_empty())?;

    match allowed.iter().find(|(code, _)| *code == value) {
        Some((_, parsed)) => Some(*parsed),
        None => {
            let codes: Vec<&str> = allowed.iter().map(|(code, _)| *code).collect();
            details.push(ValidationDetail::new(
                parameter,
                "enum",
                "INVALID_VALUE",
                format!("Must be one of {}", codes.join(", ")),
            ));
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(parameter: &str, value: &str) -> RoleViewQuery {
        let mut query = RoleViewQuery::default();

        match parameter {
            "search" => query.search = Some(value.to_owned()),
            "statusId" => query.status_id = Some(value.to_owned()),
            "partyTypeId" => query.party_type_id = Some(value.to_owned()),
            "roleStatusId" => query.role_status_id = Some(value.to_owned()),
            other => panic!("no such parameter: {other}"),
        }

        query
    }

    #[test]
    fn a_view_names_the_role_type_it_lists() {
        assert_eq!(RoleView::Supplier.role_type_code(), "SUPPLIER");
        assert_eq!(RoleView::Customer.role_type_code(), "CUSTOMER");
        assert_eq!(RoleView::Employee.role_type_code(), "EMPLOYEE");
    }

    #[test]
    fn an_empty_query_filters_on_nothing() {
        assert_eq!(
            RoleViewQuery::default()
                .filters()
                .expect("no filters parse"),
            RoleViewFilters::default()
        );
    }

    #[test]
    fn parses_every_vocabulary_it_documents() {
        let filters = RoleViewQuery {
            status_id: Some("PARTY_DISABLED".into()),
            party_type_id: Some("PERSON".into()),
            role_status_id: Some("INACTIVE".into()),
            ..RoleViewQuery::default()
        }
        .filters()
        .expect("the documented values parse");

        assert_eq!(filters.status, Some(PartyStatusCode::PartyDisabled));
        assert_eq!(filters.party_type, Some(PartyType::Person));
        assert_eq!(filters.role_status, Some(PartyRoleStatus::Inactive));
    }

    #[test]
    fn refuses_a_value_outside_the_vocabulary_rather_than_ignoring_it() {
        // The failure this is here for: a filter that silently does nothing
        // returns the whole population, and the caller reads it as the answer.
        for (parameter, value) in [
            ("statusId", "ENABLED"),
            ("partyTypeId", "ORGANISATION"),
            ("roleStatusId", "REMOVED"),
        ] {
            let error = query(parameter, value)
                .filters()
                .expect_err("a value outside the vocabulary must be refused");

            assert!(
                matches!(error, AppError::Validation { ref details }
                    if details.iter().any(|detail| detail.path == parameter)),
                "{parameter}={value} was refused without naming the parameter"
            );
        }
    }

    #[test]
    fn reports_every_bad_parameter_at_once() {
        let error = RoleViewQuery {
            status_id: Some("nope".into()),
            party_type_id: Some("nope".into()),
            ..RoleViewQuery::default()
        }
        .filters()
        .expect_err("both are refused");

        let AppError::Validation { details } = error else {
            panic!("expected a validation failure");
        };

        assert_eq!(details.len(), 2, "{details:?}");
    }

    #[test]
    fn treats_a_blank_search_as_no_search() {
        // `?search=` is what a UI sends when its box is empty, and it means
        // "everything" rather than "rows whose code contains nothing".
        assert_eq!(query("search", "   ").filters().unwrap().search, None);
    }

    #[test]
    fn trims_a_search_before_using_it() {
        assert_eq!(
            query("search", "  ACME ").filters().unwrap().search,
            Some("ACME".to_owned())
        );
    }

    #[test]
    fn refuses_a_search_longer_than_anything_it_could_match() {
        let error = query("search", &"a".repeat(MAX_SEARCH_LENGTH + 1))
            .filters()
            .expect_err("an over-long search is refused");

        assert!(matches!(error, AppError::Validation { .. }));
    }

    #[test]
    fn counts_a_search_in_characters_rather_than_bytes() {
        // Three bytes each. Counting bytes would refuse a legal search.
        let search = "た".repeat(MAX_SEARCH_LENGTH);

        assert!(query("search", &search).filters().is_ok());
    }

    #[test]
    fn carries_the_paging_parameters_through_untouched() {
        let query = RoleViewQuery {
            page: Some(3),
            page_size: Some(5_000),
            ..RoleViewQuery::default()
        };

        // Clamping belongs to `Pagination`; this only checks the hand-off.
        assert_eq!(query.pagination().page(), 3);
        assert_eq!(
            query.pagination().page_size(),
            crate::response::MAX_PAGE_SIZE
        );
    }
}
