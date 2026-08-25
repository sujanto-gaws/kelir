//! Deserialization helpers shared across modules.

use serde::{Deserialize, Deserializer};

/// Tells *absent* from *present and null*, which `Option<T>` alone cannot.
///
/// **A field typed `Option<Option<T>>` does not get this behaviour for free**,
/// and that is the trap this exists for: serde deserializes an explicit `null`
/// into the *outer* `None`, so a request saying `{"departmentId": null}` is
/// indistinguishable from one that never mentioned the field. The column can
/// then be set and never unset — a person who leaves a department keeps it
/// forever — and nothing about the type signature says so.
///
/// With `#[serde(default, deserialize_with = "present_or_absent")]` a missing
/// key stays `None` and never reaches here, while a key that is present —
/// including `null` — arrives and is wrapped in `Some`.
///
/// It began as a private helper in `master_data::domain::facility`, whose own
/// comment said it was written out rather than pulled in because two fields
/// needed it. Ten fields across four modules need it now, and three of those
/// modules had the bug, so it moved here.
pub fn present_or_absent<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;
    use serde_json::json;

    use super::*;

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Request {
        #[serde(default, deserialize_with = "present_or_absent")]
        value: Option<Option<String>>,
    }

    #[test]
    fn an_absent_field_is_none() {
        let parsed: Request = serde_json::from_value(json!({})).expect("parses");

        assert_eq!(parsed.value, None, "leave the column alone");
    }

    #[test]
    fn an_explicit_null_is_some_none() {
        let parsed: Request = serde_json::from_value(json!({ "value": null })).expect("parses");

        assert_eq!(
            parsed.value,
            Some(None),
            "clear the column — this is the case a plain Option<Option<_>> gets \
             wrong, collapsing it to the outer None"
        );
    }

    #[test]
    fn a_value_is_some_some() {
        let parsed: Request = serde_json::from_value(json!({ "value": "x" })).expect("parses");

        assert_eq!(parsed.value, Some(Some("x".to_owned())));
    }

    /// The behaviour without the helper, pinned so the difference is visible.
    ///
    /// This is not a test of our code — it is a test of the assumption the code
    /// rests on, and the reason every `Option<Option<_>>` field in this
    /// codebase carries the attribute.
    #[test]
    fn a_bare_double_option_collapses_an_explicit_null() {
        #[derive(Debug, Deserialize)]
        struct Bare {
            #[serde(default)]
            value: Option<Option<String>>,
        }

        let parsed: Bare = serde_json::from_value(json!({ "value": null })).expect("parses");

        assert_eq!(
            parsed.value, None,
            "serde reads an explicit null as the outer None, which is exactly \
             what makes a nullable column unclearable"
        );
    }
}
