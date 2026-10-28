//! **What counts as blank**, decided once for every request body string
//! ([#616](https://github.com/sujanto-gaws/kelir/issues/616)).
//!
//! Rust's `str::trim().is_empty()` asks [`char::is_whitespace`], which knows
//! the Unicode `White_Space` property: the no-break and ideographic spaces,
//! U+0085 and the line and paragraph separators. It does not know the
//! *format* characters that render as nothing at all. A role name of one
//! U+200B ZERO WIDTH SPACE was therefore not blank: it was stored, and listed
//! as a row with no visible name.
//!
//! [`is_blank_text`] is the one definition: a value is blank when every
//! character is whitespace or one of [`INVISIBLE_CODE_POINTS`]. It decides
//! *presence* only. What is stored is still the value `str::trim` leaves, so a
//! value with an invisible character at its edge is kept as it came, and a
//! length is counted on that same trimmed value.
//!
//! The browser's `String.prototype.trim()` is a convenience pre-check on a
//! hand-built form (coding standard §3.4); this function is the definition
//! that counts.

use std::ops::RangeInclusive;

/// The code points that are not [`char::is_whitespace`] yet show nothing, so
/// a value made only of them reads as empty. Every one is a format character
/// (`Cf`) with no glyph of its own. Code points may be added here; none may be
/// removed without a decision, because a value refused today would then be
/// stored.
pub const INVISIBLE_CODE_POINTS: &[RangeInclusive<char>] = &[
    // SOFT HYPHEN: drawn only where a line breaks at it, so a value of soft
    // hyphens alone shows nothing.
    '\u{00AD}'..='\u{00AD}',
    // MONGOLIAN VOWEL SEPARATOR: whitespace until Unicode 6.3 moved it to
    // `Cf`, so an older reader trimmed it and Rust no longer does.
    '\u{180E}'..='\u{180E}',
    // ZERO WIDTH SPACE, ZERO WIDTH NON-JOINER, ZERO WIDTH JOINER,
    // LEFT-TO-RIGHT MARK, RIGHT-TO-LEFT MARK: zero width by definition. The
    // first three are #616's own examples.
    '\u{200B}'..='\u{200F}',
    // The bidirectional embeddings and overrides (LRE, RLE, PDF, LRO, RLO):
    // they change the order of what follows and draw nothing themselves.
    '\u{202A}'..='\u{202E}',
    // WORD JOINER and the invisible operators (FUNCTION APPLICATION,
    // INVISIBLE TIMES, INVISIBLE SEPARATOR, INVISIBLE PLUS). WORD JOINER is
    // the zero width no-break that replaced U+FEFF in that role.
    '\u{2060}'..='\u{2064}',
    // The bidirectional isolates (LRI, RLI, FSI, PDI), the newer form of the
    // embeddings above.
    '\u{2066}'..='\u{2069}',
    // ZERO WIDTH NO-BREAK SPACE, also the byte order mark: a file or a
    // clipboard can carry one at the start of a value unseen.
    '\u{FEFF}'..='\u{FEFF}',
];

/// Whether `value` has nothing a reader can see: it is empty, or every
/// character is [`char::is_whitespace`] (which includes U+0085 and U+00A0) or
/// is in [`INVISIBLE_CODE_POINTS`].
pub fn is_blank_text(value: &str) -> bool {
    value.chars().all(|c| c.is_whitespace() || is_invisible(c))
}

fn is_invisible(c: char) -> bool {
    INVISIBLE_CODE_POINTS.iter().any(|range| range.contains(&c))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every code point in the set, both ends of each range and everything
    /// between, alone and run together, is blank.
    #[test]
    fn every_invisible_code_point_is_blank() {
        let mut all = String::new();

        for range in INVISIBLE_CODE_POINTS {
            for c in range.clone() {
                assert!(is_blank_text(&c.to_string()), "U+{:04X}", c as u32);
                all.push(c);
            }
        }

        assert!(is_blank_text(&all));
        assert!(is_blank_text(&format!(" {all}\t")));
    }

    /// #616 names five. Each is held by name, so the set cannot lose one
    /// while the walk above still passes over what is left.
    #[test]
    fn the_five_code_points_616_names_are_blank() {
        for c in ['\u{200B}', '\u{200C}', '\u{200D}', '\u{2060}', '\u{FEFF}'] {
            assert!(is_blank_text(&c.to_string()), "U+{:04X}", c as u32);
            assert!(is_blank_text(&format!("{c}{c} {c}")), "U+{:04X}", c as u32);
        }
    }

    /// Each group the set lists, by its two ends, so narrowing a range from
    /// either side is seen.
    #[test]
    fn each_end_of_each_group_is_blank() {
        for c in [
            '\u{00AD}', '\u{180E}', '\u{200B}', '\u{200F}', '\u{202A}', '\u{202E}', '\u{2060}',
            '\u{2064}', '\u{2066}', '\u{2069}', '\u{FEFF}',
        ] {
            assert!(is_blank_text(&c.to_string()), "U+{:04X}", c as u32);
        }
    }

    /// The neighbours just outside a group, where the neighbour is neither
    /// whitespace nor in another group, are visible, so widening a range is
    /// seen. Four neighbours cannot be tested this way and are not listed:
    /// U+200A, U+2029, U+202F and U+205F are whitespace already. Adding a
    /// neighbour to the set is allowed, and moves it out of this list.
    #[test]
    fn the_neighbours_of_each_group_are_not_blank() {
        for c in [
            '\u{00AC}', '\u{00AE}', '\u{180D}', '\u{180F}', '\u{2010}', '\u{2065}', '\u{206A}',
            '\u{FEFE}', '\u{FF00}',
        ] {
            assert!(!is_blank_text(&c.to_string()), "U+{:04X}", c as u32);
        }
    }

    /// What `str::trim` already treated as blank still is: the empty string,
    /// ASCII whitespace, U+0085 NEXT LINE and U+00A0 NO-BREAK SPACE.
    #[test]
    fn whitespace_and_the_empty_string_are_blank() {
        for value in [
            "",
            " ",
            "\t\r\n",
            "\u{0085}",
            "\u{00A0}",
            "\u{3000}\u{2003}",
        ] {
            assert!(is_blank_text(value), "{value:?}");
        }
    }

    /// A value with one visible character is not blank, wherever the
    /// invisible ones sit around it.
    #[test]
    fn a_visible_character_is_not_blank() {
        for value in ["a", "\u{200B}a", "a\u{FEFF}", " \u{2060}a\u{200D} ", "é"] {
            assert!(!is_blank_text(value), "{value:?}");
        }
    }
}
