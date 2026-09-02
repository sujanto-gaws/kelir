//! What the task inbox may be asked for ([#179] AC2, AC5).
//!
//! The shape is [`DocumentQuery`][dq]'s and the reasoning is that file's: the
//! vocabularies arrive as strings and are parsed here, because the extractor's
//! own refusal says `unknown variant` where a person needs
//! `Must be one of open, all`.
//!
//! `page` and `pageSize` land in the error envelope through
//! [`crate::extract::QueryParams`], which is what [#122] is open about API-wide.
//! **This list does not close [#122] and does not become another instance of the
//! bare 400** — AC5 asks for the second, and the status report says the first
//! rather than implying otherwise.
//!
//! [#122]: https://github.com/sujanto-gaws/kelir/issues/122
//! [#179]: https://github.com/sujanto-gaws/kelir/issues/179
//! [dq]: crate::modules::document::domain::DocumentQuery

use serde::Deserialize;
use utoipa::IntoParams;
use uuid::Uuid;

use crate::error::{AppError, ValidationDetail};
use crate::modules::workflow::repository::inbox::{InboxFilters, InboxScope};
use crate::response::Pagination;

/// The longest search term this list will take.
///
/// A bound rather than a policy: `ILIKE '%…%'` over a term longer than any task
/// name or document title is a scan that cannot match, and a caller sending a
/// megabyte of it is not searching.
pub const MAX_SEARCH: usize = 100;

/// The query parameters `GET /tasks` accepts.
///
/// Unknown parameters are ignored rather than refused, which is what
/// [`Pagination`] already does everywhere else in the API; `deny_unknown_fields`
/// here and nowhere else would be a difference between endpoints with no reason
/// behind it.
#[derive(Debug, Clone, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct InboxQuery {
    /// 1-based page number; values below 1 are treated as 1.
    pub page: Option<u32>,
    /// Rows per page, clamped to `response::MAX_PAGE_SIZE`.
    pub page_size: Option<u32>,
    /// `open` (the default), `overdue`, `completed`, or `all`.
    ///
    /// **Still not a status filter.** The values are what a screen needs —
    /// *what is waiting for me*, *what is late*, *what I have finished*, and
    /// *everything* — rather than the seven values the column can hold. A caller
    /// asking for `CANCELLED` would be asking a question no screen renders;
    /// `completed` answers FR-TASK-009's question, which is *has this been
    /// through my hands*, and a withdrawn task has.
    ///
    /// **One axis, four points** ([#185](https://github.com/sujanto-gaws/kelir/issues/185)
    /// AC3, [#256](https://github.com/sujanto-gaws/kelir/issues/256) AC2).
    /// `overdue ⊂ open ⊂ all` and `completed ⊂ all`, with the two subsets
    /// disjoint: a task that is late is still open, because a finished one is
    /// not late, it is done. Flags that combined would let a caller ask for
    /// *completed and overdue* — a question with no answer — and a screen would
    /// need two controls to express one choice.
    pub scope: Option<String>,
    /// Narrow the inbox to one document, which is what the document workspace
    /// asks for when it renders "your task on this document".
    pub document_id: Option<Uuid>,
    /// Free text over the task's name and the document it is about
    /// (FR-SRH-003, #256 AC3).
    ///
    /// **`q`, not `search`**, matching the shorthand every other list in this
    /// product would use if it had one; the name is the only thing about it a
    /// client has to remember.
    pub q: Option<String>,
}

/// The term as `LIKE` should see it, or nothing.
///
/// **Escaped here rather than in the statement**, because the statement is
/// shared by the page, the count and the search and a term escaped twice
/// searches for backslashes. Whitespace-only is *no search* rather than a
/// search for nothing: a screen that clears its box should get its list back.
///
/// `%` and `_` are `LIKE`'s wildcards and a person typing either means the
/// character — searching for `50%` should not return everything.
pub fn normalize_search(term: Option<&str>) -> Result<Option<String>, AppError> {
    let Some(trimmed) = term.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };

    if trimmed.chars().count() > MAX_SEARCH {
        return Err(AppError::validation(vec![ValidationDetail::new(
            "q",
            "maxLength",
            "SEARCH_TOO_LONG",
            format!("a search is at most {MAX_SEARCH} characters"),
        )]));
    }

    Ok(Some(
        trimmed
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_"),
    ))
}

impl InboxQuery {
    /// The paging half, so clamping and the 1-based page live in one place
    /// (`response::Pagination`) rather than being restated per endpoint.
    pub fn pagination(&self) -> Pagination {
        Pagination {
            page: self.page,
            page_size: self.page_size,
        }
    }

    /// The filtering half, parsed, or a 422 naming what was wrong.
    pub fn filters(&self) -> Result<InboxFilters, AppError> {
        let scope = match self.scope.as_deref().map(str::trim) {
            None | Some("") | Some("open") => InboxScope::Open,
            Some("overdue") => InboxScope::Overdue,
            Some("completed") => InboxScope::Completed,
            Some("all") => InboxScope::All,
            Some(other) => {
                return Err(AppError::validation(vec![ValidationDetail::new(
                    "scope",
                    "enum",
                    "UNKNOWN_VALUE",
                    format!(
                        "`{other}` is not a scope. Must be one of: open, overdue, completed, all"
                    ),
                )]))
            }
        };

        Ok(InboxFilters {
            scope,
            document_id: self.document_id,
            // Never from the query string: the list is a list, and one task by
            // id is `GET /tasks/{id}`.
            task_id: None,
            search: normalize_search(self.q.as_deref())?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(scope: Option<&str>) -> InboxQuery {
        InboxQuery {
            scope: scope.map(str::to_owned),
            ..Default::default()
        }
    }

    #[test]
    fn an_inbox_with_no_scope_shows_what_is_waiting() {
        // The default matters: an inbox that opened on every task anybody had
        // ever held would be a log rather than a queue.
        assert_eq!(
            query(None).filters().expect("a default").scope,
            InboxScope::Open
        );
        assert_eq!(
            query(Some("open")).filters().expect("open").scope,
            InboxScope::Open
        );
    }

    #[test]
    fn the_axis_has_four_points_and_each_is_reachable() {
        for (asked, expected) in [
            ("open", InboxScope::Open),
            ("overdue", InboxScope::Overdue),
            ("completed", InboxScope::Completed),
            ("all", InboxScope::All),
        ] {
            assert_eq!(
                query(Some(asked)).filters().expect(asked).scope,
                expected,
                "`{asked}` did not reach its point on the axis"
            );
        }
    }

    /// **`completed` is a point on the axis, not a status filter** (#256 AC2).
    ///
    /// The screen asks *has this been through my hands*, and the API answers
    /// that question rather than offering the seven values the column holds.
    #[test]
    fn completed_is_what_has_been_through_my_hands() {
        assert_eq!(
            query(Some("completed")).filters().expect("completed").scope,
            InboxScope::Completed
        );
    }

    #[test]
    fn an_unknown_scope_names_the_ones_that_exist() {
        // `unknown variant` is what the extractor would have said. A person
        // needs the list.
        let error = query(Some("archived")).filters().expect_err("refused");

        let AppError::Validation { details } = error else {
            panic!("expected a validation failure");
        };

        assert_eq!(details[0].path, "scope");
        assert!(
            details[0].message.contains("open, overdue, completed, all"),
            "{details:?}"
        );
    }

    // -----------------------------------------------------------------------
    // The search term (#256 AC3)
    // -----------------------------------------------------------------------

    /// **A wildcard typed by a person is a character, not a wildcard.**
    ///
    /// `50%` searching for everything is the version of this bug somebody
    /// notices; `a_b` quietly matching `axb` is the version nobody does.
    #[test]
    fn like_wildcards_in_a_search_term_are_escaped() {
        assert_eq!(
            normalize_search(Some("50%")).expect("a term"),
            Some("50\\%".to_owned())
        );
        assert_eq!(
            normalize_search(Some("a_b")).expect("a term"),
            Some("a\\_b".to_owned())
        );
        // The escape character itself, first, so escaping is not applied twice.
        assert_eq!(
            normalize_search(Some("a\\b")).expect("a term"),
            Some("a\\\\b".to_owned())
        );
    }

    #[test]
    fn an_empty_search_is_no_search_rather_than_a_search_for_nothing() {
        assert_eq!(normalize_search(None).expect("none"), None);
        assert_eq!(normalize_search(Some("   ")).expect("blank"), None);
        assert_eq!(
            normalize_search(Some("  desk  ")).expect("a term"),
            Some("desk".to_owned())
        );
    }

    #[test]
    fn a_search_over_the_bound_is_refused_and_names_the_field() {
        let long = "é".repeat(MAX_SEARCH + 1);
        let error = normalize_search(Some(&long)).expect_err("refused");

        let AppError::Validation { details } = error else {
            panic!("expected a validation failure");
        };

        assert_eq!(details[0].path, "q");
        // Characters rather than bytes: the bound is what a person typed.
        assert!(normalize_search(Some(&"é".repeat(MAX_SEARCH))).is_ok());
    }
}
