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
use crate::modules::workflow::repository::inbox::InboxFilters;
use crate::response::Pagination;

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
    /// `open` (the default) or `all`.
    ///
    /// **Not a status filter**, and that is deliberate: FR-TASK-009 (completed
    /// tasks) is unscheduled, so an inbox that could be asked for `CANCELLED`
    /// would be answering a question nobody has specified. The two values are
    /// what a screen needs — *what is waiting for me* against *what has been
    /// through my hands*.
    pub scope: Option<String>,
    /// Narrow the inbox to one document, which is what the document workspace
    /// asks for when it renders "your task on this document".
    pub document_id: Option<Uuid>,
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
        let open_only = match self.scope.as_deref().map(str::trim) {
            None | Some("") | Some("open") => true,
            Some("all") => false,
            Some(other) => {
                return Err(AppError::validation(vec![ValidationDetail::new(
                    "scope",
                    "enum",
                    "UNKNOWN_VALUE",
                    format!("`{other}` is not a scope. Must be one of: open, all"),
                )]))
            }
        };

        Ok(InboxFilters {
            open_only,
            document_id: self.document_id,
            // Never from the query string: the list is a list, and one task by
            // id is `GET /tasks/{id}`.
            task_id: None,
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
        assert!(query(None).filters().expect("a default").open_only);
        assert!(query(Some("open")).filters().expect("open").open_only);
    }

    #[test]
    fn all_widens_it_to_what_has_been_through_my_hands() {
        assert!(!query(Some("all")).filters().expect("all").open_only);
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
        assert!(details[0].message.contains("open, all"), "{details:?}");
    }
}
