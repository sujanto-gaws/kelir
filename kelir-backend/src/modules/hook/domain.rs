//! The three shapes of the Lifecycle Hook Contract (LHCS 1.0.0, [#339]).
//!
//! **This is the plugin ABI of the lifecycle**, and it is written from the
//! specification rather than from what the first caller needs: a handler
//! registered by a workflow definition and one registered by a document type
//! receive the same payload and return the same result, which is the whole
//! claim LHCS §1 makes.
//!
//! What is *implemented* is narrower than what is declared, and the boundary is
//! [`super`]'s module doc rather than a silence here.
//!
//! [#339]: https://github.com/sujanto-gaws/kelir/issues/339

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use uuid::Uuid;

/// Where a registration came from (LHCS §1.1).
///
/// The band a priority defaults into (§3.1) and the `source` column of
/// `document_hook_executions` (§6.12) are both this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Source {
    Core,
    DocumentType,
    Workflow,
    Plugin,
}

impl Source {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Core => "CORE",
            Self::DocumentType => "DOCUMENT_TYPE",
            Self::Workflow => "WORKFLOW",
            Self::Plugin => "PLUGIN",
        }
    }

    /// The floor of this source's priority band (LHCS §3.1), which is what a
    /// registration omitting `priority` takes.
    pub fn band_floor(self) -> i32 {
        match self {
            Self::Core => 0,
            Self::DocumentType => 100,
            Self::Workflow => 300,
            Self::Plugin => 500,
        }
    }

    /// Whether `priority` sits in this source's band.
    ///
    /// **A warning rather than a refusal**, which §3.1 states outright: the
    /// bands are a convention that keeps merged chains predictable, not a
    /// constraint. A deployment with a reason to interleave two sources is not
    /// wrong, it is unusual.
    pub fn band_holds(self, priority: i32) -> bool {
        let floor = self.band_floor();

        match self {
            Self::Core => (floor..100).contains(&priority),
            Self::DocumentType => (floor..300).contains(&priority),
            Self::Workflow => (floor..500).contains(&priority),
            Self::Plugin => priority >= floor,
        }
    }
}

/// A handler reference — the string naming executable code (LHCS §2).
///
/// **Parsed, not matched later.** §2's grammar has two shapes and the
/// difference decides what resolution means: an unknown `core:` handler is an
/// ERROR at registration, and an unknown *plugin* is an ERROR while a
/// **disabled** one is a warning that leaves the entry registered and inert. A
/// `String` carried to the call site would make that three string comparisons
/// in whatever order somebody wrote them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandlerReference {
    /// `core:<handler_name>`.
    Core(String),
    /// `plugin:<pluginId>:<handler_name>`.
    Plugin { plugin: String, handler: String },
}

impl HandlerReference {
    /// A reference as §2's pattern spells it, or `None`.
    ///
    /// Written as a walk of the segments rather than as the published regular
    /// expression, because the crate that would compile it reads `\w` and the
    /// character classes differently from ECMA-262 — the divergence **D-15**
    /// carries for the JFSS `regex` rule, and this is a grammar simple enough
    /// not to inherit it.
    pub fn parse(value: &str) -> Option<Self> {
        let mut segments = value.split(':');

        match (segments.next()?, segments.next()?, segments.next()) {
            ("core", handler, None) if is_snake(handler) => Some(Self::Core(handler.to_owned())),
            ("plugin", plugin, Some(handler))
                if is_kebab(plugin) && is_snake(handler) && segments.next().is_none() =>
            {
                Some(Self::Plugin {
                    plugin: plugin.to_owned(),
                    handler: handler.to_owned(),
                })
            }
            _ => None,
        }
    }
}

impl fmt::Display for HandlerReference {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Core(handler) => write!(formatter, "core:{handler}"),
            Self::Plugin { plugin, handler } => write!(formatter, "plugin:{plugin}:{handler}"),
        }
    }
}

/// `[a-z][a-z0-9_]*`.
fn is_snake(value: &str) -> bool {
    let mut characters = value.chars();

    characters
        .next()
        .is_some_and(|first| first.is_ascii_lowercase())
        && characters.all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
        })
}

/// `[a-z][a-z0-9-]*`.
fn is_kebab(value: &str) -> bool {
    let mut characters = value.chars();

    characters
        .next()
        .is_some_and(|first| first.is_ascii_lowercase())
        && characters.all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
}

/// Which half of a chain a handler serves (ADR-0041 §2).
///
/// **Declared by the handler, not inferred from where it is registered.** A
/// `REJECT` after commit looks like a veto and is not one, and a `MODIFY` after
/// commit looks like a write and is not one — so a handler whose only results
/// are those is before-only, and naming it in `actions` is refused at publish
/// rather than run to no effect (ADR-0016's failure, one layer out).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandlerKind {
    Before,
    After,
    Both,
}

impl HandlerKind {
    pub fn serves_before(self) -> bool {
        matches!(self, Self::Before | Self::Both)
    }

    pub fn serves_after(self) -> bool {
        matches!(self, Self::After | Self::Both)
    }
}

/// One entry of a chain (LHCS §3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registration {
    pub hook: String,
    pub handler: HandlerReference,
    pub priority: i32,
    pub config: Value,
    pub source: Source,
}

/// The lifecycle stage a hook fires at (LHCS §4's `stage` enum).
///
/// Only the stages this build reaches are listed. The rest of §4's twenty are
/// not declared as unreachable variants: an enum is a claim about what the code
/// can produce, and a variant nothing constructs reads as a stage that happens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Transition,
}

impl Stage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Transition => "TRANSITION",
        }
    }
}

/// What a handler receives (LHCS §4).
///
/// **Every field is present, and inapplicable ones are `null`** — §4 says so in
/// as many words, and it is the reason this is assembled into a `Value` rather
/// than serialized from a struct of `Option`s: `serde` would elide a `None` by
/// default and a handler written against "the full shape" would meet a missing
/// key on the one stage that did not carry it.
pub struct Invocation<'a> {
    pub hook_name: &'a str,
    pub stage: Stage,
    pub source: Source,
    pub tenant_id: Uuid,
    pub document_id: Uuid,
    pub document_type_key: &'a str,
    pub current_status: Option<&'a str>,
    pub target_status: Option<&'a str>,
    pub actor_user_id: Option<Uuid>,
    pub form_data: Value,
    pub metadata: Value,
    pub workflow_context: Value,
    pub subject: Value,
    pub config: Value,
    pub correlation_id: Uuid,
}

impl Invocation<'_> {
    /// The payload as §4 shapes it.
    pub fn as_json(&self) -> Value {
        json!({
            "hookName": self.hook_name,
            "stage": self.stage.as_str(),
            "source": self.source.as_db(),
            "tenantId": self.tenant_id,
            "documentId": self.document_id,
            "documentTypeKey": self.document_type_key,
            "currentStatus": self.current_status,
            "targetStatus": self.target_status,
            "actorUserId": self.actor_user_id,
            "formData": self.form_data,
            "metadata": self.metadata,
            "workflowContext": self.workflow_context,
            "subject": self.subject,
            "config": self.config,
            "correlationId": self.correlation_id,
            "invokedAt": chrono::Utc::now().to_rfc3339(),
        })
    }
}

/// What a `before_*` handler returns (LHCS §5.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookResult {
    /// The chain proceeds unchanged.
    Continue,
    /// The chain proceeds with this data. Later handlers see it, and so does
    /// the action.
    Modify {
        form_data: Option<Value>,
        metadata: Option<Value>,
    },
    /// The chain stops and the transaction rolls back.
    Reject(Rejection),
}

impl HookResult {
    /// The `result` column of `document_hook_executions` (§6.12, §7).
    pub fn as_db(&self) -> &'static str {
        match self {
            Self::Continue => "CONTINUE",
            Self::Modify { .. } => "MODIFY",
            Self::Reject(_) => "REJECT",
        }
    }
}

/// A refusal from a before-handler (LHCS §5.1, §6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejection {
    /// `SCREAMING_SNAKE_CASE`, machine-readable.
    pub code: String,
    pub message: String,
    /// `[{ "field", "message" }]`, surfaced ahead of the `_hook` entry.
    pub details: Vec<(String, String)>,
}

impl Rejection {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details: Vec::new(),
        }
    }
}

/// One recent execution of a handler, as the circuit breaker reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecentExecution {
    pub is_error: bool,
    /// Whether the cool-down has passed since this execution — measured on the
    /// database's clock, which is the clock `executed_at` was written by.
    pub cooled: bool,
}

/// How many consecutive `ERROR`s open a handler's breaker (ADR-0041 §2).
pub const BREAKER_THRESHOLD: usize = 5;

/// How long an open breaker waits after the handler's last `ERROR` before it
/// allows one trial (ADR-0041 §2).
pub const BREAKER_COOL_DOWN: std::time::Duration = std::time::Duration::from_secs(10 * 60);

/// Whether a handler's breaker is open, from its most recent executions,
/// newest first (ADR-0041 §2).
///
/// **Open means the last five are all `ERROR` and the newest of them has not
/// cooled.** Once ten minutes have passed since that last `ERROR` the handler
/// gets one trial: a success is a non-`ERROR` row, and the breaker is closed
/// because the last five are no longer all errors; a failure is a fresh `ERROR`,
/// which restarts the cool-down from itself. Nothing is stored but the log.
pub fn breaker_is_open(recent: &[RecentExecution]) -> bool {
    recent.len() >= BREAKER_THRESHOLD
        && recent[..BREAKER_THRESHOLD]
            .iter()
            .all(|execution| execution.is_error)
        && !recent[0].cooled
}

/// Whether the newest execution is the one that opened the breaker, and so
/// the one that reports it (ADR-0041 §2: *notified once, by the execution that
/// opened it*).
///
/// Given up to six executions, newest first. The newest five are all `ERROR`
/// and the sixth is not — or there is no sixth. A failed trial is a sixth
/// consecutive `ERROR`, so it reopens nothing and tells nobody twice.
pub fn breaker_opened_by_latest(recent: &[RecentExecution]) -> bool {
    recent.len() >= BREAKER_THRESHOLD
        && recent[..BREAKER_THRESHOLD]
            .iter()
            .all(|execution| execution.is_error)
        && recent
            .get(BREAKER_THRESHOLD)
            .is_none_or(|older| !older.is_error)
}

/// A handler's own configuration, read the way every core handler reads it.
///
/// A free function rather than a method on `Value`, so a handler that wants a
/// string setting cannot quietly accept a number by writing `to_string`.
pub fn setting<'a>(config: &'a Value, key: &str) -> Option<&'a str> {
    config.get(key).and_then(Value::as_str)
}

/// The `formData` object a handler was handed, as a map it can rewrite.
pub fn form_data_of(payload: &Value) -> Map<String, Value> {
    payload
        .get("formData")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_two_handler_reference_shapes() {
        assert_eq!(
            HandlerReference::parse("core:require_attachment"),
            Some(HandlerReference::Core("require_attachment".to_owned()))
        );
        assert_eq!(
            HandlerReference::parse("plugin:erp-connector:reserve_budget"),
            Some(HandlerReference::Plugin {
                plugin: "erp-connector".to_owned(),
                handler: "reserve_budget".to_owned(),
            })
        );
    }

    #[test]
    fn a_reference_round_trips_through_its_own_spelling() {
        for value in ["core:set_form_field", "plugin:erp-connector:reserve_budget"] {
            assert_eq!(
                HandlerReference::parse(value).expect("parses").to_string(),
                value
            );
        }
    }

    /// §2's grammar, refused where it does not hold. A reference that does not
    /// resolve at registration is an ERROR, so the parser is the first gate and
    /// a permissive one would push the failure to the call site.
    #[test]
    fn refuses_what_the_grammar_does_not_allow() {
        for value in [
            "require_attachment",          // no scheme
            "core:",                       // no handler
            "core:Require_Attachment",     // not snake_case
            "core:1st_handler",            // does not start with a letter
            "plugin:erp-connector",        // no handler
            "plugin:ErpConnector:reserve", // plugin id not kebab-case
            "plugin:erp-connector:a:b",    // a segment too many
            "other:thing",                 // not a scheme §2 defines
        ] {
            assert_eq!(HandlerReference::parse(value), None, "{value} was accepted");
        }
    }

    /// §3.1's bands, and the floor a registration omitting `priority` takes.
    #[test]
    fn each_source_defaults_into_its_own_band() {
        assert_eq!(Source::Core.band_floor(), 0);
        assert_eq!(Source::DocumentType.band_floor(), 100);
        assert_eq!(Source::Workflow.band_floor(), 300);
        assert_eq!(Source::Plugin.band_floor(), 500);

        for source in [
            Source::Core,
            Source::DocumentType,
            Source::Workflow,
            Source::Plugin,
        ] {
            assert!(source.band_holds(source.band_floor()));
        }
    }

    #[test]
    fn a_priority_outside_its_band_is_recognised_rather_than_refused() {
        // §3.1: accepted with a WARNING. The predicate exists to raise one.
        assert!(!Source::Workflow.band_holds(50));
        assert!(!Source::Core.band_holds(100));
        // The plugin band is open-ended, so nothing above its floor is outside.
        assert!(Source::Plugin.band_holds(9_000));
    }

    /// §4: *fields not applicable to the stage are `null`, never absent —
    /// handlers can rely on the full shape.*
    #[test]
    fn every_payload_field_is_present_even_when_it_is_null() {
        let invocation = Invocation {
            hook_name: "before_workflow_transition",
            stage: Stage::Transition,
            source: Source::Workflow,
            tenant_id: Uuid::nil(),
            document_id: Uuid::nil(),
            document_type_key: "PURCHASE_REQUISITION",
            current_status: None,
            target_status: None,
            actor_user_id: None,
            form_data: json!({}),
            metadata: json!({}),
            workflow_context: Value::Null,
            subject: Value::Null,
            config: json!({}),
            correlation_id: Uuid::nil(),
        };

        let payload = invocation.as_json();
        let object = payload.as_object().expect("an object");

        for field in [
            "hookName",
            "stage",
            "source",
            "tenantId",
            "documentId",
            "documentTypeKey",
            "currentStatus",
            "targetStatus",
            "actorUserId",
            "formData",
            "metadata",
            "workflowContext",
            "subject",
            "config",
            "correlationId",
            "invokedAt",
        ] {
            assert!(object.contains_key(field), "`{field}` is missing");
        }

        // Present *and* null, which is the half a `skip_serializing_if` would
        // have broken without failing the check above.
        assert!(payload["currentStatus"].is_null());
        assert!(payload["subject"].is_null());
    }

    fn run(is_error: bool, cooled: bool) -> RecentExecution {
        RecentExecution { is_error, cooled }
    }

    /// ADR-0041 §2: five `ERROR`s in a row, the newest not yet ten minutes old.
    #[test]
    fn five_recent_errors_open_the_breaker_and_four_do_not() {
        let error = run(true, false);

        assert!(breaker_is_open(&[error; 5]));
        assert!(!breaker_is_open(&[error; 4]));
        // One success among the five closes it.
        assert!(!breaker_is_open(&[
            error,
            error,
            run(false, false),
            error,
            error
        ]));
    }

    /// The cool-down is measured from the **newest** `ERROR`: once it has
    /// passed, one trial runs; a failed trial is a fresh `ERROR` and the
    /// breaker is open again.
    #[test]
    fn the_cool_down_allows_a_trial_and_a_failed_trial_restarts_it() {
        let old = run(true, true);

        assert!(!breaker_is_open(&[old; 5]), "cooled: the trial runs");
        assert!(breaker_is_open(&[run(true, false), old, old, old, old]));
    }

    /// Told once: by the fifth consecutive `ERROR`, and not by a sixth.
    #[test]
    fn only_the_execution_that_opens_the_breaker_reports_it() {
        let error = run(true, false);
        let success = run(false, true);

        assert!(breaker_opened_by_latest(&[error; 5]));
        assert!(breaker_opened_by_latest(&[
            error, error, error, error, error, success
        ]));
        assert!(!breaker_opened_by_latest(&[error; 6]), "already open");
        assert!(!breaker_opened_by_latest(&[error; 4]));
    }

    #[test]
    fn a_result_names_the_column_value_the_log_records() {
        assert_eq!(HookResult::Continue.as_db(), "CONTINUE");
        assert_eq!(
            HookResult::Modify {
                form_data: None,
                metadata: None
            }
            .as_db(),
            "MODIFY"
        );
        assert_eq!(
            HookResult::Reject(Rejection::new("BUDGET_EXCEEDED", "no")).as_db(),
            "REJECT"
        );
    }
}
