//! The rule engine around the evaluator — catalogue, dependency graph, cycle
//! detection (FR-RAD-006, [#338], decision **D-2**).
//!
//! **An evaluator answers *what does this expression compute*. An engine
//! answers *which* expressions run, *in what order*, and *what a failure
//! means*.** [`super::super::evaluator`] has been the first half since Sprint 7;
//! this is the second, and until it existed the order a definition's
//! calculations ran in was the order its author happened to type them.
//!
//! # The three questions, and where each is answered
//!
//! **Which rules run** is [`ScopeCatalogue`]. Every `data` component in a scope
//! is resolved against both companion registries: its `rules[].rule` names
//! against the [Validation Rule
//! Registry](../../../../../docs/schema/JFSS%20Validation%20Rule%20Registry.md)
//! (through [`super::validation::is_registered`], so there is one catalogue and
//! not two), and its `calculate` and `conditional.logic` operators against the
//! [Calculation Rule
//! Registry](../../../../../docs/schema/JFSS%20Calculation%20Rule%20Registry.md)
//! (through [`super::jfss::check_operators`], for the same reason). **A name
//! outside a registry is refused where the definition is written, not where it
//! is filled in.** Until this module, an unregistered `rules[].rule` was
//! discovered at submit — by the person typing, who cannot fix it — and the
//! form had by then been published and filled in.
//!
//! **In what order** is [`evaluation_order`]. JFSS §9.2 asks for topological
//! order and the [system design](../../../../../docs/design/01.%20System%20Design%20Document.md)
//! §8.2.2 recorded that it was not being done: a repeated definition-order pass
//! stood in for it until this sprint, converging on the same answer for any
//! acyclic definition by running the whole scope up to `n + 1` times. The graph
//! replaces that with one pass, and — more to the point — makes the *cycle* a
//! property of the definition rather than a loop that failed to settle.
//!
//! **What a failure means** is split by moment, which is the whole of why the
//! graph is built twice. At **publish**, a cycle is a
//! [`ValidationDetail`] against the definition and the form does not go live
//! (JFSS S12.2: *"a schema whose `calculate` or `conditional` expressions form a
//! dependency cycle is **invalid**"*). At **submit**,
//! [`super::super::service::evaluation`] asks for the same order and maps the
//! same cycle onto an S10.3 error, because a definition published before this
//! check existed is still out there and a 500 is the wrong answer to it.
//!
//! # The graph is over `var` references, in both properties
//!
//! S12.2 is explicit about both halves: *"implementations MUST build the graph
//! from every `{"var": ...}` reference in **both** properties"*. So a
//! `conditional` that reads a field is an edge exactly as a `calculate` that
//! reads one is, and a cycle running through a `conditional` is refused with
//! the rest. That is not a technicality — `grand_total` calculating from
//! `approval_reason` while `approval_reason`'s `conditional` reads
//! `grand_total` is a form whose stored shape depends on the order two
//! expressions happened to run in.
//!
//! **A scope is a record, not the document.** A repeater's row template is its
//! own graph, because a `key` inside one addresses a property of the *row*
//! (§4.3.1) — which is what makes `line_total` reading `quantity` mean the same
//! row's quantity, and what stops a top-level field of the same name from
//! joining that cycle.
//!
//! [#338]: https://github.com/sujanto-gaws/kelir/issues/338

use std::collections::{BTreeSet, HashMap};

use serde_json::Value;

use super::jfss::{
    check_operators, container_children_at, data_key, role_of, row_template, CALCULATE_OPERATORS,
    CONDITIONAL_OPERATORS,
};
use super::validation::is_registered;
use crate::error::ValidationDetail;

/// The S10.3 `code` a cyclic definition carries, at either moment.
pub(crate) const CALCULATION_CYCLE: &str = "CALCULATION_CYCLE";

/// The S10.3 `code` a rule name outside the Validation Rule Registry carries.
///
/// The same string [`super::super::service::evaluation`] raises at submit, and
/// deliberately so: it is one defect reported at two moments, and a builder
/// that learns to handle the code at publish handles it wherever it appears.
pub(crate) const RULE_NOT_REGISTERED: &str = "RULE_NOT_REGISTERED";

/// A dependency cycle, as the keys that make it up.
///
/// The keys are in *reading* order — `keys[0]`'s expression reads `keys[1]`,
/// and the last one reads `keys[0]` again — so [`Cycle::describe`] can print
/// the loop without the caller having to reconstruct it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Cycle {
    pub(crate) keys: Vec<String>,
}

impl Cycle {
    /// The loop as a person reading an error should see it: `a` → `b` → `a`.
    ///
    /// **Closed rather than left open.** A cycle printed as `a` → `b` names two
    /// fields and looks like a chain; repeating the first is what makes it
    /// visibly a loop, and JFSS S12.2 asks an authoring tool to *surface* the
    /// cycle rather than merely to refuse it.
    pub(crate) fn describe(&self) -> String {
        let mut chain: Vec<&str> = self.keys.iter().map(String::as_str).collect();

        if let Some(first) = chain.first().copied() {
            chain.push(first);
        }

        format!("`{}`", chain.join("` → `"))
    }
}

/// Every data key an expression reads in the scope it is written in.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct References {
    keys: BTreeSet<String>,
    /// `{"var": ""}` — the whole record rather than one field of it.
    whole_scope: bool,
}

impl References {
    /// A `var` path, as the key it addresses in this scope.
    ///
    /// A dotted path reads *into* a value (`address.city`), so the dependency
    /// is on the value's own key and the remainder is a property of whatever
    /// that key holds. A numeric path is a positional index into an array
    /// scope, which is not a key at all.
    fn add_path(&mut self, path: &Value) {
        let path = match path {
            Value::String(text) => text.as_str(),
            // `{"var": ["total", 0]}` — the path with a default beside it.
            Value::Array(items) => match items.first() {
                Some(Value::String(text)) => text.as_str(),
                _ => return,
            },
            _ => return,
        };

        if path.is_empty() {
            self.whole_scope = true;
            return;
        }

        let head = path.split('.').next().unwrap_or(path);

        if !head.is_empty() {
            self.keys.insert(head.to_owned());
        }
    }

    /// `missing` and `missing_some` take *names*, not expressions.
    fn add_names(&mut self, argument: &Value) {
        match argument {
            Value::Array(items) => {
                for item in items {
                    if item.is_string() {
                        self.add_path(item);
                    }
                }
            }
            other => self.add_path(other),
        }
    }
}

/// Every key `expression` reads, in the scope `expression` is written in.
///
/// **The iterator operators are the reason this is a walk and not a search for
/// `var`.** `map`, `filter`, `reduce`, `all`, `some` and `none` evaluate their
/// first argument in the current scope and every later one against each *item*
/// of what that produced — so in the registry §6.1 invoice,
/// `{"map": [{"var": "line_items"}, {"*": [{"var": "unit_price"}, …]}]}` reads
/// `line_items` here and `unit_price` in a row. Collecting `unit_price` as a
/// dependency of the top-level scope would invent an edge to whatever top-level
/// field happened to share the name, and an invented edge is an invented cycle:
/// a definition refused at publish for a loop it does not have.
fn collect_references(expression: &Value, found: &mut References) {
    match expression {
        Value::Object(map) => {
            for (operator, argument) in map {
                match operator.as_str() {
                    "var" => found.add_path(argument),
                    "missing" => found.add_names(argument),
                    // `{"missing_some": [n, ["a", "b"]]}` — the count, then the
                    // names.
                    "missing_some" => match argument.as_array().and_then(|items| items.get(1)) {
                        Some(names) => found.add_names(names),
                        None => found.add_names(argument),
                    },
                    "map" | "filter" | "reduce" | "all" | "some" | "none" => {
                        match argument.as_array().and_then(|items| items.first()) {
                            Some(source) => collect_references(source, found),
                            // The shorthand: the argument given directly rather
                            // than wrapped in a list.
                            None => collect_references(argument, found),
                        }
                    }
                    _ => collect_references(argument, found),
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_references(item, found);
            }
        }
        _ => {}
    }
}

/// One data component of a scope, with its rules resolved.
struct Entry<'a> {
    component: &'a Value,
    key: &'a str,
    /// Where the component sits in the definition — `definition.components.3`.
    path: String,
    /// What its `calculate` reads. Decides this field's own value, so a
    /// self-reference here is a cycle.
    calculate_reads: Option<References>,
    /// What its `conditional.logic` reads. Decides whether the field is
    /// *stored*, and it is evaluated against the complete payload (S10.2), so a
    /// self-reference here is legal and ordinary.
    conditional_reads: Option<References>,
}

/// One record's data components, flattened out of their layout containers, in
/// the order the definition declares them (JFSS §4.3.1).
///
/// **Layout containers are transparent and repeaters are not.** A field inside
/// a fieldset writes into the same record as its neighbours, so it belongs to
/// this graph; a field inside a datagrid row writes into the row, so it belongs
/// to that row's. That asymmetry is [`super::jfss::container_children_at`] and
/// [`super::jfss::row_template`], and it is the same line
/// [`super::super::service::evaluation`] walks — one definition of *child*, in
/// one place, because two would let a rule mean one thing per traversal.
pub(crate) struct ScopeCatalogue<'a> {
    entries: Vec<Entry<'a>>,
}

impl<'a> ScopeCatalogue<'a> {
    /// The catalogue of one scope, at the component path `path` prefixes.
    pub(crate) fn of(components: &[&'a Value], path: &str) -> Self {
        let mut entries = Vec::new();

        flatten(components, path, &mut entries);
        Self { entries }
    }

    /// The order this scope's components must be evaluated in, or the cycle
    /// that makes an order impossible.
    ///
    /// **Every data component is ordered, not only the calculated ones.** A
    /// repeater is a node here because evaluating it means evaluating its rows,
    /// and a grand total that reads `line_items` has to run after those rows —
    /// which is a dependency between a calculated field and a component that
    /// carries no `calculate` at all.
    ///
    /// Ties break on declaration order, so a definition whose author already
    /// wrote it in dependency order gets its own order back rather than a
    /// permutation of it.
    pub(crate) fn order(&self) -> Result<Vec<&'a Value>, Cycle> {
        let graph = self.graph();

        graph
            .topological()
            .map(|order| {
                order
                    .into_iter()
                    .map(|at| self.entries[at].component)
                    .collect()
            })
            .map_err(|cycle| Cycle {
                keys: cycle
                    .into_iter()
                    .map(|at| self.entries[at].key.to_owned())
                    .collect(),
            })
    }

    /// The dependency edges this scope's expressions declare.
    fn graph(&self) -> Graph {
        let mut index: HashMap<&str, usize> = HashMap::new();

        for (at, entry) in self.entries.iter().enumerate() {
            // First declaration wins. A scope with two components on one key is
            // a definition defect of its own, and resolving the reference to
            // the later one would silently change which expression an edge is
            // about.
            index.entry(entry.key).or_insert(at);
        }

        let mut graph = Graph::of(self.entries.len());

        for (at, entry) in self.entries.iter().enumerate() {
            // `calculate` decides the field's own value: reading itself is a
            // loop with one node in it, and it is the shape a fixed point could
            // only ever report as "did not settle".
            if let Some(reads) = &entry.calculate_reads {
                add_edges(&mut graph, &index, self.entries.len(), reads, at, true);
            }

            // `conditional` decides whether the value is stored, against the
            // complete payload S10.2 snapshots before anything is discarded —
            // so a component whose visibility reads its own value is ordinary
            // (`{"!!": {"var": "notes"}}`) and must not be refused as a cycle.
            if let Some(reads) = &entry.conditional_reads {
                add_edges(&mut graph, &index, self.entries.len(), reads, at, false);
            }
        }

        graph
    }
}

/// The edges one property's references contribute to `dependent`.
///
/// `self_edge` is whether a reference to `dependent`'s own key counts — see
/// [`ScopeCatalogue::graph`], which is where the difference between the two
/// properties is argued.
fn add_edges(
    graph: &mut Graph,
    index: &HashMap<&str, usize>,
    nodes: usize,
    reads: &References,
    dependent: usize,
    self_edge: bool,
) {
    if reads.whole_scope {
        // `{"var": ""}` is the whole record, which is every key in it. Exotic,
        // and left as an edge rather than as a blind spot: a `calculate` that
        // reads the record it is *part of* is genuinely self-referential, and
        // saying so is better than an order that quietly picks one.
        for dependency in 0..nodes {
            if self_edge || dependency != dependent {
                graph.depends(dependent, dependency);
            }
        }

        return;
    }

    for key in &reads.keys {
        let Some(&dependency) = index.get(key.as_str()) else {
            // A `var` naming no component of this scope. Not an error here:
            // JFSS S12.4 resolves a missing key to null, and a `conditional`
            // reading a key from a scope it cannot see is a definition problem
            // the meta-schema and the submit path already speak to.
            continue;
        };

        if self_edge || dependency != dependent {
            graph.depends(dependent, dependency);
        }
    }
}

/// A directed graph over one scope's components, as *depends on* edges.
struct Graph {
    /// `dependencies[i]` — the nodes `i` must run after.
    dependencies: Vec<BTreeSet<usize>>,
    /// `dependents[j]` — the nodes that must run after `j`. The same edges,
    /// indexed the other way, because Kahn's algorithm walks both directions.
    dependents: Vec<BTreeSet<usize>>,
}

impl Graph {
    fn of(nodes: usize) -> Self {
        Self {
            dependencies: vec![BTreeSet::new(); nodes],
            dependents: vec![BTreeSet::new(); nodes],
        }
    }

    fn depends(&mut self, dependent: usize, dependency: usize) {
        self.dependencies[dependent].insert(dependency);
        self.dependents[dependency].insert(dependent);
    }

    /// Kahn's algorithm, taking the lowest-numbered ready node each time.
    ///
    /// Lowest-numbered rather than any ready node, so the answer is a function
    /// of the definition and not of a hash order — two runs of the same
    /// definition produce the same payload, which is a property the
    /// Tamper-Proof Pattern needs and a `HashSet` would quietly remove.
    fn topological(&self) -> Result<Vec<usize>, Vec<usize>> {
        let nodes = self.dependencies.len();
        let mut remaining: Vec<usize> = self.dependencies.iter().map(BTreeSet::len).collect();
        let mut ready: BTreeSet<usize> = (0..nodes).filter(|&at| remaining[at] == 0).collect();
        let mut order = Vec::with_capacity(nodes);

        while let Some(&at) = ready.iter().next() {
            ready.remove(&at);
            order.push(at);

            for &dependent in &self.dependents[at] {
                remaining[dependent] -= 1;

                if remaining[dependent] == 0 {
                    ready.insert(dependent);
                }
            }
        }

        if order.len() == nodes {
            Ok(order)
        } else {
            Err(self.cycle(&remaining))
        }
    }

    /// One cycle among the nodes Kahn could not place.
    ///
    /// **Every node left over still has an unplaced dependency** — that is what
    /// being left over means — so following dependencies from any of them
    /// cannot run out, and the first node met twice closes a loop. Following a
    /// dependency is following a `var` reference, so the path comes out in
    /// reading order and [`Cycle::describe`] prints it as `a` → `b` → `a`
    /// without reversing anything.
    fn cycle(&self, remaining: &[usize]) -> Vec<usize> {
        let Some(start) = remaining.iter().position(|&degree| degree > 0) else {
            return Vec::new();
        };

        let mut seen_at: HashMap<usize, usize> = HashMap::new();
        let mut path = Vec::new();
        let mut node = start;

        loop {
            if let Some(&at) = seen_at.get(&node) {
                return path.split_off(at);
            }

            seen_at.insert(node, path.len());
            path.push(node);

            match self.dependencies[node]
                .iter()
                .copied()
                .find(|&next| remaining[next] > 0)
            {
                Some(next) => node = next,
                // Unreachable while `remaining` is Kahn's own leftover count,
                // and answering with the path rather than panicking keeps a
                // future caller's mistake a poor message instead of a 500.
                None => return path,
            }
        }
    }
}

/// One scope's data components, layout containers walked through.
fn flatten<'a>(components: &[&'a Value], path: &str, entries: &mut Vec<Entry<'a>>) {
    for (index, component) in components.iter().enumerate() {
        let here = if path.is_empty() {
            index.to_string()
        } else {
            format!("{path}.{index}")
        };

        flatten_one(component, &here, entries);
    }
}

fn flatten_one<'a>(component: &'a Value, here: &str, entries: &mut Vec<Entry<'a>>) {
    if role_of(component) == Some("layout") {
        for (child, child_path) in container_children_at(component, here) {
            flatten_one(child, &child_path, entries);
        }

        return;
    }

    let Some(key) = data_key(component) else {
        return;
    };

    entries.push(Entry {
        component,
        key,
        path: here.to_owned(),
        calculate_reads: component.get("calculate").map(references),
        conditional_reads: component
            .get("conditional")
            .and_then(|conditional| conditional.get("logic"))
            .map(references),
    });
}

fn references(expression: &Value) -> References {
    let mut found = References::default();

    collect_references(expression, &mut found);
    found
}

/// The order one scope's components must be evaluated in (JFSS §9.2).
///
/// The runtime half of this module, and the one
/// [`super::super::service::evaluation`] calls on every write. It asks the same
/// question the publish gate asks, of the same graph, so a definition that was
/// refused at publish cannot be evaluated in some other order at submit.
pub(crate) fn evaluation_order<'a>(components: &[&'a Value]) -> Result<Vec<&'a Value>, Cycle> {
    ScopeCatalogue::of(components, "").order()
}

/// Every problem the rule engine can see in a definition before it is stored.
///
/// **Refused where it is written, not where it is filled in.** A form is
/// authored once and submitted thousands of times, and every failure here is
/// one nothing the person filling it in can do anything about: a rule name no
/// registry defines, an operator the registry forbids, a pair of fields that
/// compute from each other. JFSS S12.2 makes the last of those *invalid* rather
/// than merely awkward, and asks an authoring tool to surface it.
pub(crate) fn definition_errors(definition: &Value) -> Vec<ValidationDetail> {
    let Some(components) = definition.get("components").and_then(Value::as_array) else {
        // Not a JFSS document at all, which the meta-schema is the one to say
        // so — reporting a missing graph beside it would be this module
        // answering a question it was not asked.
        return Vec::new();
    };

    let mut details = Vec::new();
    let components: Vec<&Value> = components.iter().collect();

    check_scope(&components, "definition.components", &mut details);
    details
}

/// One scope: its catalogue, its rule names, its order — then its rows.
fn check_scope(components: &[&Value], path: &str, details: &mut Vec<ValidationDetail>) {
    let catalogue = ScopeCatalogue::of(components, path);

    for entry in &catalogue.entries {
        check_rule_names(entry, details);
        check_operator_sets(entry, details);
    }

    if let Err(cycle) = catalogue.order() {
        // Reported against the first component on the loop rather than against
        // the definition as a whole: a builder highlights a path, and
        // `definition` highlights the document.
        let path = catalogue
            .entries
            .iter()
            .find(|entry| cycle.keys.first().is_some_and(|key| key == entry.key))
            .map_or_else(|| path.to_owned(), |entry| entry.path.clone());

        details.push(ValidationDetail::new(
            path,
            "cycle",
            CALCULATION_CYCLE,
            format!(
                "these fields compute from one another in a loop — {} — so there is no order \
                 in which they can be evaluated; JFSS S12.2 makes such a definition invalid",
                cycle.describe()
            ),
        ));
    }

    // A row template is its own record and therefore its own graph (§4.3.1).
    for entry in &catalogue.entries {
        if let Some(template) = row_template(entry.component) {
            let rows: Vec<&Value> = template.iter().collect();

            check_scope(&rows, &format!("{}.components", entry.path), details);
        }
    }
}

/// The `rules[].rule` names one component declares, against the Validation Rule
/// Registry.
///
/// **The same membership question [`super::validation`] asks at submit, asked
/// at publish instead.** It was already a refusal — S8.1.1 makes an unknown
/// rule an error rather than a skipped arm — but it was raised against the
/// person filling in the form, who cannot fix a definition, after the form had
/// been published and filled in. The catalogue is the layer that can ask it
/// earlier, and this is the whole of what asking earlier costs.
fn check_rule_names(entry: &Entry<'_>, details: &mut Vec<ValidationDetail>) {
    let rules = entry
        .component
        .get("rules")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);

    for (index, rule) in rules.iter().enumerate() {
        let name = rule.get("rule").and_then(Value::as_str).unwrap_or_default();

        if is_registered(name) {
            continue;
        }

        details.push(ValidationDetail::new(
            format!("{}.rules.{index}", entry.path),
            "rule",
            RULE_NOT_REGISTERED,
            format!(
                "`{name}` is not a rule in the JFSS Validation Rule Registry, so `{}` cannot \
                 be checked against it — adding a rule is registry §4, not a branch in a \
                 validator",
                entry.key
            ),
        ));
    }
}

/// The operators one component's expressions use, against the Calculation Rule
/// Registry and the `conditional` floor.
///
/// **This is the walk [`super::jfss`] used to carry**, moved here rather than
/// added beside it. It was a second traversal of the component tree with its
/// own idea of §4.3.1's three container shapes, which is the duplication
/// `container_children_at`'s own doc comment warns about — and the catalogue
/// already has to visit every data component to build the graph. One walk, both
/// registries, which is what makes AC1's *resolves against both* a single gate
/// rather than two that could disagree about which components exist.
fn check_operator_sets(entry: &Entry<'_>, details: &mut Vec<ValidationDetail>) {
    if let Some(calculate) = entry.component.get("calculate") {
        check_operators(
            calculate,
            CALCULATE_OPERATORS,
            &format!("{}.calculate", entry.path),
            details,
        );
    }

    if let Some(logic) = entry
        .component
        .get("conditional")
        .and_then(|conditional| conditional.get("logic"))
    {
        check_operators(
            logic,
            CONDITIONAL_OPERATORS,
            &format!("{}.conditional.logic", entry.path),
            details,
        );
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn field(key: &str, calculate: Value) -> Value {
        json!({
            "id": key, "role": "data", "type": "number", "key": key, "label": key,
            "validation": {"type": "number"},
            "calculate": calculate,
        })
    }

    fn plain(key: &str) -> Value {
        json!({
            "id": key, "role": "data", "type": "number", "key": key, "label": key,
            "validation": {"type": "number"},
        })
    }

    fn definition(components: Vec<Value>) -> Value {
        json!({"formId": "f", "version": "2.0.1", "components": components})
    }

    fn keys(order: &[&Value]) -> Vec<String> {
        order
            .iter()
            .filter_map(|component| data_key(component).map(str::to_owned))
            .collect()
    }

    fn ordered(components: &[Value]) -> Vec<String> {
        let refs: Vec<&Value> = components.iter().collect();

        keys(&evaluation_order(&refs).expect("acyclic"))
    }

    fn codes(details: &[ValidationDetail]) -> Vec<&str> {
        details.iter().map(|detail| detail.code.as_str()).collect()
    }

    // -- The order ---------------------------------------------------------

    /// **AC2, at the unit.** The definition declares `c`, then `b`, then `a`;
    /// the data says `a` feeds `b` feeds `c`.
    ///
    /// The mutation that must make this red is replacing
    /// [`ScopeCatalogue::order`]'s body with `Ok(definition order)` — which is
    /// exactly the behaviour this module was written to replace.
    #[test]
    fn orders_a_chain_declared_backwards_by_its_dependencies() {
        let components = vec![
            field("c", json!({"+": [{"var": "b"}, 1]})),
            field("b", json!({"+": [{"var": "a"}, 1]})),
            plain("a"),
        ];

        assert_eq!(ordered(&components), ["a", "b", "c"]);
    }

    #[test]
    fn leaves_an_independent_scope_in_declaration_order() {
        // Nothing depends on anything, so the answer is the author's own order
        // rather than a permutation of it.
        let components = vec![plain("a"), plain("b"), plain("c")];

        assert_eq!(ordered(&components), ["a", "b", "c"]);
    }

    #[test]
    fn orders_a_repeater_before_the_total_that_reads_it() {
        // The registry §6.1 shape, declared backwards: the grand total is
        // first and the rows it sums are second.
        let components = vec![
            field(
                "grand_total",
                json!({"sum": [{"map": [
                    {"var": "line_items"},
                    {"*": [{"var": "unit_price"}, {"var": "quantity"}]}
                ]}]}),
            ),
            json!({
                "id": "lines", "role": "data", "type": "datagrid", "key": "line_items",
                "label": "Lines", "validation": {"type": "array"},
                "components": [plain("quantity"), plain("unit_price")]
            }),
        ];

        assert_eq!(ordered(&components), ["line_items", "grand_total"]);
    }

    /// The iterator scope, which is the edge a naive `var` search invents.
    ///
    /// `unit_price` is read inside the `map`'s body, against a *row*. A
    /// top-level field of the same name is a different field, and an edge to it
    /// would be a dependency the definition does not have — here it would close
    /// a loop and refuse a definition that is fine.
    #[test]
    fn does_not_read_an_iterator_body_as_a_dependency_of_the_outer_scope() {
        let components = vec![
            field("unit_price", json!({"+": [{"var": "grand_total"}, 1]})),
            field(
                "grand_total",
                json!({"sum": [{"map": [
                    {"var": "line_items"},
                    {"*": [{"var": "unit_price"}, {"var": "quantity"}]}
                ]}]}),
            ),
            json!({
                "id": "lines", "role": "data", "type": "datagrid", "key": "line_items",
                "label": "Lines", "validation": {"type": "array"},
                "components": [plain("quantity")]
            }),
        ];

        assert_eq!(
            ordered(&components),
            ["line_items", "grand_total", "unit_price"]
        );
    }

    #[test]
    fn reads_a_field_through_a_layout_container_as_a_sibling() {
        // A fieldset is transparent: `total` and `amount` write into one
        // record, so the edge between them is real and the order is by data.
        let components = vec![
            json!({
                "id": "panel", "role": "layout", "type": "fieldset",
                "components": [field("total", json!({"*": [{"var": "amount"}, 2]}))]
            }),
            plain("amount"),
        ];

        assert_eq!(ordered(&components), ["amount", "total"]);
    }

    #[test]
    fn reads_a_field_through_a_named_slot_container_too() {
        // §4.3.1's other two shapes. A traversal that knows only `components`
        // silently drops everything inside a column.
        let components = vec![
            json!({
                "id": "cols", "role": "layout", "type": "columns",
                "columns": [{"width": 6, "components": [
                    field("total", json!({"*": [{"var": "amount"}, 2]}))
                ]}]
            }),
            plain("amount"),
        ];

        assert_eq!(ordered(&components), ["amount", "total"]);
    }

    #[test]
    fn a_var_naming_nothing_in_the_scope_is_not_an_edge() {
        let components = vec![field("total", json!({"*": [{"var": "nowhere"}, 2]}))];

        assert_eq!(ordered(&components), ["total"]);
    }

    #[test]
    fn reads_a_dotted_path_as_a_dependency_on_its_head() {
        let components = vec![
            field("city", json!({"var": "address.city"})),
            plain("address"),
        ];

        assert_eq!(ordered(&components), ["address", "city"]);
    }

    #[test]
    fn reads_a_var_with_a_default_beside_it() {
        let components = vec![
            field("total", json!({"+": [{"var": ["amount", 0]}, 1]})),
            plain("amount"),
        ];

        assert_eq!(ordered(&components), ["amount", "total"]);
    }

    // -- The cycles --------------------------------------------------------

    fn cycle_of(components: &[Value]) -> Cycle {
        let refs: Vec<&Value> = components.iter().collect();

        evaluation_order(&refs).expect_err("cyclic")
    }

    /// **AC3, at the unit**: both fields are named.
    #[test]
    fn refuses_two_fields_that_calculate_from_each_other() {
        let cycle = cycle_of(&[
            field("a", json!({"+": [{"var": "b"}, 1]})),
            field("b", json!({"+": [{"var": "a"}, 1]})),
        ]);

        assert_eq!(cycle.keys.len(), 2);
        assert!(cycle.keys.contains(&"a".to_owned()));
        assert!(cycle.keys.contains(&"b".to_owned()));
    }

    #[test]
    fn refuses_a_field_that_calculates_from_itself() {
        let cycle = cycle_of(&[field("a", json!({"+": [{"var": "a"}, 1]}))]);

        assert_eq!(cycle.keys, ["a"]);
        assert_eq!(cycle.describe(), "`a` → `a`");
    }

    #[test]
    fn refuses_a_longer_loop_and_names_every_field_on_it() {
        let cycle = cycle_of(&[
            field("a", json!({"var": "b"})),
            field("b", json!({"var": "c"})),
            field("c", json!({"var": "a"})),
        ]);

        assert_eq!(cycle.keys.len(), 3);
        assert_eq!(cycle.describe(), "`a` → `b` → `c` → `a`");
    }

    /// S12.2 says *both* properties, and this is the shape that makes it worth
    /// saying: neither expression is a loop on its own.
    #[test]
    fn refuses_a_loop_that_runs_through_a_conditional() {
        let mut reason = plain("approval_reason");
        reason["conditional"] =
            json!({"action": "show", "logic": {">": [{"var": "grand_total"}, 1000]}});

        let cycle = cycle_of(&[
            field("grand_total", json!({"+": [{"var": "approval_reason"}, 0]})),
            reason,
        ]);

        assert_eq!(cycle.keys.len(), 2);
    }

    /// The mirror of the test above, and the reason a `conditional` self-edge
    /// is dropped: S10.2 decides visibility against the complete payload, so a
    /// component whose own value decides whether it is stored is ordinary.
    #[test]
    fn a_conditional_reading_its_own_field_is_not_a_cycle() {
        let mut notes = plain("notes");
        notes["conditional"] = json!({"action": "show", "logic": {"!!": {"var": "notes"}}});

        assert_eq!(ordered(&[notes]), ["notes"]);
    }

    #[test]
    fn a_calculate_reading_the_whole_record_is_a_cycle() {
        // `{"var": ""}` is every key in the scope, which includes the field
        // being computed — so the field is computed from a record containing
        // itself.
        let cycle = cycle_of(&[field("snapshot", json!({"var": ""})), plain("a")]);

        assert!(cycle.keys.contains(&"snapshot".to_owned()));
    }

    #[test]
    fn a_conditional_reading_the_whole_record_is_not_a_cycle() {
        // It reads every *other* key, which is an ordering constraint and not a
        // loop — so `a` runs first and the definition publishes.
        let mut notes = plain("notes");
        notes["conditional"] = json!({"action": "show", "logic": {"!!": {"var": ""}}});

        assert_eq!(ordered(&[notes, plain("a")]), ["a", "notes"]);
    }

    #[test]
    fn reads_missing_as_a_reference_to_the_keys_it_names() {
        let mut total = field("total", json!({"+": [{"var": "amount"}, 1]}));
        total["conditional"] = json!({"action": "hide", "logic": {"missing": ["amount"]}});

        assert_eq!(ordered(&[total, plain("amount")]), ["amount", "total"]);
    }

    // -- The definition gate -----------------------------------------------

    #[test]
    fn refuses_a_cyclic_definition_at_the_definition_level() {
        let details = definition_errors(&definition(vec![
            field("a", json!({"+": [{"var": "b"}, 1]})),
            field("b", json!({"+": [{"var": "a"}, 1]})),
        ]));

        assert_eq!(codes(&details), [CALCULATION_CYCLE]);
        assert!(details[0].message.contains("`a`"));
        assert!(details[0].message.contains("`b`"));
        assert_eq!(details[0].path, "definition.components.0");
    }

    #[test]
    fn finds_a_cycle_inside_a_row_template() {
        // A row is its own record, so its loop is its own loop — and one that a
        // graph over the top-level scope alone would never see.
        let details = definition_errors(&definition(vec![json!({
            "id": "lines", "role": "data", "type": "datagrid", "key": "line_items",
            "label": "Lines", "validation": {"type": "array"},
            "components": [
                field("x", json!({"var": "y"})),
                field("y", json!({"var": "x"})),
            ]
        })]));

        assert_eq!(codes(&details), [CALCULATION_CYCLE]);
        assert_eq!(details[0].path, "definition.components.0.components.0");
    }

    #[test]
    fn a_row_key_does_not_join_a_top_level_cycle_of_the_same_name() {
        // `quantity` at the top level and `quantity` in a row are two fields.
        // One graph over both would put them on the same node and refuse a
        // definition that is fine.
        let details = definition_errors(&definition(vec![
            field("quantity", json!({"+": [{"var": "base"}, 1]})),
            plain("base"),
            json!({
                "id": "lines", "role": "data", "type": "datagrid", "key": "line_items",
                "label": "Lines", "validation": {"type": "array"},
                "components": [plain("quantity"), field("total", json!({"var": "quantity"}))]
            }),
        ]));

        assert!(details.is_empty(), "{details:?}");
    }

    /// **AC1**: the Validation Rule Registry half of the catalogue.
    #[test]
    fn refuses_a_rule_name_no_registry_declares() {
        let mut component = plain("password");
        component["rules"] = json!([{"rule": "looksNice", "scope": "both", "message": "m"}]);

        let details = definition_errors(&definition(vec![component]));

        assert_eq!(codes(&details), [RULE_NOT_REGISTERED]);
        assert_eq!(details[0].path, "definition.components.0.rules.0");
        assert!(details[0].message.contains("looksNice"));
        assert!(details[0].message.contains("password"));
    }

    #[test]
    fn accepts_a_rule_name_the_registry_declares() {
        let mut component = plain("confirm");
        component["rules"] = json!([{"rule": "matchesField", "scope": "both", "params": {"target": "a"},
                    "message": "m"}]);

        assert!(definition_errors(&definition(vec![component, plain("a")])).is_empty());
    }

    /// A `server`-scoped rule this build does not enforce is *registered*, and
    /// the catalogue's question at publish is membership rather than
    /// enforcement. Refusing it here would make a definition unpublishable for
    /// a gap the submit path already reports against the right moment.
    #[test]
    fn a_registered_rule_this_build_does_not_enforce_still_publishes() {
        let mut component = plain("code");
        component["rules"] = json!([{"rule": "unique", "scope": "server", "message": "m"}]);

        assert!(definition_errors(&definition(vec![component])).is_empty());
    }

    /// **AC1**: the Calculation Rule Registry half, reached through the
    /// catalogue rather than through the meta-schema walk — which is what makes
    /// the two halves one gate.
    #[test]
    fn refuses_an_operator_no_registry_declares() {
        let details = definition_errors(&definition(vec![field("a", json!({"flagd": ["x"]}))]));

        assert_eq!(codes(&details), ["OPERATOR_NOT_REGISTERED"]);
        assert!(details[0].message.contains("flagd"));
        assert_eq!(details[0].path, "definition.components.0.calculate");
    }

    #[test]
    fn a_definition_that_is_not_a_document_is_left_to_the_meta_schema() {
        assert!(definition_errors(&json!({"nope": true})).is_empty());
    }
}
