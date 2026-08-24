//! Numbering rules and scoped sequences (FR-DTYPE-004).
//!
//! A document type says how its documents are numbered — `PR-{year}-{sequence}`
//! — and a sequence advances per scope, so `PR-2026-000001` starts again at
//! `PR-2027-000001`.
//!
//! **This module is where the check-then-act shape lives**, and the project has
//! produced that defect three times: #105 (a concurrent role assignment), #133
//! (two re-parentings closing a loop) and #137 (a delete racing a child).
//! Reading a sequence, adding one, and writing it back is the same shape at its
//! purest, under concurrency, deciding a number a document keeps forever.
//! [`allocate`] is written against coding standard §2.5's rule, which those
//! three produced.
//!
//! **Two failures are specifically not acceptable**, and they pull in opposite
//! directions:
//!
//! - *A duplicate.* Two submissions in one scope taking the same number. Never
//!   acceptable, under any policy.
//! - *A consumed gap.* A number allocated and then lost to a rolled-back
//!   transaction. Acceptable for some rules and a compliance failure for
//!   others, which is why [`GapPolicy`] is a column rather than an assumption.

use chrono::{DateTime, Datelike, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::{AppError, ValidationDetail};

/// Longest `ruleTemplate` §6.3 holds — `rule_template VARCHAR(200)`.
pub const MAX_TEMPLATE_LENGTH: usize = 200;
/// The padding window `ck_document_type_numbering_rules_padding` enforces.
pub const MIN_PADDING: i32 = 1;
pub const MAX_PADDING: i32 = 20;

/// What resets the sequence (§6.3's `CHECK`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SequenceScope {
    /// One sequence for the type, forever.
    Global,
    /// Restarts each calendar year.
    Year,
    /// Restarts each calendar month.
    Month,
    /// Restarts each year, and runs separately per department.
    DepartmentYear,
}

impl SequenceScope {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Global => "GLOBAL",
            Self::Year => "YEAR",
            Self::Month => "MONTH",
            Self::DepartmentYear => "DEPARTMENT_YEAR",
        }
    }

    /// An unknown stored value reads as `Global`.
    ///
    /// Fails towards *not resetting*: a scope nobody recognises must not be
    /// read as one that restarts the sequence, because restarting it is how a
    /// number gets issued twice.
    pub fn from_db(value: &str) -> Self {
        match value {
            "YEAR" => Self::Year,
            "MONTH" => Self::Month,
            "DEPARTMENT_YEAR" => Self::DepartmentYear,
            _ => Self::Global,
        }
    }

    /// Whether this scope needs a department to identify its bucket.
    pub fn needs_department(self) -> bool {
        matches!(self, Self::DepartmentYear)
    }
}

/// Whether a rule's sequence may lose numbers to failed submissions.
///
/// The distinction is the whole of `0016_numbering_gap_policy.sql`: the two are
/// different products, they have different concurrency costs, and a schema that
/// left it implicit would leave a deployment to discover which one it had.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GapPolicy {
    /// The number is allocated inside the submitting transaction and rolls back
    /// with it. Concurrent submissions of this type serialise until commit.
    Gapless,
    /// The number is allocated and committed separately, so it survives a
    /// failed submission as a gap and the rule row is held only momentarily.
    AllowGaps,
}

impl GapPolicy {
    pub fn allows_gaps(self) -> bool {
        matches!(self, Self::AllowGaps)
    }

    pub fn from_db(allow_gaps: bool) -> Self {
        if allow_gaps {
            Self::AllowGaps
        } else {
            Self::Gapless
        }
    }
}

/// A numbering rule as the API returns it.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NumberingRule {
    pub id: Uuid,
    pub document_type_id: Uuid,
    pub rule_template: String,
    pub sequence_scope: SequenceScope,
    pub sequence_padding: i32,
    pub gap_policy: GapPolicy,
    /// The bucket the counter is currently in — `2026` for a `YEAR` rule.
    pub sequence_key: String,
    /// The number the next document of this type will take, in that bucket.
    pub next_sequence: i64,
    pub is_active: bool,
}

/// Setting a type's numbering rule.
///
/// There is no create-versus-update distinction on the wire, because
/// `uq_document_type_numbering_rules_active` allows one active rule per type:
/// a type has a numbering rule or it does not, and `PUT` says so more honestly
/// than a `POST` that conflicts the second time.
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetNumberingRuleRequest {
    pub rule_template: String,
    pub sequence_scope: SequenceScope,
    pub sequence_padding: Option<i32>,
    pub gap_policy: Option<GapPolicy>,
    /// Where the counter starts. Absent means 1, and it may not be lowered past
    /// a number already issued — see [`validate_set`].
    pub next_sequence: Option<i64>,
}

/// The placeholders a template may use.
///
/// `{sequence}` is required: a template without it names every document the
/// same thing, and the unique index on `documents.document_number` would then
/// refuse the second document of the type — at submit time, having already
/// done the work.
const SEQUENCE_PLACEHOLDER: &str = "{sequence}";
const KNOWN_PLACEHOLDERS: [&str; 4] = ["{sequence}", "{year}", "{month}", "{department}"];

/// What a number is generated against.
#[derive(Debug, Clone, Copy)]
pub struct AllocationContext {
    /// When the document is being numbered. Passed in rather than read from the
    /// clock here so a test can place a submission in a given year without
    /// waiting for one.
    pub at: DateTime<Utc>,
    /// The department the document is requested for, for a `DEPARTMENT_YEAR`
    /// rule. Any other scope ignores it.
    pub department_id: Option<Uuid>,
}

/// The bucket a context falls in, as stored in `sequence_key`.
///
/// **This is what decides when a sequence restarts**, so it is a pure function
/// with its own tests rather than an expression inside the allocator: a bucket
/// computed one way on write and another way on read is a sequence that
/// restarts at the wrong moment, and the wrong moment is "twice in one year".
pub fn scope_key(scope: SequenceScope, context: &AllocationContext) -> String {
    match scope {
        SequenceScope::Global => String::new(),
        SequenceScope::Year => context.at.year().to_string(),
        SequenceScope::Month => format!("{:04}-{:02}", context.at.year(), context.at.month()),
        SequenceScope::DepartmentYear => format!(
            "{}:{}",
            context
                .department_id
                .map(|id| id.to_string())
                // A department-scoped rule with no department is refused before
                // this is reached; the fallback keeps the key total rather than
                // silently sharing the unscoped bucket with every other one.
                .unwrap_or_else(|| "NO-DEPARTMENT".to_owned()),
            context.at.year()
        ),
    }
}

/// Renders a number from a template.
///
/// Unknown placeholders are left alone rather than blanked. A template carrying
/// `{quarter}` is a mistake, and `PR-{quarter}-000001` says which mistake —
/// where `PR--000001` looks like a formatting bug in something else.
pub fn render(template: &str, sequence: i64, padding: i32, context: &AllocationContext) -> String {
    let padded = format!(
        "{:0width$}",
        sequence,
        width = padding.clamp(MIN_PADDING, MAX_PADDING) as usize
    );

    template
        .replace(SEQUENCE_PLACEHOLDER, &padded)
        .replace("{year}", &context.at.year().to_string())
        .replace("{month}", &format!("{:02}", context.at.month()))
        .replace(
            "{department}",
            &context
                .department_id
                .map(|id| id.to_string())
                .unwrap_or_default(),
        )
}

/// Every placeholder a template uses that this renderer does not know.
fn unknown_placeholders(template: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = template;

    while let Some(start) = rest.find('{') {
        let Some(length) = rest[start..].find('}') else {
            break;
        };

        let placeholder = &rest[start..start + length + 1];

        if !KNOWN_PLACEHOLDERS.contains(&placeholder) && !found.contains(&placeholder.to_owned()) {
            found.push(placeholder.to_owned());
        }

        rest = &rest[start + length + 1..];
    }

    found
}

pub fn validate_set(
    request: &SetNumberingRuleRequest,
    issued_so_far: Option<i64>,
) -> Result<(), AppError> {
    let mut details = Vec::new();
    let template = request.rule_template.trim();

    if template.is_empty() {
        details.push(ValidationDetail::new(
            "ruleTemplate",
            "required",
            "REQUIRED",
            "ruleTemplate is required",
        ));
    } else if template.chars().count() > MAX_TEMPLATE_LENGTH {
        details.push(ValidationDetail::new(
            "ruleTemplate",
            "maxLength",
            "TOO_LONG",
            format!("ruleTemplate must be at most {MAX_TEMPLATE_LENGTH} characters"),
        ));
    } else {
        if !template.contains(SEQUENCE_PLACEHOLDER) {
            details.push(ValidationDetail::new(
                "ruleTemplate",
                "format",
                "MISSING_SEQUENCE",
                "ruleTemplate must contain {sequence}; without it every document \
                 of this type is numbered the same, and the second one is \
                 refused at submit having already done the work",
            ));
        }

        for unknown in unknown_placeholders(template) {
            details.push(ValidationDetail::new(
                "ruleTemplate",
                "format",
                "UNKNOWN_PLACEHOLDER",
                format!(
                    "`{unknown}` is not a placeholder this renderer knows; the \
                     known ones are {}",
                    KNOWN_PLACEHOLDERS.join(", ")
                ),
            ));
        }

        if request.sequence_scope == SequenceScope::DepartmentYear
            && !template.contains("{department}")
        {
            // Not fatal on its own — a department-scoped counter still restarts
            // per department — but the numbers of two departments would then be
            // indistinguishable, and the unique index on `document_number`
            // would refuse the second department's first document.
            details.push(ValidationDetail::new(
                "ruleTemplate",
                "format",
                "SCOPE_NOT_IN_TEMPLATE",
                "a DEPARTMENT_YEAR rule must put {department} in the template, \
                 or two departments produce the same number and the second is \
                 refused",
            ));
        }
    }

    if let Some(padding) = request.sequence_padding {
        if !(MIN_PADDING..=MAX_PADDING).contains(&padding) {
            details.push(ValidationDetail::new(
                "sequencePadding",
                "range",
                "OUT_OF_RANGE",
                format!("sequencePadding must be between {MIN_PADDING} and {MAX_PADDING}"),
            ));
        }
    }

    if let Some(next) = request.next_sequence {
        if next < 1 {
            details.push(ValidationDetail::new(
                "nextSequence",
                "range",
                "OUT_OF_RANGE",
                "nextSequence must be at least 1",
            ));
        } else if let Some(issued) = issued_so_far {
            if next < issued {
                // Rewinding a counter re-issues numbers that documents already
                // hold, and the collision surfaces at submit time on a document
                // that has nothing to do with the edit.
                details.push(ValidationDetail::new(
                    "nextSequence",
                    "range",
                    "ALREADY_ISSUED",
                    format!(
                        "nextSequence may not be lowered below {issued}, which \
                         this rule has already reached; rewinding it re-issues \
                         numbers documents already hold"
                    ),
                ));
            }
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

    fn context(year: i32, month: u32) -> AllocationContext {
        AllocationContext {
            at: chrono::NaiveDate::from_ymd_opt(year, month, 15)
                .expect("a date")
                .and_hms_opt(12, 0, 0)
                .expect("a time")
                .and_utc(),
            department_id: None,
        }
    }

    fn request(template: &str, scope: SequenceScope) -> SetNumberingRuleRequest {
        SetNumberingRuleRequest {
            rule_template: template.to_owned(),
            sequence_scope: scope,
            sequence_padding: Some(6),
            gap_policy: None,
            next_sequence: None,
        }
    }

    fn details(error: AppError) -> Vec<ValidationDetail> {
        match error {
            AppError::Validation { details } => details,
            other => panic!("expected a validation error, got {other:?}"),
        }
    }

    #[test]
    fn renders_the_registry_example() {
        assert_eq!(
            render("PR-{year}-{sequence}", 123, 6, &context(2026, 8)),
            "PR-2026-000123"
        );
    }

    #[test]
    fn pads_to_the_configured_width() {
        assert_eq!(render("{sequence}", 7, 3, &context(2026, 8)), "007");
        assert_eq!(render("{sequence}", 7, 1, &context(2026, 8)), "7");
    }

    #[test]
    fn a_sequence_wider_than_its_padding_is_not_truncated() {
        // Padding is a minimum width, not a maximum. Truncating would issue a
        // number that collides with one issued 1,000,000 documents ago.
        assert_eq!(
            render("{sequence}", 1_234_567, 6, &context(2026, 8)),
            "1234567"
        );
    }

    #[test]
    fn renders_the_month_two_digits_wide() {
        // `2026-8` sorts after `2026-10`, and a document number is sorted far
        // more often than it is parsed.
        assert_eq!(
            render("INV-{year}{month}-{sequence}", 1, 4, &context(2026, 8)),
            "INV-202608-0001"
        );
    }

    #[test]
    fn leaves_an_unknown_placeholder_alone() {
        assert_eq!(
            render("PR-{quarter}-{sequence}", 1, 2, &context(2026, 8)),
            "PR-{quarter}-01",
            "blanking it would look like a formatting bug in something else"
        );
    }

    #[test]
    fn a_global_scope_never_changes_bucket() {
        assert_eq!(scope_key(SequenceScope::Global, &context(2026, 1)), "");
        assert_eq!(scope_key(SequenceScope::Global, &context(2031, 12)), "");
    }

    #[test]
    fn a_year_scope_changes_bucket_at_the_year_boundary_and_not_within_it() {
        assert_eq!(scope_key(SequenceScope::Year, &context(2026, 1)), "2026");
        assert_eq!(scope_key(SequenceScope::Year, &context(2026, 12)), "2026");
        assert_eq!(scope_key(SequenceScope::Year, &context(2027, 1)), "2027");
    }

    #[test]
    fn a_month_scope_changes_bucket_every_month() {
        assert_eq!(
            scope_key(SequenceScope::Month, &context(2026, 8)),
            "2026-08"
        );
        assert_eq!(
            scope_key(SequenceScope::Month, &context(2026, 9)),
            "2026-09"
        );
        assert_ne!(
            scope_key(SequenceScope::Month, &context(2026, 1)),
            scope_key(SequenceScope::Month, &context(2027, 1)),
            "January is not January: the year is part of the bucket"
        );
    }

    #[test]
    fn a_department_year_scope_separates_departments_and_years() {
        let first = Uuid::now_v7();
        let second = Uuid::now_v7();

        let mut a = context(2026, 8);
        a.department_id = Some(first);
        let mut b = context(2026, 8);
        b.department_id = Some(second);
        let mut c = context(2027, 8);
        c.department_id = Some(first);

        assert_ne!(
            scope_key(SequenceScope::DepartmentYear, &a),
            scope_key(SequenceScope::DepartmentYear, &b)
        );
        assert_ne!(
            scope_key(SequenceScope::DepartmentYear, &a),
            scope_key(SequenceScope::DepartmentYear, &c)
        );
    }

    #[test]
    fn requires_a_sequence_placeholder() {
        let details = details(
            validate_set(&request("PR-{year}", SequenceScope::Year), None).expect_err("refused"),
        );

        assert!(details
            .iter()
            .any(|detail| detail.code == "MISSING_SEQUENCE"));
    }

    #[test]
    fn refuses_a_placeholder_the_renderer_does_not_know() {
        let details = details(
            validate_set(
                &request("PR-{quarter}-{sequence}", SequenceScope::Year),
                None,
            )
            .expect_err("refused"),
        );

        assert!(details
            .iter()
            .any(|detail| detail.code == "UNKNOWN_PLACEHOLDER"));
    }

    #[test]
    fn a_department_scoped_rule_must_name_the_department_in_its_template() {
        let details = details(
            validate_set(
                &request("PR-{year}-{sequence}", SequenceScope::DepartmentYear),
                None,
            )
            .expect_err("refused"),
        );

        assert!(details
            .iter()
            .any(|detail| detail.code == "SCOPE_NOT_IN_TEMPLATE"));
    }

    #[test]
    fn accepts_a_department_scoped_rule_that_does() {
        assert!(validate_set(
            &request(
                "PR-{department}-{year}-{sequence}",
                SequenceScope::DepartmentYear
            ),
            None
        )
        .is_ok());
    }

    #[test]
    fn refuses_rewinding_a_counter_past_a_number_already_issued() {
        let mut rewind = request("PR-{year}-{sequence}", SequenceScope::Year);
        rewind.next_sequence = Some(5);

        let details = details(validate_set(&rewind, Some(42)).expect_err("refused"));

        assert!(details.iter().any(|detail| detail.code == "ALREADY_ISSUED"));
    }

    #[test]
    fn allows_advancing_a_counter() {
        // Skipping ahead is how a deployment migrating from another system
        // continues its existing numbering, and it issues nothing twice.
        let mut advance = request("PR-{year}-{sequence}", SequenceScope::Year);
        advance.next_sequence = Some(5000);

        assert!(validate_set(&advance, Some(42)).is_ok());
    }

    #[test]
    fn an_unknown_stored_scope_reads_as_global() {
        // Fails towards not resetting: a scope nobody recognises must not be
        // read as one that restarts the sequence, because restarting it is how
        // a number gets issued twice.
        assert_eq!(SequenceScope::from_db("YEAR"), SequenceScope::Year);
        assert_eq!(SequenceScope::from_db("QUARTER"), SequenceScope::Global);
    }
}
