//! Resolving a chain and running it (LHCS §3.1, §5.1, §6; architectures/01
//! §12.5; [#339]).
//!
//! [#339]: https://github.com/sujanto-gaws/kelir/issues/339

use std::time::{Duration, Instant};

use serde_json::Value;
use uuid::Uuid;

use super::domain::{HandlerReference, HookResult, Invocation, Registration, Rejection, Source};
use super::{handlers, repository};
use crate::error::{AppError, ValidationDetail};
use crate::modules::rad::evaluator::RuleEvaluator;

/// The per-handler time budget (architectures/01 §12.5, default 2s).
const HANDLER_BUDGET: Duration = Duration::from_secs(2);
/// The whole-chain budget (§12.5, default 5s).
const CHAIN_BUDGET: Duration = Duration::from_secs(5);

/// The S10.3 code a rejected chain carries (LHCS §6).
pub const HOOK_REJECTED: &str = "HOOK_REJECTED";

/// One resolved chain, in the order it runs.
///
/// **Merged across sources and sorted once** (LHCS §3.1: *lower runs first;
/// ties resolve by registration order*). Sorting is stable, so two entries of
/// equal priority keep the order the sources were concatenated in — registry
/// first, then the definition's own — which is the registration order the
/// specification means.
pub fn merge(registry: Vec<Registration>, workflow: Vec<Registration>) -> Vec<Registration> {
    let mut chain = registry;

    chain.extend(workflow);
    chain.sort_by_key(|entry| entry.priority);
    chain
}

/// The registrations a JWSS `guards` array declares (LHCS §3, JWSS §7).
///
/// **The hook name is implied and may be omitted** — §3's own footnote — so a
/// `guards` entry that names one must name `before_workflow_transition`, and
/// [`registration_errors`] is where that is enforced. Here it is simply set.
pub fn guards_of(transition: &Value) -> Vec<Registration> {
    entries_of(transition, "guards", super::BEFORE_WORKFLOW_TRANSITION)
}

fn entries_of(transition: &Value, key: &str, hook: &str) -> Vec<Registration> {
    transition
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter(|entry| {
            entry
                .get("isEnabled")
                .and_then(Value::as_bool)
                .unwrap_or(true)
        })
        .filter_map(|entry| {
            Some(Registration {
                hook: hook.to_owned(),
                handler: HandlerReference::parse(entry.get("handler")?.as_str()?)?,
                priority: entry
                    .get("priority")
                    .and_then(Value::as_i64)
                    .map_or(Source::Workflow.band_floor(), |priority| priority as i32),
                config: entry
                    .get("config")
                    .cloned()
                    .unwrap_or_else(|| Value::Object(Default::default())),
                source: Source::Workflow,
            })
        })
        .collect()
}

/// What a chain's run produced.
pub struct ChainOutcome {
    /// The form payload as the chain left it — the original where nothing
    /// modified it, so a caller can use this unconditionally.
    pub form_data: Value,
    /// True where at least one handler returned `MODIFY`. The caller re-runs
    /// the form's own validation in that case, which is §5.1's *modifications
    /// MUST still validate against the document's form schema; the engine
    /// re-validates after the chain*.
    pub modified: bool,
}

/// Runs a before-chain inside the caller's transaction (§12.5).
///
/// **Every handler's execution is logged before its result is acted on**, which
/// is §1.2's fourth conformance point taken literally: a `REJECT` rolls the
/// transaction back and takes its own log row with it, and that is correct —
/// the log describes what happened to a document, and nothing did.
///
/// **A `REJECT` returns immediately.** Later handlers do not run, which is
/// §5.1's *chain stops*, and the error is §6's envelope.
#[allow(
    clippy::too_many_arguments,
    reason = "the invocation's fields are the payload's; a struct would rename them"
)]
pub async fn run_before_chain(
    transaction: &mut sqlx::PgTransaction<'_>,
    evaluator: &RuleEvaluator,
    chain: &[Registration],
    invocation: &mut Invocation<'_>,
    workflow_transition_ref: Option<&str>,
) -> Result<ChainOutcome, AppError> {
    let started = Instant::now();
    let mut modified = false;

    for entry in chain {
        // The chain budget is checked between handlers rather than around the
        // whole loop, so the handler that overran is the one the log names.
        if started.elapsed() > CHAIN_BUDGET {
            return Err(timed_out(&entry.handler, invocation.hook_name, "the chain"));
        }

        invocation.source = entry.source;
        invocation.config = entry.config.clone();

        let payload = invocation.as_json();
        let began = Instant::now();

        let result = match &entry.handler {
            HandlerReference::Core(name) => match handlers::resolve(name) {
                Some(handler) => handler(&payload, evaluator),
                // Unreachable through a published definition — §2 makes this an
                // ERROR at registration and `registration_errors` refuses it —
                // so meeting one here means a row was written another way.
                // Refused rather than skipped, because a chain that silently
                // drops a handler is a guard reported as passed.
                None => HookResult::Reject(Rejection::new(
                    "HOOK_HANDLER_UNKNOWN",
                    format!("`{}` is not a handler this build performs", entry.handler),
                )),
            },
            // §2: a `plugin:` reference of an unknown plugin is an ERROR. There
            // are no plugins, so every one of them is unknown.
            HandlerReference::Plugin { .. } => HookResult::Reject(Rejection::new(
                "HOOK_PLUGIN_UNAVAILABLE",
                format!(
                    "`{}` is a plugin handler and this build runs no plugins",
                    entry.handler
                ),
            )),
        };

        let elapsed = began.elapsed();
        // Measured rather than enforced: every handler here is synchronous core
        // Rust, so there is nothing to interrupt part-way. The budget becomes a
        // cancellation when a handler can block — a plugin runtime, an
        // integration call — and until then this is what makes an overrun
        // visible rather than merely slow.
        let overran = elapsed > HANDLER_BUDGET;

        repository::record(
            transaction,
            &repository::Execution {
                tenant_id: invocation.tenant_id,
                source: entry.source,
                hook_id: None,
                workflow_transition_ref,
                document_id: invocation.document_id,
                hook_name: invocation.hook_name,
                handler_reference: &entry.handler.to_string(),
                result: if overran { "ERROR" } else { result.as_db() },
                duration_ms: elapsed.as_millis().min(i32::MAX as u128) as i32,
                error_message: overran
                    .then(|| format!("exceeded the {HANDLER_BUDGET:?} handler budget"))
                    .as_deref(),
            },
        )
        .await?;

        if overran {
            return Err(timed_out(&entry.handler, invocation.hook_name, "a handler"));
        }

        match result {
            HookResult::Continue => {}
            HookResult::Modify {
                form_data,
                metadata,
            } => {
                // **Later handlers see the modified payload** (§5.1), which is
                // why this writes back into the invocation rather than
                // collecting changes for the end.
                if let Some(form_data) = form_data {
                    invocation.form_data = form_data;
                    modified = true;
                }

                if let Some(metadata) = metadata {
                    invocation.metadata = metadata;
                    modified = true;
                }
            }
            HookResult::Reject(rejection) => {
                return Err(rejected(&rejection, &entry.handler, invocation.hook_name));
            }
        }
    }

    Ok(ChainOutcome {
        form_data: invocation.form_data.clone(),
        modified,
    })
}

/// LHCS §6's envelope.
///
/// `error.code` is always `HOOK_REJECTED`; the handler's own code appears in
/// the `_hook` detail *with* the handler reference and the hook name, because
/// a code alone does not say which of three registrations produced it.
fn rejected(rejection: &Rejection, handler: &HandlerReference, hook_name: &str) -> AppError {
    let mut details: Vec<ValidationDetail> = rejection
        .details
        .iter()
        .map(|(field, message)| {
            ValidationDetail::new(field.clone(), "hook", HOOK_REJECTED, message.clone())
        })
        .collect();

    // Handler-supplied entries pass through ahead of the `_hook` entry (§6).
    details.push(ValidationDetail::new(
        "_hook",
        "hook",
        HOOK_REJECTED,
        format!("{} by {handler} ({hook_name})", rejection.code),
    ));

    AppError::Validation { details }
}

/// §5.1: *a timeout is treated as `REJECT` with `rejectCode: "HOOK_TIMEOUT"`.*
fn timed_out(handler: &HandlerReference, hook_name: &str, what: &str) -> AppError {
    rejected(
        &Rejection::new(
            "HOOK_TIMEOUT",
            format!("{what} on this transition took too long and was refused"),
        ),
        handler,
        hook_name,
    )
}

/// Every registration in a definition, resolved (LHCS §2, §3.2).
///
/// **Called at publish**, which is what makes §2's *a reference MUST resolve at
/// registration time* true. Three refusals, and they are different mistakes:
///
/// - a reference that does not match §2's grammar at all;
/// - a `core:` handler this build does not perform — §2's ERROR;
/// - a `hook` naming the wrong kind for its position — §3.2's kind constraint,
///   which is how an `after_*` name inside `guards` is caught.
///
/// A `plugin:` reference is refused with its own sentence rather than folded
/// into the second: the grammar accepted it, the plugin is simply not there,
/// and telling somebody their handler name is wrong when their plugin is
/// missing sends them to the wrong file.
pub fn registration_errors(definition: &Value) -> Vec<ValidationDetail> {
    let mut details = Vec::new();

    let transitions = definition
        .get("transitions")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();

    for (index, transition) in transitions.iter().enumerate() {
        for (key, implied) in [
            ("guards", super::BEFORE_WORKFLOW_TRANSITION),
            ("actions", super::AFTER_WORKFLOW_TRANSITION),
        ] {
            let entries = transition
                .get(key)
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default();

            for (at, entry) in entries.iter().enumerate() {
                let path = format!("definition.transitions.{index}.{key}.{at}");

                check_entry(entry, &path, implied, &mut details);
            }
        }
    }

    details
}

fn check_entry(entry: &Value, path: &str, implied: &str, details: &mut Vec<ValidationDetail>) {
    // §3's footnote: the name may be omitted, and if present it must match.
    if let Some(declared) = entry.get("hook").and_then(Value::as_str) {
        if declared != implied {
            details.push(ValidationDetail::new(
                format!("{path}.hook"),
                "LHCS-3.2",
                "HOOK_NAME_MISMATCH",
                format!(
                    "`{declared}` is not the hook this position registers — a `{}` entry is \
                     `{implied}`, and the name may be omitted entirely",
                    path.rsplit('.').nth(1).unwrap_or("guards")
                ),
            ));
        }
    }

    let Some(reference) = entry.get("handler").and_then(Value::as_str) else {
        // The meta-schema requires `handler`, so a missing one is already an
        // INVALID_DEFINITION; saying it twice would be this check reporting the
        // shape error in its own words.
        return;
    };

    let Some(handler) = HandlerReference::parse(reference) else {
        details.push(ValidationDetail::new(
            format!("{path}.handler"),
            "LHCS-2",
            "HANDLER_REFERENCE_INVALID",
            format!(
                "`{reference}` is not a handler reference — it is `core:<name>` or \
                 `plugin:<pluginId>:<name>`, both lower case"
            ),
        ));

        return;
    };

    match handler {
        HandlerReference::Core(ref name) if handlers::resolve(name).is_none() => {
            details.push(ValidationDetail::new(
                format!("{path}.handler"),
                "LHCS-2",
                "HANDLER_NOT_FOUND",
                format!(
                    "`{handler}` is not a handler this build performs; the core handlers are {}",
                    handlers::available()
                ),
            ));
        }
        HandlerReference::Plugin { ref plugin, .. } => {
            details.push(ValidationDetail::new(
                format!("{path}.handler"),
                "LHCS-2",
                "HANDLER_PLUGIN_UNKNOWN",
                format!(
                    "`{handler}` needs the `{plugin}` plugin, and this build runs no plugins — \
                     a registration that could never run is refused rather than stored inert"
                ),
            ));
        }
        _ => {}
    }
}

/// The `<workflowKey>@<revision>:<from>-><to>` §6.12 records for a
/// workflow-sourced execution.
pub fn transition_ref(workflow_key: &str, revision: i32, from: &str, to: &str) -> String {
    format!("{workflow_key}@{revision}:{from}->{to}")
}

/// The `workflowContext` object of LHCS §4.
#[allow(clippy::too_many_arguments)]
pub fn workflow_context(
    workflow_key: &str,
    revision: i32,
    instance_id: Uuid,
    state: &str,
    from: &str,
    to: &str,
    action: &str,
) -> Value {
    serde_json::json!({
        "workflowKey": workflow_key,
        "workflowRevision": revision,
        "instanceId": instance_id,
        "state": state,
        "transition": { "from": from, "to": to, "action": action },
        // No task: a guard runs on the transition, and the only transitions
        // this build fires a chain for are a service task's, which has none.
        "taskId": null,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn entry(handler: &str, priority: Option<i64>) -> Value {
        let mut entry = json!({ "handler": handler });

        if let Some(priority) = priority {
            entry["priority"] = json!(priority);
        }

        entry
    }

    fn registration(handler: &str, priority: i32, source: Source) -> Registration {
        Registration {
            hook: super::super::BEFORE_WORKFLOW_TRANSITION.to_owned(),
            handler: HandlerReference::parse(handler).expect("parses"),
            priority,
            config: json!({}),
            source,
        }
    }

    fn codes(details: &[ValidationDetail]) -> Vec<&str> {
        details.iter().map(|detail| detail.code.as_str()).collect()
    }

    // -- The merge --------------------------------------------------------

    #[test]
    fn a_chain_runs_in_ascending_priority_across_sources() {
        let chain = merge(
            vec![registration(
                "core:continue_always",
                200,
                Source::DocumentType,
            )],
            vec![registration("core:reject_when", 300, Source::Workflow)],
        );

        assert_eq!(chain[0].priority, 200);
        assert_eq!(chain[1].priority, 300);
    }

    /// §3.1: *ties resolve by registration order.* A registry entry and a
    /// workflow entry on the same number keep the order they were merged in,
    /// which is what a stable sort buys and an unstable one would lose
    /// intermittently.
    #[test]
    fn a_tie_keeps_registration_order() {
        let chain = merge(
            vec![registration(
                "core:continue_always",
                300,
                Source::DocumentType,
            )],
            vec![registration("core:reject_when", 300, Source::Workflow)],
        );

        assert_eq!(chain[0].source, Source::DocumentType);
        assert_eq!(chain[1].source, Source::Workflow);
    }

    /// A definition may put a guard ahead of a registry entry — §3.1 calls the
    /// bands a convention, not a constraint, so the merge must order by the
    /// number rather than by the source.
    #[test]
    fn a_source_does_not_outrank_a_number() {
        let chain = merge(
            vec![registration(
                "core:continue_always",
                250,
                Source::DocumentType,
            )],
            vec![registration("core:reject_when", 10, Source::Workflow)],
        );

        assert_eq!(chain[0].source, Source::Workflow);
    }

    // -- Reading a definition's guards ------------------------------------

    #[test]
    fn a_guard_defaults_into_the_workflow_band() {
        let guards = guards_of(&json!({"guards": [entry("core:continue_always", None)]}));

        assert_eq!(guards[0].priority, Source::Workflow.band_floor());
        assert_eq!(guards[0].hook, super::super::BEFORE_WORKFLOW_TRANSITION);
    }

    #[test]
    fn a_guard_keeps_the_priority_it_declares() {
        let guards = guards_of(&json!({"guards": [entry("core:continue_always", Some(310))]}));

        assert_eq!(guards[0].priority, 310);
    }

    /// §3: *disabled entries stay registered but are skipped by the resolver.*
    #[test]
    fn a_disabled_guard_is_not_in_the_chain() {
        let guards = guards_of(&json!({
            "guards": [{"handler": "core:continue_always", "isEnabled": false}]
        }));

        assert!(guards.is_empty());
    }

    #[test]
    fn a_transition_with_no_guards_has_an_empty_chain() {
        assert!(guards_of(&json!({"from": "A", "to": "B"})).is_empty());
    }

    // -- Registration errors ----------------------------------------------

    #[test]
    fn refuses_a_core_handler_this_build_does_not_perform() {
        let details = registration_errors(&json!({
            "transitions": [{"guards": [entry("core:reserve_bugdet", None)]}]
        }));

        assert_eq!(codes(&details), ["HANDLER_NOT_FOUND"]);
        assert_eq!(details[0].path, "definition.transitions.0.guards.0.handler");
        // And it lists what is available, so a typo is fixable from the message.
        assert!(
            details[0].message.contains("core:reject_when"),
            "{}",
            details[0].message
        );
    }

    #[test]
    fn accepts_a_core_handler_it_does_perform() {
        assert!(registration_errors(&json!({
            "transitions": [{"guards": [entry("core:set_form_field", None)]}]
        }))
        .is_empty());
    }

    #[test]
    fn refuses_a_reference_that_is_not_the_grammar() {
        let details = registration_errors(&json!({
            "transitions": [{"guards": [entry("RequireAttachment", None)]}]
        }));

        assert_eq!(codes(&details), ["HANDLER_REFERENCE_INVALID"]);
    }

    /// A missing plugin and a mistyped handler are different problems, and
    /// telling somebody their handler is wrong when their plugin is absent
    /// sends them to the wrong file.
    #[test]
    fn refuses_a_plugin_handler_with_its_own_sentence() {
        let details = registration_errors(&json!({
            "transitions": [{"guards": [entry("plugin:erp-connector:reserve_budget", None)]}]
        }));

        assert_eq!(codes(&details), ["HANDLER_PLUGIN_UNKNOWN"]);
        assert!(details[0].message.contains("erp-connector"));
        assert!(details[0].message.contains("no plugins"));
    }

    /// §3.2's kind constraint: an `after_*` name inside `guards`.
    #[test]
    fn refuses_an_after_hook_name_in_a_guards_entry() {
        let details = registration_errors(&json!({
            "transitions": [{
                "guards": [{
                    "handler": "core:continue_always",
                    "hook": "after_workflow_transition"
                }]
            }]
        }));

        assert_eq!(codes(&details), ["HOOK_NAME_MISMATCH"]);
    }

    #[test]
    fn accepts_the_implied_name_written_out() {
        assert!(registration_errors(&json!({
            "transitions": [{
                "guards": [{
                    "handler": "core:continue_always",
                    "hook": "before_workflow_transition"
                }]
            }]
        }))
        .is_empty());
    }

    /// `actions` are validated even though they are never invoked. A
    /// registration that could never resolve should be refused whether or not
    /// this build would have run it — the alternative is a definition that
    /// publishes today and breaks on the release that starts firing them.
    #[test]
    fn an_actions_entry_is_checked_too_though_it_does_not_fire() {
        let details = registration_errors(&json!({
            "transitions": [{"actions": [entry("core:not_a_handler", None)]}]
        }));

        assert_eq!(codes(&details), ["HANDLER_NOT_FOUND"]);
        assert_eq!(
            details[0].path,
            "definition.transitions.0.actions.0.handler"
        );
    }

    #[test]
    fn an_actions_entry_naming_the_before_hook_is_the_wrong_kind() {
        let details = registration_errors(&json!({
            "transitions": [{
                "actions": [{
                    "handler": "core:continue_always",
                    "hook": "before_workflow_transition"
                }]
            }]
        }));

        assert_eq!(codes(&details), ["HOOK_NAME_MISMATCH"]);
    }

    #[test]
    fn reports_every_bad_registration_rather_than_the_first() {
        let details = registration_errors(&json!({
            "transitions": [
                {"guards": [entry("core:nope", None), entry("core:continue_always", None)]},
                {"guards": [entry("nonsense", None)]}
            ]
        }));

        assert_eq!(details.len(), 2);
        assert_eq!(details[0].path, "definition.transitions.0.guards.0.handler");
        assert_eq!(details[1].path, "definition.transitions.1.guards.0.handler");
    }

    // -- The shapes the log and the payload need --------------------------

    #[test]
    fn a_transition_ref_is_what_the_execution_log_records() {
        assert_eq!(
            transition_ref("purchase_approval", 3, "PENDING", "APPROVED"),
            "purchase_approval@3:PENDING->APPROVED"
        );
    }

    #[test]
    fn the_workflow_context_carries_the_shape_lhcs_names() {
        let context = workflow_context("k", 1, Uuid::nil(), "S", "S", "T", "AUTO");

        assert_eq!(context["workflowKey"], "k");
        assert_eq!(context["transition"]["from"], "S");
        assert_eq!(context["transition"]["action"], "AUTO");
        assert!(context["taskId"].is_null());
    }
}
