//! Lookup fields — the one component whose options come from the database
//! rather than from the definition (FR-RAD-007, [#161]).
//!
//! Every other component is a function of the definition it is declared in. A
//! lookup is a function of master data, which is what makes it the component
//! that has to answer a question none of the others do: **a lookup must not
//! become a way to read master data the caller could not read directly.**
//!
//! The answer this module implements is that a lookup checks no permission of
//! its own. It asks the master-data module for a page of the same list the
//! master-data endpoint serves, and that module refuses first — so what a lookup
//! opens is exactly what `GET /master-data/suppliers` and
//! `GET /master-data/facilities` open, and the two cannot drift apart because
//! there is only one check. See [`super::super::service::lookup`] for the rest
//! of it, including why a caller without the permission is refused rather than
//! handed an empty list.
//!
//! [#161]: https://github.com/sujanto-gaws/kelir/issues/161

use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::error::{AppError, ValidationDetail};
use crate::response::Pagination;

/// Longest `search` a lookup accepts, past which the request is a 422 rather
/// than a scan.
///
/// The same bound and the same reason as the role views': it matches the
/// `VARCHAR(200)` name columns it searches (Database Schema §4), so nothing
/// longer can match anything and accepting it would only be work.
pub const MAX_SEARCH_LENGTH: usize = 200;

/// The master-data sources a form definition may bind a lookup to.
///
/// **An enum rather than a key taken from configuration, and that is the
/// security decision of this module.** A lookup source decides two things: which
/// query runs, and which permission the caller must hold for it. Taking either
/// from a row in `rad_lookup_definitions` would mean an identifier reaching SQL
/// from data — which coding standard §2.5 requires an allow-list for — and,
/// worse, a permission chosen by the same row: a misconfigured lookup would then
/// be a permission bypass that reads as a typo. `RoleView` is the same shape for
/// the same reason.
///
/// So `rad_lookup_definitions` stays without an endpoint, as `0014_rad.sql` left
/// it. Its `source_type` vocabulary — `ENTITY`, `ENUM`, `API`, `STATIC` — is a
/// configurable-lookup feature, and configurable lookups need an answer to
/// "which permission does this row require" that a row cannot give.
///
/// The four below are the master data a document actually points at. A source is
/// here when the master-data module can serve it *searchably*: a lookup that
/// returns every party is a lookup that stops working at the size where it
/// matters, so a source whose only list takes a page number and no filter is not
/// yet a source. That is why there is no bare `party` — adding one means giving
/// `list_parties` a search first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LookupSource {
    Supplier,
    Customer,
    Employee,
    Facility,
}

/// Every source, in the order a form author is shown them.
pub const SOURCES: [LookupSource; 4] = [
    LookupSource::Customer,
    LookupSource::Employee,
    LookupSource::Facility,
    LookupSource::Supplier,
];

impl LookupSource {
    /// The key a form definition names, and the path segment the API takes.
    pub fn key(self) -> &'static str {
        match self {
            Self::Supplier => "supplier",
            Self::Customer => "customer",
            Self::Employee => "employee",
            Self::Facility => "facility",
        }
    }

    /// The source a key names, or `None` — which is a 404 at the API and a 422
    /// at save, because those are different failures: one is a URL nobody
    /// serves, the other is a definition that would render a field with nothing
    /// behind it.
    pub fn from_key(key: &str) -> Option<Self> {
        SOURCES.into_iter().find(|source| source.key() == key)
    }

    /// Every key, for a refusal that tells the author what they may write.
    ///
    /// A validator that says "not a lookup source" and stops leaves them
    /// guessing at a vocabulary that exists in one place in the codebase.
    pub fn keys() -> String {
        SOURCES
            .into_iter()
            .map(Self::key)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// One option a lookup offers.
///
/// **`value` and `label` are JFSS's option shape** (§4.2, `$defs/options`), so a
/// renderer binds these to a chooser without translating them. `value` is the
/// record's id: it is what a document stores to point at master data, and it is
/// how every other route in this API addresses the same record.
///
/// **`description` is not part of that shape, deliberately.** It carries the
/// business identifier — a supplier number, a facility code — because two
/// suppliers may share a name and a chooser that cannot tell them apart is not a
/// chooser. It is safe to add precisely because a lookup resolves at render time
/// and is never written back into a stored `options` array: the meta-schema's
/// `additionalProperties: false` on an option object would refuse it there, and
/// that refusal is what keeps the two shapes apart rather than a convention.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LookupOption {
    /// The record's id — what a document stores to reference it.
    pub value: String,
    /// What a person calls the record.
    pub label: String,
    /// The business identifier, where the record carries one.
    pub description: Option<String>,
}

/// What a caller may ask a lookup for (#161 AC4).
///
/// Paging and a search, and nothing else. The filters that decide *which*
/// records a form may offer at all — an enabled party, a role it currently holds
/// — are the server's and are applied in `master_data::service`, not here: a
/// renderer that had to remember them would be a renderer that could forget
/// them.
///
/// Unknown parameters are ignored rather than refused, which is what
/// [`Pagination`] does everywhere else in this API.
#[derive(Debug, Clone, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct LookupQuery {
    /// 1-based page number; values below 1 are treated as 1.
    pub page: Option<u32>,
    /// Rows per page, clamped to `response::MAX_PAGE_SIZE`.
    pub page_size: Option<u32>,
    /// Case-insensitive substring of the record's name or its business
    /// identifier. `%` and `_` in it match themselves.
    pub search: Option<String>,
}

impl LookupQuery {
    /// The paging half, so clamping and the 1-based page stay in one place.
    pub fn pagination(&self) -> Pagination {
        Pagination {
            page: self.page,
            page_size: self.page_size,
        }
    }

    /// The search, trimmed and bounded, or a 422 saying why not.
    ///
    /// A blank search means *everything*, which is what a chooser sends when its
    /// box is empty — not "records whose name contains nothing".
    pub fn search(&self) -> Result<Option<String>, AppError> {
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
            return Err(AppError::validation(vec![ValidationDetail::new(
                "search",
                "maxLength",
                "TOO_LONG",
                format!("Must be at most {MAX_SEARCH_LENGTH} characters"),
            )]));
        }

        Ok(search)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_search(value: &str) -> LookupQuery {
        LookupQuery {
            search: Some(value.to_owned()),
            ..LookupQuery::default()
        }
    }

    #[test]
    fn every_source_round_trips_through_its_key() {
        for source in SOURCES {
            assert_eq!(LookupSource::from_key(source.key()), Some(source));
        }
    }

    #[test]
    fn refuses_a_key_outside_the_allow_list() {
        // The check that keeps an identifier out of a query. `parties` is the
        // near miss worth naming: it is a real table and not a source.
        for key in ["parties", "party", "SUPPLIER", "", "mdm_parties"] {
            assert_eq!(LookupSource::from_key(key), None, "{key} must not resolve");
        }
    }

    #[test]
    fn names_every_source_when_it_refuses_one() {
        let keys = LookupSource::keys();

        for source in SOURCES {
            assert!(keys.contains(source.key()), "{keys} omits {}", source.key());
        }
    }

    #[test]
    fn treats_a_blank_search_as_no_search() {
        assert_eq!(with_search("   ").search().expect("blank parses"), None);
    }

    #[test]
    fn trims_a_search_before_using_it() {
        assert_eq!(
            with_search("  ACME ").search().expect("parses"),
            Some("ACME".to_owned())
        );
    }

    #[test]
    fn refuses_a_search_longer_than_anything_it_could_match() {
        let error = with_search(&"a".repeat(MAX_SEARCH_LENGTH + 1))
            .search()
            .expect_err("an over-long search is refused");

        assert!(matches!(error, AppError::Validation { .. }));
    }

    #[test]
    fn counts_a_search_in_characters_rather_than_bytes() {
        // Three bytes each. Counting bytes would refuse a legal search.
        assert!(with_search(&"た".repeat(MAX_SEARCH_LENGTH))
            .search()
            .is_ok());
    }

    #[test]
    fn carries_the_paging_parameters_through_untouched() {
        let query = LookupQuery {
            page: Some(3),
            page_size: Some(5_000),
            ..LookupQuery::default()
        };

        assert_eq!(query.pagination().page(), 3);
        assert_eq!(
            query.pagination().page_size(),
            crate::response::MAX_PAGE_SIZE
        );
    }
}
