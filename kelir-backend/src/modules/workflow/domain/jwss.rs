//! Validating a stored workflow definition (FR-WF-001, [#174] AC1, AC3).
//!
//! Three checks, and they catch different things — the arrangement
//! [`jfss`][crate::modules::rad::domain::jfss] uses one module over, for the
//! same reasons and deliberately in the same shape. A reader who has understood
//! one should recognise the other rather than have to learn it again.
//!
//! **Shape**, against the JWSS v1.0.0 meta-schema. That artifact is normative —
//! *"where this document and the Meta-Schema disagree, the Meta-Schema is
//! normative"* ([JWSS](../../../../../docs/schema/JSON%20Workflow%20Schema.md)
//! §1.3) — so a definition that does not validate is not a JWSS document and is
//! refused rather than stored.
//!
//! **Operators**, against the [Calculation Rule
//! Registry](../../../../../docs/schema/JFSS%20Calculation%20Rule%20Registry.md),
//! which JWSS §6.2 makes the bound on every `condition`, assignment
//! `expression` and variable `source`. The meta-schema deliberately does not do
//! this: its `jsonLogic` definition accepts any object. **D-10**'s engine
//! evaluates a far wider surface than the registry approves, identically on both
//! sides — and two runtimes agreeing on an operator nobody approved is two
//! runtimes agreeing on something the registry calls FORBIDDEN.
//!
//! **Structure**, S1–S4, S6, S7, S9 and S10 of JWSS §8. Reachability, dead ends,
//! duplicate triples, a transition out of a final state, a fallback that is not
//! last: none of it is expressible in JSON Schema, and §8's own closing
//! paragraph says so.
//!
//! **S5 and S12 are the two §8 rules that *are*, and they live in the
//! meta-schema alone.** Both are conditions on one transition object —
//! `AUTO` must not declare `allowedBy`, and must not demand a comment nobody
//! is there to give — and the meta-schema's `if`/`then`/`else` on that object
//! expresses each directly. Restating them in [`structural_errors`] would be a
//! second answer to a question the normative artifact already answers.
//!
//! # Which operator set a condition is bounded by, and why it is not `calculate`'s
//!
//! JWSS §6.2 says conditions use "only operators registered in the Calculation
//! Rule Registry". Read literally that forbids `<=`, which §2.3 of the registry
//! excludes from `calculate` — and the JWSS's **own worked example** in §10 uses
//! `{"<=": [...]}` in a transition condition. The specification contradicts
//! itself on this point.
//!
//! It is resolved the way `jfss.rs` already resolved the identical question for
//! `conditional.logic`, and by importing that answer rather than writing a
//! second one: a condition returns a boolean, §2.3's stated reason for
//! forbidding comparisons in `calculate` is that they return booleans rather
//! than numbers, and that reason makes them exactly what a condition wants. So
//! the bound here is [`CONDITIONAL_OPERATORS`], the same constant, and when
//! **D-15** makes that tier normative it moves for both consumers at once.
//!
//! # S8 emits nothing, and S11 is not implemented
//!
//! **S8 is a `SHOULD`** — *"every state that is the target of a non-`AUTO`
//! transition and is not final SHOULD declare a `task`; a stateless wait is a
//! publish WARNING"* — and this validator has no warning channel:
//! [`validate_definition`] returns refusals, and a `ValidationDetail` is a
//! refusal. Emitting one would turn a `SHOULD` into a `MUST` and refuse a
//! definition the specification permits.
//!
//! That is not hypothetical. [#183](https://github.com/sujanto-gaws/kelir/issues/183)
//! is the first item to depend on it: JWSS §10's own `RETURNED` state declares
//! no task, because a returned document is with its author rather than in
//! anybody's inbox, and the `RESUBMIT` edge out of it is authorized by its
//! `allowedBy`. **A stateless wait is the shape return has**, so refusing it
//! would refuse the specification's own example.
//!
//! What is missing is the warning, not the permission — a definition whose
//! *approval* state forgot its task publishes silently and generates work for
//! nobody. That needs a channel this API does not have, and it is recorded here
//! rather than left to be discovered.
//!
//! # S11 is not implemented, and this file is where a reader finds that out
//!
//! S11 resolves `guards` and `actions` handler references at publish. There is
//! nothing to resolve them against: `document_lifecycle_hooks` has no reader,
//! and the hook chain of architectures/01 §12.4.2 is unbuilt. So handler entries
//! are **stored and not executed** — accepted rather than refused, so that a
//! definition authored today does not have to be rewritten when the chain lands.
//! [`super::super::service::engine`] states in one place that it does not invoke
//! them, because a stored handler must not read as evidence that it runs.
//!
//! [#174]: https://github.com/sujanto-gaws/kelir/issues/174

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::OnceLock;

use serde_json::Value;

use super::task_type;
use crate::error::ValidationDetail;
use crate::modules::rad::domain::jfss::{check_operators, CONDITIONAL_OPERATORS};

/// The meta-schema, vendored into the crate.
///
/// A copy of `docs/schema/jwss-meta-v1.0.0.json`, which stays canonical. It is
/// duplicated for [`jfss`][crate::modules::rad::domain::jfss]'s reason: the
/// release image builds from the `kelir-backend` directory alone, so
/// `include_str!` cannot reach `docs/`, and a validator that read the schema off
/// disk at startup would make a deployment's correctness depend on a file
/// somebody remembered to copy. `tests/workflow_jwss_meta_schema.rs` compares
/// the two, so the duplicate cannot drift quietly.
const META_SCHEMA: &str = include_str!("../jwss-meta-v1.0.0.json");

/// The platform statuses a state may map onto (§6.6's `CHECK`, JWSS §3).
///
/// Listed here as well as in the meta-schema because S9 needs to reason about
/// the set — *at least one state maps to `COMPLETED` or `CANCELLED`* — and a
/// rule that reads a vocabulary it cannot name is a rule written twice.
///
/// **`DRAFT` is in this list on purpose, and `RESOLVABLE_ASSIGNEE_TYPES` below
/// is what narrowing looks like when narrowing is right** (**D-46**, from
/// [#278](https://github.com/sujanto-gaws/kelir/issues/278)). A non-final state
/// mapping to `DRAFT` leaves the document editable while its approval runs, and
/// that is what let a discard strand a live process. The fix was the guard, not
/// this set: `DRAFT` is [JWSS §10]'s own example, Kelir projects it correctly,
/// and `document::service::delete_document` now asks `workflow_instances`
/// rather than reading a status as a proxy for one. `super`'s header carries
/// the reasoning.
///
/// [JWSS §10]: ../../../../../docs/schema/JSON%20Workflow%20Schema.md
const DOCUMENT_STATUSES: &[&str] = &[
    "DRAFT",
    "SUBMITTED",
    "IN_REVIEW",
    "PENDING_APPROVAL",
    "APPROVED",
    "REJECTED",
    "RETURNED",
    "COMPLETED",
    "ARCHIVED",
    "CANCELLED",
];

/// The `assigneeType` values this implementation can resolve
/// ([JWSS §5.3](../../../../../docs/schema/JSON%20Workflow%20Schema.md)).
///
/// JWSS declares six. `MANAGER_OF_OWNER` and `EXPRESSION` are absent and are
/// refused **at save** by [`assignment_errors`], naming what would have to exist
/// first — see that function. This is a registry-level narrowing of an open
/// vocabulary, the same relationship `domain::lookup::LookupSource` has to
/// JFSS's open component `type`.
pub const RESOLVABLE_ASSIGNEE_TYPES: &[&str] = &["USER", "ROLE", "DEPARTMENT_ROLE", "OWNER"];

/// The compiled meta-schema. Compiled once — compiling it per request would put
/// ~6 KB of JSON Schema on the save path of every workflow.
fn validator() -> &'static jsonschema::Validator {
    static VALIDATOR: OnceLock<jsonschema::Validator> = OnceLock::new();

    VALIDATOR.get_or_init(|| {
        let schema: Value =
            serde_json::from_str(META_SCHEMA).expect("the vendored JWSS meta-schema is valid JSON");

        jsonschema::validator_for(&schema).expect("the vendored JWSS meta-schema compiles")
    })
}

/// Validates a workflow definition, returning every problem rather than the
/// first.
///
/// Every problem, because a workflow definition is written by a person and a
/// validator that reports one error per round trip turns a ten-mistake document
/// into ten round trips.
pub fn validate_definition(definition: &Value) -> Vec<ValidationDetail> {
    let mut details = shape_errors(definition);

    // The structural rules read a document the shape check may have just
    // refused, so they are written to tolerate anything: a missing `states` is
    // an empty list here rather than a panic. Reporting a caller's *other*
    // mistakes alongside their first one is worth more than the tidiness of
    // stopping.
    details.extend(operator_errors(definition));
    details.extend(assignment_errors(definition));
    details.extend(task_type_errors(definition));
    // LHCS §2: *a reference MUST resolve at registration time.* Publishing is
    // that moment for a workflow definition, and `hook::service` owns the
    // question so that a document type's registrations and a workflow's are
    // resolved by one piece of code (#339).
    details.extend(crate::modules::hook::service::registration_errors(
        definition,
    ));
    details.extend(structural_errors(definition));
    details
}

/// Every state's `taskType`, resolved against what this engine performs
/// ([#339](https://github.com/sujanto-gaws/kelir/issues/339) AC4, AC5).
///
/// **The meta-schema checks the vocabulary and this checks the engine**, and
/// the two are different questions. `taskType` has an `enum` in §3.1, so a
/// value outside it was already refused by [`shape_errors`] — what was *not*
/// refused is a value inside it that nothing performs, and there were four of
/// those. A `SERVICE_TASK` published happily and then generated a human task
/// waiting for a person who was not coming.
///
/// Refused **at publish**, which is where the author is. The alternative is the
/// state this replaces: a definition accepted, an instance started, and a
/// stalled approval discovered by whoever was waiting for it.
fn task_type_errors(definition: &Value) -> Vec<ValidationDetail> {
    let mut details = Vec::new();

    for (index, state) in states(definition).iter().enumerate() {
        let Some(task) = state.get("task") else {
            continue;
        };

        let declared = task.get("taskType").and_then(Value::as_str);

        if let Err(refusal) = task_type::parse(declared) {
            details.push(detail(
                format!("definition.states.{index}.task.taskType"),
                "S3",
                "TASK_TYPE_NOT_PERFORMED",
                refusal.to_string(),
            ));
        }
    }

    details
}

/// Meta-schema violations, as validation details naming the JSON path.
fn shape_errors(definition: &Value) -> Vec<ValidationDetail> {
    validator()
        .iter_errors(definition)
        .map(|error| {
            // `instance_path` is a JSON Pointer (`/states/0/code`); the
            // envelope's `path` is dotted, which is what every other validation
            // detail in this API uses.
            let path = error.instance_path().to_string();
            let dotted = path.trim_start_matches('/').replace('/', ".");
            let dotted = if dotted.is_empty() {
                "definition".to_owned()
            } else {
                format!("definition.{dotted}")
            };

            ValidationDetail::new(dotted, "jwss", "INVALID_DEFINITION", error.to_string())
        })
        .collect()
}

/// Every JSON Logic expression in the document, checked against the one
/// approved set (S10).
fn operator_errors(definition: &Value) -> Vec<ValidationDetail> {
    let mut details = Vec::new();

    for (index, transition) in transitions(definition).iter().enumerate() {
        if let Some(condition) = transition.get("condition") {
            check_operators(
                condition,
                CONDITIONAL_OPERATORS,
                &format!("definition.transitions.{index}.condition"),
                &mut details,
            );
        }

        if let Some(expression) = transition
            .get("allowedBy")
            .and_then(|rule| rule.get("expression"))
        {
            check_operators(
                expression,
                CONDITIONAL_OPERATORS,
                &format!("definition.transitions.{index}.allowedBy.expression"),
                &mut details,
            );
        }
    }

    for (index, variable) in array(definition, "variables").iter().enumerate() {
        if let Some(source) = variable.get("source") {
            check_operators(
                source,
                CONDITIONAL_OPERATORS,
                &format!("definition.variables.{index}.source"),
                &mut details,
            );
        }
    }

    for (index, state) in states(definition).iter().enumerate() {
        if let Some(expression) = state
            .get("task")
            .and_then(|task| task.get("assignment"))
            .and_then(|rule| rule.get("expression"))
        {
            check_operators(
                expression,
                CONDITIONAL_OPERATORS,
                &format!("definition.states.{index}.task.assignment.expression"),
                &mut details,
            );
        }
    }

    details
}

/// Assignment rules this implementation cannot resolve, refused at save.
///
/// **Refused rather than deferred to run time**, which is `jfss.rs`'s discipline
/// applied to the other definition kind: a definition is written once and
/// executed many times, and the execution path has no good failure. A workflow
/// that publishes cleanly and then cannot assign its first task is a stalled
/// instance nobody is told about, which is exactly the outcome
/// [#176](https://github.com/sujanto-gaws/kelir/issues/176) AC1 names.
///
/// The message names what would have to exist, because "unsupported" tells the
/// person writing the workflow nothing they can act on.
fn assignment_errors(definition: &Value) -> Vec<ValidationDetail> {
    let mut details = Vec::new();

    for (index, state) in states(definition).iter().enumerate() {
        if let Some(rule) = state.get("task").and_then(|task| task.get("assignment")) {
            check_assignee_type(
                rule,
                &format!("definition.states.{index}.task.assignment"),
                &mut details,
            );
        }
    }

    for (index, transition) in transitions(definition).iter().enumerate() {
        if let Some(rule) = transition.get("allowedBy") {
            check_assignee_type(
                rule,
                &format!("definition.transitions.{index}.allowedBy"),
                &mut details,
            );
        }
    }

    details
}

fn check_assignee_type(rule: &Value, path: &str, details: &mut Vec<ValidationDetail>) {
    // The §5.2 shorthand strings normalize to `OWNER`, `ROLE` and `USER`, all
    // three of which are resolvable, so a string form is never refused here.
    let Some(assignee_type) = rule.get("assigneeType").and_then(Value::as_str) else {
        return;
    };

    if RESOLVABLE_ASSIGNEE_TYPES.contains(&assignee_type) {
        return;
    }

    let reason = match assignee_type {
        "MANAGER_OF_OWNER" => {
            "there is no user-to-manager relation in this schema — `departments.manager_party_id` \
             names a party, and party-to-user is not resolvable (FR-ORG-002 is unbuilt). Use \
             `DEPARTMENT_ROLE` with the approving role, which is what a manager approval means in \
             terms Kelir can resolve"
        }
        "EXPRESSION" => {
            "an expression resolving to a principal needs a directory context this engine does not \
             build; §6.1's context is document, formData, variables and actor. Use `ROLE` or \
             `DEPARTMENT_ROLE`"
        }
        _ => "it is not one of the assignee types this engine resolves",
    };

    details.push(ValidationDetail::new(
        format!("{path}.assigneeType"),
        "registry",
        "ASSIGNEE_TYPE_NOT_RESOLVABLE",
        format!(
            "`{assignee_type}` cannot be resolved: {reason}. Kelir resolves {} \
             (JWSS §5.3)",
            RESOLVABLE_ASSIGNEE_TYPES.join(", ")
        ),
    ));
}

/// JWSS §8's structural rules, in order, less the four this function does not own.
///
/// S5 and S12 belong to the meta-schema; **S8 is a `SHOULD` with no warning
/// channel to emit into**, and S11 has nothing to resolve against. The module
/// documentation says why for each. The rules are numbered in the `rule` field
/// of each detail so that a caller can look one up in the specification rather
/// than pattern-matching on prose.
fn structural_errors(definition: &Value) -> Vec<ValidationDetail> {
    let mut details = Vec::new();

    let states = states(definition);
    let transitions = transitions(definition);

    let mut codes: BTreeSet<&str> = BTreeSet::new();
    let mut finals: BTreeSet<&str> = BTreeSet::new();
    let mut mapped_statuses: BTreeSet<&str> = BTreeSet::new();

    for (index, state) in states.iter().enumerate() {
        let Some(code) = state.get("code").and_then(Value::as_str) else {
            continue;
        };

        // S3, first half.
        if !codes.insert(code) {
            details.push(detail(
                format!("definition.states.{index}.code"),
                "S3",
                "DUPLICATE_STATE",
                format!("`{code}` is declared more than once; state codes are unique"),
            ));
        }

        if state
            .get("isFinal")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            finals.insert(code);
        }

        // S9, first half. The meta-schema enumerates the same set, so this only
        // fires on a document that already failed the shape check — and it
        // fires anyway, because a caller with two mistakes should learn both.
        match state.get("mapsToDocumentStatus").and_then(Value::as_str) {
            Some(status) if DOCUMENT_STATUSES.contains(&status) => {
                mapped_statuses.insert(status);
            }
            Some(status) => details.push(detail(
                format!("definition.states.{index}.mapsToDocumentStatus"),
                "S9",
                "UNKNOWN_DOCUMENT_STATUS",
                format!("`{status}` is not a platform document status"),
            )),
            None => {}
        }
    }

    // S1. The declared initial state is also what S6 walks forward from, so it
    // is carried rather than looked up twice.
    let declared_initial = definition.get("initialState").and_then(Value::as_str);
    let initial = match declared_initial {
        Some(initial) if !codes.contains(initial) => {
            details.push(detail(
                "definition.initialState",
                "S1",
                "UNDECLARED_STATE",
                format!("`{initial}` is not a declared state"),
            ));
            None
        }
        Some(initial) if finals.contains(initial) => {
            details.push(detail(
                "definition.initialState",
                "S1",
                "INITIAL_STATE_IS_FINAL",
                format!("`{initial}` is final, so nothing that starts there can move"),
            ));
            Some(initial)
        }
        other => other,
    };

    // S2, S3's second half, S4, and the ends of each edge.
    let mut triples: BTreeSet<(String, String, String)> = BTreeSet::new();

    for (index, transition) in transitions.iter().enumerate() {
        let from = transition.get("from").and_then(Value::as_str).unwrap_or("");
        let to = transition.get("to").and_then(Value::as_str).unwrap_or("");
        let action = transition
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or("");

        for (end, code) in [("from", from), ("to", to)] {
            if !code.is_empty() && !codes.contains(code) {
                details.push(detail(
                    format!("definition.transitions.{index}.{end}"),
                    "S2",
                    "UNDECLARED_STATE",
                    format!("`{code}` is not a declared state"),
                ));
            }
        }

        if !triples.insert((from.to_owned(), action.to_owned(), to.to_owned())) {
            details.push(detail(
                format!("definition.transitions.{index}"),
                "S3",
                "DUPLICATE_TRANSITION",
                format!("`{from}` --{action}--> `{to}` is declared more than once"),
            ));
        }

        // S4.
        if finals.contains(from) {
            details.push(detail(
                format!("definition.transitions.{index}.from"),
                "S4",
                "TRANSITION_FROM_FINAL",
                format!("`{from}` is final and nothing leaves a final state"),
            ));
        }
    }

    // S5 and S12 are the meta-schema's `if`/`then`/`else` on the transition
    // object, which is why neither has an arm here: `AUTO` must not declare
    // `allowedBy` and everything else must (S5), and `AUTO` must not declare
    // `requiresComment: true` (S12) because there is no caller to give one, so
    // the edge could never fire. Restating either would be a second answer to a
    // question the normative artifact already answers.

    details.extend(reachability_errors(initial, &codes, &finals, transitions));
    details.extend(fallback_errors(transitions));

    // S9, second half.
    if !states.is_empty()
        && !mapped_statuses.contains("COMPLETED")
        && !mapped_statuses.contains("CANCELLED")
    {
        details.push(detail(
            "definition.states",
            "S9",
            "NO_TERMINAL_STATUS",
            "no state maps to COMPLETED or CANCELLED, so no document this workflow \
             drives can ever finish",
        ));
    }

    details
}

/// S6 — every state reachable from `initialState`, and a final state reachable
/// from every non-final state.
///
/// This is [#174](https://github.com/sujanto-gaws/kelir/issues/174) AC3 in full:
/// *"a definition whose transitions do not form a reachable, terminating graph
/// is refused at save time rather than at run time. A workflow that can deadlock
/// is a workflow that will."*
///
/// Two searches rather than one, because the two failures are different and a
/// caller fixes them differently. An **orphan** is a state nothing routes to —
/// usually a typo or an edge somebody forgot. A **dead end** is a state a
/// document can enter and never leave, which is a stuck approval and the more
/// expensive of the two.
fn reachability_errors<'a>(
    initial: Option<&'a str>,
    codes: &BTreeSet<&'a str>,
    finals: &BTreeSet<&'a str>,
    transitions: &'a [Value],
) -> Vec<ValidationDetail> {
    let mut details = Vec::new();

    if codes.is_empty() {
        return details;
    }

    let mut forward: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut backward: BTreeMap<&str, Vec<&str>> = BTreeMap::new();

    for transition in transitions {
        let (Some(from), Some(to)) = (
            transition.get("from").and_then(Value::as_str),
            transition.get("to").and_then(Value::as_str),
        ) else {
            continue;
        };

        if !codes.contains(from) || !codes.contains(to) {
            // Already reported by S2. Walking an edge that names a state the
            // definition does not declare would produce a second complaint
            // about one mistake.
            continue;
        }

        forward.entry(from).or_default().push(to);
        backward.entry(to).or_default().push(from);
    }

    if let Some(initial) = initial {
        for orphan in codes.difference(&walk(&forward, [initial])) {
            details.push(detail(
                "definition.states",
                "S6",
                "UNREACHABLE_STATE",
                format!("`{orphan}` cannot be reached from the initial state"),
            ));
        }
    }

    // Backwards from every final state: anything the search does not touch
    // cannot reach one.
    let can_finish = walk(&backward, finals.iter().copied());

    for state in codes.difference(&can_finish) {
        if finals.contains(state) {
            continue;
        }

        details.push(detail(
            "definition.states",
            "S6",
            "DEAD_END_STATE",
            format!(
                "no final state is reachable from `{state}`, so a document that gets \
                 there can never finish"
            ),
        ));
    }

    details
}

/// S7 — transitions sharing `from` + `action` have at most one fallback.
///
/// The specification also says the fallback "is evaluated last regardless of
/// document order", which is the *engine's* obligation rather than the
/// validator's; [`super::super::service::engine`] honours it and says so.
fn fallback_errors(transitions: &[Value]) -> Vec<ValidationDetail> {
    let mut fallbacks: BTreeMap<(&str, &str), usize> = BTreeMap::new();
    let mut details = Vec::new();

    for transition in transitions {
        if transition.get("condition").is_some() {
            continue;
        }

        let key = (
            transition.get("from").and_then(Value::as_str).unwrap_or(""),
            transition
                .get("action")
                .and_then(Value::as_str)
                .unwrap_or(""),
        );

        *fallbacks.entry(key).or_default() += 1;
    }

    for ((from, action), count) in fallbacks {
        if count > 1 {
            details.push(detail(
                "definition.transitions",
                "S7",
                "AMBIGUOUS_FALLBACK",
                format!(
                    "{count} transitions leave `{from}` on {action} with no condition; at \
                     most one may be the fallback, or which one fires depends on \
                     document order"
                ),
            ));
        }
    }

    details
}

/// A breadth-first walk of an adjacency map from a set of roots, roots included.
fn walk<'a>(
    edges: &BTreeMap<&'a str, Vec<&'a str>>,
    roots: impl IntoIterator<Item = &'a str>,
) -> BTreeSet<&'a str> {
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut queue: VecDeque<&str> = VecDeque::new();

    for root in roots {
        if seen.insert(root) {
            queue.push_back(root);
        }
    }

    while let Some(node) = queue.pop_front() {
        for next in edges.get(node).into_iter().flatten() {
            if seen.insert(next) {
                queue.push_back(next);
            }
        }
    }

    seen
}

fn detail(
    path: impl Into<String>,
    rule: &str,
    code: &str,
    message: impl Into<String>,
) -> ValidationDetail {
    ValidationDetail::new(path, rule, code, message)
}

fn states(definition: &Value) -> &[Value] {
    array(definition, "states")
}

fn transitions(definition: &Value) -> &[Value] {
    array(definition, "transitions")
}

/// A named array of the document, or an empty slice.
///
/// Empty rather than `Option`, because every caller of it runs *after* a shape
/// check that may already have refused the document: a missing `states` is a
/// meta-schema violation somebody has been told about, and the structural rules
/// exist to report the caller's other mistakes alongside it rather than to
/// stop at the first.
fn array<'a>(definition: &'a Value, key: &str) -> &'a [Value] {
    definition
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}
