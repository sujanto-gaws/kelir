//! What a definition's `validation` and `rules` decide about one value, on the
//! server (JFSS §5, §6; FR-RAD-006, [#164]).
//!
//! **This is the other half of a `both`-scoped rule, and it is a port rather
//! than a second opinion.** JFSS §6.1 scopes a rule `client`, `server` or
//! `both`, and `both` means *"real-time UX + strict security re-evaluation"* —
//! the same rule decided twice, by two runtimes, which S1.2.2 calls Polyglot
//! Parity. The frontend's copy is `features/rad/renderer/validation.ts`, and
//! every function below is written against it keyword for keyword and message
//! for message. Where the two could not be made identical the divergence is
//! named in a comment on the spot rather than left for whoever meets it.
//!
//! **Pure, and deliberately so.** A value and the scope its `key` addresses go
//! in; a violation or nothing comes out. Nothing here touches the database, the
//! caller's claims or the request — which is what lets
//! [`crate::modules::rad::service::evaluation`] run inside Sprint 9's numbering
//! transaction ([#168](https://github.com/sujanto-gaws/kelir/issues/168))
//! without dragging a submission row along.
//!
//! **This is the rule catalogue and not the rule engine.** The dependency
//! graph, cycle detection and error-code mapping are FR-RAD-006 in Sprints
//! 14–16 under decision **D-2**. What is here is the catalogue's *membership*
//! question — is this a rule the [Validation Rule
//! Registry](../../../../../docs/schema/JFSS%20Validation%20Rule%20Registry.md)
//! defines, and what does it decide — which a submission cannot avoid
//! answering.
//!
//! **A rule nobody has heard of is an error, never a skipped arm** (registry §4
//! step 4, JFSS S8.1.1). The reference implementation returns `true` there,
//! commented as failing open, and *a rule that fails open is a rule that is not
//! there, reported as a rule that passed.* That is the operator-parity spike's
//! defect one layer down.
//!
//! [#164]: https://github.com/sujanto-gaws/kelir/issues/164

use serde_json::{Map, Value};

use super::jfss::role_of;

/// Why a field is not acceptable, in the two parts JFSS S10.3 names.
///
/// `rule` is the keyword or registry rule that decided it. The S10.3 `path` is
/// the caller's to supply: a violation does not know where in the tree it was
/// raised, and a row's path is not a property of the row's own scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldViolation {
    pub rule: String,
    pub message: String,
}

impl FieldViolation {
    fn new(rule: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            rule: rule.into(),
            message: message.into(),
        }
    }
}

/// A rule the registry defines, that this side is the right side for, and that
/// Kelir does not yet decide.
///
/// **It refuses the submission rather than passing.** The browser has somewhere
/// to defer to — its `undecided` tier says *"checked when this form is
/// submitted"* and means here. This side has nobody below it. Reporting a
/// `unique` as satisfied because no uniqueness check is written yet is the same
/// failure as the unknown-rule arm, with a better excuse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnenforcedRule {
    pub rule: String,
    pub reason: String,
}

/// A rule name the Validation Rule Registry does not define.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownRule {
    pub rule: String,
}

/// What a definition decides about one value, and what it leaves undecided.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FieldOutcome {
    /// The single reason the field is refused, or none.
    pub violation: Option<FieldViolation>,
    /// Registry rules this side is responsible for and does not implement.
    pub unenforced: Vec<UnenforcedRule>,
    /// Rule names outside the registry — a defect in the definition.
    pub unknown: Vec<UnknownRule>,
}

/// Where the registry says a rule can run (§3.1–§3.3).
///
/// **Not the scope the definition declares.** The two are different questions:
/// a definition's `scope` says where the author wants the rule run, and this
/// says where it *can* be. A definition pairing `unique` with `scope: "both"`
/// is stored happily and cannot be decided in a browser whatever it declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RegistryScope {
    Client,
    Server,
    Both,
}

/// One entry of the Validation Rule Registry, as this side sees it.
struct RegistryRule {
    scope: RegistryScope,
    /// Decides the rule here. `None` exactly when `unenforceable` is `Some`.
    decide: Option<fn(&RuleContext<'_>) -> bool>,
    /// Why this side does not decide it. `None` exactly when `decide` is.
    unenforceable: Option<&'static str>,
}

/// What a registry rule is handed to decide a value.
struct RuleContext<'a> {
    value: &'a Value,
    /// The record the component's `key` addresses — the payload at the top
    /// level, a **row object** inside a repeater (§4.3.1). Which is what makes
    /// `matchesField` mean the right thing in both: a rule inside a row
    /// template targets a sibling column of the same row, not a top-level field
    /// that happens to share its name.
    scope: &'a Map<String, Value>,
    params: &'a Map<String, Value>,
}

/// Every rule name the registry defines. **This is the catalogue**, and a name
/// outside it is an [`UnknownRule`].
///
/// A `match` rather than a lookup table, so adding a name without deciding what
/// it means does not compile.
fn registry_rule(name: &str) -> Option<RegistryRule> {
    let entry = match name {
        // --- §3.1, scope `both`: shared data integrity --------------------
        //
        // Decided here *and* in the browser. Neither side is trusting the
        // other: the browser's copy is for the person typing, and this one is
        // what the stored row was actually checked against.
        "matchesField" => RegistryRule {
            scope: RegistryScope::Both,
            decide: Some(|context| context.value == scope_target(context)),
            unenforceable: None,
        },
        "notMatchesField" => RegistryRule {
            scope: RegistryScope::Both,
            decide: Some(|context| context.value != scope_target(context)),
            unenforceable: None,
        },
        // §3.1's `regex`, and the registry's own warning applies to every use
        // of it. `new RegExp` in the browser and the `regex` crate here read
        // the same pattern differently: this crate refuses lookahead and
        // backreferences at compile time, and reads `\d` as Unicode `Nd` where
        // ECMA-262 reads it as ASCII. For a `both`-scoped rule that means the
        // two sides can reach opposite verdicts on one input.
        //
        // **It is narrowed at the write rather than here** (**D-15**, ADR-0038;
        // registry 1.5.3). `refuse_pattern` refuses a definition carrying a
        // construct on its list, and **the list is not every divergence**
        // (#465): a pattern reaching this arm is one the list does not name,
        // not one both sides are known to agree about. Nothing is re-checked
        // here: a second scan would be a second definition of the dialect.
        //
        // An uncompilable pattern is a violation rather than a pass, which is
        // the browser's `catch` arm too: a rule that could not be applied has
        // not been satisfied.
        "regex" => RegistryRule {
            scope: RegistryScope::Both,
            decide: Some(|context| {
                if is_empty(context.value) {
                    return true;
                }

                let pattern = context
                    .params
                    .get("pattern")
                    .map(js_string)
                    .unwrap_or_default();
                let flags = context
                    .params
                    .get("flags")
                    .map(js_string)
                    .unwrap_or_default();

                matches_pattern(&pattern, &flags, &js_string(context.value))
            }),
            unenforceable: None,
        },
        "oneOf" => RegistryRule {
            scope: RegistryScope::Both,
            decide: Some(|context| param_values(context).iter().any(|v| v == context.value)),
            unenforceable: None,
        },
        "notOneOf" => RegistryRule {
            scope: RegistryScope::Both,
            decide: Some(|context| !param_values(context).iter().any(|v| v == context.value)),
            unenforceable: None,
        },

        // --- §3.2, scope `client`: UX enhancements ------------------------
        //
        // Ignored here, and that is §6.1's table rather than a gap: *"Frontend
        // Only. UX enhancements. Ignored by backend."* Neither lets data
        // through that this side would otherwise refuse — the registry's own
        // note says the backend relies on `validation.minLength` and
        // `validation.pattern` for password security, and both are checked
        // below.
        "passwordStrength" | "async" => RegistryRule {
            scope: RegistryScope::Client,
            decide: None,
            unenforceable: Some("the registry scopes it to the browser (§3.2)"),
        },

        // --- §3.3, scope `server`: security and business logic ------------
        //
        // **This side is the right side, and Kelir does not implement them
        // yet.** So a definition carrying one cannot be submitted at all,
        // rather than being submitted with the rule silently unrun. The
        // Sprint 8 construction plan §8 records the same choice from the other
        // end: a `unique` was deliberately kept out of the shared fixture,
        // because carrying one would have obliged this issue to write a real
        // uniqueness check against a real table before that fixture could be
        // submitted at all.
        "unique" => RegistryRule {
            scope: RegistryScope::Server,
            decide: None,
            unenforceable: Some(
                "a uniqueness check is a database query against the table this form writes \
                 to, and a form does not write to a table yet — documents are Sprint 9 \
                 (FR-DOC-001..007)",
            ),
        },
        "exists" => RegistryRule {
            scope: RegistryScope::Server,
            decide: None,
            unenforceable: Some(
                "a foreign-key check needs the entity the value references, which a form \
                 submission does not yet declare — document links are Sprint 9 (FR-DOC-011)",
            ),
        },
        "authorized" => RegistryRule {
            scope: RegistryScope::Server,
            decide: None,
            unenforceable: Some(
                "a per-value permission check is FR-DTYPE-008's document security level, \
                 which is unbuilt; the submission's own permission is checked before any \
                 rule here runs",
            ),
        },
        _ => return None,
    };

    Some(entry)
}

/// Whether the Validation Rule Registry defines `name`.
///
/// **The catalogue's membership question, asked without a value in hand.**
/// [`validate_field`] answers it as a side effect of deciding one submitted
/// value, which is too late for the only person who can act on the answer: a
/// rule name nobody defines is a defect in the *definition*, and the author is
/// gone by the time somebody is filling the form in. [`super::engine`] asks it
/// at publish over the same `match`, so the two moments cannot disagree about
/// what the registry contains.
pub(crate) fn is_registered(name: &str) -> bool {
    registry_rule(name).is_some()
}

/// `params.target`'s value in the scope the rule was raised in.
///
/// **Structural equality where the browser has reference equality.** `===` in
/// JavaScript compares two parsed objects by identity, so `matchesField`
/// between two array-valued fields is `false` there however equal they look and
/// `true` here. The direction is safe — the browser refuses first, so nothing
/// the browser blocked reaches this side — and the shape is exotic: the rule
/// exists for "confirm your password" and "confirm the email address", which
/// are strings. It is named because a silent difference is what S1.2.2 is
/// about.
fn scope_target<'a>(context: &'a RuleContext<'a>) -> &'a Value {
    static ABSENT: Value = Value::Null;

    let target = context
        .params
        .get("target")
        .map(js_string)
        .unwrap_or_default();

    context.scope.get(&target).unwrap_or(&ABSENT)
}

/// The values a rule compares against, when its params carry a list.
fn param_values<'a>(context: &'a RuleContext<'a>) -> &'a [Value] {
    context
        .params
        .get("values")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

/// JFSS §5's *"present and non-empty"*.
///
/// **`false` is a value, not an absence.** The browser's copy says the same and
/// gives the reason: a client that called `false` empty would refuse
/// submissions this side accepts, which is the divergence class S10.1.1 was
/// errata'd to close. A consent checkbox that must be ticked is expressible
/// without inventing a second meaning for `required` — `validation.enum: [true]`
/// says it, and `enum` is checked below.
pub fn is_empty(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(text) => text.is_empty(),
        Value::Array(items) => items.is_empty(),
        _ => false,
    }
}

/// Whether a present value is the `type` the definition declares.
fn matches_type(value: &Value, declared: &str) -> bool {
    match declared {
        "string" => value.is_string(),
        "number" => value.as_f64().is_some_and(f64::is_finite),
        // `Number.isInteger(2.0)` is true in the browser, and `2.0` is how a
        // JavaScript runtime spells a whole number. The test is therefore on
        // the value and not on the encoding: reading `is_i64` here instead
        // would refuse every whole number a browser ever submitted.
        "integer" => value
            .as_f64()
            .is_some_and(|number| number.is_finite() && number.fract() == 0.0),
        "boolean" => value.is_boolean(),
        "array" => value.is_array(),
        // Every non-array object. JFSS uses `object` for shapes no component
        // type currently collects, and refusing one for not being an array
        // would be this file inventing a constraint.
        "object" => value.is_object(),
        // A `type` outside §5's six is refused by the meta-schema at save, so a
        // stored definition cannot carry one. Answering `false` rather than
        // `true` keeps the unreachable branch on the safe side.
        _ => false,
    }
}

/// A value as JavaScript's `String(...)` spells it.
///
/// Used where the browser's copy applies a regular expression to a value that
/// is not necessarily a string. **The number case is the one that matters:**
/// `serde_json` prints the float `42.0` as `42.0` and JavaScript prints `42`,
/// so a `pattern` of `^[0-9]+$` over a numeric field would hold in the browser
/// and fail here on a decimal point only one side writes.
pub fn js_string(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => "null".to_owned(),
        Value::Bool(true) => "true".to_owned(),
        Value::Bool(false) => "false".to_owned(),
        Value::Number(number) => match number.as_f64() {
            Some(as_float) if as_float.fract() == 0.0 && as_float.abs() < 1e21 => {
                format!("{}", as_float as i128)
            }
            _ => number.to_string(),
        },
        // `String([1, 2])` is "1,2" and `String({})` is "[object Object]".
        // Neither is a shape a `pattern` or a `regex` should ever meet — both
        // are declared on `string` fields — and both are spelled the browser's
        // way anyway, because "should never happen" is not a reason for the two
        // sides to answer differently when it does.
        Value::Array(items) => items
            .iter()
            .map(|item| match item {
                Value::Null => String::new(),
                other => js_string(other),
            })
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => "[object Object]".to_owned(),
    }
}

/// A pattern as the browser's `new RegExp(pattern, flags).test(value)` means
/// it.
///
/// Unanchored on both sides — `test` searches, and so does `Regex::is_match` —
/// and an uncompilable pattern is `false`, which is the browser's `catch` arm.
/// **Every flag but `i`, `m` and `s` is dropped rather than refused.** That is
/// harmless for `g` and `d`, which mean nothing to a single `test`. **It is not
/// harmless for the rest** (#482, record 17 finding 1): the browser anchors
/// the match at the start under `y`, refuses identity escapes such as `\-`
/// under `u` and `v`, and throws on a letter it does not know or one given
/// twice. Validation Rule Registry
/// 1.5.2 tabulates them.
fn matches_pattern(pattern: &str, flags: &str, value: &str) -> bool {
    compile_pattern(pattern, flags)
        .map(|compiled| compiled.is_match(value))
        .unwrap_or(false)
}

/// The one place a pattern is built, so the check at save and the evaluation at
/// submit cannot disagree about what this backend accepts.
fn compile_pattern(pattern: &str, flags: &str) -> Result<regex::Regex, regex::Error> {
    regex::RegexBuilder::new(pattern)
        .case_insensitive(flags.contains('i'))
        .multi_line(flags.contains('m'))
        .dot_matches_new_line(flags.contains('s'))
        .build()
}

/// Why this backend will not honour a pattern as written.
///
/// **Both arms are refusals at save rather than verdicts at submit**
/// ([#391](https://github.com/sujanto-gaws/kelir/issues/391), **D-15**), and
/// they fail for opposite reasons: the first is loud on this side and the
/// second is silent on both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PatternRefusal {
    /// The `regex` crate cannot build it — lookahead, backreferences, or a
    /// syntax error. `matches_pattern` maps that to `false`, so **every value
    /// fails a rule nobody can satisfy**, which is right at submit and wrong at
    /// save.
    Uncompilable { reason: String },
    /// A construct both engines compile and **read differently**, so the two
    /// decide one input opposite ways with nothing raised on either side.
    ///
    /// **This is the refusal [ADR-0038] §1 argues is the important one** — a
    /// construct only the server enforces is loud, and a construct both sides
    /// accept while disagreeing is silent.
    ///
    /// [ADR-0038]: ../../../../../docs/architectures/adr/0038.%20Kelir%20Patterns%20Are%20the%20Linear-Time%20Subset.md
    Divergent(DivergentConstruct),
}

/// A construct the `regex` crate and ECMA-262 both accept and read differently.
///
/// **Each arm carries its own reason, because they do not share one.**
/// [#413](https://github.com/sujanto-gaws/kelir/issues/413): one message said
/// *ECMA-262's classes are ASCII and this crate's are Unicode* for all of
/// `\d`, `\w` and `\s`, and told an author whose pattern used `\s` to write
/// `[0-9]`. **`\s` is Unicode on both sides** — the two sets differ at exactly
/// U+0085 and U+FEFF, in opposite directions — so the reason was wrong and the
/// remedy belonged to a different pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DivergentConstruct {
    /// A bare `\d`, `\D`, `\w`, `\W`, `\s` or `\S`.
    Class(char),
    /// `\b` or `\B`, whose word-character set is ASCII in ECMA-262 and Unicode
    /// here. **The one arm with no portable spelling**: `\d` becomes `[0-9]`
    /// and a POSIX name becomes the class it abbreviates, but a boundary has to
    /// be re-expressed with the characters around it.
    WordBoundary(char),
    /// A POSIX bracket expression such as `[[:digit:]]`. **ECMA-262 has no such
    /// syntax**: the browser reads it as an ordinary class containing `[`, `:`
    /// and the letters of the name, which is a different set entirely rather
    /// than a wider or narrower one.
    PosixClass(String),
    /// `\p{…}` or `\P{…}`. **ECMA-262 reads these only under the `u` flag**,
    /// which this renderer does not pass unless the rule asks for it; without
    /// it `\p` is an identity escape and the browser reads a literal `p`.
    UnicodeProperty(char),
}

/// The refusal a pattern earns, or `None` if this backend will honour it.
///
/// **The compile check runs first and returns alone.** A pattern that does not
/// build cannot be scanned for anything else, and reporting a class problem
/// beside a syntax error would be guessing at text the parser rejected.
pub(crate) fn refuse_pattern(pattern: &str, flags: &str) -> Option<PatternRefusal> {
    if let Err(error) = compile_pattern(pattern, flags) {
        return Some(PatternRefusal::Uncompilable {
            reason: compile_reason(&error),
        });
    }

    divergent_construct(pattern).map(PatternRefusal::Divergent)
}

/// The crate's own explanation, reduced to the sentence a form author needs.
///
/// `regex::Error`'s `Display` is a three-line report with the pattern and a
/// caret under the offending span; its last `error:` line is the diagnosis
/// (*look-around … is not supported*), which is the half that tells an author
/// what to do. The whole report is kept when there is no such line.
fn compile_reason(error: &regex::Error) -> String {
    let rendered = error.to_string();

    rendered
        .lines()
        .filter_map(|line| line.trim().strip_prefix("error: "))
        .next_back()
        .map_or_else(
            || rendered.split_whitespace().collect::<Vec<_>>().join(" "),
            str::to_owned,
        )
}

/// The first construct in a pattern that the two engines read differently.
///
/// **An escaped backslash is not an escape**: `\\d` is a literal backslash
/// followed by a literal `d` and is not a class, so the scan consumes the
/// character after every backslash rather than only looking at it.
///
/// **The boundary this scan draws**, which [coding standard] §2.9 asks to be
/// stated rather than left to be inferred from an absence of findings:
///
/// - **Covered:** `\d \D \w \W \s \S` anywhere; `\b` and `\B` outside a
///   bracket expression; `\p{…}` and `\P{…}` anywhere; POSIX bracket
///   expressions (`[[:alpha:]]`, negated or not) inside one.
/// - **Not covered, deliberately:** `\b` *inside* a bracket expression, which
///   ECMA-262 reads as a backspace and ~~this crate reads the same way~~ this
///   crate refuses to compile (corrected 2026-09-17, #482), so the compile
///   check refuses it first; and any other construct the `regex` crate refuses
///   outright, which [`refuse_pattern`]'s compile check has already returned
///   on before this runs.
/// - **Not covered, and it is a limit rather than a decision:** a divergence
///   neither engine expresses as syntax — case folding under `i` differs on a
///   handful of code points, and nothing here detects that.
/// - **Not covered, and not known to be covered until 2026-09-16** (#465,
///   record 16 finding 2): constructs this crate compiles that ECMA-262 reads
///   differently or refuses. They include `.` against `\r`, U+2028 and
///   characters outside the BMP; an astral literal in a class; nested classes
///   and set operations (`[[a-c]]`, `&&`, `--`); braced and 8-digit escapes
///   (`\x{41}`, `\u{41}`, `\U00000041`); `\A`, `\z`, `\<`, `\>` and `\a`;
///   inline flags and named groups (`(?i)`, `(?P<x>…)`, `(?x)`); and `$` under
///   `m` before `\r\n`. Record 17 and #482 added more on 2026-09-17: a negated
///   class against a character outside the BMP, a class opening with `]`,
///   `~~`, a space inside `{n, m}`, stacked quantifiers, flags other than `i`,
///   `m` and `s`, and `i` itself on `k` and `s`. **This scan is a list, so a
///   construct missing from it is stored**; ADR-0038's 2026-09-16 amendment says what that means for the
///   dialect.
///
/// [coding standard]: ../../../../../docs/standards/01.%20Coding%20Standard.md
fn divergent_construct(pattern: &str) -> Option<DivergentConstruct> {
    let characters: Vec<char> = pattern.chars().collect();
    let mut index = 0;
    let mut in_class = false;

    while index < characters.len() {
        let character = characters[index];

        if character == '\\' {
            let Some(&escaped) = characters.get(index + 1) else {
                // A trailing backslash, which the compile check has already
                // refused — reaching here at all would mean it did not.
                break;
            };
            index += 2;

            match escaped {
                'd' | 'D' | 'w' | 'W' | 's' | 'S' => {
                    return Some(DivergentConstruct::Class(escaped));
                }
                'p' | 'P' => return Some(DivergentConstruct::UnicodeProperty(escaped)),
                // Inside a class this is a backspace on both sides, and the two
                // agree. Outside it, it is the boundary the engines disagree on.
                'b' | 'B' if !in_class => {
                    return Some(DivergentConstruct::WordBoundary(escaped));
                }
                // Any other escape, consumed so its argument is not re-read.
                _ => {}
            }

            continue;
        }

        if !in_class && character == '[' {
            in_class = true;
            index += 1;

            // A leading `^` negates and a leading `]` is an ordinary member;
            // neither opens anything, so neither is re-read as structure.
            if characters.get(index) == Some(&'^') {
                index += 1;
            }
            if characters.get(index) == Some(&']') {
                index += 1;
            }

            continue;
        }

        if in_class {
            if character == '[' && characters.get(index + 1) == Some(&':') {
                if let Some(name) = posix_class_name(&characters, index + 2) {
                    return Some(DivergentConstruct::PosixClass(name));
                }
            }

            if character == ']' {
                in_class = false;
            }
        }

        index += 1;
    }

    None
}

/// The name in a POSIX bracket expression, given the index just past `[:`.
///
/// Returns `None` for anything that is not one — `[[:` can open an ordinary
/// class containing `[` and `:`, and treating that as POSIX would refuse a
/// pattern both engines agree about.
fn posix_class_name(characters: &[char], start: usize) -> Option<String> {
    let mut index = start;
    let mut name = String::new();

    while index + 1 < characters.len() {
        if characters[index] == ':' && characters[index + 1] == ']' {
            return (!name.is_empty()).then_some(name);
        }

        // `[:^alpha:]` negates, and the caret is part of the name as written.
        let belongs_to_the_name = characters[index].is_ascii_alphabetic()
            || (name.is_empty() && characters[index] == '^');

        if !belongs_to_the_name {
            return None;
        }

        name.push(characters[index]);
        index += 1;
    }

    None
}

/// §5's `format` keyword.
///
/// **Digit classes are pinned to ASCII rather than written `\d`**, which is the
/// registry's own interim guidance under the `regex` warning and the reason
/// these are hand-written character tests rather than patterns: ECMA-262 `\d`
/// is ASCII-only while this crate's is Unicode `Nd`, so `\d` here and `\d` in
/// the browser disagree about `٣٤٥` with nothing raised on either side.
///
/// Two of the six cannot be made identical to the browser's, and are named
/// rather than hidden — both against decision **D-15**:
///
/// - **`uri`** is `new URL(value)` there and a scheme test here. WHATWG URL
///   parsing is a specification of its own and porting it is not a validation
///   keyword's job. What both sides agree on is that a URI without a scheme is
///   not one, and that a special scheme needs an authority.
/// - **`date-time`** is `Date.parse` there, which accepts a wide range of
///   non-standard spellings, and RFC 3339 (plus the `datetime-local` control's
///   own spelling) here. The browser is the more permissive of the two, so this
///   side refuses inputs it accepted — a submit-time surprise rather than a
///   hole, and the direction that fails safe.
fn matches_format(format: &str, value: &str) -> bool {
    match format {
        // Deliberately permissive, as the HTML `email` input is: the only proof
        // an address exists is a message delivered to it, and a stricter
        // pattern refuses real addresses long before it catches a fake one.
        // `^[^\s@]+@[^\s@]+\.[^\s@]+$`, spelled out.
        "email" => {
            let mut halves = value.split('@');
            let (Some(local), Some(domain), None) = (halves.next(), halves.next(), halves.next())
            else {
                return false;
            };

            let clean = |part: &str| !part.is_empty() && !part.chars().any(char::is_whitespace);

            clean(local)
                && clean(domain)
                && domain
                    .find('.')
                    .is_some_and(|dot| dot > 0 && dot < domain.len() - 1)
        }
        "uri" => matches_uri(value),
        // A shape *and* a real date: `2026-02-31` matches every pattern anybody
        // writes for a date and is not a day.
        "date" => {
            is_shaped(value, "dddd-dd-dd")
                && chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").is_ok()
        }
        // Bounded rather than "two digits, a colon, two digits": `25:99` is a
        // shape and not a time, and the round trip that catches `2026-02-31`
        // has no equivalent here.
        "time" => matches_time(value),
        "date-time" => {
            value.len() > 10
                && is_shaped(&value[..10], "dddd-dd-dd")
                && matches!(value.as_bytes().get(10), Some(b'T') | Some(b' '))
                && parses_as_date_time(value)
        }
        "uuid" => is_shaped(value, "hhhhhhhh-hhhh-hhhh-hhhh-hhhhhhhhhhhh"),
        // An unrecognised `format` is an annotation rather than a constraint,
        // which is JSON Schema's own default and the browser's `?? true`.
        _ => true,
    }
}

/// Whether `value` has exactly the shape `mask` describes: `d` an ASCII digit,
/// `h` an ASCII hex digit, anything else itself.
fn is_shaped(value: &str, mask: &str) -> bool {
    if value.len() != mask.len() {
        return false;
    }

    value
        .bytes()
        .zip(mask.bytes())
        .all(|(byte, expected)| match expected {
            b'd' => byte.is_ascii_digit(),
            b'h' => byte.is_ascii_hexdigit(),
            other => byte == other,
        })
}

/// `HH:MM` or `HH:MM:SS`, bounded to a real clock.
fn matches_time(value: &str) -> bool {
    let mut parts = value.split(':');
    let (Some(hours), Some(minutes)) = (parts.next(), parts.next()) else {
        return false;
    };
    let seconds = parts.next();

    if parts.next().is_some() {
        return false;
    }

    let bounded = |part: &str, ceiling: u32| {
        part.len() == 2
            && part.bytes().all(|byte| byte.is_ascii_digit())
            && part.parse::<u32>().is_ok_and(|number| number < ceiling)
    };

    bounded(hours, 24) && bounded(minutes, 60) && seconds.is_none_or(|part| bounded(part, 60))
}

/// A scheme, and an authority where the scheme is one that requires one.
///
/// The WHATWG special schemes are the ones `new URL` refuses without a host —
/// `http://` alone throws there — while every other scheme is opaque to it, so
/// `mailto:someone` parses. That is the part of URL parsing a `format` keyword
/// can honestly reproduce.
fn matches_uri(value: &str) -> bool {
    let Some(colon) = value.find(':') else {
        return false;
    };

    let (scheme, rest) = value.split_at(colon);
    let rest = &rest[1..];

    let mut characters = scheme.chars();
    let valid_scheme = characters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && characters.all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.')
        });

    if !valid_scheme {
        return false;
    }

    const SPECIAL: [&str; 6] = ["http", "https", "ws", "wss", "ftp", "file"];

    if SPECIAL.contains(&scheme.to_ascii_lowercase().as_str()) {
        // `file:` is the one special scheme WHATWG lets carry an empty host.
        return rest.starts_with("//") && (scheme.eq_ignore_ascii_case("file") || rest.len() > 2);
    }

    !rest.is_empty()
}

/// RFC 3339, or a local date and time carrying no offset.
///
/// The second arm is what `Date.parse("2026-08-27T09:30")` accepts, which is
/// the spelling an `<input type="datetime-local">` produces — so refusing it
/// would refuse the value the browser's own control emits.
fn parses_as_date_time(value: &str) -> bool {
    if chrono::DateTime::parse_from_rfc3339(value).is_ok() {
        return true;
    }

    let normalized = value.replacen(' ', "T", 1);

    [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M",
    ]
    .iter()
    .any(|layout| chrono::NaiveDateTime::parse_from_str(&normalized, layout).is_ok())
}

/// What a failing keyword says when the definition supplies nothing.
///
/// `validation.messages` overrides any of these per keyword (§5). These are the
/// fallback, and they are the browser's sentences character for character: a
/// person shown one message as they type and a different one after submitting
/// learns that two different things checked their field.
fn default_message(keyword: &str, validation: &Map<String, Value>) -> String {
    let shown = |property: &str| {
        validation
            .get(property)
            .map(js_string)
            .unwrap_or_else(|| "undefined".to_owned())
    };

    match keyword {
        "required" => "This field is required.".to_owned(),
        "type" => format!("This field takes a {}.", shown("type")),
        "minLength" => format!("Use at least {} characters.", shown("minLength")),
        "maxLength" => format!("Use at most {} characters.", shown("maxLength")),
        "minimum" => format!("Enter {} or more.", shown("minimum")),
        "maximum" => format!("Enter {} or less.", shown("maximum")),
        "pattern" => "This value is not in the expected format.".to_owned(),
        "format" => format!("Enter a valid {}.", shown("format")),
        "enum" => "Choose one of the offered values.".to_owned(),
        "uniqueItems" => "Every row must be different.".to_owned(),
        "uniqueBy" => "Two rows repeat a value that must be unique.".to_owned(),
        _ => "This value is not accepted.".to_owned(),
    }
}

/// The `validation` object's keywords, in the order a person would want them.
///
/// **One violation per field rather than a list**, which is the browser's
/// shape: a form shows one message under an input, and reporting "required" and
/// "too short" about the same empty box at once is two ways of saying it is
/// empty.
///
/// **Every keyword after `required` is skipped for an empty value**, which is
/// JSON Schema's own semantics and what keeps an optional field optional: a
/// blank `needed_by` is not a malformed date.
fn check_validation(validation: &Map<String, Value>, value: &Value) -> Option<FieldViolation> {
    let fail = |keyword: &str| {
        let message = validation
            .get("messages")
            .and_then(Value::as_object)
            .and_then(|messages| messages.get(keyword))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| default_message(keyword, validation));

        Some(FieldViolation::new(keyword, message))
    };

    if is_empty(value) {
        return if validation.get("required") == Some(&Value::Bool(true)) {
            fail("required")
        } else {
            None
        };
    }

    let declared = validation.get("type").and_then(Value::as_str).unwrap_or("");

    if !matches_type(value, declared) {
        return fail("type");
    }

    let number = |property: &str| validation.get(property).and_then(Value::as_f64);

    if let Value::String(text) = value {
        // `.length` in the browser counts UTF-16 code units, so an emoji is two
        // there and one `char` here. Counting code units on this side is what
        // keeps a `maxLength` from admitting a string the browser refused.
        let length = text.encode_utf16().count() as f64;

        if number("minLength").is_some_and(|minimum| length < minimum) {
            return fail("minLength");
        }

        if number("maxLength").is_some_and(|maximum| length > maximum) {
            return fail("maxLength");
        }

        if let Some(pattern) = validation.get("pattern").and_then(Value::as_str) {
            if !matches_pattern(pattern, "", text) {
                return fail("pattern");
            }
        }

        if let Some(format) = validation.get("format").and_then(Value::as_str) {
            if !matches_format(format, text) {
                return fail("format");
            }
        }
    }

    if let Some(as_float) = value.as_f64() {
        if number("minimum").is_some_and(|minimum| as_float < minimum) {
            return fail("minimum");
        }

        if number("maximum").is_some_and(|maximum| as_float > maximum) {
            return fail("maximum");
        }
    }

    if let Some(allowed) = validation.get("enum").and_then(Value::as_array) {
        if !allowed.iter().any(|candidate| candidate == value) {
            return fail("enum");
        }
    }

    if let Value::Array(items) = value {
        if validation.get("uniqueItems") == Some(&Value::Bool(true)) {
            let mut seen: Vec<&Value> = Vec::with_capacity(items.len());

            for item in items {
                if seen.contains(&item) {
                    return fail("uniqueItems");
                }

                seen.push(item);
            }
        }

        let keys = unique_by_keys(validation);

        if !keys.is_empty() && repeats_a_key(items, &keys) {
            return fail("uniqueBy");
        }
    }

    None
}

/// The keys a row must be unique by. §5 allows one or several.
fn unique_by_keys(validation: &Map<String, Value>) -> Vec<String> {
    match validation.get("uniqueBy") {
        Some(Value::String(single)) => vec![single.clone()],
        Some(Value::Array(several)) => several
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

/// Whether the rows of an array repeat a value under the declared keys.
fn repeats_a_key(rows: &[Value], keys: &[String]) -> bool {
    let mut seen: Vec<Vec<Value>> = Vec::with_capacity(rows.len());

    for row in rows {
        let Some(record) = row.as_object() else {
            continue;
        };

        // Every declared key together, so `uniqueBy: ["sku", "warehouse"]`
        // means the pair is unique rather than each column on its own.
        let fingerprint: Vec<Value> = keys
            .iter()
            .map(|key| record.get(key).cloned().unwrap_or(Value::Null))
            .collect();

        if seen.contains(&fingerprint) {
            return true;
        }

        seen.push(fingerprint);
    }

    false
}

/// Everything a definition says about one value: §5's keywords, then §6's
/// rules.
///
/// **Basic before advanced**, because §5's keywords describe the value's own
/// shape and §6's rules describe its relationship to other values — telling
/// somebody their password does not match the confirmation, when what they
/// typed is four characters long, answers the wrong question.
///
/// **Every rule name is resolved even when the field is empty**, though an
/// empty field's advanced verdicts are discarded. The two are separate:
/// resolving the name is what raises on one nobody defines, and a defect in the
/// definition must not be able to hide behind whatever was left blank.
pub fn validate_field(
    component: &Value,
    value: &Value,
    scope: &Map<String, Value>,
) -> FieldOutcome {
    debug_assert_eq!(
        role_of(component),
        Some("data"),
        "only a data component holds a value"
    );

    let no_object = Map::new();
    let validation = component
        .get("validation")
        .and_then(Value::as_object)
        .unwrap_or(&no_object);

    let mut outcome = FieldOutcome {
        violation: check_validation(validation, value),
        ..FieldOutcome::default()
    };

    let empty = is_empty(value);
    let mut advanced: Option<FieldViolation> = None;

    for rule in component
        .get("rules")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
    {
        let name = rule.get("rule").and_then(Value::as_str).unwrap_or("");

        let Some(entry) = registry_rule(name) else {
            outcome.unknown.push(UnknownRule {
                rule: name.to_owned(),
            });
            continue;
        };

        // The definition's own `scope` first: an author who scoped a `both`
        // rule to `client` has said where they want it run, and JFSS §6.1 makes
        // that the property's meaning. Only then does the catalogue get asked
        // whether this side could have run it anyway. The browser's copy makes
        // the mirror-image check against `server`.
        if rule.get("scope").and_then(Value::as_str) == Some("client") {
            continue;
        }

        // A rule the *registry* scopes to the browser. §6.1's table says the
        // backend ignores these, so ignoring one is the specification rather
        // than a gap — unlike the `server` tier below, where this side is the
        // only side and silence would be a check reported as run.
        if entry.scope == RegistryScope::Client {
            continue;
        }

        let Some(decide) = entry.decide else {
            outcome.unenforced.push(UnenforcedRule {
                rule: name.to_owned(),
                reason: entry.unenforceable.unwrap_or_default().to_owned(),
            });
            continue;
        };

        let params = rule
            .get("params")
            .and_then(Value::as_object)
            .unwrap_or(&no_object);

        let holds = decide(&RuleContext {
            value,
            scope,
            params,
        });

        if !holds && !empty && advanced.is_none() {
            // §6.2 makes `message` required, so a definition that reaches here
            // has one. The fallback is for a document stored before that was
            // enforced.
            advanced = Some(FieldViolation::new(
                name,
                rule.get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("This value is not accepted."),
            ));
        }
    }

    outcome.violation = outcome.violation.or(advanced);
    outcome
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// The dialect's boundary, as a table rather than as prose.
    ///
    /// **Every row of [#413]'s own table is here, plus the constructs that must
    /// keep working.** The issue's finding was that three constructs decided
    /// one input two ways and were stored with a `201`; the refusals are what
    /// close that, and the accepted half is what stops the close from being a
    /// blanket refusal of anything containing a backslash or a bracket.
    ///
    /// **Seen red, 2026-09-13**: `divergent_construct` returning `None` always
    /// reddens this on `caf\b`, the first refused row.
    ///
    /// [#413]: https://github.com/sujanto-gaws/kelir/issues/413
    #[test]
    fn the_dialect_refuses_what_it_says_it_refuses_and_no_more() {
        let refused = [
            // The three [#413] found still divergent, in its own spellings.
            r"caf\b",
            r"^[[:digit:]]+$",
            r"^\p{Nd}+$",
            // The half Sprint 16 closed, which must stay closed.
            r"^\d{4}$",
            r"^\w+$",
            r"^\s+$",
            // The same three constructs in their other spellings.
            r"\B",
            r"^\P{L}+$",
            r"[[:^alpha:]]",
            // A class inside a bracket expression still diverges.
            r"^[\d]+$",
        ];

        for pattern in refused {
            assert!(
                matches!(
                    refuse_pattern(pattern, ""),
                    Some(PatternRefusal::Divergent(_))
                ),
                "`{pattern}` should be refused as divergent"
            );
        }

        let accepted = [
            // The remedies each message names, which must not themselves be
            // refused — a message telling an author to write `[0-9]` is only
            // worth reading if `[0-9]` is accepted.
            r"^[0-9]{4}$",
            r"^[A-Za-z0-9_]+$",
            r"^[ \t\r\n]+$",
            r"(^|[^A-Za-z0-9_])caf",
            // An escaped backslash is not an escape.
            r"^\\d$",
            // A bracket expression that merely contains a colon.
            r"^[a:b]+$",
        ];

        for pattern in accepted {
            assert_eq!(
                refuse_pattern(pattern, ""),
                None,
                "`{pattern}` should be accepted"
            );
        }
    }

    /// **The `\s` message names two code points, so this sends them.**
    ///
    /// [Coding standard] §2.9: a comment stating what something returns for a
    /// named input has a test that sends that input. The refusal for `\s` says
    /// the two sides differ at exactly **U+0085 NEXT LINE**, which only this
    /// crate matches, and **U+FEFF BYTE ORDER MARK**, which only the browser
    /// does. **This pins the half that runs here.** The browser half is
    /// [#413](https://github.com/sujanto-gaws/kelir/issues/413)'s own measured
    /// table and cannot be run from Rust; what this stops is the Rust claim
    /// going stale against a crate upgrade while the message still asserts it.
    ///
    /// **Seen red, 2026-09-13**: swapping the two expectations reddens both.
    ///
    /// [Coding standard]: ../../../../../docs/standards/01.%20Coding%20Standard.md
    #[test]
    fn this_crates_whitespace_class_is_what_the_refusal_says_it_is() {
        let whitespace = compile_pattern(r"^\s$", "").expect("`\\s` builds");

        assert!(
            whitespace.is_match("\u{0085}"),
            "U+0085 is whitespace to this crate, which is why the message says only this side matches it"
        );
        assert!(
            !whitespace.is_match("\u{FEFF}"),
            "U+FEFF is not whitespace to this crate, which is why the message says only the browser matches it"
        );

        // The uncontroversial members, so the test is about the two edges
        // rather than about `\s` working at all.
        assert!(whitespace.is_match(" "));
        assert!(whitespace.is_match("\t"));
    }

    /// **`\b` inside a bracket expression is a backspace on both sides.**
    ///
    /// It is the one place the two engines agree about a `\b`, so refusing it
    /// would refuse a portable pattern — which is the boundary
    /// [`divergent_construct`] states and this is the test that holds it.
    ///
    /// **Seen red, 2026-09-13**: dropping the `if !in_class` guard on the
    /// `'b' | 'B'` arm refuses `[\b]` and reddens this.
    #[test]
    fn a_backspace_escape_is_not_a_word_boundary() {
        // Whether the crate compiles `[\b]` at all is its business; what this
        // pins is that the scan does not call it a boundary.
        assert!(!matches!(
            divergent_construct(r"[\b]"),
            Some(DivergentConstruct::WordBoundary(_))
        ));

        // Outside a class it is the boundary, in both spellings.
        assert_eq!(
            divergent_construct(r"caf\b"),
            Some(DivergentConstruct::WordBoundary('b'))
        );
        assert_eq!(
            divergent_construct(r"\Bcaf"),
            Some(DivergentConstruct::WordBoundary('B'))
        );
    }

    /// **The first divergence wins, and the compile check wins over all of
    /// them.** A pattern that does not build cannot be scanned for anything
    /// else, and reporting a class problem beside a syntax error would be
    /// guessing at text the parser rejected.
    ///
    /// **Seen red, 2026-09-13**: moving the `unpinned`/divergence scan above
    /// the compile check in [`refuse_pattern`] reddens the third assertion.
    #[test]
    fn the_compile_check_answers_before_the_dialect_scan() {
        assert_eq!(
            divergent_construct(r"\d and \p{L}"),
            Some(DivergentConstruct::Class('d')),
            "the earlier construct is the one reported"
        );

        assert_eq!(
            refuse_pattern(r"^[0-9]+$", ""),
            None,
            "a portable pattern earns no refusal"
        );

        assert!(
            matches!(
                refuse_pattern(r"(?=x)\d", ""),
                Some(PatternRefusal::Uncompilable { .. })
            ),
            "a pattern that cannot be built is reported as that, not as a class"
        );
    }

    /// **`[[:` does not always open a POSIX class**, and the scan says so on
    /// its own rather than relying on the compile check to cover for it.
    ///
    /// `^[[:]+$` is a bracket expression holding `[` and `:`. The `regex` crate
    /// happens to refuse it — *unclosed character class*, because it reads
    /// `[[:` as the start of a POSIX name — so [`refuse_pattern`] never reaches
    /// the scan for this input. **That makes the crate's behaviour the thing
    /// being relied on**, and relying on it silently is how a scan acquires a
    /// bug nobody can see. Asserted directly instead.
    ///
    /// **Seen red, 2026-09-13**: `posix_class_name` returning `Some(name)`
    /// instead of `None` when its loop runs out reddens the `[[:dig` assertion.
    /// **It came back green on the first run and that was the finding** — the
    /// branch was reachable only through a pattern the compile check refuses
    /// first, so the two unterminated cases below were added to reach it.
    #[test]
    fn a_bracket_and_a_colon_are_not_a_posix_class() {
        assert_eq!(divergent_construct(r"^[[:]+$"), None);
        assert_eq!(divergent_construct(r"^[a:b]+$"), None);

        // **A name that never reaches `:]` is not a class either**, and this
        // assertion exists because a mutation found it uncovered: the `regex`
        // crate refuses `[[:dig` outright, so [`refuse_pattern`]'s compile check
        // answered first and nothing ever reached the end of the scan's loop.
        // A gate absorbing a branch is the usual cause of a green mutation
        // ([coding standard] §2.9), and the fix is a fixture that reaches it.
        assert_eq!(divergent_construct(r"[[:dig"), None);
        assert_eq!(divergent_construct(r"[[:"), None);

        // The real thing, in both spellings, is still caught.
        assert_eq!(
            divergent_construct(r"^[[:digit:]]+$"),
            Some(DivergentConstruct::PosixClass("digit".to_owned()))
        );
        assert_eq!(
            divergent_construct(r"[[:^alpha:]]"),
            Some(DivergentConstruct::PosixClass("^alpha".to_owned()))
        );
    }

    fn field(validation: Value) -> Value {
        json!({
            "id": "f", "role": "data", "type": "textfield", "key": "f", "label": "F",
            "validation": validation,
        })
    }

    fn verdict(component: &Value, value: Value) -> Option<String> {
        validate_field(component, &value, &Map::new())
            .violation
            .map(|violation| violation.rule)
    }

    #[test]
    fn an_empty_required_field_is_refused_and_an_empty_optional_one_is_not() {
        let required = field(json!({"type": "string", "required": true}));
        let optional = field(json!({"type": "string"}));

        assert_eq!(verdict(&required, json!("")), Some("required".to_owned()));
        assert_eq!(verdict(&required, json!(null)), Some("required".to_owned()));
        assert_eq!(verdict(&optional, json!(null)), None);
    }

    /// The parity-preserving reading rather than the convenient one: the
    /// browser sees `false` as present too, and a side that called it empty
    /// would disagree with the other about whether a form may be submitted.
    #[test]
    fn a_false_checkbox_is_present_rather_than_empty() {
        let component = field(json!({"type": "boolean", "required": true}));

        assert_eq!(verdict(&component, json!(false)), None);
    }

    #[test]
    fn a_whole_number_a_browser_submitted_is_still_an_integer() {
        let component = field(json!({"type": "integer"}));

        assert_eq!(verdict(&component, json!(2.0)), None);
        assert_eq!(verdict(&component, json!(2)), None);
        assert_eq!(verdict(&component, json!(2.5)), Some("type".to_owned()));
    }

    #[test]
    fn every_keyword_after_required_is_skipped_for_an_empty_value() {
        let component = field(json!({"type": "string", "format": "date"}));

        assert_eq!(verdict(&component, json!("")), None);
    }

    #[test]
    fn a_date_is_a_shape_and_a_real_day() {
        let component = field(json!({"type": "string", "format": "date"}));

        assert_eq!(verdict(&component, json!("2026-08-27")), None);
        assert_eq!(
            verdict(&component, json!("2026-02-31")),
            Some("format".to_owned())
        );
        assert_eq!(
            verdict(&component, json!("27-08-2026")),
            Some("format".to_owned())
        );
    }

    #[test]
    fn a_time_is_bounded_to_a_real_clock() {
        let component = field(json!({"type": "string", "format": "time"}));

        assert_eq!(verdict(&component, json!("09:30")), None);
        assert_eq!(verdict(&component, json!("09:30:15")), None);
        assert_eq!(
            verdict(&component, json!("25:99")),
            Some("format".to_owned())
        );
    }

    #[test]
    fn a_uri_needs_a_scheme_and_a_special_scheme_needs_a_host() {
        let component = field(json!({"type": "string", "format": "uri"}));

        assert_eq!(verdict(&component, json!("https://kelir.test/a")), None);
        assert_eq!(
            verdict(&component, json!("mailto:someone@kelir.test")),
            None
        );
        assert_eq!(
            verdict(&component, json!("http://")),
            Some("format".to_owned())
        );
        assert_eq!(
            verdict(&component, json!("kelir.test")),
            Some("format".to_owned())
        );
    }

    /// The browser counts UTF-16 code units, and so does this, so a
    /// `maxLength` admits and refuses the same strings on both sides.
    #[test]
    fn a_length_is_counted_the_way_the_browser_counts_it() {
        let component = field(json!({"type": "string", "maxLength": 2}));

        // One `char`, two UTF-16 code units — `"🙂".length` is 2.
        assert_eq!(verdict(&component, json!("🙂")), None);
        assert_eq!(
            verdict(&component, json!("🙂a")),
            Some("maxLength".to_owned())
        );
    }

    #[test]
    fn a_definition_message_replaces_the_default_one() {
        let component = field(json!({
            "type": "string",
            "required": true,
            "messages": {"required": "Every request needs a title."},
        }));

        let outcome = validate_field(&component, &json!(null), &Map::new());

        assert_eq!(
            outcome.violation.map(|violation| violation.message),
            Some("Every request needs a title.".to_owned())
        );
    }

    /// The registry's own §4 step 4, and the reason it is a rule: the reference
    /// implementation returns `true` here, which reports a check that never ran
    /// as a check that passed.
    #[test]
    fn a_rule_outside_the_registry_is_reported_rather_than_skipped() {
        let mut component = field(json!({"type": "string"}));
        component["rules"] = json!([{"rule": "looksNice", "scope": "both", "params": {}}]);

        let outcome = validate_field(&component, &json!("x"), &Map::new());

        assert_eq!(
            outcome.unknown,
            vec![UnknownRule {
                rule: "looksNice".to_owned()
            }]
        );
    }

    /// The `server` tier has nobody below it, so silence would be the same
    /// failure as the unknown arm with a better excuse.
    #[test]
    fn a_server_rule_this_side_does_not_implement_is_reported_rather_than_passed() {
        let mut component = field(json!({"type": "string"}));
        component["rules"] = json!([{"rule": "unique", "scope": "server", "params": {}}]);

        let outcome = validate_field(&component, &json!("x"), &Map::new());

        assert_eq!(outcome.unenforced.len(), 1, "got {outcome:?}");
        assert_eq!(outcome.unenforced[0].rule, "unique");
        assert!(
            outcome.violation.is_none(),
            "it is not the value that is wrong"
        );
    }

    /// §6.1's table, not a gap: *"Frontend Only. UX enhancements. Ignored by
    /// backend."*
    #[test]
    fn a_client_scoped_registry_rule_is_ignored_here() {
        let mut component = field(json!({"type": "string"}));
        component["rules"] =
            json!([{"rule": "passwordStrength", "scope": "both", "params": {"minScore": 3}}]);

        let outcome = validate_field(&component, &json!("x"), &Map::new());

        assert!(outcome.unenforced.is_empty(), "got {outcome:?}");
        assert!(outcome.unknown.is_empty(), "got {outcome:?}");
    }

    #[test]
    fn a_rule_the_definition_scopes_to_the_client_does_not_run_here() {
        let mut component = field(json!({"type": "string"}));
        component["rules"] = json!([{
            "rule": "regex", "scope": "client",
            "params": {"pattern": "^[A-Z]{2}-[0-9]{4}$"},
            "message": "A cost centre looks like FN-0142.",
        }]);

        assert_eq!(verdict(&component, json!("nope")), None);
    }

    #[test]
    fn the_registry_regex_rule_is_decided_here_when_the_definition_asks_for_both() {
        let mut component = field(json!({"type": "string"}));
        component["rules"] = json!([{
            "rule": "regex", "scope": "both",
            "params": {"pattern": "^[A-Z]{2}-[0-9]{4}$"},
            "message": "A cost centre looks like FN-0142.",
        }]);

        assert_eq!(verdict(&component, json!("FN-0142")), None);
        assert_eq!(
            verdict(&component, json!("fn-142")),
            Some("regex".to_owned())
        );
    }

    /// A rule that cannot be applied has not been satisfied — the browser's
    /// `catch` arm, and the safe direction for a pattern one engine compiles
    /// and the other refuses (**D-15**).
    #[test]
    fn a_pattern_this_engine_cannot_compile_is_a_violation_rather_than_a_pass() {
        let mut component = field(json!({"type": "string"}));
        component["rules"] = json!([{
            "rule": "regex", "scope": "both",
            "params": {"pattern": "(?=lookahead)"},
            "message": "no",
        }]);

        assert_eq!(
            verdict(&component, json!("lookahead")),
            Some("regex".to_owned())
        );
    }

    #[test]
    fn matches_field_compares_against_the_scope_the_rule_was_raised_in() {
        let mut component = field(json!({"type": "string"}));
        component["rules"] = json!([{
            "rule": "matchesField", "scope": "both",
            "params": {"target": "confirmation"},
            "message": "The two do not match.",
        }]);

        let mut scope = Map::new();
        scope.insert("confirmation".to_owned(), json!("hunter2"));

        assert!(validate_field(&component, &json!("hunter2"), &scope)
            .violation
            .is_none());
        assert_eq!(
            validate_field(&component, &json!("hunter3"), &scope)
                .violation
                .map(|violation| violation.rule),
            Some("matchesField".to_owned())
        );
    }

    #[test]
    fn rows_repeating_a_declared_key_are_refused_and_the_pair_is_what_is_unique() {
        let component = field(json!({"type": "array", "uniqueBy": ["sku", "warehouse"]}));

        assert_eq!(
            verdict(
                &component,
                json!([{"sku": "A", "warehouse": "W1"}, {"sku": "A", "warehouse": "W2"}]),
            ),
            None
        );
        assert_eq!(
            verdict(
                &component,
                json!([{"sku": "A", "warehouse": "W1"}, {"sku": "A", "warehouse": "W1"}]),
            ),
            Some("uniqueBy".to_owned())
        );
    }

    /// A number reaching a pattern is spelled the way the browser spells it.
    /// `serde_json` prints `42.0`, JavaScript prints `42`, and `^[0-9]+$` over
    /// a numeric field would hold on one side and fail on the other.
    #[test]
    fn a_whole_number_is_spelled_without_a_decimal_point() {
        assert_eq!(js_string(&json!(42.0)), "42");
        assert_eq!(js_string(&json!(42)), "42");
        assert_eq!(js_string(&json!(42.5)), "42.5");
        assert_eq!(js_string(&json!(true)), "true");
        assert_eq!(js_string(&json!([1, 2])), "1,2");
    }
}
