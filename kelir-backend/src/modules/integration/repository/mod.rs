//! Queries for `external_systems`, `integration_endpoints` and
//! `integration_credentials` (Database Schema §12.1–§12.3).
//!
//! **Every statement filters `tenant_id`**, and every read of a child row
//! filters its `external_system_id` as well, so an endpoint or a credential is
//! found only under the system it belongs to. `0046_integration.sql` makes a
//! cross-tenant child row unwritable as well (its composite keys); the
//! predicates here are what make one unreadable, and neither depends on the
//! other.
//!
//! Soft-deleted rows are excluded everywhere. Only a credential is ever
//! soft-deleted by this module: a system is deactivated, and an endpoint set
//! `INACTIVE`, instead.

pub mod credential;
pub mod endpoint;
pub mod external_system;

/// `%term%` with `\`, `%` and `_` escaped, so a caller searching for `_` finds
/// an underscore rather than every row.
///
/// PostgreSQL's default `LIKE` escape is the backslash, which is why no
/// `ESCAPE` clause appears in the query that binds this. The same rule
/// `master_data::repository::like_contains` states; that one is private to its
/// module, as repositories are (coding standard §2.2).
fn like_contains(search: &str) -> String {
    let mut pattern = String::with_capacity(search.len() + 2);
    pattern.push('%');

    for character in search.chars() {
        if matches!(character, '\\' | '%' | '_') {
            pattern.push('\\');
        }
        pattern.push(character);
    }

    pattern.push('%');
    pattern
}

/// `(was it sent, the value)` for a nullable column an update may set, leave
/// or clear.
fn split<T>(field: Option<Option<T>>) -> (bool, Option<T>) {
    match field {
        None => (false, None),
        Some(value) => (true, value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wildcard_in_a_search_matches_itself() {
        assert_eq!(like_contains("SAP_ERP"), "%SAP\\_ERP%");
        assert_eq!(like_contains("100%"), "%100\\%%");
        assert_eq!(like_contains("a\\b"), "%a\\\\b%");
    }
}
