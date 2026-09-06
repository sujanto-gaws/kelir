//! What entering a state asks for — a person, or the product (FR-WF-005,
//! [#339]).
//!
//! **`taskType` has been a vocabulary nothing reads.** [JWSS](../../../../../docs/schema/JSON%20Workflow%20Schema.md)
//! §3.1 declares six values; `graph.rs` stored the raw string, defaulted it to
//! `APPROVAL_TASK`, and nothing branched on it. So a definition could say
//! `"taskType": "SERVICE_TASK"`, validate, publish, and then generate a human
//! task that sits in somebody's inbox waiting for a person who is not coming.
//!
//! That is the worst of the three possible states — worse than refusing it,
//! because it fails silently and late.
//!
//! # Two of six are implemented, and four are refused
//!
//! **[`Self::Approval`] and [`Self::Service`].** An approval task is a row a
//! person decides; a service task is a step the engine performs and advances
//! past, running the guards of its `AUTO` transition
//! ([`super::super::service::engine`]).
//!
//! **The other four are refused at publish, naming themselves** ([#339] AC5).
//! `USER_TASK`, `REVIEW_TASK` and `DATA_ENTRY_TASK` are *plausible* as human
//! tasks — they would create the same row an approval does — and that is
//! exactly the problem: accepting them would mean four enum members behaving
//! identically while claiming to be different things, which is the defect this
//! module closes for the fifth. `SIGNATURE_TASK` is worse than plausible:
//! nothing in this product captures a signature, so a definition asking for one
//! would get an approval **recorded as a signature that never happened**.
//!
//! **A refusal is reversible and a silent substitution is not.** A definition
//! refused at publish is one sentence to the person writing it. A definition
//! accepted and quietly reinterpreted is a claim in an audit trail, and it is
//! discovered by whoever relies on it.
//!
//! # Why this is an enum and not a `CHECK`
//!
//! The column stays `VARCHAR`, because [JWSS](../../../../../docs/schema/JSON%20Workflow%20Schema.md)
//! owns this vocabulary and a database constraint would be a second copy of it
//! that a specification revision could not move. What changes is that the value
//! is **parsed** on the way in — at publish, where the author is — rather than
//! read back hopefully.
//!
//! [#339]: https://github.com/sujanto-gaws/kelir/issues/339

use std::fmt;

/// A `taskType` this engine performs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskType {
    /// A row a person decides. The default JWSS §3.1 states, and what every
    /// task before [#339] was.
    Approval,
    /// A step the engine performs. Writes no task row and fires the state's
    /// `AUTO` transition in the same transaction.
    Service,
}

impl TaskType {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Approval => "APPROVAL_TASK",
            Self::Service => "SERVICE_TASK",
        }
    }

    /// Whether this type produces a row in somebody's inbox.
    ///
    /// The one question the engine asks, named rather than matched inline: a
    /// third implemented type would answer it here and nowhere else.
    pub fn is_human(self) -> bool {
        matches!(self, Self::Approval)
    }
}

impl fmt::Display for TaskType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_db())
    }
}

/// Every value JWSS §3.1's `taskType` enum allows.
///
/// **Including the ones this engine refuses**, because the refusal has to tell
/// them apart from a typo: `REVIEW_TASK` is a real member of the vocabulary
/// that is not built, and `REVUE_TASK` is a mistake. Those deserve different
/// sentences.
pub const DECLARED: [&str; 6] = [
    "USER_TASK",
    "APPROVAL_TASK",
    "REVIEW_TASK",
    "SERVICE_TASK",
    "SIGNATURE_TASK",
    "DATA_ENTRY_TASK",
];

/// Why a `taskType` was not accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskTypeRefusal {
    /// A member of JWSS's enum that this engine does not perform.
    NotImplemented(&'static str),
    /// Not in JWSS's enum at all.
    NotDeclared(String),
}

impl fmt::Display for TaskTypeRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotImplemented(value) => write!(
                formatter,
                "`{value}` is a JWSS task type this engine does not perform, so a state \
                 declaring one would generate an ordinary approval task under another name \
                 — this build performs `APPROVAL_TASK` and `SERVICE_TASK`{}",
                signature_note(value)
            ),
            Self::NotDeclared(value) => write!(
                formatter,
                "`{value}` is not a JWSS task type at all; the vocabulary is {}",
                DECLARED
                    .iter()
                    .map(|declared| format!("`{declared}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

/// The extra sentence a signature task earns.
///
/// It is the one refusal where the substitution would be a false record rather
/// than a mislabelled one, and saying so is what stops somebody reading the
/// refusal as bureaucratic.
fn signature_note(value: &str) -> &'static str {
    if value == "SIGNATURE_TASK" {
        ". Nothing in this product captures a signature, so accepting it would record an \
         approval as a signature that never happened"
    } else {
        ""
    }
}

/// A declared `taskType`, or why it is refused.
///
/// **An absent value is [`TaskType::Approval`]**, which is JWSS §3.1's own
/// `default` rather than this function's convenience.
pub fn parse(value: Option<&str>) -> Result<TaskType, TaskTypeRefusal> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(TaskType::Approval);
    };

    match value {
        "APPROVAL_TASK" => Ok(TaskType::Approval),
        "SERVICE_TASK" => Ok(TaskType::Service),
        other => match DECLARED.iter().find(|declared| **declared == other) {
            Some(declared) => Err(TaskTypeRefusal::NotImplemented(declared)),
            None => Err(TaskTypeRefusal::NotDeclared(other.to_owned())),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn performs_the_two_it_implements() {
        assert_eq!(parse(Some("APPROVAL_TASK")), Ok(TaskType::Approval));
        assert_eq!(parse(Some("SERVICE_TASK")), Ok(TaskType::Service));
    }

    /// JWSS §3.1's own `default`, not a convenience of this parser.
    #[test]
    fn an_absent_task_type_is_an_approval() {
        assert_eq!(parse(None), Ok(TaskType::Approval));
        assert_eq!(parse(Some("   ")), Ok(TaskType::Approval));
    }

    /// **AC5.** Each of the four names itself, so an author learns which of
    /// their six choices they cannot have.
    #[test]
    fn refuses_the_four_it_does_not_perform_and_names_each() {
        for value in [
            "USER_TASK",
            "REVIEW_TASK",
            "DATA_ENTRY_TASK",
            "SIGNATURE_TASK",
        ] {
            let refusal = parse(Some(value)).expect_err("refused");

            assert_eq!(refusal, TaskTypeRefusal::NotImplemented(value));
            assert!(refusal.to_string().contains(value), "{refusal}");
        }
    }

    /// A member of the vocabulary and a typo are different mistakes, and the
    /// refusal says which.
    #[test]
    fn a_typo_is_told_apart_from_a_type_that_is_merely_unbuilt() {
        let typo = parse(Some("REVUE_TASK")).expect_err("refused");

        assert_eq!(typo, TaskTypeRefusal::NotDeclared("REVUE_TASK".to_owned()));
        // And it lists the vocabulary, because somebody who mistyped needs to
        // see the spelling rather than be told they were wrong.
        assert!(typo.to_string().contains("REVIEW_TASK"), "{typo}");
        assert!(!typo.to_string().contains("does not perform"), "{typo}");
    }

    /// The one refusal whose substitution would be a false record rather than a
    /// mislabelled one.
    #[test]
    fn a_signature_task_is_refused_with_the_reason_that_makes_it_different() {
        let refusal = parse(Some("SIGNATURE_TASK")).expect_err("refused");

        assert!(refusal.to_string().contains("never happened"), "{refusal}");

        // And the other three do not carry that sentence, which is what stops
        // it reading as boilerplate.
        let review = parse(Some("REVIEW_TASK")).expect_err("refused");

        assert!(!review.to_string().contains("never happened"), "{review}");
    }

    #[test]
    fn only_an_approval_reaches_an_inbox() {
        assert!(TaskType::Approval.is_human());
        assert!(!TaskType::Service.is_human());
    }

    /// The declared set is JWSS's, and this is what would catch it drifting
    /// from the vendored meta-schema.
    #[test]
    fn the_declared_set_is_the_meta_schemas() {
        let meta: serde_json::Value =
            serde_json::from_str(include_str!("../jwss-meta-v1.0.0.json"))
                .expect("the vendored meta-schema is valid JSON");
        let declared = meta["$defs"]["taskSpec"]["properties"]["taskType"]["enum"]
            .as_array()
            .expect("taskType declares an enum");

        assert_eq!(declared.len(), DECLARED.len());

        for value in declared {
            let value = value.as_str().expect("a string");

            assert!(
                DECLARED.contains(&value),
                "the meta-schema declares `{value}` and this module has not heard of it"
            );
        }
    }
}
