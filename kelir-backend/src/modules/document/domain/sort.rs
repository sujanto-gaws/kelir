//! What a document list may be ordered by ([#340], FR-RAD-003, FR-DOC-013).
//!
//! **An allow-list, because the alternative is an identifier from a request in
//! an `ORDER BY`.** Coding standard §2.5 keeps every statement static so
//! `sqlx::query!` can check it, and §6.4 requires dynamic identifiers to be
//! allow-listed. A closed enum satisfies both at once: the sort arrives as a
//! name, is resolved to a variant or refused, and the statement that consumes
//! it names every column it could order by in its own text.
//!
//! **The set is smaller than the row, deliberately.** `entity_id` and `id` are
//! UUIDs — v7, so they happen to order by creation time, which is exactly the
//! trap: a list sorted by `entityId` would look chronological and be a sort on
//! a foreign key. `entity_type` has three values and sorts nothing useful.
//! Neither is offered, and a definition asking for one gets a refusal that
//! names what it may ask for instead.
//!
//! Until [#340] there was no sort at all: `list_documents` ordered
//! `created_at DESC, id DESC` and the parameter did not exist. That default is
//! unchanged and is what every caller that does not ask for a sort still gets.
//!
//! [#340]: https://github.com/sujanto-gaws/kelir/issues/340

use std::fmt;

/// A column a document list may be ordered by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentSortKey {
    DocumentRef,
    DocumentNumber,
    DocumentTypeCode,
    Title,
    Status,
    Priority,
    SubmittedAt,
    CreatedAt,
    UpdatedAt,
}

impl DocumentSortKey {
    /// The name a definition or a query parameter uses, which is the
    /// `DocumentSummary` field name in `camelCase` — the same spelling the
    /// column carries on the wire, so a list definition names a column once and
    /// sorts by the name it displayed.
    pub fn as_name(self) -> &'static str {
        match self {
            Self::DocumentRef => "documentRef",
            Self::DocumentNumber => "documentNumber",
            Self::DocumentTypeCode => "documentTypeCode",
            Self::Title => "title",
            Self::Status => "status",
            Self::Priority => "priority",
            Self::SubmittedAt => "submittedAt",
            Self::CreatedAt => "createdAt",
            Self::UpdatedAt => "updatedAt",
        }
    }

    /// The token the statement's `ORDER BY` compares against.
    ///
    /// `snake_case` and separate from [`Self::as_name`] on purpose: the wire
    /// name is a contract with a client and the SQL token is a contract with
    /// one statement, and letting one file's rename silently move the other is
    /// how a sort quietly stops sorting.
    pub fn as_db(self) -> &'static str {
        match self {
            Self::DocumentRef => "document_ref",
            Self::DocumentNumber => "document_number",
            Self::DocumentTypeCode => "document_type_code",
            Self::Title => "title",
            Self::Status => "status",
            Self::Priority => "priority",
            Self::SubmittedAt => "submitted_at",
            Self::CreatedAt => "created_at",
            Self::UpdatedAt => "updated_at",
        }
    }

    /// Every key, for a refusal that says what *is* allowed.
    ///
    /// A refusal naming only the bad value tells somebody their sort is wrong
    /// and leaves them guessing; the whole point of an allow-list is that it
    /// can be shown.
    pub const ALL: [Self; 9] = [
        Self::DocumentRef,
        Self::DocumentNumber,
        Self::DocumentTypeCode,
        Self::Title,
        Self::Status,
        Self::Priority,
        Self::SubmittedAt,
        Self::CreatedAt,
        Self::UpdatedAt,
    ];

    /// A name as a key, or `None` for anything outside the allow-list.
    ///
    /// Accepts the `camelCase` wire name and the `snake_case` column, because
    /// [Database Schema](../../../../../docs/design/02.%20Database%20Schema.md)
    /// §5.7's own example column keys are `document_number` and
    /// `form_data.amount` — so a definition written against the schema's
    /// example must not be refused for spelling a column the way the schema
    /// spells it.
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();

        Self::ALL
            .into_iter()
            .find(|key| key.as_name() == value || key.as_db() == value)
    }

    /// The keys, listed for a message.
    pub fn allowed() -> String {
        Self::ALL
            .iter()
            .map(|key| format!("`{}`", key.as_name()))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

impl fmt::Display for DocumentSortKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_name())
    }
}

/// A column and a direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocumentSort {
    pub key: DocumentSortKey,
    pub descending: bool,
}

impl Default for DocumentSort {
    /// **Newest first**, which is what a list of documents is for: the thing
    /// somebody is looking for is almost always the one they made most
    /// recently. It is what `list_documents` did before a sort existed, and
    /// every caller that does not ask for one still gets it.
    fn default() -> Self {
        Self {
            key: DocumentSortKey::CreatedAt,
            descending: true,
        }
    }
}

impl DocumentSort {
    /// A `key` and a direction as a definition or a query parameter spells
    /// them, or `None` naming which half was wrong.
    ///
    /// `dir` is the [Database Schema](../../../../../docs/design/02.%20Database%20Schema.md)
    /// §5.6 spelling — `default_sort_json` is documented as
    /// `[{"key":"created_at","dir":"desc"}]` — and an absent direction is
    /// ascending, which is SQL's own default and the one a reader assumes.
    pub fn parse(key: &str, direction: Option<&str>) -> Result<Self, SortRefusal> {
        let Some(key) = DocumentSortKey::parse(key) else {
            return Err(SortRefusal::UnknownKey(key.trim().to_owned()));
        };

        let descending = match direction.map(str::trim).filter(|value| !value.is_empty()) {
            None | Some("asc") | Some("ASC") => false,
            Some("desc") | Some("DESC") => true,
            Some(other) => return Err(SortRefusal::UnknownDirection(other.to_owned())),
        };

        Ok(Self { key, descending })
    }
}

/// Why a sort could not be read.
///
/// Two variants rather than one string, so the caller can say which half of
/// `{"key": …, "dir": …}` it is about — a definition author who mistyped the
/// direction should not be told to check the column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SortRefusal {
    UnknownKey(String),
    UnknownDirection(String),
}

impl fmt::Display for SortRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownKey(key) => write!(
                formatter,
                "`{key}` is not a column a document list can be ordered by — one of {} is",
                DocumentSortKey::allowed()
            ),
            Self::UnknownDirection(direction) => write!(
                formatter,
                "`{direction}` is not a sort direction; it is `asc` or `desc`"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_is_what_the_list_did_before_a_sort_existed() {
        let sort = DocumentSort::default();

        assert_eq!(sort.key, DocumentSortKey::CreatedAt);
        assert!(sort.descending);
    }

    #[test]
    fn reads_a_key_by_its_wire_name_and_by_its_column() {
        // The schema's own §5.7 example spells a column key `document_number`,
        // so both spellings have to resolve or a definition written against the
        // documentation is refused.
        assert_eq!(
            DocumentSortKey::parse("documentNumber"),
            Some(DocumentSortKey::DocumentNumber)
        );
        assert_eq!(
            DocumentSortKey::parse("document_number"),
            Some(DocumentSortKey::DocumentNumber)
        );
    }

    #[test]
    fn every_key_round_trips_through_both_spellings() {
        for key in DocumentSortKey::ALL {
            assert_eq!(DocumentSortKey::parse(key.as_name()), Some(key));
            assert_eq!(DocumentSortKey::parse(key.as_db()), Some(key));
        }
    }

    #[test]
    fn refuses_a_key_outside_the_allow_list_and_names_the_alternatives() {
        // `entityId` is a real column and is deliberately not sortable: it is a
        // v7 UUID, so it would look chronological while sorting a foreign key.
        let refusal = DocumentSort::parse("entityId", None).expect_err("refused");

        assert_eq!(refusal, SortRefusal::UnknownKey("entityId".to_owned()));

        let message = refusal.to_string();

        assert!(message.contains("entityId"), "{message}");
        assert!(message.contains("createdAt"), "{message}");
    }

    #[test]
    fn refuses_a_direction_that_is_neither_and_says_which_half_is_wrong() {
        let refusal = DocumentSort::parse("title", Some("sideways")).expect_err("refused");

        assert_eq!(
            refusal,
            SortRefusal::UnknownDirection("sideways".to_owned())
        );
        assert!(refusal.to_string().contains("direction"));
    }

    #[test]
    fn an_absent_direction_is_ascending() {
        let sort = DocumentSort::parse("title", None).expect("read");

        assert!(!sort.descending);
    }

    #[test]
    fn reads_the_schemas_own_default_sort_shape() {
        // `[{"key":"created_at","dir":"desc"}]`, §5.6.
        let sort = DocumentSort::parse("created_at", Some("desc")).expect("read");

        assert_eq!(sort, DocumentSort::default());
    }

    /// The two spellings are separate on purpose, and this is what would catch
    /// one being renamed into the other.
    #[test]
    fn a_wire_name_is_not_its_column_except_where_they_agree() {
        assert_eq!(DocumentSortKey::Title.as_name(), "title");
        assert_eq!(DocumentSortKey::Title.as_db(), "title");
        assert_eq!(DocumentSortKey::SubmittedAt.as_name(), "submittedAt");
        assert_eq!(DocumentSortKey::SubmittedAt.as_db(), "submitted_at");
    }
}
