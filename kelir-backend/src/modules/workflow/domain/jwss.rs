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
//! Since R-11 (2026-10-09), JWSS §6.2 names the registry's **§2.5 conditional
//! tier**: the tier a form's `conditional.logic` uses, §2.1 and §2.2 plus the
//! comparison, logical, membership and presence operators that §2.3 forbids in
//! `calculate`. That is what lets the JWSS's own worked example in §10 put
//! `{"<=": [...]}` in a transition condition.
//!
//! The reason is the one `jfss.rs` gives for `conditional.logic`, and this file
//! imports that answer rather than writing a second one: a condition returns a
//! boolean, §2.3 forbids comparisons in `calculate` because they return
//! booleans rather than numbers, and that makes them exactly what a condition
//! wants. So the bound here is [`CONDITIONAL_OPERATORS`], the same constant,
//! and it holds every `condition`, assignment `expression` and variable
//! `source` to §2.5. A change to that tier moves both consumers at once.
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

use super::graph::RoleReference;
use super::task_type;
use crate::error::ValidationDetail;
use crate::modules::rad::domain::engine::{reads_computed_path, visit_paths};
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

/// The detail code a publish refuses a role that is not live with
/// ([`dead_role_errors`]).
pub const ROLE_NOT_LIVE: &str = "ROLE_NOT_LIVE";

/// The detail code a publish refuses a `departmentScope` on a `ROLE` rule with
/// ([`role_scope_errors`]).
pub const ROLE_TAKES_NO_SCOPE: &str = "ROLE_TAKES_NO_SCOPE";

/// The refusal for a condition reading a path the engine does not build
/// ([#706](https://github.com/sujanto-gaws/kelir/issues/706), **D-115** B).
pub const CONDITION_PATH_NOT_BUILT: &str = "CONDITION_PATH_NOT_BUILT";

/// The refusal for a condition working out the path it reads as it runs
/// ([#789](https://github.com/sujanto-gaws/kelir/issues/789), JWSS R-18).
///
/// A code of its own because [`CONDITION_PATH_NOT_BUILT`]'s message names the
/// path read, and here there is none to name.
pub const CONDITION_PATH_COMPUTED: &str = "CONDITION_PATH_COMPUTED";

/// The keys of the `document` half of JWSS §6.1's condition context.
///
/// **The engine's builder is held to this list**:
/// `service::engine::document_facts` spells these keys and no other, and its
/// unit test asserts the two agree, so a key added to one and not the other is
/// a failing test rather than a publish that refuses a path the engine builds.
/// `amount` is absent on purpose (**D-115** B): a condition wanting the amount
/// reads `formData.amount`.
pub const DOCUMENT_FACTS: &[&str] = &[
    "status",
    "documentTypeId",
    "documentNumber",
    "documentTypeKey",
];

/// The keys of the `actor` half, held to `service::engine::actor_facts` the
/// same way.
pub const ACTOR_FACTS: &[&str] = &["userId", "roles", "departmentId"];

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

/// The refusal a publish gives for each role reference that is not live
/// ([JWSS §5.3](../../../../../docs/schema/JSON%20Workflow%20Schema.md),
/// **D-111**, [#572]).
///
/// **Pure, and the read is the caller's.** `live` is the role codes
/// `service::definition::publish_definition` found live in the tenant, read
/// `FOR KEY SHARE` in the publish's transaction. Each reference whose code is
/// not among them is one detail, in the shape the S-rule details use, so a
/// definition naming one dead role in three places is told about all three.
///
/// A live role is a `roles` row in the tenant whose `deleted_at` is `NULL`.
/// A role has no other state that stops it resolving: it has no status, and a
/// role nobody holds still resolves, its tasks waiting for a holder.
///
/// [#572]: https://github.com/sujanto-gaws/kelir/issues/572
pub fn dead_role_errors(
    references: &[RoleReference],
    live: &BTreeSet<String>,
) -> Vec<ValidationDetail> {
    references
        .iter()
        .filter(|reference| !live.contains(&reference.role_code))
        .map(|reference| {
            detail(
                reference.path.clone(),
                "liveRole",
                ROLE_NOT_LIVE,
                format!(
                    "`{}` is not a live role in this tenant: it was deleted, or no role has \
                     that code. Every task or decision it decides would be refused as \
                     ASSIGNMENT_UNRESOLVED, so the definition is not published. Create the \
                     role, or name a live one (JWSS §5.3)",
                    reference.role_code
                ),
            )
        })
        .collect()
}

/// The refusal a publish gives for each `ROLE` rule carrying a
/// `departmentScope` ([JWSS §5.3](../../../../../docs/schema/JSON%20Workflow%20Schema.md),
/// **D-114** A, [#613]).
///
/// **A `ROLE` rule resolves tenant-wide, and takes no scope.**
/// `service::assignment::normalize` builds a `ROLE` principal with no
/// department and never reads the key, so a scope on one reads as "the
/// approver of that department" and offers the task, or the decision, to every
/// holder of the role in the tenant. `DEPARTMENT_ROLE` is the type that takes
/// one. The meta-schema admits the key on every type, so this is a narrowing at
/// publish, as [`dead_role_errors`] is, and not a shape rule.
///
/// **Pure, and the places are D-111's**: a state's `task.assignment` and a
/// transition's `allowedBy`, states first, each in document order, one detail a
/// place. The `"ROLE:X"` shorthand is a string and carries no scope, and an
/// escalation's `assignment` is not read, because nothing executes it (JWSS
/// §3.1). Any `departmentScope` key counts, `""` included: the author wrote one.
///
/// [#613]: https://github.com/sujanto-gaws/kelir/issues/613
pub fn role_scope_errors(definition: &Value) -> Vec<ValidationDetail> {
    let assignments = states(definition)
        .iter()
        .enumerate()
        .filter_map(|(index, state)| {
            let rule = state.get("task")?.get("assignment")?;
            Some((format!("definition.states.{index}.task.assignment"), rule))
        });
    let edges = transitions(definition)
        .iter()
        .enumerate()
        .filter_map(|(index, transition)| {
            let rule = transition.get("allowedBy")?;
            Some((format!("definition.transitions.{index}.allowedBy"), rule))
        });

    assignments
        .chain(edges)
        .filter_map(|(path, rule)| {
            if rule.get("assigneeType").and_then(Value::as_str) != Some("ROLE") {
                return None;
            }

            let scope = rule.get("departmentScope")?;
            let scope = scope
                .as_str()
                .map_or_else(|| scope.to_string(), str::to_owned);

            Some(detail(
                format!("{path}.departmentScope"),
                "roleScope",
                ROLE_TAKES_NO_SCOPE,
                format!(
                    "`{scope}` is a department scope on a ROLE rule, and a ROLE rule takes \
                     none: it is resolved across the whole tenant, so the scope would not \
                     limit who is offered the task or may take the decision. Use \
                     DEPARTMENT_ROLE to name a role within a department, or remove the \
                     scope (JWSS §5.3)"
                ),
            ))
        })
        .collect()
}

/// The refusal a publish gives for each path a transition `condition` or a
/// variable `source` reads that the engine does not build
/// ([JWSS §6.1, §6.4](../../../../../docs/schema/JSON%20Workflow%20Schema.md),
/// **D-115** B, [#706]).
///
/// **Why at publish.** A path the engine does not build reads as `null`, a
/// comparison against `null` is false, and the condition takes another branch
/// without a word: the silent fallback #706 is about. Refusing at evaluation
/// would turn approvals already running into 422s, which JWSS §9 forbids, and
/// could not tell an absent `formData.<key>` from a misspelt one. So this is a
/// narrowing at publish, as [`role_scope_errors`] is, and a revision already
/// published is not re-checked: it keeps the fallback it was published with.
///
/// **What is built**: [`DOCUMENT_FACTS`] under `document`, [`ACTOR_FACTS`]
/// under `actor` (and an index after `actor.roles`), anything under
/// `formData`, a key the definition declares under `variables`, and each bare
/// root. A `source` reading `variables.*` at instance start reads an empty
/// object, and a declared key is still accepted there.
///
/// **What is read is [`visit_paths`]'s**, the walk the form engine's
/// dependency graph uses: each `var`, `missing` and `missing_some` path, a
/// `var`'s default, an iterator's source and a `reduce`'s initial value, but
/// not an iterator's body, which addresses the element. One detail per
/// distinct path per expression, in the order it is read, transitions before
/// variables. `allowedBy.expression` and `assignment.expression` are not read:
/// `EXPRESSION` is refused at save (JWSS §5.3).
///
/// **A path worked out as the condition runs cannot be checked, so it is
/// refused** ([#789], JWSS R-18, [`CONDITION_PATH_COMPUTED`]): a `var` whose
/// operand is an object or an array opening with an object or an array, the
/// scope jump `[[1], "document.amount"]` among them, and a `missing` or
/// `missing_some` name that is an expression. [`reads_computed_path`] finds
/// them, in iterator bodies too, and the expression is told once, after its
/// unbuilt paths. At publish and not at save, as every path refusal is.
///
/// **Pure**: it reads the definition and nothing else, and takes no lock.
///
/// [#706]: https://github.com/sujanto-gaws/kelir/issues/706
/// [#789]: https://github.com/sujanto-gaws/kelir/issues/789
pub fn condition_path_errors(definition: &Value) -> Vec<ValidationDetail> {
    let variables = array(definition, "variables");
    let declared: BTreeSet<&str> = variables
        .iter()
        .filter_map(|variable| variable.get("key")?.as_str())
        .collect();

    let conditions =
        transitions(definition)
            .iter()
            .enumerate()
            .filter_map(|(index, transition)| {
                let condition = transition.get("condition")?;
                Some((
                    format!("definition.transitions.{index}.condition"),
                    condition,
                ))
            });
    let sources = variables
        .iter()
        .enumerate()
        .filter_map(|(index, variable)| {
            let source = variable.get("source")?;
            Some((format!("definition.variables.{index}.source"), source))
        });

    let mut details = Vec::new();

    for (path, expression) in conditions.chain(sources) {
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        let mut unbuilt: Vec<&str> = Vec::new();

        visit_paths(expression, &mut |read| {
            if !is_built(read, &declared) && seen.insert(read) {
                unbuilt.push(read);
            }
        });

        details.extend(unbuilt.into_iter().map(|read| {
            detail(
                path.clone(),
                "conditionPath",
                CONDITION_PATH_NOT_BUILT,
                not_built_message(read),
            )
        }));

        if reads_computed_path(expression) {
            details.push(detail(
                path,
                "conditionPath",
                CONDITION_PATH_COMPUTED,
                COMPUTED_PATH_MESSAGE.to_owned(),
            ));
        }
    }

    details
}

/// Whether the engine builds `path` in JWSS §6.1's context.
///
/// An empty path is no path: `{"var": ""}` reads the whole context, which is
/// built.
fn is_built(path: &str, declared: &BTreeSet<&str>) -> bool {
    if path.is_empty() {
        return true;
    }

    let (root, rest) = match path.split_once('.') {
        Some((root, rest)) => (root, Some(rest)),
        None => (path, None),
    };

    match (root, rest) {
        ("document" | "actor" | "formData" | "variables", None) | ("formData", Some(_)) => true,
        ("document", Some(key)) => DOCUMENT_FACTS.contains(&key),
        ("actor", Some(key)) => {
            ACTOR_FACTS.contains(&key)
                || key.strip_prefix("roles.").is_some_and(|index| {
                    !index.is_empty() && index.bytes().all(|byte| byte.is_ascii_digit())
                })
        }
        ("variables", Some(key)) => declared.contains(key.split('.').next().unwrap_or(key)),
        _ => false,
    }
}

/// What a refusal of a computed path says. It names no path: not knowing the
/// path is the fault.
const COMPUTED_PATH_MESSAGE: &str = "A `var`, `missing` or `missing_some` in this expression \
     works out the path it reads as it runs, or climbs out of its scope to read it, so a \
     publish cannot check the path: one the engine does not build would read as null and the \
     condition would take another branch without saying so. Write the path as a string, with \
     any default beside it (JWSS §6.4).";

/// What a refusal of `read` says: the path, why it is refused, and what the
/// engine builds.
fn not_built_message(read: &str) -> String {
    let built = DOCUMENT_FACTS
        .iter()
        .map(|key| format!("document.{key}"))
        .chain(ACTOR_FACTS.iter().map(|key| format!("actor.{key}")))
        .collect::<Vec<_>>()
        .join(", ");
    let instead = if read == "document.amount" {
        " The amount is not a document fact: read `formData.amount`."
    } else {
        ""
    };

    format!(
        "`{read}` is not a path the engine builds, so it would read as null and the \
         condition would take another branch without saying so. A condition reads \
         {built}, formData.<key>, or variables.<key> for a variable this definition \
         declares (JWSS §6.1).{instead}"
    )
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::rad::evaluator::RuleEvaluator;

    fn reference(path: &str, role_code: &str) -> RoleReference {
        RoleReference {
            path: path.to_owned(),
            role_code: role_code.to_owned(),
        }
    }

    #[test]
    fn every_reference_to_a_role_that_is_not_live_is_refused_at_its_path() {
        let references = [
            reference("definition.states.0.task.assignment.roleCode", "LIVE"),
            reference("definition.transitions.0.allowedBy", "GONE"),
            reference("definition.transitions.1.allowedBy.roleCode", "GONE"),
        ];
        let live = BTreeSet::from(["LIVE".to_owned()]);

        let details = dead_role_errors(&references, &live);

        let paths: Vec<&str> = details.iter().map(|detail| detail.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "definition.transitions.0.allowedBy",
                "definition.transitions.1.allowedBy.roleCode"
            ]
        );
        assert!(details
            .iter()
            .all(|detail| detail.code == ROLE_NOT_LIVE && detail.rule == "liveRole"));
        assert!(
            details[0].message.contains("`GONE`"),
            "{}",
            details[0].message
        );
    }

    #[test]
    fn a_role_code_is_matched_exactly() {
        // As `assignment::direct` resolves it: a code differing in case is
        // another role, and would be refused at run time.
        let live = BTreeSet::from(["APPROVER".to_owned()]);

        assert_eq!(
            dead_role_errors(&[reference("p", "approver")], &live).len(),
            1
        );
        assert!(dead_role_errors(&[reference("p", "APPROVER")], &live).is_empty());
    }

    /// A definition with one task state and one edge, each holding `rule`.
    fn with_rules(assignment: Value, allowed_by: Value) -> Value {
        serde_json::json!({
            "states": [
                { "code": "A", "task": { "assignment": assignment } },
                { "code": "DONE", "isFinal": true }
            ],
            "transitions": [
                { "from": "A", "to": "DONE", "action": "APPROVE", "allowedBy": allowed_by }
            ]
        })
    }

    fn scoped(assignee_type: &str, scope: &str) -> Value {
        serde_json::json!({
            "assigneeType": assignee_type,
            "roleCode": "APPROVER",
            "userId": "00000000-0000-0000-0000-000000000000",
            "departmentScope": scope
        })
    }

    fn paths(details: &[ValidationDetail]) -> Vec<&str> {
        details.iter().map(|detail| detail.path.as_str()).collect()
    }

    #[test]
    fn role_scope_errors_refuses_a_scope_on_a_role_rule_at_the_scope() {
        let definition = with_rules(scoped("ROLE", "REQUESTED_DEPARTMENT"), json_str("OWNER"));

        let details = role_scope_errors(&definition);

        assert_eq!(
            paths(&details),
            ["definition.states.0.task.assignment.departmentScope"]
        );
        assert_eq!(details[0].rule, "roleScope");
        assert_eq!(details[0].code, ROLE_TAKES_NO_SCOPE);
        assert!(
            details[0].message.contains("`REQUESTED_DEPARTMENT`")
                && details[0].message.contains("DEPARTMENT_ROLE"),
            "{}",
            details[0].message
        );
    }

    #[test]
    fn role_scope_errors_refuses_a_scope_on_a_role_allowed_by() {
        let definition = with_rules(
            serde_json::json!({ "assigneeType": "OWNER" }),
            scoped("ROLE", "DEPT-FIN"),
        );

        assert_eq!(
            paths(&role_scope_errors(&definition)),
            ["definition.transitions.0.allowedBy.departmentScope"]
        );
    }

    /// `""` is a scope the author wrote, and the key is what is refused.
    #[test]
    fn role_scope_errors_counts_an_empty_scope() {
        let definition = with_rules(scoped("ROLE", ""), json_str("OWNER"));

        let details = role_scope_errors(&definition);

        assert_eq!(details.len(), 1);
        assert!(details[0].message.contains("``"), "{}", details[0].message);
    }

    #[test]
    fn role_scope_errors_reads_no_other_rule_and_no_escalation() {
        let none = serde_json::json!({ "assigneeType": "ROLE", "roleCode": "APPROVER" });

        for (assignment, allowed_by) in [
            // No scope, and the shorthand, which cannot carry one.
            (none.clone(), json_str("ROLE:APPROVER")),
            (none.clone(), json_str("ROLE:APPROVER:DEPT-FIN")),
            // Every other type with a scope.
            (
                scoped("DEPARTMENT_ROLE", "OWNER_DEPARTMENT"),
                scoped("DEPARTMENT_ROLE", "DEPT-FIN"),
            ),
            (scoped("USER", "DEPT-FIN"), scoped("OWNER", "DEPT-FIN")),
        ] {
            let definition = with_rules(assignment.clone(), allowed_by.clone());
            assert!(
                role_scope_errors(&definition).is_empty(),
                "{assignment} / {allowed_by}"
            );
        }

        // Nothing executes an escalation (JWSS §3.1), so its rule is not read.
        let mut definition = with_rules(none, json_str("OWNER"));
        definition["states"][0]["task"]["escalation"] =
            serde_json::json!({ "afterHours": 24, "assignment": scoped("ROLE", "DEPT-FIN") });
        assert!(role_scope_errors(&definition).is_empty());
    }

    /// States before transitions, each in document order, one detail a place.
    #[test]
    fn role_scope_errors_refuses_each_place_in_document_order() {
        let definition = serde_json::json!({
            "states": [
                { "code": "A", "task": { "assignment": scoped("ROLE", "OWNER_DEPARTMENT") } },
                { "code": "B", "task": { "assignment": scoped("DEPARTMENT_ROLE", "DEPT-FIN") } },
                { "code": "C", "task": { "assignment": scoped("ROLE", "DEPT-FIN") } },
                { "code": "DONE", "isFinal": true }
            ],
            "transitions": [
                { "from": "A", "to": "B", "action": "APPROVE",
                  "allowedBy": scoped("ROLE", "REQUESTED_DEPARTMENT") },
                { "from": "B", "to": "C", "action": "APPROVE", "allowedBy": "ROLE:APPROVER" },
                { "from": "C", "to": "DONE", "action": "APPROVE",
                  "allowedBy": scoped("ROLE", "") }
            ]
        });

        assert_eq!(
            paths(&role_scope_errors(&definition)),
            [
                "definition.states.0.task.assignment.departmentScope",
                "definition.states.2.task.assignment.departmentScope",
                "definition.transitions.0.allowedBy.departmentScope",
                "definition.transitions.2.allowedBy.departmentScope",
            ]
        );
    }

    fn json_str(value: &str) -> Value {
        Value::String(value.to_owned())
    }

    // -- #706: a condition path the engine does not build --------------------

    /// A definition with one conditioned edge and the given variables.
    fn reading(condition: Value, variables: Value) -> Value {
        serde_json::json!({
            "states": [{ "code": "A" }, { "code": "DONE", "isFinal": true }],
            "transitions": [
                { "from": "A", "to": "DONE", "action": "APPROVE", "condition": condition }
            ],
            "variables": variables
        })
    }

    fn refused_reads(definition: &Value) -> Vec<(String, String)> {
        condition_path_errors(definition)
            .into_iter()
            .map(|detail| {
                assert_eq!(detail.code, CONDITION_PATH_NOT_BUILT);
                assert_eq!(detail.rule, "conditionPath");
                let read = detail
                    .message
                    .split('`')
                    .nth(1)
                    .expect("the message names the path in backticks")
                    .to_owned();
                (detail.path, read)
            })
            .collect()
    }

    fn var(path: &str) -> Value {
        serde_json::json!({ "var": path })
    }

    const CONDITION: &str = "definition.transitions.0.condition";

    /// The `(path, code)` of every detail, for a refusal that may hold either
    /// code, each detail being `conditionPath`'s.
    fn refused_codes(details: &[ValidationDetail]) -> Vec<(String, String)> {
        details
            .iter()
            .map(|detail| {
                assert_eq!(detail.rule, "conditionPath");
                (detail.path.clone(), detail.code.clone())
            })
            .collect()
    }

    /// The unbuilt paths a condition is refused for, in the order read.
    fn read_paths(condition: &Value) -> Vec<String> {
        refused_reads(&reading(condition.clone(), serde_json::json!([])))
            .into_iter()
            .map(|(_, read)| read)
            .collect()
    }

    #[test]
    fn condition_path_errors_refuses_each_unbuilt_path_at_its_expression() {
        for path in [
            "document.amount",
            "document.foo",
            "document.status.code",
            "actor.email",
            "actor.roles.first",
            "actor.roles.",
            "actor.userId.x",
            "amount",
            "variables.undeclared",
            "variables.",
            "Document.status",
        ] {
            assert_eq!(
                refused_reads(&reading(var(path), serde_json::json!([]))),
                [(
                    "definition.transitions.0.condition".to_owned(),
                    path.to_owned()
                )],
                "{path}"
            );
        }
    }

    #[test]
    fn condition_path_errors_accepts_every_path_the_engine_builds() {
        let variables =
            serde_json::json!([{ "key": "amount", "source": { "var": "formData.amount" } }]);

        for path in [
            "document.status",
            "document.documentTypeId",
            "document.documentNumber",
            "document.documentTypeKey",
            "actor.userId",
            "actor.roles",
            "actor.roles.0",
            "actor.roles.12",
            "actor.departmentId",
            "formData",
            "formData.amount",
            "formData.lines.0.qty",
            "variables.amount",
            "variables.amount.deeper",
            "document",
            "actor",
            "variables",
            "",
        ] {
            assert!(
                condition_path_errors(&reading(var(path), variables.clone())).is_empty(),
                "{path}"
            );
        }
    }

    /// The constants are what the engine builds, and the message names them.
    #[test]
    fn the_message_names_the_path_and_the_built_paths_and_the_amounts_place() {
        let details = condition_path_errors(&reading(
            serde_json::json!({ "and": [var("document.amount"), var("actor.email")] }),
            serde_json::json!([]),
        ));

        assert_eq!(details.len(), 2);
        for detail in &details {
            for built in [
                "document.status",
                "document.documentTypeKey",
                "actor.roles",
                "actor.departmentId",
                "formData.<key>",
            ] {
                assert!(
                    detail.message.contains(built),
                    "{built}: {}",
                    detail.message
                );
            }
        }
        assert!(
            details[0].message.contains("`formData.amount`"),
            "{}",
            details[0].message
        );
        assert!(
            !details[1].message.contains("`formData.amount`"),
            "{}",
            details[1].message
        );
    }

    #[test]
    fn condition_path_errors_reads_missing_missing_some_and_a_default() {
        let condition = serde_json::json!({ "or": [
            { "missing": ["formData.a", "document.amount"] },
            { "missing_some": [1, ["actor.email", "formData.b"]] },
            { "var": ["document.foo", 0] },
            { "missing": "variables.nope" }
        ] });

        assert_eq!(
            refused_reads(&reading(condition, serde_json::json!([])))
                .into_iter()
                .map(|(_, read)| read)
                .collect::<Vec<_>>(),
            [
                "document.amount",
                "actor.email",
                "document.foo",
                "variables.nope"
            ]
        );
    }

    /// A `var` with a non-string path names no path, and none of these computes
    /// one (#789 AC9): `[1, "document.amount"]` is the key `1` with a literal
    /// default beside it.
    #[test]
    fn condition_path_errors_ignores_a_var_that_names_no_path() {
        for operand in [
            serde_json::json!(0),
            serde_json::json!(null),
            serde_json::json!([]),
            serde_json::json!(""),
            serde_json::json!([1, "document.amount"]),
            serde_json::json!(["formData.x", 0]),
        ] {
            let condition = serde_json::json!({ "var": operand });

            assert!(
                condition_path_errors(&reading(condition, serde_json::json!([]))).is_empty(),
                "{operand}"
            );
        }
    }

    /// #789 AC9: a path with a default beside it computes nothing, so an
    /// unbuilt one is refused for what it names and not for its shape.
    #[test]
    fn a_path_with_a_default_is_refused_only_for_the_path_it_names() {
        assert_eq!(
            refused_reads(&reading(
                serde_json::json!({ "var": ["total", 0] }),
                serde_json::json!([])
            )),
            [(CONDITION.to_owned(), "total".to_owned())]
        );
    }

    /// AC8: the element-scoped operand addresses the element, and is not read;
    /// the iterator's source is, and so is `reduce`'s initial value (#789),
    /// which is the form engine's rule, shared. The evaluator reads it in the
    /// outer scope, before there is an element.
    #[test]
    fn condition_path_errors_reads_an_iterators_source_and_not_its_element() {
        for operator in ["map", "filter", "all", "some", "none", "reduce"] {
            let accepted =
                serde_json::json!({ operator: [var("formData.lines"), { ">": [var("qty"), 1] }] });
            assert!(
                condition_path_errors(&reading(accepted, serde_json::json!([]))).is_empty(),
                "{operator}"
            );

            let refused =
                serde_json::json!({ operator: [var("document.lines"), { ">": [var("qty"), 1] }] });
            assert_eq!(
                refused_reads(&reading(refused, serde_json::json!([])))
                    .into_iter()
                    .map(|(_, read)| read)
                    .collect::<Vec<_>>(),
                ["document.lines"],
                "{operator}"
            );
        }

        let reduce = serde_json::json!({ "reduce": [
            var("formData.lines"),
            { "+": [var("current.qty"), var("accumulator")] },
            var("document.amount")
        ] });
        assert_eq!(
            refused_reads(&reading(reduce, serde_json::json!([]))),
            [(CONDITION.to_owned(), "document.amount".to_owned())]
        );
    }

    /// #789 AC3: `qty` as an initial value is the context's and not an
    /// element's, the body beside it is still the element's, and a `reduce`
    /// nested in an initial value has both its source and its initial value
    /// read, source first.
    #[test]
    fn a_reduces_initial_value_is_read_in_the_outer_scope() {
        let fold = serde_json::json!({ "+": [var("current.qty"), var("accumulator")] });
        let nested = serde_json::json!({ "reduce": [
            var("formData.lines"),
            fold,
            { "reduce": [var("document.lines"), fold, var("actor.email")] }
        ] });

        assert_eq!(
            read_paths(&serde_json::json!({ "reduce": [var("formData.lines"), fold, var("qty")] })),
            ["qty"]
        );
        assert_eq!(read_paths(&nested), ["document.lines", "actor.email"]);
    }

    /// #789 AC5, record 24's P1. All three read the amount, the evaluator
    /// answers true for all three, and all three are refused.
    #[test]
    fn three_ways_of_reading_the_amount_are_all_refused() {
        let reduce = serde_json::json!({ "==": [{ "reduce": [
            var("formData.lines"),
            { "+": [var("current"), var("accumulator")] },
            var("document.amount")
        ] }, 7] });
        let computed =
            serde_json::json!({ "==": [{ "var": { "if": [true, "document.amount", "x"] } }, 7] });
        let plain = serde_json::json!({ "==": [var("document.amount"), 7] });

        let data = serde_json::json!({ "document": { "amount": 7 }, "formData": {} });
        let evaluator = RuleEvaluator::new();
        for condition in [&reduce, &computed, &plain] {
            assert_eq!(evaluator.evaluate(condition, &data), Ok(Value::Bool(true)));
        }

        for (condition, code) in [
            (reduce, CONDITION_PATH_NOT_BUILT),
            (computed, CONDITION_PATH_COMPUTED),
            (plain, CONDITION_PATH_NOT_BUILT),
        ] {
            let details = condition_path_errors(&reading(condition, serde_json::json!([])));

            assert_eq!(
                refused_codes(&details),
                [(CONDITION.to_owned(), code.to_owned())]
            );
        }
    }

    /// #789 AC8: each shape of a computed `var`, in every place a body can hold
    /// it, in a condition and in a variable's `source`.
    #[test]
    fn a_computed_var_is_refused_in_each_shape_and_wherever_it_sits() {
        let operands = [
            serde_json::json!({ "if": [true, "formData.a", "x"] }),
            serde_json::json!({ "var": "formData.which" }),
            serde_json::json!([{ "if": [true, "formData.a", "x"] }]),
            serde_json::json!([{ "if": [true, "formData", "x"] }, "a"]),
            serde_json::json!([[1], "formData.a"]),
            serde_json::json!([[0], "formData.a"]),
            serde_json::json!([["formData", "a"]]),
        ];

        for operand in operands {
            let computed = serde_json::json!({ "var": operand });
            let lines = var("formData.lines");

            for expression in [
                computed.clone(),
                serde_json::json!({ "==": [computed, 1] }),
                serde_json::json!({ "map": [lines, computed] }),
                serde_json::json!({ "filter": [lines, computed] }),
                serde_json::json!({ "all": [lines, computed] }),
                serde_json::json!({ "some": [lines, computed] }),
                serde_json::json!({ "none": [lines, computed] }),
                serde_json::json!({ "reduce": [lines, computed, 0] }),
                serde_json::json!({ "reduce": [lines, var("current"), computed] }),
                serde_json::json!({ "some": [lines, { "some": [var("tags"), computed] }] }),
                serde_json::json!({ "some": [computed, var("qty")] }),
                serde_json::json!({ "var": ["formData.x", computed] }),
                serde_json::json!({ "missing_some": [computed, ["formData.a"]] }),
            ] {
                let details =
                    condition_path_errors(&reading(expression.clone(), serde_json::json!([])));
                assert_eq!(
                    refused_codes(&details),
                    [(CONDITION.to_owned(), CONDITION_PATH_COMPUTED.to_owned())],
                    "{expression}"
                );

                let details = condition_path_errors(&serde_json::json!({
                    "variables": [{ "key": "v", "source": expression }]
                }));
                assert_eq!(
                    refused_codes(&details),
                    [(
                        "definition.variables.0.source".to_owned(),
                        CONDITION_PATH_COMPUTED.to_owned()
                    )]
                );
            }
        }
    }

    /// #789 AC8: the message says why and what to write, and names no path.
    #[test]
    fn a_computed_paths_message_names_no_path() {
        let details = condition_path_errors(&reading(
            serde_json::json!({ "var": [[1], "document.amount"] }),
            serde_json::json!([]),
        ));

        assert_eq!(details[0].rule, "conditionPath");
        assert_eq!(
            details[0].message,
            "A `var`, `missing` or `missing_some` in this expression works out the path it \
             reads as it runs, or climbs out of its scope to read it, so a publish cannot \
             check the path: one the engine does not build would read as null and the \
             condition would take another branch without saying so. Write the path as a \
             string, with any default beside it (JWSS §6.4)."
        );
    }

    /// #789 AC10: a default that is an expression reads what it says, beside a
    /// string or a number, and a built path there is accepted.
    #[test]
    fn a_vars_default_is_read() {
        for first in [serde_json::json!("formData.x"), serde_json::json!(0)] {
            let default = |path: &str| serde_json::json!({ "var": [first, var(path)] });

            assert_eq!(read_paths(&default("document.amount")), ["document.amount"]);
            assert!(read_paths(&default("actor.userId")).is_empty());
        }

        // The engine evaluates nothing after the default, and `[null, …]` and
        // `[true, …]` take none: run in the closure test's premise.
        assert!(read_paths(
            &serde_json::json!({ "var": ["formData.x", 0, var("document.amount")] })
        )
        .is_empty());
        assert!(
            read_paths(&serde_json::json!({ "var": [null, var("document.amount")] })).is_empty()
        );
    }

    /// #789 AC11: what the operands of `missing` and `missing_some` read is
    /// read, the count included; a name worked out as the condition runs is
    /// refused, after the path it was worked out from; literal names keep
    /// working, in a list inside a list too.
    #[test]
    fn the_operands_of_missing_are_read_and_a_computed_name_is_refused() {
        let amount = var("document.amount");
        let not_built = (CONDITION.to_owned(), CONDITION_PATH_NOT_BUILT.to_owned());
        let computed = (CONDITION.to_owned(), CONDITION_PATH_COMPUTED.to_owned());

        for (expression, expected) in [
            (
                serde_json::json!({ "missing": amount }),
                vec![not_built.clone(), computed.clone()],
            ),
            (
                serde_json::json!({ "missing": ["formData.a", amount] }),
                vec![not_built.clone(), computed.clone()],
            ),
            (
                serde_json::json!({ "missing_some": [1, amount] }),
                vec![not_built.clone(), computed.clone()],
            ),
            (
                serde_json::json!({ "missing_some": [1, ["formData.a", amount]] }),
                vec![not_built.clone(), computed.clone()],
            ),
            (
                serde_json::json!({ "missing_some": [amount, ["formData.a"]] }),
                vec![not_built.clone()],
            ),
            (
                serde_json::json!({ "missing": [["formData.a", "document.amount"]] }),
                vec![not_built.clone()],
            ),
            (
                serde_json::json!({ "missing": { "if": [true, "formData.a", "formData.b"] } }),
                vec![computed.clone()],
            ),
            (
                serde_json::json!({ "missing": var("formData.names") }),
                vec![computed.clone()],
            ),
            (
                serde_json::json!({ "missing_some": [1, [[var("formData.names")]]] }),
                vec![computed.clone()],
            ),
            (
                serde_json::json!({ "some": [var("formData.lines"), { "missing": var("name") }] }),
                vec![computed],
            ),
            (
                serde_json::json!({ "missing": ["formData.a", 3, ["formData.b"], "actor.userId"] }),
                vec![],
            ),
            (
                serde_json::json!({ "missing_some": [var("formData.n"), ["formData.a"]] }),
                vec![],
            ),
        ] {
            let details =
                condition_path_errors(&reading(expression.clone(), serde_json::json!([])));

            assert_eq!(refused_codes(&details), expected, "{expression}");
        }
    }

    /// #789 AC12: an expression's unbuilt paths in the order read, an initial
    /// value after its source and whichever comes first in the expression, then
    /// its computed path, once; transitions before variables.
    #[test]
    fn a_computed_path_follows_its_expressions_unbuilt_paths_once() {
        let computed = serde_json::json!({ "var": { "var": "formData.which" } });
        let definition = serde_json::json!({
            "transitions": [
                { "condition": { "and": [
                    computed,
                    { "reduce": [var("document.lines"), var("current"), var("document.foo")] },
                    computed,
                    var("document.foo"),
                    var("actor.email")
                ] } },
                { "condition": computed }
            ],
            "variables": [
                { "key": "v", "source": { "+": [computed, var("amount")] } }
            ]
        });

        let details = condition_path_errors(&definition);

        assert_eq!(
            refused_codes(&details)
                .iter()
                .map(|(path, code)| (path.as_str(), code.as_str()))
                .collect::<Vec<_>>(),
            [
                (CONDITION, CONDITION_PATH_NOT_BUILT),
                (CONDITION, CONDITION_PATH_NOT_BUILT),
                (CONDITION, CONDITION_PATH_NOT_BUILT),
                (CONDITION, CONDITION_PATH_COMPUTED),
                (
                    "definition.transitions.1.condition",
                    CONDITION_PATH_COMPUTED
                ),
                ("definition.variables.0.source", CONDITION_PATH_NOT_BUILT),
                ("definition.variables.0.source", CONDITION_PATH_COMPUTED),
            ]
        );
        assert!(details[0].message.contains("`document.lines`"));
        assert!(details[1].message.contains("`document.foo`"));
        assert!(details[2].message.contains("`actor.email`"));
        assert!(details[5].message.contains("`amount`"));
    }

    /// One detail per distinct path per expression, in the order read; a path
    /// read in two expressions is told at each; transitions before variables.
    #[test]
    fn condition_path_errors_tells_each_path_once_per_expression_in_order() {
        let definition = serde_json::json!({
            "states": [{ "code": "A" }, { "code": "B" }, { "code": "DONE", "isFinal": true }],
            "transitions": [
                { "from": "A", "to": "B", "action": "APPROVE",
                  "condition": { "and": [var("document.foo"), var("amount"), var("document.foo")] } },
                { "from": "A", "to": "DONE", "action": "REJECT" },
                { "from": "B", "to": "DONE", "action": "APPROVE",
                  "condition": var("document.foo") }
            ],
            "variables": [
                { "key": "declared", "source": var("formData.amount") },
                { "key": "computed",
                  "source": { "+": [var("variables.declared"), var("variables.computed"),
                                    var("variables.later"), var("actor.email")] } },
                { "key": "unsourced" }
            ]
        });

        assert_eq!(
            refused_reads(&definition),
            [
                ("definition.transitions.0.condition", "document.foo"),
                ("definition.transitions.0.condition", "amount"),
                ("definition.transitions.2.condition", "document.foo"),
                ("definition.variables.1.source", "variables.later"),
                ("definition.variables.1.source", "actor.email"),
            ]
            .map(|(path, read)| (path.to_owned(), read.to_owned()))
        );
    }

    /// AC8: `allowedBy.expression` and `assignment.expression` are not read:
    /// `EXPRESSION` is refused at save.
    #[test]
    fn condition_path_errors_reads_no_assignment_expression() {
        let rule = serde_json::json!({
            "assigneeType": "EXPRESSION", "expression": var("document.amount")
        });
        let definition = serde_json::json!({
            "states": [
                { "code": "A", "task": { "assignment": rule.clone() } },
                { "code": "DONE", "isFinal": true }
            ],
            "transitions": [
                { "from": "A", "to": "DONE", "action": "APPROVE", "allowedBy": rule }
            ]
        });

        assert!(condition_path_errors(&definition).is_empty());
    }

    // -- #789: every way a condition can reach an unbuilt path ----------------

    /// A context shaped as JWSS §6.1's, with `document` carrying unbuilt keys
    /// when `sentinel` is set, so a condition's answer can be compared with
    /// them and without them.
    fn context(sentinel: bool) -> Value {
        let mut document = serde_json::json!({ "status": "SUBMITTED" });

        if sentinel {
            document["amount"] = serde_json::json!(7);
            document["where"] = serde_json::json!("document.amount");
            document["names"] = serde_json::json!(["formData.absent"]);
            document["name"] = serde_json::json!("formData.absent");
            document["level"] = serde_json::json!([1]);
        }

        serde_json::json!({
            "document": document,
            "actor": { "userId": "u", "roles": [], "departmentId": null },
            "formData": {
                "on": true,
                "nums": [1, 2],
                "rows": [{ "qty": 1, "tags": [1] }, { "qty": 2, "tags": [2] }]
            },
            "variables": {}
        })
    }

    /// Whether `condition`'s answer changes with the sentinel in the context.
    fn depends_on_the_sentinel(condition: &Value) -> bool {
        let evaluator = RuleEvaluator::new();

        evaluator.evaluate(condition, &context(true))
            != evaluator.evaluate(condition, &context(false))
    }

    /// Each row was run against `datalogic-rs` 5.2.0, not traced: the premise
    /// in the closure test fails if a shape stops reading the path.
    fn shapes_reaching_an_unbuilt_path() -> Vec<(&'static str, Value)> {
        let amount = var("document.amount");
        let fold = serde_json::json!({ "+": [var("current"), var("accumulator")] });
        let jump = serde_json::json!({ "var": [[1], "document.amount"] });
        let is_seven = serde_json::json!({ "==": [jump, 7] });
        let chosen = serde_json::json!({ "if": [var("formData.on"), "document.amount", "x"] });

        vec![
            ("plain", serde_json::json!({ "==": [amount, 7] })),
            (
                "H1 over numbers",
                serde_json::json!({ "==": [
                    { "reduce": [var("formData.nums"), fold, amount] }, 10
                ] }),
            ),
            (
                "H1 over a map",
                serde_json::json!({ "==": [
                    { "reduce": [
                        { "map": [var("formData.rows"), var("qty")] }, fold, amount
                    ] }, 10
                ] }),
            ),
            (
                "H1 over nothing",
                serde_json::json!({ "==": [
                    { "reduce": [var("formData.lines"), fold, amount] }, 7
                ] }),
            ),
            (
                "H2 a constant if",
                serde_json::json!({ "==": [
                    { "var": { "if": [true, "document.amount", "x"] } }, 7
                ] }),
            ),
            (
                "H2 an if",
                serde_json::json!({ "==": [{ "var": chosen }, 7] }),
            ),
            (
                "H2 a var",
                serde_json::json!({ "==": [{ "var": var("document.where") }, 7] }),
            ),
            (
                "H3 opening with an expression",
                serde_json::json!({ "==": [{ "var": [chosen] }, 7] }),
            ),
            (
                "H3 a chain opening with an expression",
                serde_json::json!({ "==": [
                    { "var": [{ "if": [var("formData.on"), "document", "x"] }, "amount"] }, 7
                ] }),
            ),
            (
                "H3 a computed level",
                serde_json::json!({ "==": [
                    { "var": [var("document.level"), "document.amount"] }, 7
                ] }),
            ),
            (
                "H3 opening with an array",
                serde_json::json!({ "==": [{ "var": [["document", "amount"]] }, 7] }),
            ),
            (
                "H3 the jump outside a body",
                serde_json::json!({ "==": [jump, 7] }),
            ),
            (
                "H3 the jump in some",
                serde_json::json!({ "some": [var("formData.nums"), is_seven] }),
            ),
            (
                "H3 the jump in all",
                serde_json::json!({ "all": [var("formData.rows"), is_seven] }),
            ),
            (
                "H3 the jump in none",
                serde_json::json!({ "none": [var("formData.rows"), is_seven] }),
            ),
            (
                "H3 the jump in filter",
                serde_json::json!({ "!!": [{ "filter": [var("formData.rows"), is_seven] }] }),
            ),
            (
                "H3 the jump in map",
                serde_json::json!({ "in": [7, { "map": [var("formData.rows"), jump] }] }),
            ),
            (
                "H3 the jump in reduce",
                serde_json::json!({ "==": [{ "reduce": [var("formData.nums"), jump, 0] }, 7] }),
            ),
            (
                "H3 the jump up two bodies",
                serde_json::json!({ "some": [var("formData.rows"), { "some": [var("tags"), {
                    "==": [{ "var": [[2], "document.amount"] }, 7]
                }] }] }),
            ),
            (
                "H4 a default",
                serde_json::json!({ "==": [{ "var": ["formData.absent", amount] }, 7] }),
            ),
            (
                "H4 a default beside a number",
                serde_json::json!({ "==": [{ "var": [0, amount] }, 7] }),
            ),
            (
                "H5 missing with a var",
                serde_json::json!({ "!": [{ "missing": var("document.names") }] }),
            ),
            (
                "H5 missing with a var in its list",
                serde_json::json!({ "!": [{ "missing": [var("document.names")] }] }),
            ),
            (
                "H5 missing with a list in its list",
                serde_json::json!({ "!": [{ "missing": [["document.amount"]] }] }),
            ),
            (
                "H5 missing with a var in a list in its list",
                serde_json::json!({ "!": [{ "missing": [[var("document.name")]] }] }),
            ),
            (
                "H5 missing with a computed name",
                serde_json::json!({ "!": [{ "missing": chosen }] }),
            ),
            (
                "H5 missing_some's count",
                serde_json::json!({ "!": [{ "missing_some": [
                    amount, ["formData.on", "formData.absent"]
                ] }] }),
            ),
            (
                "H5 missing_some's names",
                serde_json::json!({ "!": [{ "missing_some": [1, var("document.names")] }] }),
            ),
            (
                "H5 a var in missing_some's names",
                serde_json::json!({ "!": [{ "missing_some": [1, [var("document.name")]] }] }),
            ),
        ]
    }

    /// **#789 AC7, the closure test**: where the answer depends on an unbuilt
    /// path, a publish refuses the condition. The sentinel is what makes the
    /// premise a run and not a claim, and the failures are gathered so that a
    /// red run names every shape.
    #[test]
    fn a_condition_whose_answer_depends_on_an_unbuilt_path_is_refused() {
        let mut accepted = Vec::new();

        for (shape, condition) in shapes_reaching_an_unbuilt_path() {
            assert!(
                depends_on_the_sentinel(&condition),
                "{shape} does not read the sentinel: {condition}"
            );

            if condition_path_errors(&reading(condition, serde_json::json!([]))).is_empty() {
                accepted.push(shape);
            }
        }

        assert!(accepted.is_empty(), "accepted: {accepted:#?}");
    }

    /// **H6, H8 and H10, run**: nothing else reaches the context. A plain path
    /// in a body reads the element, a quantifier's third operand is an
    /// evaluation error, and neither `$` nor `..` climbs out of a scope.
    #[test]
    fn a_body_reads_its_element_and_nothing_climbs_out_of_it_by_a_plain_path() {
        let is_seven = serde_json::json!({ "==": [var("document.amount"), 7] });

        for condition in [
            serde_json::json!({ "some": [var("formData.nums"), is_seven] }),
            serde_json::json!({ "some": [var("formData.rows"), is_seven] }),
            serde_json::json!({ "==": [
                { "reduce": [var("formData.rows"), var("document.amount"), 0] }, 7
            ] }),
            serde_json::json!({ "some": [var("formData.rows"),
                { "!": [{ "missing": ["document.amount"] }] }] }),
            serde_json::json!({ "some": [var("formData.nums"), { ">": [var(""), 0] }, is_seven] }),
            serde_json::json!({ "some": [var("formData.nums"),
                { "==": [var("../document.amount"), 7] }] }),
            serde_json::json!({ "some": [var("formData.nums"),
                { "==": [var("..document.amount"), 7] }] }),
        ] {
            assert!(!depends_on_the_sentinel(&condition), "{condition}");
            assert!(
                condition_path_errors(&reading(condition.clone(), serde_json::json!([])))
                    .is_empty(),
                "{condition}"
            );
        }
    }

    #[test]
    fn condition_path_errors_reads_nothing_from_a_definition_with_no_expressions() {
        assert!(condition_path_errors(&serde_json::json!({})).is_empty());
        assert!(condition_path_errors(&serde_json::json!({ "transitions": "x" })).is_empty());
    }
}
