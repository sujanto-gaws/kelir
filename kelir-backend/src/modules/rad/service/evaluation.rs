//! Re-evaluating a submitted payload before anything is stored — the
//! Tamper-Proof Pattern (JFSS S8.1, S10.2; FR-RAD-010, FR-RAD-006, [#164]).
//!
//! **The client's arithmetic is not trusted, and this is where that is true.**
//! S8.1 requires the backend to re-evaluate every `calculate` expression and
//! *overwrite* the submitted value before persistence; S10.2 requires the same
//! for every `conditional`, discarding the values of components that resolve to
//! hidden. What comes out of [`secure_payload`] is therefore the server's own
//! answer, and a caller that stores it stores nothing the browser computed.
//!
//! The [operator-parity spike](../../../../../projects/spikes/01.%20JFSS%20Operator%20Parity.md)
//! measured what the absence of this looks like: the Calculation Rule Registry
//! §6.1 invoice persisting a grand total of **0 in place of 42**, with nothing
//! logged and nothing refused. That was a library defect; not re-evaluating at
//! all is the same outcome by design.
//!
//! **Pure — no database, no permission check, no `AppState`** (construction
//! plan §6.2). That separation is the deliverable rather than a preference:
//! Sprint 9's [#168](https://github.com/sujanto-gaws/kelir/issues/168) submits
//! *through* this re-evaluation inside a numbering transaction, and that
//! sentence is only true if the re-evaluation is callable without dragging a
//! submission row along with it. [`super::submission`] is what adds the
//! permission, the tenant scope and the write.
//!
//! # The order of operations
//!
//! 1. **Project the payload onto the definition.** Every declared `key` is
//!    present — a missing one resolves to `Value::Null`, which is JFSS S12.4 as
//!    errata **E-1** restates it for `serde_json` rather than a typed zero — and
//!    every *un*declared key is dropped.
//! 2. **Overwrite `sequenceKey`** with the row's 1-based position (§9.2). A
//!    line-item table that reads 1, 3, 4 after a deletion is what the property
//!    exists to prevent, and a client is not the thing that gets to decide it.
//! 3. **Re-evaluate every `calculate`** by *declared* mode (S4.2.3, S8.1.1),
//!    to a fixed point.
//! 4. **Re-evaluate every `conditional`** against the payload and discard the
//!    values of the components that resolve to hidden (S10.2).
//! 5. **Run `validation` and the `rules`** scoped `server` or `both`
//!    ([`super::super::domain::validation`]).
//!
//! **Steps 3 and 4 are in that order, and the construction plan §6.3 numbers
//! them the other way round.** JFSS §9.2 lists *Calculation Overwrite* before
//! *Conditional Stripping*, and where the plan and the specification disagree
//! the specification is the authority the plan itself names. It also matters:
//! a `conditional` keyed on a computed field — `{"var": "grand_total"}` — would
//! otherwise be decided from the number the *client* sent, so tampering with a
//! total would move which fields get persisted. Recorded as decision **D-27**.
//!
//! **Every failure is collected rather than the first**, for the reason
//! `validate_definition` gives about a form definition: a person who has more
//! than one problem should be told about all of them rather than discovering
//! them one round trip at a time.
//!
//! [#164]: https://github.com/sujanto-gaws/kelir/issues/164

use serde_json::{Map, Value};

use super::super::domain::jfss::{container_children, data_key, role_of, row_template};
use super::super::domain::validation::validate_field;
use super::super::evaluator::{normalize_numeric, RuleEvaluator};
use crate::error::ValidationDetail;

/// The rule the S10.3 envelope names when an expression produced no value.
const EVALUATION_FAILED: &str = "EVALUATION_FAILED";

/// A definition, a submitted payload, and the payload that may be stored.
///
/// `Err` carries the S10.3 envelope's `details`: one entry per problem, each
/// naming the dot-notation `path` of the field it is about. The caller turns
/// that into the response — it is [`crate::error::AppError::validation`], whose
/// `ValidationDetail` is S10.3's shape and nothing else.
pub fn secure_payload(
    definition: &Value,
    submitted: &Value,
) -> Result<Value, Vec<ValidationDetail>> {
    secure_payload_with(&RuleEvaluator::new(), definition, submitted)
}

/// The same, over an evaluator the caller already holds.
///
/// The engine carries the operator table and the configuration, both read-only
/// after construction, so a caller submitting in a loop — or Sprint 9's
/// numbering transaction, which does other work around this — builds one rather
/// than one per call.
pub fn secure_payload_with(
    evaluator: &RuleEvaluator,
    definition: &Value,
    submitted: &Value,
) -> Result<Value, Vec<ValidationDetail>> {
    let Some(submitted) = submitted.as_object() else {
        return Err(vec![ValidationDetail::new(
            "payload",
            "type",
            "PAYLOAD_NOT_AN_OBJECT",
            "a submission is a JSON object keyed by the form's data keys (JFSS S10.1)",
        )]);
    };

    let no_components = Vec::new();
    let components = refs(
        definition
            .get("components")
            .and_then(Value::as_array)
            .unwrap_or(&no_components),
    );

    let mut pass = Evaluation {
        evaluator,
        failures: Vec::new(),
    };

    // 1 and 2.
    let mut payload = pass.project(&components, submitted);
    pass.apply_sequence(&components, &mut payload);

    // 3.
    let calculation_failures = pass.calculate_to_fixed_point(&components, &mut payload);
    pass.failures.extend(calculation_failures);

    // 4. Visibility is decided once, against the payload as it stands after the
    // calculations and before anything is discarded — which is S10.2's
    // "complete submitted payload" with S8.1's overwrite already applied. It is
    // decided once rather than twice because a second evaluation would run
    // against a payload the first one had already taken values out of, and a
    // conditional whose own input was discarded would then answer differently
    // from the answer that discarded it.
    let mut visible = Vec::new();
    let mut hidden = Vec::new();
    // S10.2's "complete submitted payload", snapshotted before anything is
    // taken out of it.
    let complete = Value::Object(payload.clone());

    pass.resolve_visibility(&components, &complete, "", &mut visible, &mut hidden);

    for (scope_path, key) in hidden {
        if let Some(scope) = scope_at_mut(&mut payload, &scope_path) {
            scope.remove(&key);
        }
    }

    // 5. Against the payload the hidden values have *already* been taken out
    // of: §9.2 requires a hidden component be treated "as absent for
    // validation", so a `matchesField` targeting one compares against absent.
    // The browser has no stripping step and compares against the live value, so
    // this is a stated divergence — and the specification's own wording is what
    // settles which side is right.
    let validation_failures = pass.validate_all(&visible, &payload);
    pass.failures.extend(validation_failures);

    if pass.failures.is_empty() {
        Ok(Value::Object(payload))
    } else {
        Err(pass.failures)
    }
}

/// One re-evaluation, and the problems it has found so far.
struct Evaluation<'a> {
    evaluator: &'a RuleEvaluator,
    failures: Vec<ValidationDetail>,
}

/// A data component that survived its `conditional`, and where its value lives.
struct VisibleField<'a> {
    component: &'a Value,
    /// The record its `key` addresses — `""` for the payload, `line_items.0`
    /// for the first row of a repeater.
    scope_path: String,
    key: String,
}

impl VisibleField<'_> {
    /// The S10.3 dot-notation path of this field's own value.
    fn path(&self) -> String {
        if self.scope_path.is_empty() {
            self.key.clone()
        } else {
            format!("{}.{}", self.scope_path, self.key)
        }
    }
}

impl Evaluation<'_> {
    // -- 1. Projection ----------------------------------------------------

    /// The submitted payload, projected onto the keys the definition declares.
    ///
    /// **Two things happen here and both are S10.1's.** A declared key the
    /// client omitted becomes `Value::Null` — S12.4 as errata **E-1** restates
    /// it for `serde_json`, and the Calculation Rule Registry §7.3
    /// normalization is what later turns it into a number. An *un*declared key
    /// is dropped rather than refused: a definition revised between the render
    /// and the submit is the same routine race S10.1.1 refuses to treat as an
    /// attack, and storing a key no component collects would put data in a row
    /// that nothing can ever render.
    fn project(
        &mut self,
        components: &[&Value],
        submitted: &Map<String, Value>,
    ) -> Map<String, Value> {
        let mut scope = Map::new();

        self.project_into(components, submitted, &mut scope, "");
        scope
    }

    fn project_into(
        &mut self,
        components: &[&Value],
        submitted: &Map<String, Value>,
        scope: &mut Map<String, Value>,
        scope_path: &str,
    ) {
        for component in components {
            if role_of(component) == Some("layout") {
                self.project_into(&container_children(component), submitted, scope, scope_path);
                continue;
            }

            let Some(key) = data_key(component) else {
                continue;
            };

            let submitted_value = submitted.get(key).cloned().unwrap_or(Value::Null);

            let projected = match (row_template(component), &submitted_value) {
                (Some(template), Value::Array(rows)) => {
                    let path_of = |index: usize| join_path(scope_path, &format!("{key}.{index}"));
                    let mut projected_rows = Vec::with_capacity(rows.len());

                    for (index, row) in rows.iter().enumerate() {
                        let Some(row) = row.as_object() else {
                            // A row that is not an object cannot be addressed
                            // by the template's keys at all, so nothing below
                            // could validate it. Refused rather than stored:
                            // this is not a shape any renderer produces.
                            self.failures.push(ValidationDetail::new(
                                path_of(index),
                                "type",
                                "ROW_NOT_AN_OBJECT",
                                "a repeater's rows are objects keyed by its row template (JFSS §4.3.1)",
                            ));
                            continue;
                        };

                        let mut projected_row = Map::new();

                        self.project_into(
                            &refs(template),
                            row,
                            &mut projected_row,
                            &path_of(index),
                        );
                        projected_rows.push(Value::Object(projected_row));
                    }

                    Value::Array(projected_rows)
                }
                _ => submitted_value,
            };

            scope.insert(key.to_owned(), projected);
        }
    }

    // -- 2. Sequencing ----------------------------------------------------

    /// `sequenceKey`: the 1-based row index, written into a child `key` (§4.2).
    ///
    /// **Overwritten rather than checked** — §9.2 says *"Overwrite `sequenceKey`
    /// values to guarantee sequential integrity"*, and §10.2 puts sequencing
    /// beside calculation under *"Backend overwrites to prevent tampering"*.
    /// The browser recomputes it on every write for the same reason: removing
    /// row 2 of four has to renumber the two below it.
    ///
    /// It runs before the calculations, because an expression may read the
    /// number — a per-row surcharge that depends on line position is exactly
    /// the shape `sequenceKey` exists for.
    fn apply_sequence(&mut self, components: &[&Value], scope: &mut Map<String, Value>) {
        for component in components {
            if role_of(component) == Some("layout") {
                self.apply_sequence(&container_children(component), scope);
                continue;
            }

            let (Some(key), Some(template)) = (data_key(component), row_template(component)) else {
                continue;
            };
            let sequence_key = component
                .get("sequenceKey")
                .and_then(Value::as_str)
                .map(str::to_owned);

            let Some(Value::Array(rows)) = scope.get_mut(key) else {
                continue;
            };

            for (index, row) in rows.iter_mut().enumerate() {
                let Some(row) = row.as_object_mut() else {
                    continue;
                };

                if let Some(sequence_key) = &sequence_key {
                    row.insert(sequence_key.clone(), Value::from(index + 1));
                }

                // A datagrid inside a row is a shape JFSS permits, and its own
                // rows need numbering too.
                self.apply_sequence(&refs(template), row);
            }
        }
    }

    // -- 3. Calculation ---------------------------------------------------

    /// Every `calculate` the definition carries, run until the payload settles.
    ///
    /// **A fixed point rather than a topological order, and the difference is
    /// the graph decision D-2 reserves.** §9.2 asks for topological order so
    /// that a chain (`c` depends on `b` depends on `a`) converges in one pass;
    /// building the graph is the rule engine's, in Sprints 14–16, and half a
    /// graph here would be exactly the erosion the construction plan §1 names.
    /// Repeating a definition-order pass until nothing moves reaches the same
    /// answer for any acyclic definition, and it is what the browser does — its
    /// calculation watcher re-runs on every value change, including the changes
    /// the calculations themselves make.
    ///
    /// **The bound is what makes a cyclic definition a refusal rather than a
    /// hang.** A chain of `n` calculated fields declared in the worst possible
    /// order settles after `n` passes and the `n + 1`-th confirms it, so a
    /// definition that has still not settled by then has a cycle — which S12.2
    /// makes invalid and which nothing rejects at save yet, because the
    /// detector is the same graph D-2 reserved. Refusing beats persisting a
    /// half-converged payload.
    fn calculate_to_fixed_point(
        &mut self,
        components: &[&Value],
        payload: &mut Map<String, Value>,
    ) -> Vec<ValidationDetail> {
        let bound = count_calculated(components) + 1;

        for _ in 0..bound {
            let mut failures = Vec::new();
            let mut changed = false;

            self.calculate_pass(components, payload, "", &mut failures, &mut changed);

            if !changed {
                return failures;
            }
        }

        vec![ValidationDetail::new(
            "definition",
            "cycle",
            "CALCULATION_DID_NOT_SETTLE",
            format!(
                "the `calculate` expressions did not reach a stable answer in {bound} passes, \
                 which means they depend on one another in a cycle — JFSS S12.2 makes such a \
                 definition invalid"
            ),
        )]
    }

    /// One definition-order pass over a scope, and the rows beneath it.
    ///
    /// The scope is cloned before each evaluation rather than once per pass, so
    /// a field written earlier in the pass is visible to the next expression —
    /// which is what lets a chain declared in order settle in a single pass, and
    /// is exactly what the browser's in-place mutation gives it for free. The
    /// cost is one clone of the scope per calculated field; a definition is
    /// capped at a megabyte and the largest example in the JFSS documents is
    /// under 8 KB.
    fn calculate_pass(
        &mut self,
        components: &[&Value],
        scope: &mut Map<String, Value>,
        scope_path: &str,
        failures: &mut Vec<ValidationDetail>,
        changed: &mut bool,
    ) {
        for component in components {
            if role_of(component) == Some("layout") {
                self.calculate_pass(
                    &container_children(component),
                    scope,
                    scope_path,
                    failures,
                    changed,
                );
                continue;
            }

            let Some(key) = data_key(component) else {
                continue;
            };

            if let Some(expression) = component.get("calculate") {
                // S4.2.3, branched on the **declared** mode and never on
                // whether the operators look deterministic — S8.1.1 forbids
                // inferring it, because that would require every language to
                // maintain an identical operator classification and the
                // Calculation Rule Registry does not carry one. A missing mode
                // is `derived`, which the specification states rather than
                // leaves to an implementation's default.
                let derived =
                    component.get("calculateMode").and_then(Value::as_str) != Some("generated");
                let unresolved = scope.get(key).is_none_or(Value::is_null);

                // Case C's guard comes before the evaluation rather than after
                // it: a resolved generated value is never recomputed, and
                // evaluating one only to throw the answer away would make
                // "exactly once" a statement about the write rather than about
                // the evaluation.
                if derived || unresolved {
                    let path = join_path(scope_path, key);
                    let data = Value::Object(scope.clone());

                    let computed = match self.evaluator.evaluate(expression, &data) {
                        Ok(value) => Some(value),
                        Err(error) => {
                            // **This is where a division by zero lands**
                            // (decision **D-24**). The browser renders the
                            // field blank and does not block typing, because on
                            // a zero-filled payload an average fails before the
                            // first keystroke; the submission is where the same
                            // failure is refused, with the field named.
                            //
                            // The engine's own words are logged and not
                            // returned: they name operators and argument
                            // shapes, which is a description of the definition
                            // rather than of what anybody typed.
                            tracing::info!(
                                field = %path,
                                error = %error.message(),
                                "a calculate expression produced no value",
                            );

                            failures.push(ValidationDetail::new(
                                path,
                                "calculate",
                                EVALUATION_FAILED,
                                "this field is computed from the others and its expression \
                                 produced no value — a division by zero does not produce one \
                                 (Calculation Rule Registry §3.1)",
                            ));

                            None
                        }
                    };

                    let next = if derived {
                        computed
                            .map(|value| coerce(component, value))
                            .unwrap_or(Value::Null)
                    } else {
                        // Case C's priority 2 then 3. `null` is "the operator
                        // yielded null", which the table answers with
                        // `defaultValue` — so the check is before the numeric
                        // wrapper rather than after it, where §7.3 would
                        // already have turned the null into a 0 nobody asked
                        // for.
                        match computed {
                            Some(Value::Null) | None => component
                                .get("defaultValue")
                                .cloned()
                                .unwrap_or(Value::Null),
                            Some(value) => coerce(component, value),
                        }
                    };

                    if scope.get(key) != Some(&next) {
                        scope.insert(key.to_owned(), next);
                        *changed = true;
                    }
                }
            }

            // A repeater: the same walk over the template, once per row,
            // against that row — because a template's `key`s address properties
            // of the row (§4.3.1) and an expression written in one means its own
            // row's siblings.
            let Some(template) = row_template(component) else {
                continue;
            };
            let Some(Value::Array(rows)) = scope.get_mut(key) else {
                continue;
            };

            for (index, row) in rows.iter_mut().enumerate() {
                let row_path = join_path(scope_path, &format!("{key}.{index}"));

                if let Some(row) = row.as_object_mut() {
                    self.calculate_pass(&refs(template), row, &row_path, failures, changed);
                }
            }
        }
    }

    // -- 4. Conditionals --------------------------------------------------

    /// Which data components survive their `conditional`, and which do not.
    ///
    /// A hidden **container** hides everything under it, which is why this
    /// collects rather than filters in place: the keys to discard are spread
    /// across a subtree the walk stops descending into.
    fn resolve_visibility<'d>(
        &mut self,
        components: &[&'d Value],
        scope_value: &Value,
        scope_path: &str,
        visible: &mut Vec<VisibleField<'d>>,
        hidden: &mut Vec<(String, String)>,
    ) {
        for component in components {
            if !self.is_visible(component, scope_value, scope_path) {
                collect_keys_under(component, scope_path, hidden);
                continue;
            }

            if role_of(component) == Some("layout") {
                self.resolve_visibility(
                    &container_children(component),
                    scope_value,
                    scope_path,
                    visible,
                    hidden,
                );
                continue;
            }

            let Some(key) = data_key(component) else {
                continue;
            };

            visible.push(VisibleField {
                component,
                scope_path: scope_path.to_owned(),
                key: key.to_owned(),
            });

            let (Some(template), Some(Value::Array(rows))) =
                (row_template(component), scope_value.get(key))
            else {
                continue;
            };

            for (index, row) in rows.iter().enumerate() {
                if row.is_object() {
                    let row_path = join_path(scope_path, &format!("{key}.{index}"));

                    self.resolve_visibility(&refs(template), row, &row_path, visible, hidden);
                }
            }
        }
    }

    /// JFSS §7: whether a `conditional` leaves this component rendered.
    ///
    /// `enable` and `disable` decide whether a control accepts input, which is
    /// a statement about a browser and not about a payload — a disabled field
    /// still holds and still submits its value.
    ///
    /// **A conditional that cannot be evaluated is a refusal**, not a default.
    /// The browser leaves such a component alone, because on a payload whose
    /// arithmetic has not settled the alternative is a form that hides fields it
    /// will show a moment later. This side has no next moment: persisting a
    /// value the client hid would leak it, and discarding one the client showed
    /// would lose it, so the submission is refused and the component is treated
    /// as visible meanwhile — which keeps the rest of the report consistent
    /// with what the person filling in the form was looking at.
    fn is_visible(&mut self, component: &Value, scope_value: &Value, scope_path: &str) -> bool {
        let Some(conditional) = component.get("conditional") else {
            return true;
        };

        let action = conditional.get("action").and_then(Value::as_str);

        if !matches!(action, Some("show") | Some("hide")) {
            return true;
        }

        let Some(logic) = conditional.get("logic") else {
            return true;
        };

        let holds = match self.evaluator.evaluate(logic, scope_value) {
            Ok(result) => result == Value::Bool(true),
            Err(error) => {
                let path = join_path(
                    scope_path,
                    data_key(component)
                        .or_else(|| component.get("id").and_then(Value::as_str))
                        .unwrap_or("component"),
                );

                tracing::info!(
                    component = %path,
                    error = %error.message(),
                    "a conditional expression produced no value",
                );

                self.failures.push(ValidationDetail::new(
                    path,
                    "conditional",
                    EVALUATION_FAILED,
                    "whether this component applies is decided by an expression, and that \
                     expression produced no value — so the server cannot tell whether its \
                     value belongs in the stored form (JFSS S10.2)",
                ));

                true
            }
        };

        if action == Some("show") {
            holds
        } else {
            !holds
        }
    }

    // -- 5. Validation ----------------------------------------------------

    /// `validation` and the `rules` scoped `server` or `both`, over the fields
    /// that survived step 4.
    fn validate_all(
        &mut self,
        visible: &[VisibleField<'_>],
        payload: &Map<String, Value>,
    ) -> Vec<ValidationDetail> {
        let mut failures = Vec::new();
        let empty_scope = Map::new();

        for field in visible {
            let path = field.path();
            let scope = scope_at(payload, &field.scope_path).unwrap_or(&empty_scope);
            let value = scope.get(&field.key).cloned().unwrap_or(Value::Null);
            let outcome = validate_field(field.component, &value, scope);

            if let Some(violation) = outcome.violation {
                failures.push(ValidationDetail::new(
                    path.clone(),
                    violation.rule,
                    "VALIDATION_FAILED",
                    violation.message,
                ));
            }

            // A rule name nobody defines is a defect in the *definition*, and
            // S8.1.1 makes an unknown one a refusal rather than a skipped arm.
            // The message says whose problem it is: nothing the person filling
            // in the form typed can make it go away.
            for unknown in outcome.unknown {
                failures.push(ValidationDetail::new(
                    path.clone(),
                    unknown.rule.clone(),
                    "RULE_NOT_REGISTERED",
                    format!(
                        "`{}` is not a rule in the JFSS Validation Rule Registry, so this \
                         field cannot be checked — adding one is registry §4, not a branch \
                         in a validator",
                        unknown.rule
                    ),
                ));
            }

            // A rule the registry defines, scopes to this side, and Kelir does
            // not decide. Refused rather than passed: this side is the last
            // one, and a check reported as run because nobody wrote it is the
            // failure the whole unknown-operator argument is about.
            for unenforced in outcome.unenforced {
                failures.push(ValidationDetail::new(
                    path.clone(),
                    unenforced.rule.clone(),
                    "RULE_NOT_ENFORCED",
                    format!(
                        "`{}` is a server-side rule this build does not enforce, so this form \
                         cannot be submitted — {}",
                        unenforced.rule, unenforced.reason
                    ),
                ));
            }
        }

        failures
    }
}

/// A computed result as the field's declared type wants it.
///
/// Numeric fields go through the Calculation Rule Registry §7.3 normalization
/// and every other type takes the raw result: the wrapper is about arithmetic,
/// and putting a boolean or a string through it would turn every computed label
/// into `0`. The browser's `coerce` makes the same split on the same keyword.
fn coerce(component: &Value, result: Value) -> Value {
    let declared = component
        .get("validation")
        .and_then(|validation| validation.get("type"))
        .and_then(Value::as_str);

    if matches!(declared, Some("number") | Some("integer")) {
        Value::from(normalize_numeric(&result))
    } else {
        result
    }
}

/// How many `calculate` expressions the definition carries, rows aside.
///
/// The bound on the fixed-point loop. A repeater's template is counted once
/// rather than once per row, which is correct for the loop's purpose: rows do
/// not depend on one another, so adding rows adds width and not depth.
fn count_calculated(components: &[&Value]) -> usize {
    components
        .iter()
        .map(|component| {
            let here = usize::from(
                role_of(component) == Some("data") && component.get("calculate").is_some(),
            );
            let children = count_calculated(&container_children(component));
            let rows = row_template(component)
                .map(|template| count_calculated(&refs(template)))
                .unwrap_or(0);

            here + children + rows
        })
        .sum()
}

/// An owned component array as a slice of references.
///
/// One shape for both, so a single walk serves a definition's own `components`
/// and the borrowed children [`container_children`] hands back — the closed set
/// of §4.3.1 shapes stays in one place, and none of the four walks below can
/// acquire its own idea of what a child is.
fn refs(components: &[Value]) -> Vec<&Value> {
    components.iter().collect()
}

/// `line_items.0` and `quantity` become `line_items.0.quantity`; an empty
/// prefix leaves the segment alone.
fn join_path(prefix: &str, segment: &str) -> String {
    if prefix.is_empty() {
        segment.to_owned()
    } else {
        format!("{prefix}.{segment}")
    }
}

/// The record a scope path names, inside a payload.
///
/// Walked rather than resolved through `Value::pointer`, because that would
/// need an owned `Value` to borrow out of and this borrows the payload itself.
fn scope_at<'a>(
    payload: &'a Map<String, Value>,
    scope_path: &str,
) -> Option<&'a Map<String, Value>> {
    if scope_path.is_empty() {
        return Some(payload);
    }

    let mut segments = scope_path.split('.');
    let mut current: &Value = payload.get(segments.next()?)?;

    for segment in segments {
        current = match current {
            Value::Array(items) => items.get(segment.parse::<usize>().ok()?)?,
            Value::Object(map) => map.get(segment)?,
            _ => return None,
        };
    }

    current.as_object()
}

/// The same, mutably, for discarding a hidden component's value.
fn scope_at_mut<'a>(
    payload: &'a mut Map<String, Value>,
    scope_path: &str,
) -> Option<&'a mut Map<String, Value>> {
    if scope_path.is_empty() {
        return Some(payload);
    }

    let mut segments = scope_path.split('.');
    let mut current: &mut Value = payload.get_mut(segments.next()?)?;

    for segment in segments {
        current = match current {
            Value::Array(items) => items.get_mut(segment.parse::<usize>().ok()?)?,
            Value::Object(map) => map.get_mut(segment)?,
            _ => return None,
        };
    }

    current.as_object_mut()
}

/// Every `(scope path, key)` a subtree would have written, for discarding it.
///
/// A repeater contributes only its own key: removing that removes the rows and
/// everything in them.
fn collect_keys_under(component: &Value, scope_path: &str, found: &mut Vec<(String, String)>) {
    if let Some(key) = data_key(component) {
        found.push((scope_path.to_owned(), key.to_owned()));
        return;
    }

    for child in container_children(component) {
        collect_keys_under(child, scope_path, found);
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// The Calculation Rule Registry §6.1 invoice, which is the pattern the
    /// whole Tamper-Proof argument is built on — a repeater with a per-row
    /// `derived` total and a grand total over `sum` and `map`.
    fn invoice() -> Value {
        json!({
            "formId": "invoice",
            "version": "2.0.1",
            "components": [
                {
                    "id": "lines", "role": "data", "type": "datagrid",
                    "key": "line_items", "label": "Lines",
                    "sequenceKey": "line_no",
                    "validation": {"type": "array"},
                    "components": [
                        {"id": "no", "role": "data", "type": "number", "key": "line_no",
                         "label": "Line", "readOnly": true, "validation": {"type": "integer"}},
                        {"id": "q", "role": "data", "type": "number", "key": "quantity",
                         "label": "Quantity", "validation": {"type": "integer"}},
                        {"id": "p", "role": "data", "type": "number", "key": "unit_price",
                         "label": "Unit price", "validation": {"type": "number"}},
                        {"id": "lt", "role": "data", "type": "number", "key": "line_total",
                         "label": "Line total", "validation": {"type": "number"},
                         "calculate": {"*": [{"var": "unit_price"}, {"var": "quantity"}]}}
                    ]
                },
                {
                    "id": "total", "role": "data", "type": "number",
                    "key": "grand_total", "label": "Grand total",
                    "validation": {"type": "number"},
                    "calculate": {"sum": [{"map": [
                        {"var": "line_items"},
                        {"*": [{"var": "unit_price"}, {"var": "quantity"}]}
                    ]}]}
                }
            ]
        })
    }

    fn two_lines_worth_42() -> Value {
        json!({
            "line_items": [
                {"quantity": 2, "unit_price": 10},
                {"quantity": 2, "unit_price": 11}
            ]
        })
    }

    fn secure(definition: &Value, submitted: &Value) -> Value {
        secure_payload(definition, submitted).expect("the payload is accepted")
    }

    fn refusal(definition: &Value, submitted: &Value) -> Vec<ValidationDetail> {
        secure_payload(definition, submitted).expect_err("the payload is refused")
    }

    /// **The security control this whole module exists for** (#164 AC1, AC3).
    ///
    /// The mutation that must make it red is removing the `calculate` branch in
    /// [`Evaluation::calculate_pass`] — the arm that writes the computed value
    /// over the submitted one.
    #[test]
    fn a_tampered_total_is_replaced_by_the_one_the_rules_produce() {
        let mut submitted = two_lines_worth_42();
        submitted["grand_total"] = json!(0);
        submitted["line_items"][0]["line_total"] = json!(0);

        let payload = secure(&invoice(), &submitted);

        assert_eq!(payload["grand_total"], json!(42.0));
        assert_eq!(payload["line_items"][0]["line_total"], json!(20.0));
        assert_eq!(payload["line_items"][1]["line_total"], json!(22.0));
    }

    /// The spike's own defect, stated as a property of this module rather than
    /// of the evaluator: a 42-rupiah invoice does not become free.
    #[test]
    fn an_honest_client_gets_the_same_answer_it_computed() {
        let mut submitted = two_lines_worth_42();
        submitted["grand_total"] = json!(42);

        assert_eq!(secure(&invoice(), &submitted)["grand_total"], json!(42.0));
    }

    /// JFSS §9.2: *"Overwrite `sequenceKey` values to guarantee sequential
    /// integrity"*. A client that renumbers its own rows is a client deciding
    /// what row three is called.
    #[test]
    fn row_numbers_are_the_servers_and_not_the_clients() {
        let mut submitted = two_lines_worth_42();
        submitted["line_items"][0]["line_no"] = json!(7);
        submitted["line_items"][1]["line_no"] = json!(7);

        let payload = secure(&invoice(), &submitted);

        assert_eq!(payload["line_items"][0]["line_no"], json!(1));
        assert_eq!(payload["line_items"][1]["line_no"], json!(2));
    }

    /// S12.4 as errata **E-1** restates it: a missing key resolves to
    /// `Value::Null`, and §7.3's normalization is what turns it into a number.
    #[test]
    fn a_key_the_client_omitted_is_present_and_null() {
        let payload = secure(&invoice(), &json!({}));

        assert_eq!(payload["line_items"], json!(null));
        // The grand total over an absent array: `sum` of nothing is 0, which is
        // §8.2's null-safety rule and not an error.
        assert_eq!(payload["grand_total"], json!(0.0));
    }

    /// A key no component collects is data nothing could ever render. Dropped
    /// rather than refused, for the reason S10.1.1 gives about hidden keys: a
    /// definition revised between the render and the submit is a routine race.
    #[test]
    fn a_key_the_definition_does_not_declare_is_dropped() {
        let mut submitted = two_lines_worth_42();
        submitted["is_admin"] = json!(true);

        let payload = secure(&invoice(), &submitted);

        assert!(payload.get("is_admin").is_none(), "got {payload}");
    }

    fn conditional_form() -> Value {
        json!({
            "formId": "conditional", "version": "2.0.1",
            "components": [
                {"id": "b", "role": "data", "type": "number", "key": "budget",
                 "label": "Budget", "validation": {"type": "number"}},
                {"id": "j", "role": "data", "type": "textarea", "key": "justification",
                 "label": "Justification", "validation": {"type": "string"},
                 "conditional": {"action": "show", "logic": {">": [{"var": "budget"}, 1000]}}}
            ]
        })
    }

    /// S10.2 and #164 AC2. The mutation that must make it red is removing the
    /// `hidden` removal loop in [`secure_payload_with`].
    #[test]
    fn a_value_submitted_for_a_hidden_field_is_not_stored() {
        let payload = secure(
            &conditional_form(),
            &json!({"budget": 10, "justification": "smuggled"}),
        );

        assert!(payload.get("justification").is_none(), "got {payload}");
        assert_eq!(payload["budget"], json!(10));
    }

    #[test]
    fn the_same_field_is_stored_when_its_condition_holds() {
        let payload = secure(
            &conditional_form(),
            &json!({"budget": 5000, "justification": "The desks are failing."}),
        );

        assert_eq!(payload["justification"], json!("The desks are failing."));
    }

    /// A hidden *container* takes everything under it, which is the case a walk
    /// that filtered fields one at a time would miss.
    #[test]
    fn a_hidden_container_discards_every_field_beneath_it() {
        let definition = json!({
            "formId": "nested", "version": "2.0.1",
            "components": [
                {"id": "flag", "role": "data", "type": "checkbox", "key": "detailed",
                 "label": "Detailed", "validation": {"type": "boolean"}},
                {"id": "panel", "role": "layout", "type": "columns",
                 "conditional": {"action": "show", "logic": {"var": "detailed"}},
                 "columns": [
                     {"components": [{"id": "a", "role": "data", "type": "textfield",
                                      "key": "note_a", "label": "A",
                                      "validation": {"type": "string"}}]},
                     {"components": [{"id": "b", "role": "data", "type": "textfield",
                                      "key": "note_b", "label": "B",
                                      "validation": {"type": "string"}}]}
                 ]}
            ]
        });

        let payload = secure(
            &definition,
            &json!({"detailed": false, "note_a": "x", "note_b": "y"}),
        );

        assert!(payload.get("note_a").is_none(), "got {payload}");
        assert!(payload.get("note_b").is_none(), "got {payload}");
    }

    /// **The reason this module evaluates the calculations before the
    /// conditionals** (decision **D-27**). The construction plan §6.3 numbers
    /// them the other way round; JFSS §9.2 lists *Calculation Overwrite* before
    /// *Conditional Stripping*, and this is what the order buys: a conditional
    /// keyed on a computed field is decided from the server's number.
    #[test]
    fn a_conditional_over_a_computed_field_reads_the_servers_number() {
        let definition = json!({
            "formId": "gate", "version": "2.0.1",
            "components": [
                {"id": "q", "role": "data", "type": "number", "key": "quantity",
                 "label": "Quantity", "validation": {"type": "integer"}},
                {"id": "t", "role": "data", "type": "number", "key": "total",
                 "label": "Total", "validation": {"type": "number"},
                 "calculate": {"*": [{"var": "quantity"}, 10]}},
                {"id": "a", "role": "data", "type": "textfield", "key": "approval",
                 "label": "Approval", "validation": {"type": "string"},
                 "conditional": {"action": "show", "logic": {">": [{"var": "total"}, 1000]}}}
            ]
        });

        // The client claims a total of 5,000 on a quantity of one. The server
        // computes 10, so the approval branch is not open and the value that
        // came with it is not stored.
        let payload = secure(
            &definition,
            &json!({"quantity": 1, "total": 5000, "approval": "signed"}),
        );

        assert_eq!(payload["total"], json!(10.0));
        assert!(payload.get("approval").is_none(), "got {payload}");
    }

    /// S4.2.3 Case C, and S8.1.1's rule that the mode is **declared** rather
    /// than inferred from the operators.
    #[test]
    fn a_generated_field_keeps_a_value_it_already_has_and_a_derived_one_never_does() {
        let definition = json!({
            "formId": "modes", "version": "2.0.1",
            "components": [
                {"id": "b", "role": "data", "type": "number", "key": "budget",
                 "label": "Budget", "validation": {"type": "number"}},
                {"id": "base", "role": "data", "type": "number", "key": "baseline",
                 "label": "Baseline", "validation": {"type": "number"},
                 "defaultValue": 0, "calculateMode": "generated",
                 "calculate": {"var": "budget"}},
                {"id": "d", "role": "data", "type": "number", "key": "doubled",
                 "label": "Doubled", "validation": {"type": "number"},
                 "calculate": {"*": [{"var": "budget"}, 2]}}
            ]
        });

        let resolved = secure(
            &definition,
            &json!({"budget": 500, "baseline": 100, "doubled": 999}),
        );

        assert_eq!(
            resolved["baseline"],
            json!(100),
            "a resolved generated value stands"
        );
        assert_eq!(
            resolved["doubled"],
            json!(1000.0),
            "a derived value never does"
        );

        let fresh = secure(&definition, &json!({"budget": 500, "baseline": null}));

        assert_eq!(
            fresh["baseline"],
            json!(500.0),
            "an unresolved one is evaluated"
        );
    }

    /// A chain declared in the worst possible order still settles, which is
    /// what the fixed point buys in place of the topological order §9.2 asks
    /// for and **D-2** reserves for the rule engine.
    #[test]
    fn a_chain_declared_backwards_still_settles() {
        let definition = json!({
            "formId": "chain", "version": "2.0.1",
            "components": [
                {"id": "c", "role": "data", "type": "number", "key": "c", "label": "c",
                 "validation": {"type": "number"}, "calculate": {"+": [{"var": "b"}, 1]}},
                {"id": "b", "role": "data", "type": "number", "key": "b", "label": "b",
                 "validation": {"type": "number"}, "calculate": {"+": [{"var": "a"}, 1]}},
                {"id": "a", "role": "data", "type": "number", "key": "a", "label": "a",
                 "validation": {"type": "number"}}
            ]
        });

        let payload = secure(&definition, &json!({"a": 1}));

        assert_eq!(payload["b"], json!(2.0));
        assert_eq!(payload["c"], json!(3.0));
    }

    /// S12.2 makes a cyclic definition invalid and nothing rejects one at save
    /// yet — the detector is the same graph **D-2** reserved. Refusing beats
    /// persisting a half-converged payload, and beats not terminating.
    #[test]
    fn a_cyclic_definition_is_refused_rather_than_looping() {
        let definition = json!({
            "formId": "cycle", "version": "2.0.1",
            "components": [
                {"id": "a", "role": "data", "type": "number", "key": "a", "label": "a",
                 "validation": {"type": "number"}, "calculate": {"+": [{"var": "b"}, 1]}},
                {"id": "b", "role": "data", "type": "number", "key": "b", "label": "b",
                 "validation": {"type": "number"}, "calculate": {"+": [{"var": "a"}, 1]}}
            ]
        });

        let details = refusal(&definition, &json!({}));

        assert!(
            details
                .iter()
                .any(|detail| detail.code == "CALCULATION_DID_NOT_SETTLE"),
            "got {details:?}"
        );
    }

    /// **Decision D-24 at the submission** (#164 AC6, construction plan §6.3
    /// step 6). The browser renders the field blank while the form is being
    /// filled in; here the same failure is a refusal naming the field.
    #[test]
    fn a_division_by_zero_refuses_the_submission_and_names_the_field() {
        let definition = json!({
            "formId": "average", "version": "2.0.1",
            "components": [
                {"id": "t", "role": "data", "type": "number", "key": "total",
                 "label": "Total", "validation": {"type": "number"}},
                {"id": "c", "role": "data", "type": "number", "key": "count",
                 "label": "Count", "validation": {"type": "integer"}},
                {"id": "avg", "role": "data", "type": "number", "key": "average",
                 "label": "Average", "validation": {"type": "number"},
                 "calculate": {"/": [{"var": "total"}, {"var": "count"}]}}
            ]
        });

        let details = refusal(&definition, &json!({"total": 10, "count": 0}));

        assert_eq!(details.len(), 1, "got {details:?}");
        assert_eq!(details[0].path, "average");
        assert_eq!(details[0].code, "EVALUATION_FAILED");
    }

    /// S10.3's dot-notation path, which is the reason the envelope's field is
    /// called `path` and not `key`: a row's address is not a bare key.
    #[test]
    fn a_row_level_failure_is_named_by_its_dot_notation_path() {
        let mut definition = invoice();
        definition["components"][0]["components"][1]["validation"] = json!({
            "type": "integer",
            "minimum": 1,
            "messages": {"minimum": "A line orders at least one."},
        });

        let mut submitted = two_lines_worth_42();
        submitted["line_items"][1]["quantity"] = json!(0);

        let details = refusal(&definition, &submitted);

        assert_eq!(details.len(), 1, "got {details:?}");
        assert_eq!(details[0].path, "line_items.1.quantity");
        assert_eq!(details[0].rule, "minimum");
        assert_eq!(details[0].message, "A line orders at least one.");
    }

    /// Every problem rather than the first, for the reason
    /// `validate_definition` gives: a person with three mistakes should not
    /// discover them one round trip at a time.
    #[test]
    fn every_failure_is_reported_rather_than_the_first() {
        let definition = json!({
            "formId": "many", "version": "2.0.1",
            "components": [
                {"id": "a", "role": "data", "type": "textfield", "key": "a", "label": "A",
                 "validation": {"type": "string", "required": true}},
                {"id": "b", "role": "data", "type": "textfield", "key": "b", "label": "B",
                 "validation": {"type": "string", "required": true}}
            ]
        });

        let details = refusal(&definition, &json!({}));

        assert_eq!(details.len(), 2, "got {details:?}");
    }

    /// §9.2: a hidden component is *"treated as absent for validation"*, so a
    /// `required` field on a branch nobody took does not block a submission.
    #[test]
    fn a_required_field_on_a_hidden_branch_does_not_block_the_submission() {
        let mut definition = conditional_form();
        definition["components"][1]["validation"] = json!({"type": "string", "required": true});

        secure(&definition, &json!({"budget": 10, "justification": null}));
    }

    #[test]
    fn the_same_field_blocks_it_when_the_branch_is_open() {
        let mut definition = conditional_form();
        definition["components"][1]["validation"] = json!({"type": "string", "required": true});

        let details = refusal(&definition, &json!({"budget": 5000, "justification": null}));

        assert_eq!(details[0].path, "justification");
        assert_eq!(details[0].rule, "required");
    }

    /// S8.1.1: an unknown rule is a refusal, never a pass. A definition can
    /// carry one — `domain/jfss.rs` checks the meta-schema, the approved
    /// operator set and the lookup allow-list at save, and a rule *name* is
    /// none of the three.
    #[test]
    fn a_rule_outside_the_registry_refuses_the_submission() {
        let mut definition = conditional_form();
        definition["components"][0]["rules"] =
            json!([{"rule": "looksNice", "scope": "both", "params": {}, "message": "no"}]);

        let details = refusal(&definition, &json!({"budget": 10}));

        assert_eq!(details[0].code, "RULE_NOT_REGISTERED");
        assert_eq!(details[0].path, "budget");
    }

    #[test]
    fn a_server_rule_this_build_does_not_enforce_refuses_the_submission() {
        let mut definition = conditional_form();
        definition["components"][0]["rules"] =
            json!([{"rule": "unique", "scope": "server", "params": {}, "message": "taken"}]);

        let details = refusal(&definition, &json!({"budget": 10}));

        assert_eq!(details[0].code, "RULE_NOT_ENFORCED");
    }

    #[test]
    fn a_payload_that_is_not_an_object_is_refused() {
        let details = refusal(&invoice(), &json!([1, 2, 3]));

        assert_eq!(details[0].code, "PAYLOAD_NOT_AN_OBJECT");
    }

    #[test]
    fn a_repeater_row_that_is_not_an_object_is_refused() {
        let details = refusal(&invoice(), &json!({"line_items": ["not a row"]}));

        assert_eq!(details[0].code, "ROW_NOT_AN_OBJECT");
        assert_eq!(details[0].path, "line_items.0");
    }

    /// A `display` component's `calculate` produces something to look at, not
    /// something to store — §4.4 makes it the text the component renders.
    #[test]
    fn a_display_components_calculation_is_not_stored() {
        let mut definition = invoice();
        definition["components"]
            .as_array_mut()
            .unwrap()
            .push(json!({
                "id": "echo", "role": "display", "type": "paragraph",
                "calculate": {"var": "grand_total"}
            }));

        let payload = secure(&definition, &two_lines_worth_42());

        assert_eq!(payload.as_object().unwrap().len(), 2, "got {payload}");
    }
}
