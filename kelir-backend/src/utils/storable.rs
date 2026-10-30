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
//!
//! **A path capture and a JSON body joined them**
//! ([#657](https://github.com/sujanto-gaws/kelir/issues/657),
//! [#668](https://github.com/sujanto-gaws/kelir/issues/668); ADR-0050). The
//! NUL in either is refused by its extractor, `PathParam` and `JsonBody`,
//! through [`nul_refusal`]. A date in a body is refused while the body is
//! read, by the `deserialize_with` helpers below — [`date`], [`option_date`],
//! [`present_or_absent_date`], [`instant`] and [`option_instant`] — which
//! every date field of a request struct names. A test in
//! `tests/path_param_refusals.rs` reads the source and fails on a field that
//! does not.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Deserializer};

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

// ---------------------------------------------------------------------------
// Dates in a body (#668)
// ---------------------------------------------------------------------------

/// What a body date's refusal carries through serde, which carries only a
/// message. `crate::extract` reads it off the front of the message and answers
/// `range` and [`OUT_OF_RANGE`], where any other failure to read a field is
/// `INVALID_TYPE`. No serde or chrono message begins with it.
const OUT_OF_RANGE_SENTINEL: &str = "storable-out-of-range: ";

/// The refusal's message without its sentinel, when `message` is a body
/// date's refusal.
pub fn out_of_range_reason(message: &str) -> Option<&str> {
    message.strip_prefix(OUT_OF_RANGE_SENTINEL)
}

/// PostgreSQL's first `date`, 4714-11-24 BC, the day its first `timestamptz`
/// begins. Its last, in year 5874897, is beyond chrono's 262142, so no parsed
/// `NaiveDate` can pass it.
fn first_postgres_date() -> NaiveDate {
    NaiveDate::from_ymd_opt(-4713, 11, 24).expect("4714-11-24 BC is a date chrono holds")
}

/// Whether PostgreSQL can hold `day` as a `date`: [`within_postgres_range`]'s
/// sibling for a day.
pub fn date_within_postgres_range(day: NaiveDate) -> bool {
    day >= first_postgres_date()
}

fn storable_date<E: serde::de::Error>(day: NaiveDate) -> Result<NaiveDate, E> {
    if date_within_postgres_range(day) {
        Ok(day)
    } else {
        Err(E::custom(format!(
            "{OUT_OF_RANGE_SENTINEL}must be on or after -4713-11-24 (4714 BC), the first \
             date PostgreSQL stores"
        )))
    }
}

fn storable_instant<E: serde::de::Error>(at: DateTime<Utc>) -> Result<DateTime<Utc>, E> {
    if within_postgres_range(at) {
        Ok(at)
    } else {
        Err(E::custom(format!(
            "{OUT_OF_RANGE_SENTINEL}must be between -4713-11-24T00:00:00Z (4714 BC) and \
             294276-12-31T23:59:59.999999Z, the range PostgreSQL stores"
        )))
    }
}

/// `#[serde(deserialize_with = "crate::utils::storable::date")]` on a
/// `NaiveDate` field of a request: the date, refused while the body is read if
/// PostgreSQL cannot store it.
///
/// **A helper per field, not a newtype.** A `StorableDate` would carry the
/// bound in the type, and would also ripple into every bind, every comparison
/// and every OpenAPI `value_type` the field touches. The attribute changes
/// nothing past the read, and the source scan in
/// `tests/path_param_refusals.rs` keeps a field from going without it.
pub fn date<'de, D: Deserializer<'de>>(deserializer: D) -> Result<NaiveDate, D::Error> {
    storable_date(NaiveDate::deserialize(deserializer)?)
}

/// [`date`] for an `Option<NaiveDate>`. Name it with `#[serde(default)]`: a
/// field read through a helper is no longer optional by its type alone.
pub fn option_date<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<NaiveDate>, D::Error> {
    Option::<NaiveDate>::deserialize(deserializer)?
        .map(storable_date)
        .transpose()
}

/// [`option_date`] with `crate::utils::serde::present_or_absent`'s meaning,
/// for an `Option<Option<NaiveDate>>` whose `null` clears the column. Named
/// with `#[serde(default)]`, so an absent field stays `None`.
pub fn present_or_absent_date<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Option<NaiveDate>>, D::Error> {
    option_date(deserializer).map(Some)
}

/// [`date`] for a `DateTime<Utc>`, against `timestamptz`'s range at
/// nanosecond precision, as [`refuse_out_of_range`] compares.
pub fn instant<'de, D: Deserializer<'de>>(deserializer: D) -> Result<DateTime<Utc>, D::Error> {
    storable_instant(DateTime::<Utc>::deserialize(deserializer)?)
}

/// [`instant`] for an `Option<DateTime<Utc>>`, named with
/// `#[serde(default)]`.
pub fn option_instant<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<DateTime<Utc>>, D::Error> {
    Option::<DateTime<Utc>>::deserialize(deserializer)?
        .map(storable_instant)
        .transpose()
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

    #[derive(Debug, serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Body {
        #[serde(default, deserialize_with = "option_date")]
        day: Option<NaiveDate>,
        #[serde(default, deserialize_with = "present_or_absent_date")]
        cleared: Option<Option<NaiveDate>>,
        #[serde(default, deserialize_with = "option_instant")]
        at: Option<DateTime<Utc>>,
    }

    fn read(body: serde_json::Value) -> Result<Body, String> {
        serde_json::from_value(body).map_err(|error| error.to_string())
    }

    #[test]
    fn the_first_date_is_postgresqls_and_the_day_before_is_not() {
        let first = NaiveDate::from_ymd_opt(-4713, 11, 24).expect("a date");
        assert!(date_within_postgres_range(first));
        assert!(!date_within_postgres_range(
            first.pred_opt().expect("a day")
        ));
        assert!(date_within_postgres_range(NaiveDate::MAX));
        // The same day the first `timestamptz` begins.
        assert_eq!(
            first
                .and_hms_opt(0, 0, 0)
                .expect("midnight")
                .and_utc()
                .timestamp_micros(),
            POSTGRES_MIN_MICROS
        );
    }

    #[test]
    fn a_body_date_is_read_at_the_bound_and_refused_before_it() {
        let first = read(
            serde_json::json!({ "day": "-4713-11-24", "cleared": "-4713-11-24",
                                             "at": "-4713-11-24T00:00:00Z" }),
        )
        .expect("the first values are read");
        assert_eq!(
            first.day.map(|day| day.to_string()).as_deref(),
            Some("-4713-11-24")
        );
        assert!(first.cleared.flatten().is_some());
        assert!(first.at.is_some());

        for body in [
            serde_json::json!({ "day": "-4713-11-23" }),
            serde_json::json!({ "cleared": "-4713-11-23" }),
            serde_json::json!({ "at": "-4713-11-23T23:59:59.999999999Z" }),
        ] {
            let message = read(body.clone()).expect_err("below the range");
            assert!(
                out_of_range_reason(&message).is_some(),
                "{body}: the refusal does not carry the sentinel: {message}"
            );
        }
    }

    #[test]
    fn absent_null_and_a_wrong_type_are_what_they_were() {
        let absent = read(serde_json::json!({})).expect("nothing is required");
        assert_eq!((absent.day, absent.cleared, absent.at), (None, None, None));

        let null = read(serde_json::json!({ "day": null, "cleared": null })).expect("nulls");
        assert_eq!(
            (null.day, null.cleared),
            (None, Some(None)),
            "null still clears"
        );

        let message = read(serde_json::json!({ "day": "not a date" })).expect_err("a parse");
        assert!(out_of_range_reason(&message).is_none(), "{message}");
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
