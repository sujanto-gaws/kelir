//! What the document list may be asked for (FR-DOC-013, FR-SRH-001, [#171]).
//!
//! The shape is [`RoleViewQuery`][rvq]'s and the reasoning behind it is that
//! file's: the vocabularies arrive as strings and are parsed here, because the
//! extractor's own refusal says `unknown variant` where a person needs
//! `Must be one of DRAFT, SUBMITTED, …`, and because every bad filter is
//! collected before any is reported so a caller who got two wrong learns both
//! from one response.
//!
//! `page` and `pageSize` land in the error envelope through
//! [`crate::extract::QueryParams`], which is what [#122] is open about API-wide.
//! **This list does not close #122 and does not become a fourth instance of the
//! bare 400** — [#171]'s AC5 asks for the second, and the status report says the
//! first rather than implying otherwise.
//!
//! # What `search` means, decided before an index was chosen for it
//!
//! A case-insensitive substring of the **document number, the document ref, or
//! the title** — the three things a person has in their hand when they are
//! looking for a document they have seen before.
//!
//! **It is deliberately not a search of `form_data_json`.** That is FR-SRH-002's
//! full-text search, which is unscheduled, and a `LIKE` over a JSONB blob would
//! be a slow, silent and partial version of it: it would match a key name as
//! readily as a value, miss anything numeric, and make the real implementation
//! look like a regression when it finally arrived and returned different rows.
//!
//! [#122]: https://github.com/sujanto-gaws/kelir/issues/122
//! [#171]: https://github.com/sujanto-gaws/kelir/issues/171
//! [rvq]: crate::modules::master_data::domain::RoleViewQuery

use serde::Deserialize;
use utoipa::IntoParams;
use uuid::Uuid;

use super::document::DocumentPriority;
use super::link::EntityType;
use super::status::DocumentStatus;
use crate::error::{AppError, ValidationDetail};
use crate::response::Pagination;

/// The same bound the role view puts on its search term, and the same reason:
/// a search term is an index scan's argument, and an unbounded one is an
/// unbounded scan.
pub const MAX_SEARCH_LENGTH: usize = 100;

/// The query parameters `GET /documents` accepts.
///
/// Unknown parameters are ignored rather than refused, which is what
/// [`Pagination`] already does everywhere else in the API; `deny_unknown_fields`
/// here and nowhere else would be a difference between endpoints with no reason
/// behind it (coding standard §1.1).
#[derive(Debug, Clone, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct DocumentQuery {
    /// 1-based page number; values below 1 are treated as 1.
    pub page: Option<u32>,
    /// Rows per page, clamped to `response::MAX_PAGE_SIZE`.
    pub page_size: Option<u32>,
    /// Case-insensitive substring of the document number, the document ref or
    /// the title. `%` and `_` in it match themselves.
    pub search: Option<String>,
    pub document_type_id: Option<Uuid>,
    /// `DRAFT`, `SUBMITTED`, `IN_REVIEW`, `PENDING_APPROVAL`, `APPROVED`,
    /// `REJECTED`, `RETURNED`, `COMPLETED`, `ARCHIVED` or `CANCELLED`.
    pub status: Option<String>,
    pub priority: Option<String>,
    /// `PARTY` or `FACILITY`. Filtering on an entity requires both halves, for
    /// the reason [`super::link`] gives about the pair.
    pub entity_type: Option<String>,
    pub entity_id: Option<Uuid>,
}

impl DocumentQuery {
    /// The paging half, so clamping and the 1-based page live in one place
    /// (`response::Pagination`) rather than being restated per endpoint.
    pub fn pagination(&self) -> Pagination {
        Pagination {
            page: self.page,
            page_size: self.page_size,
        }
    }

    /// The filtering half, parsed and bounded, or a 422 naming what was wrong.
    pub fn filters(&self) -> Result<DocumentFilters, AppError> {
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
            self.status.as_deref(),
            "status",
            &[
                ("DRAFT", DocumentStatus::Draft),
                ("SUBMITTED", DocumentStatus::Submitted),
                ("IN_REVIEW", DocumentStatus::InReview),
                ("PENDING_APPROVAL", DocumentStatus::PendingApproval),
                ("APPROVED", DocumentStatus::Approved),
                ("REJECTED", DocumentStatus::Rejected),
                ("RETURNED", DocumentStatus::Returned),
                ("COMPLETED", DocumentStatus::Completed),
                ("ARCHIVED", DocumentStatus::Archived),
                ("CANCELLED", DocumentStatus::Cancelled),
            ],
            &mut details,
        );

        let priority = parse_choice(
            self.priority.as_deref(),
            "priority",
            &[
                ("LOW", DocumentPriority::Low),
                ("NORMAL", DocumentPriority::Normal),
                ("HIGH", DocumentPriority::High),
                ("URGENT", DocumentPriority::Urgent),
            ],
            &mut details,
        );

        let entity_type = parse_choice(
            self.entity_type.as_deref(),
            "entityType",
            &[
                ("PARTY", EntityType::Party),
                ("FACILITY", EntityType::Facility),
            ],
            &mut details,
        );

        // The pair rule, on the filter as well as on the write. An `entityId`
        // alone would silently match documents linked to a *facility* with that
        // id as well as a party — which is the ambiguity #170 AC1 exists to
        // forbid, and a filter that quietly returns the wrong rows is worse
        // than one that refuses.
        match (entity_type, self.entity_id) {
            (Some(_), None) => details.push(pair_detail(
                "entityId",
                "filtering by entityType alone would return every document linked to \
                 any record of that kind, which is not a filter anybody asked for",
            )),
            (None, Some(_)) => details.push(pair_detail(
                "entityType",
                "an entityId alone could mean a party or a facility, and the two are \
                 different records",
            )),
            _ => {}
        }

        if details.is_empty() {
            Ok(DocumentFilters {
                search,
                document_type_id: self.document_type_id,
                status,
                priority,
                entity_type,
                entity_id: self.entity_id,
                // Never from the request. `DocumentQuery` has no `listId`, and
                // the rendered-list path sets this itself from the list the
                // caller opened.
                list_id: None,
            })
        } else {
            Err(AppError::validation(details))
        }
    }
}

fn pair_detail(path: &str, message: &str) -> ValidationDetail {
    ValidationDetail::new(path, "required", "INCOMPLETE_ENTITY_FILTER", message)
}

/// The parsed filters, ready for the repository to bind.
///
/// `None` in every member means *do not filter on this*, which is the shape the
/// SQL expects: each predicate is written so that a null parameter matches every
/// row, keeping one static statement rather than a query assembled from strings
/// (coding standard §2.5).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocumentFilters {
    pub search: Option<String>,
    pub document_type_id: Option<Uuid>,
    /// Narrows to the documents of every type that names this list
    /// (`document_types.list_id`), which is what a *rendered* list is over
    /// ([#340](https://github.com/sujanto-gaws/kelir/issues/340)).
    ///
    /// **Not a query parameter, and that is the point.** Nothing parses it out
    /// of a request: `GET /documents` cannot set it, and the rendered-list path
    /// sets it from the list the caller opened. A client-settable `listId`
    /// would be a second way to select rows that the list definition was
    /// supposed to decide.
    pub list_id: Option<Uuid>,
    pub status: Option<DocumentStatus>,
    pub priority: Option<DocumentPriority>,
    pub entity_type: Option<EntityType>,
    pub entity_id: Option<Uuid>,
}

/// Parses one query parameter against the values it is allowed to take.
///
/// Unrecognised values are refused rather than ignored. The leniency
/// [`DocumentStatus::from_db`] uses is for values already in the database, where
/// a `CHECK` has vouched for them; applied here it would turn `?status=DRAFTED`
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
            details.push(ValidationDetail::new(
                parameter,
                "enum",
                "UNKNOWN_VALUE",
                format!(
                    "Must be one of {}",
                    allowed
                        .iter()
                        .map(|(code, _)| *code)
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            ));
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query() -> DocumentQuery {
        DocumentQuery::default()
    }

    #[test]
    fn no_parameters_filters_on_nothing() {
        assert_eq!(
            query().filters().expect("an empty query is legitimate"),
            DocumentFilters::default()
        );
    }

    #[test]
    fn an_unknown_status_is_refused_rather_than_ignored() {
        // Ignoring it would answer the whole population to a caller who asked
        // for one slice of it, and they would read that as the answer.
        let mut parameters = query();
        parameters.status = Some("DRAFTED".to_owned());

        let error = parameters.filters().expect_err("DRAFTED is not a status");
        let AppError::Validation { details } = error else {
            panic!("expected a validation failure");
        };

        assert_eq!(details[0].path, "status");
        assert!(details[0].message.contains("DRAFT"), "{details:?}");
    }

    #[test]
    fn every_bad_parameter_is_reported_from_one_request() {
        let mut parameters = query();
        parameters.status = Some("NOPE".to_owned());
        parameters.priority = Some("ALSO_NOPE".to_owned());
        parameters.search = Some("x".repeat(MAX_SEARCH_LENGTH + 1));

        let error = parameters
            .filters()
            .expect_err("three parameters are wrong");
        let AppError::Validation { details } = error else {
            panic!("expected a validation failure");
        };

        assert_eq!(
            details.len(),
            3,
            "a caller who got three wrong learns one at a time: {details:?}"
        );
    }

    #[test]
    fn an_entity_filter_needs_both_halves() {
        // #170 AC1 on the read side. `entityId` alone would match a party and a
        // facility that happened to share an id, and a filter that quietly
        // returns the wrong rows is worse than one that refuses.
        let mut only_id = query();
        only_id.entity_id = Some(Uuid::now_v7());
        assert!(only_id.filters().is_err());

        let mut only_type = query();
        only_type.entity_type = Some("PARTY".to_owned());
        assert!(only_type.filters().is_err());

        let mut both = query();
        both.entity_type = Some("PARTY".to_owned());
        both.entity_id = Some(Uuid::now_v7());
        assert!(both.filters().is_ok());
    }

    #[test]
    fn a_blank_search_is_not_a_filter() {
        // `?search=` from a form whose box was cleared must not become a search
        // for the empty string, which matches everything and looks like it
        // worked.
        let mut parameters = query();
        parameters.search = Some("   ".to_owned());

        assert_eq!(
            parameters
                .filters()
                .expect("a blank search is legitimate")
                .search,
            None
        );
    }

    #[test]
    fn every_status_the_column_allows_can_be_filtered_on() {
        // A status a document can be in and a caller cannot filter for is a
        // slice of the population nobody can see. Discovered rather than
        // listed, which is the Sprint 6 retrospective's rule about a test that
        // asserts a project-wide property.
        for status in [
            DocumentStatus::Draft,
            DocumentStatus::Submitted,
            DocumentStatus::InReview,
            DocumentStatus::PendingApproval,
            DocumentStatus::Approved,
            DocumentStatus::Rejected,
            DocumentStatus::Returned,
            DocumentStatus::Completed,
            DocumentStatus::Archived,
            DocumentStatus::Cancelled,
        ] {
            let mut parameters = query();
            parameters.status = Some(status.as_db().to_owned());

            assert_eq!(
                parameters
                    .filters()
                    .unwrap_or_else(|_| panic!("{} is not filterable", status.as_db()))
                    .status,
                Some(status)
            );
        }
    }
}
