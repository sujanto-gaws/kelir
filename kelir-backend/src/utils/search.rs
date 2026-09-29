//! The one rule for a list's `search` parameter: what counts as a search, and
//! how it reaches an `ILIKE`.
//!
//! The searchable configuration lists bind their term the same way — `($n::text
//! IS NULL OR key ILIKE $n OR name ILIKE $n)` — so the two halves live here
//! rather than beside each query. Two older searches still escape with private
//! copies of the same rule, `document::repository::list::escaped` and
//! `task_inbox::domain::normalize_search`; both are correct, and both bind a
//! bare term that their SQL wraps in `%`, so folding them in is a change to
//! their statements rather than to an import. **A second copy of an escaping rule is a
//! second escaping rule**: this file replaced two private copies of
//! [`like_contains`] (`integration::repository` and `master_data::repository`,
//! both correct) when six more lists took a search at once
//! ([#525](https://github.com/sujanto-gaws/kelir/issues/525)).
//!
//! **What a search may contain is decided here for every list, including those
//! two.** [`parse_search`] is the trim, the blank-is-absent rule and the NUL
//! refusal; every list that takes a search reads its term through it, whether
//! it reports one refusal ([`search_term`]) or collects several
//! (`DocumentQuery::filters`, `RoleViewQuery::filters`).
//!
//! No list searched this way has a trigram index. The search is a substring
//! match, which a b-tree cannot serve, and an index chosen before the query has
//! been measured is chosen on a guess — Database Schema §6.16 says the same of
//! the documents list, and System Design Document §12.3 says it of these.

use crate::error::{AppError, ValidationDetail};

/// The `code` a NUL in a search is refused with.
pub const NUL_IN_SEARCH: &str = "INVALID_CHARACTER";

/// A search term with its surrounding whitespace trimmed; blank is no search;
/// a NUL anywhere in it is a refusal naming `path`.
///
/// A chooser whose box has been cleared sends nothing, but a hand-written
/// `?search=` or `?search=%20` must mean the same thing rather than a pattern
/// that happens to match every row.
///
/// **A NUL is refused rather than stripped.** PostgreSQL `text` cannot hold
/// 0x00, so binding one fails the statement with *invalid byte sequence for
/// encoding "UTF8": 0x00* and the caller sees a 500. Stripping it would answer a
/// search nobody asked for — `a\0b` becoming `ab` — and a list that quietly
/// answers a different question is the failure this module exists to prevent.
/// No row can contain a NUL either, so the refusal loses no match.
///
/// `path` is the parameter's name as the caller sent it: `search` everywhere
/// except the task inbox, whose parameter is `q`.
pub fn parse_search<'a>(
    path: &str,
    value: Option<&'a str>,
) -> Result<Option<&'a str>, ValidationDetail> {
    let Some(term) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };

    if term.contains('\0') {
        return Err(ValidationDetail::new(
            path,
            "pattern",
            NUL_IN_SEARCH,
            "Must not contain a NUL character",
        ));
    }

    Ok(Some(term))
}

/// [`parse_search`] on the `search` parameter, as a 422 of its own.
pub fn search_term(value: Option<&str>) -> Result<Option<&str>, AppError> {
    parse_search("search", value).map_err(|detail| AppError::validation(vec![detail]))
}

/// Wraps a search term in a `%…%` pattern that matches it literally.
///
/// `LIKE` reads `%` as *any run of characters* and `_` as *any character*, so a
/// caller searching for the literal string `100%` would otherwise match every
/// row beginning `100`, and `a_b` would match `axb`. Both are escaped, and so is
/// the escape character itself — escaping `\` last would double the
/// backslashes this function had just introduced.
///
/// PostgreSQL's default `LIKE` escape is the backslash, which is why no
/// `ESCAPE` clause appears in the queries that bind this.
pub fn like_contains(search: &str) -> String {
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

/// [`search_term`] then [`like_contains`]: the value a repository binds.
pub fn contains_pattern(value: Option<&str>) -> Result<Option<String>, AppError> {
    Ok(search_term(value)?.map(like_contains))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_a_plain_search_in_a_contains_pattern() {
        assert_eq!(like_contains("ACME"), "%ACME%");
    }

    #[test]
    fn escapes_a_percent_so_it_matches_itself() {
        // Without this, searching `100%` returns every code starting `100`, and
        // the caller reads the extra rows as matches.
        assert_eq!(like_contains("100%"), "%100\\%%");
    }

    #[test]
    fn escapes_an_underscore_so_it_matches_itself() {
        assert_eq!(like_contains("A_B"), "%A\\_B%");
        assert_eq!(like_contains("SAP_ERP"), "%SAP\\_ERP%");
    }

    #[test]
    fn escapes_the_escape_character_itself() {
        // `\%` from a caller is a literal backslash followed by a literal
        // percent, not an escaped percent.
        assert_eq!(like_contains("\\%"), "%\\\\\\%%");
        assert_eq!(like_contains("a\\b"), "%a\\\\b%");
    }

    #[test]
    fn leaves_a_search_that_needs_no_escaping_alone() {
        assert_eq!(like_contains("SUP-0001"), "%SUP-0001%");
    }

    #[test]
    fn a_blank_search_is_no_search() {
        assert_eq!(search_term(None).unwrap(), None);
        assert_eq!(search_term(Some("")).unwrap(), None);
        assert_eq!(search_term(Some("   ")).unwrap(), None);
        assert_eq!(search_term(Some("  po ")).unwrap(), Some("po"));
        assert_eq!(contains_pattern(Some(" ")).unwrap(), None);
        assert_eq!(
            contains_pattern(Some(" a_b ")).unwrap(),
            Some("%a\\_b%".to_owned())
        );
    }

    fn refusal(path: &str, value: &str) -> ValidationDetail {
        parse_search(path, Some(value)).expect_err("a NUL is refused")
    }

    #[test]
    fn a_nul_anywhere_in_a_search_is_refused_on_the_field_it_came_in() {
        // Alone, inside, leading and trailing: PostgreSQL rejects 0x00 in
        // `text` wherever it sits, so every position is a 422 rather than a 500.
        for value in ["\0", "a\0b", "\0ab", "ab\0", " \0 "] {
            let detail = refusal("search", value);

            assert_eq!(detail.path, "search", "{value:?}");
            assert_eq!(detail.code, NUL_IN_SEARCH, "{value:?}");
        }

        // The inbox's parameter is `q`, and the refusal names it.
        assert_eq!(refusal("q", "a\0b").path, "q");
    }

    #[test]
    fn a_nul_is_refused_rather_than_stripped() {
        // `a\0b` searched as `ab` would answer a question nobody asked.
        let error = search_term(Some("a\0b")).expect_err("refused");
        let AppError::Validation { details } = error else {
            panic!("expected a validation failure");
        };

        assert_eq!(details.len(), 1);
        assert_eq!(details[0].path, "search");
        assert!(contains_pattern(Some("\0")).is_err());
    }

    #[test]
    fn other_control_characters_are_still_searches() {
        // Only NUL cannot be stored; the rest are legal `text` and match
        // nothing, which is an honest answer rather than an error.
        assert_eq!(
            search_term(Some("a\u{1}\u{7f}b")).unwrap(),
            Some("a\u{1}\u{7f}b")
        );
        // Whitespace controls are trimmed, which leaves a blank search.
        assert_eq!(search_term(Some("\n\t")).unwrap(), None);
    }
}
