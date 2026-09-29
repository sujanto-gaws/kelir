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
//! No list searched this way has a trigram index. The search is a substring
//! match, which a b-tree cannot serve, and an index chosen before the query has
//! been measured is chosen on a guess — Database Schema §6.16 says the same of
//! the documents list, and System Design Document §12.3 says it of these.

/// A search term with its surrounding whitespace trimmed; blank is no search.
///
/// A chooser whose box has been cleared sends nothing, but a hand-written
/// `?search=` or `?search=%20` must mean the same thing rather than a pattern
/// that happens to match every row.
pub fn search_term(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
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
pub fn contains_pattern(value: Option<&str>) -> Option<String> {
    search_term(value).map(like_contains)
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
        assert_eq!(search_term(None), None);
        assert_eq!(search_term(Some("")), None);
        assert_eq!(search_term(Some("   ")), None);
        assert_eq!(search_term(Some("  po ")), Some("po"));
        assert_eq!(contains_pattern(Some(" ")), None);
        assert_eq!(contains_pattern(Some(" a_b ")), Some("%a\\_b%".to_owned()));
    }
}
