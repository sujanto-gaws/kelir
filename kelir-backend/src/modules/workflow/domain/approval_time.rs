//! How long the caller's documents took to be decided (FR-RPT-006, [#461]).
//!
//! # One time per document, and what it runs between
//!
//! **D-83** (product owner, 2026-09-16) fixed the measurement, and this file is
//! where the arithmetic over it lives: a document's approval time runs from the
//! `started_at` of its **first** `workflow_instances` row to the `completed_at`
//! of the instance that ended `APPROVED` or `REJECTED`. Which documents have one
//! at all — the caller's own, decided inside the window, last instance a
//! decision — is the statement's to say, in
//! `workflow::repository::instance::decided_document_seconds`; this file takes
//! the times it returns and summarises them.
//!
//! **Returned rounds count, however the definition shapes them.** A `RETURN`
//! into a state that is not final leaves the instance running and a resubmission
//! moves the same instance on (`document::service::submit`'s
//! `resubmit_workflow`), so the one instance's `started_at` is already the first
//! submission. A definition whose return state *is* final ends the instance with
//! `RETURNED`, and a later submission starts another — which is why the start is
//! the earliest instance's rather than the deciding one's. Both shapes measure
//! from the moment the document was first sent.
//!
//! # The summary is computed here rather than in SQL
//!
//! `percentile_cont` would have answered the median in the statement. It is
//! done here instead because **what a median of an even count is** is a
//! decision, and a decision belongs where a unit test can hold it: the mean of
//! the middle two, rounded down to the second. The statement returns one number
//! per decided document in the window — a person's own ninety days of
//! decisions, not the tenant's — so carrying them across costs nothing a
//! dashboard load would notice.
//!
//! [#461]: https://github.com/sujanto-gaws/kelir/issues/461

use serde::Serialize;
use utoipa::ToSchema;

/// The approval time card's numbers (FR-RPT-006, [#461]).
///
/// **Seconds, not a formatted duration.** How to say *two days and four hours*
/// is the screen's, and a server that picked a unit would be choosing a
/// rounding a client could not undo.
///
/// [#461]: https://github.com/sujanto-gaws/kelir/issues/461
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalTime {
    /// How far back a decision counts, in days — the window the numbers below
    /// were read over, so a card can say *in the last 90 days* without holding
    /// its own copy of the number.
    pub window_days: i32,
    /// How many of the caller's documents were decided inside the window: the
    /// count behind [`Self::median_seconds`] and [`Self::slowest_seconds`].
    pub documents: i64,
    /// The middle approval time, in seconds. With an even count it is the mean
    /// of the middle two, rounded down to the second.
    ///
    /// **`null` when [`Self::documents`] is zero**, rather than `0`: a median of
    /// nothing is not *instant*, and a card reading zero seconds would say so.
    pub median_seconds: Option<i64>,
    /// The longest approval time, in seconds; `null` when nothing was decided.
    pub slowest_seconds: Option<i64>,
}

impl ApprovalTime {
    /// Summarises one approval time per decided document.
    ///
    /// The times arrive in no promised order — the statement has no `ORDER BY`
    /// over them, and the summary does not ask it for one — so they are sorted
    /// here before the middle is read.
    pub fn summarise(window_days: i32, mut seconds: Vec<i64>) -> Self {
        seconds.sort_unstable();

        let count = seconds.len();
        let median_seconds = match count {
            0 => None,
            _ if count % 2 == 1 => Some(seconds[count / 2]),
            _ => Some((seconds[count / 2 - 1] + seconds[count / 2]).div_euclid(2)),
        };

        Self {
            window_days,
            documents: i64::try_from(count).unwrap_or(i64::MAX),
            median_seconds,
            slowest_seconds: seconds.last().copied(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 86_400;

    #[test]
    fn nothing_decided_has_a_zero_count_and_no_times() {
        assert_eq!(
            ApprovalTime::summarise(90, vec![]),
            ApprovalTime {
                window_days: 90,
                documents: 0,
                median_seconds: None,
                slowest_seconds: None,
            }
        );
    }

    #[test]
    fn an_odd_count_takes_the_middle_time_whatever_order_the_times_arrive_in() {
        // Neither ascending nor descending, so a summary that read the middle
        // position without sorting gets 10 days, and one that took the first
        // or last as the slowest gets 2 or 1.
        let summary = ApprovalTime::summarise(90, vec![2 * DAY, 10 * DAY, DAY]);

        assert_eq!(summary.documents, 3);
        assert_eq!(summary.median_seconds, Some(2 * DAY));
        assert_eq!(summary.slowest_seconds, Some(10 * DAY));
    }

    #[test]
    fn an_even_count_takes_the_mean_of_the_middle_two_rounded_down() {
        // The middle two are 3 and 8 seconds: the mean is 5.5, which is neither
        // of them, so taking the lower or the upper middle fails here — and the
        // half second is dropped rather than rounded up.
        let summary = ApprovalTime::summarise(90, vec![40, 8, 1, 3]);

        assert_eq!(summary.documents, 4);
        assert_eq!(summary.median_seconds, Some(5));
        assert_eq!(summary.slowest_seconds, Some(40));
    }

    #[test]
    fn one_document_is_its_own_median_and_its_own_slowest() {
        let summary = ApprovalTime::summarise(30, vec![7 * DAY]);

        assert_eq!(summary.window_days, 30);
        assert_eq!(summary.median_seconds, Some(7 * DAY));
        assert_eq!(summary.slowest_seconds, Some(7 * DAY));
    }

    #[test]
    fn the_card_serialises_in_camel_case_with_nulls_for_nothing_decided() {
        assert_eq!(
            serde_json::to_value(ApprovalTime::summarise(90, vec![])).expect("serialise"),
            serde_json::json!({
                "windowDays": 90,
                "documents": 0,
                "medianSeconds": null,
                "slowestSeconds": null,
            })
        );
    }
}
