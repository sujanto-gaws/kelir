//! Document metadata — the tenant's own key/value annotations (FR-DOC-006).
//!
//! **Stored apart from form data, and that is [#167]'s AC3 rather than a
//! preference.** They have different owners and different lifetimes: form data
//! is what a person typed into a form whose shape the *definition* decides, and
//! metadata is what a deployment attaches to documents regardless of which form
//! they render — a cost centre, a source system's identifier, a batch tag.
//! Merging them into one object would make a definition's `key` collide with an
//! integration's, and the failure would surface as a form field mysteriously
//! holding a value nobody typed.
//!
//! `document_metadata` is the table (Database Schema §6.9), and its
//! `(document_id, metadata_key)` unique index is what makes a key single-valued.
//!
//! # Sent means replaced
//!
//! A `metadata` object that is **present** on an update replaces the stored set
//! entirely; absent leaves it alone. That is the shape
//! [`replace_workflows`][rw] established for a collection hanging off an
//! aggregate, and the shape `UpdateDocumentTypeRequest` already uses — one rule
//! for collections across the API rather than a per-endpoint choice between
//! merge and replace that a caller has to look up.
//!
//! [#167]: https://github.com/sujanto-gaws/kelir/issues/167
//! [rw]: crate::modules::document_type::repository::replace_workflows

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::error::{AppError, ValidationDetail};

/// How many keys one document may carry.
///
/// A bound rather than none: the table has a row per key and the aggregate
/// loads all of them, so an unbounded set is an unbounded read on every
/// document open.
pub const MAX_METADATA_KEYS: usize = 100;

/// `document_metadata.metadata_key` is `VARCHAR(64)`.
pub const MAX_KEY_LENGTH: usize = 64;

/// What a metadata value is, as far as the platform is concerned.
///
/// The column is `TEXT` and this says how to read it. It is **not** a
/// validation: `document_metadata.data_type` tells a consumer whether `"2026"`
/// is a number or a string, and a consumer that guessed would sort a report
/// wrongly. Guessing from the text is the alternative and it is worse — `"01"`
/// and `"1"` are the same number and different strings, and only the writer
/// knows which was meant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MetadataType {
    String,
    Number,
    Boolean,
    Date,
}

impl MetadataType {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::String => "STRING",
            Self::Number => "NUMBER",
            Self::Boolean => "BOOLEAN",
            Self::Date => "DATE",
        }
    }

    /// Reads a value already in the database. The column has a `CHECK`, so the
    /// fallback is unreachable rather than lenient.
    pub fn from_db(value: &str) -> Self {
        match value {
            "NUMBER" => Self::Number,
            "BOOLEAN" => Self::Boolean,
            "DATE" => Self::Date,
            _ => Self::String,
        }
    }
}

/// One metadata entry as the API carries it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MetadataEntry {
    pub value: String,
    /// Absent means `STRING`, which is what an integration posting a bare value
    /// means and saves it from having to say.
    #[serde(default = "string_type")]
    pub data_type: MetadataType,
}

fn string_type() -> MetadataType {
    MetadataType::String
}

/// A document's metadata: keys to entries.
///
/// A **map** on the wire rather than an array of `{key, value}` objects,
/// because the storage makes a key single-valued and a map is the shape that
/// cannot express a duplicate. An array could carry the same key twice and the
/// API would have to decide which one won — a decision with no right answer that
/// the shape makes unnecessary.
///
/// `BTreeMap` rather than `HashMap` so the serialization is ordered, which is
/// what keeps an audit `ChangeSet` from reporting a change because two equal
/// sets encoded differently. That is finding 5 of the Sprint 8 construction,
/// which cost a browser flow to find.
pub type MetadataSet = BTreeMap<String, MetadataEntry>;

/// Refuses a metadata set the storage cannot hold or a reader cannot use.
pub fn validate(metadata: &MetadataSet) -> Result<(), AppError> {
    let mut details = Vec::new();

    if metadata.len() > MAX_METADATA_KEYS {
        details.push(ValidationDetail::new(
            "metadata",
            "maxItems",
            "TOO_MANY_KEYS",
            format!(
                "a document may carry at most {MAX_METADATA_KEYS} metadata keys and this \
                 one carries {}",
                metadata.len()
            ),
        ));
    }

    for key in metadata.keys() {
        if key.trim().is_empty() {
            details.push(ValidationDetail::new(
                "metadata",
                "required",
                "EMPTY_KEY",
                "a metadata key is what a consumer looks the value up by, so it cannot \
                 be blank",
            ));
            continue;
        }

        if key.chars().count() > MAX_KEY_LENGTH {
            details.push(ValidationDetail::new(
                format!("metadata.{key}"),
                "maxLength",
                "KEY_TOO_LONG",
                format!("a metadata key is at most {MAX_KEY_LENGTH} characters"),
            ));
        }
    }

    if details.is_empty() {
        Ok(())
    } else {
        Err(AppError::validation(details))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(value: &str) -> MetadataEntry {
        MetadataEntry {
            value: value.to_owned(),
            data_type: MetadataType::String,
        }
    }

    #[test]
    fn a_bare_value_is_a_string() {
        // An integration posting {"costCentre": {"value": "CC-1"}} should not
        // have to say so.
        let parsed: MetadataEntry =
            serde_json::from_str(r#"{"value": "CC-1"}"#).expect("a bare value parses");

        assert_eq!(parsed.data_type, MetadataType::String);
    }

    #[test]
    fn a_misspelled_member_is_refused() {
        // #62 again. `dataType` misspelled would silently become STRING and a
        // report would sort "10" before "9".
        let refused: Result<MetadataEntry, _> =
            serde_json::from_str(r#"{"value": "1", "type": "NUMBER"}"#);

        assert!(refused.is_err());
    }

    #[test]
    fn a_blank_key_is_refused() {
        let mut metadata = MetadataSet::new();
        metadata.insert("   ".to_owned(), entry("something"));

        assert!(validate(&metadata).is_err());
    }

    #[test]
    fn a_key_longer_than_the_column_is_refused_before_the_database_sees_it() {
        // VARCHAR(64) would refuse it as a 500. Refusing here names the key.
        let mut metadata = MetadataSet::new();
        metadata.insert("k".repeat(MAX_KEY_LENGTH + 1), entry("v"));

        let error = validate(&metadata).expect_err("an over-long key is refused");
        let AppError::Validation { details } = error else {
            panic!("expected a validation failure");
        };

        assert_eq!(details[0].code, "KEY_TOO_LONG");
    }

    #[test]
    fn a_set_at_the_bound_is_accepted_and_one_past_it_is_not() {
        let mut metadata = MetadataSet::new();
        for index in 0..MAX_METADATA_KEYS {
            metadata.insert(format!("k{index}"), entry("v"));
        }
        assert!(validate(&metadata).is_ok());

        metadata.insert("one-too-many".to_owned(), entry("v"));
        assert!(validate(&metadata).is_err());
    }

    #[test]
    fn every_data_type_round_trips_through_the_database_spelling() {
        for data_type in [
            MetadataType::String,
            MetadataType::Number,
            MetadataType::Boolean,
            MetadataType::Date,
        ] {
            assert_eq!(MetadataType::from_db(data_type.as_db()), data_type);
        }
    }
}
