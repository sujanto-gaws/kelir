//! **What PostgreSQL can store**, decided once for every request
//! ([#601](https://github.com/sujanto-gaws/kelir/issues/601)).
//!
//! Two values a Rust type holds happily fail a PostgreSQL statement when they
//! are bound into it, and the caller is then told `INTERNAL_ERROR` for a
//! mistake in their own request:
//!
//! - **a NUL (0x00) in a string.** `text` cannot hold one; the statement fails
//!   with *invalid byte sequence for encoding "UTF8": 0x00*. A NUL can arrive
//!   in any query-string value as `%00`, so `crate::extract::QueryParams`
//!   refuses it before any handler runs, naming the parameter
//!   ([`nul_refusal`]).
//! - **an instant outside `timestamptz`'s range.** chrono parses dates back to
//!   year -262143; PostgreSQL's first `timestamptz` is 4714 BC. A service that
//!   takes a date range calls [`refuse_out_of_range`] before its query.
//!
//! **Why the two halves live in different places.** A NUL is wrong in every
//! string of every request whatever the parameter means, so the extractor can
//! refuse it without knowing the type it is filling. A date is only a date once
//! the type says so — `from=-5000-01-01` is a perfectly good *string* — so the
//! bound is checked by the code that knows which fields are instants.
//!
//! Both halves answer the same envelope: a 422 `VALIDATION_ERROR` whose detail
//! names the parameter as the caller sent it.

use chrono::{DateTime, Utc};

use crate::error::{AppError, ValidationDetail};

/// The `code` a NUL is refused with, wherever it arrives.
pub const INVALID_CHARACTER: &str = "INVALID_CHARACTER";

/// The `code` an instant PostgreSQL cannot store is refused with.
pub const OUT_OF_RANGE: &str = "OUT_OF_RANGE";

/// The refusal of a NUL in `path` — the parameter or field as the caller
/// spelled it.
pub fn nul_refusal(path: impl Into<String>) -> ValidationDetail {
    ValidationDetail::new(
        path,
        "pattern",
        INVALID_CHARACTER,
        "Must not contain a NUL character",
    )
}

/// PostgreSQL's first `timestamptz`, `4714-11-24 00:00:00+00 BC` — Julian
/// day 0 — in microseconds from the Unix epoch. chrono counts years
/// astronomically, so 4714 BC is year -4713: this is `-4713-11-24T00:00:00Z`.
const POSTGRES_MIN_MICROS: i64 = -210_866_803_200_000_000;

/// The first instant **past** PostgreSQL's last `timestamptz`,
/// `294277-01-01 00:00:00+00`, in microseconds from the Unix epoch. It does
/// not fit an `i64` of Unix microseconds, which is why it is an `i128`.
///
/// chrono's last instant is in year 262143, so no parsed `DateTime` reaches
/// this bound today; a year above 262143 is refused by the parse before
/// [`refuse_out_of_range`] runs. The check stays so the range is PostgreSQL's
/// whole, not whatever chrono's happens to be.
const POSTGRES_END_MICROS: i128 = 9_224_318_016_000_000_000;

/// Whether PostgreSQL can hold `at` as a `timestamptz`. Compared at
/// nanosecond precision: an instant a nanosecond before the minimum is
/// refused, though sqlx's truncation to microseconds would carry it onto the
/// minimum itself.
pub fn within_postgres_range(at: DateTime<Utc>) -> bool {
    let nanos =
        i128::from(at.timestamp()) * 1_000_000_000 + i128::from(at.timestamp_subsec_nanos());
    unix_nanos_within_postgres_range(nanos)
}

/// [`within_postgres_range`] over Unix nanoseconds, where the upper bound —
/// out of chrono's reach — can be tested.
fn unix_nanos_within_postgres_range(nanos: i128) -> bool {
    nanos >= i128::from(POSTGRES_MIN_MICROS) * 1_000 && nanos < POSTGRES_END_MICROS * 1_000
}

/// A 422 naming **every** bound PostgreSQL cannot hold, not only the first:
/// a caller whose `from` and `to` are both wrong is told both at once.
///
/// Each pair is the parameter as the caller spelled it and its parsed value.
pub fn refuse_out_of_range(bounds: &[(&str, Option<DateTime<Utc>>)]) -> Result<(), AppError> {
    let details: Vec<ValidationDetail> = bounds
        .iter()
        .filter_map(|&(field, at)| match at {
            Some(at) if !within_postgres_range(at) => Some(ValidationDetail::new(
                field,
                "range",
                OUT_OF_RANGE,
                format!(
                    "{field} must be between -4713-11-24T00:00:00Z (4714 BC) and \
                     294276-12-31T23:59:59.999999Z, the range PostgreSQL stores"
                ),
            )),
            _ => None,
        })
        .collect();

    if details.is_empty() {
        Ok(())
    } else {
        Err(AppError::validation(details))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> DateTime<Utc> {
        text.parse().expect("a timestamp")
    }

    fn refused(bounds: &[(&str, Option<DateTime<Utc>>)]) -> Vec<(String, String, String)> {
        match refuse_out_of_range(bounds) {
            Ok(()) => Vec::new(),
            Err(AppError::Validation { details }) => details
                .into_iter()
                .map(|detail| (detail.path, detail.rule, detail.code))
                .collect(),
            Err(other) => panic!("not a validation refusal: {other:?}"),
        }
    }

    fn out(field: &str) -> (String, String, String) {
        (
            field.to_owned(),
            "range".to_owned(),
            OUT_OF_RANGE.to_owned(),
        )
    }

    #[test]
    fn the_bounds_are_postgresqls_own() {
        // PostgreSQL's MIN_TIMESTAMP and END_TIMESTAMP are microseconds from
        // 2000-01-01; the constants are the same instants from 1970-01-01.
        const POSTGRES_EPOCH_UNIX_MICROS: i64 = 946_684_800_000_000;
        assert_eq!(
            POSTGRES_MIN_MICROS,
            -211_813_488_000_000_000 + POSTGRES_EPOCH_UNIX_MICROS
        );
        assert_eq!(
            POSTGRES_END_MICROS,
            9_223_371_331_200_000_000_i128 + i128::from(POSTGRES_EPOCH_UNIX_MICROS)
        );
        assert_eq!(
            at("-4713-11-24T00:00:00Z").timestamp_micros(),
            POSTGRES_MIN_MICROS
        );
    }

    #[test]
    fn the_lower_bound_is_accepted_and_a_nanosecond_before_it_is_not() {
        let first = at("-4713-11-24T00:00:00Z");
        assert!(refused(&[("from", Some(first)), ("to", Some(first))]).is_empty());

        let before = at("-4713-11-23T23:59:59.999999999Z");
        assert_eq!(refused(&[("from", Some(before))]), vec![out("from")]);
        assert_eq!(refused(&[("to", Some(before))]), vec![out("to")]);
    }

    #[test]
    fn the_upper_bound_lies_beyond_every_instant_chrono_can_parse() {
        // chrono stops in year 262143, short of PostgreSQL's 294276: its last
        // instant is accepted, and nothing later parses to be refused.
        assert!(refused(&[("to", Some(DateTime::<Utc>::MAX_UTC))]).is_empty());
        assert!(within_postgres_range(DateTime::<Utc>::MAX_UTC));
        assert!("+262144-01-01T00:00:00Z".parse::<DateTime<Utc>>().is_err());

        // The bound itself, in nanoseconds where chrono cannot reach:
        // 294276-12-31T23:59:59.999999999Z is held, 294277-01-01 is not.
        let end = POSTGRES_END_MICROS * 1_000;
        assert!(unix_nanos_within_postgres_range(end - 1));
        assert!(!unix_nanos_within_postgres_range(end));
        let min = i128::from(POSTGRES_MIN_MICROS) * 1_000;
        assert!(unix_nanos_within_postgres_range(min));
        assert!(!unix_nanos_within_postgres_range(min - 1));
    }

    #[test]
    fn every_bound_out_of_range_is_named_not_only_the_first() {
        let before = at("-5000-01-01T00:00:00Z");
        assert_eq!(
            refused(&[("from", Some(before)), ("to", Some(before))]),
            vec![out("from"), out("to")]
        );
    }

    #[test]
    fn an_absent_bound_is_not_a_refusal() {
        assert!(refused(&[("from", None), ("to", None)]).is_empty());
    }

    #[test]
    fn a_nul_is_refused_on_the_path_it_came_in() {
        let detail = nul_refusal("objectType");
        assert_eq!(
            (detail.path.as_str(), detail.code.as_str()),
            ("objectType", INVALID_CHARACTER)
        );
    }
}
