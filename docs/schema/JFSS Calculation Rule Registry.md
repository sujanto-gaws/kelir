# JFSS Calculation Rule Registry
**Version:** 1.4.0
**Status:** Active Standard
**Last updated:** 2026-08-21
**Pairs with:** JFSS v2.0.1
**Maintainers:** Full-Stack Engineering Team

## 1. Purpose & The Polyglot Contract

The JFSS Calculation Rule Registry defines the standardized JSON Logic operators that can be utilized within the `calculate` property of any component.

The registry exists to prevent the "Polyglot Parity Problem" from breaking the tamper-proof calculation pattern. Without it, a frontend developer could use an operator in a calculation that the Vue frontend renders perfectly but the Rust backend cannot recalculate, causing either a 500 error or a silent security bypass where the backend trusts the frontend's calculated value.

### 1.1 The Polyglot Contract
Kelir runs a Vue frontend and a **Rust** backend, so using a calculation operator is a **binding architectural commitment** across exactly two runtimes.

Two, not three: earlier versions of this registry carried a Go column and Go implementations for a backend that does not exist and is not planned. Two runtimes is not a weaker contract than three — the parity problem lives at the language boundary, and there is still a boundary.

Before using an operator in a `calculate` property, the engineering team must ensure:
1. **Frontend Parity:** The operator is supported by `json-logic-js` (Vue), or registered as a custom operator there.
2. **Backend Parity:** The operator is supported by the Rust JSON Logic library, or has been explicitly implemented as a custom operator — and that the library **accepts custom operators at all**, which not every candidate does (Section 4).
3. **Tamper-Proof Guarantee:** The backend can recalculate the exact same expression to overwrite the frontend's submitted value, **and can fail loudly if it cannot** (Section 4).

**If an operator is not in this registry, it is FORBIDDEN from use in the `calculate` property.**

---

## 2. Operator Support Matrix

This matrix defines which JSON Logic operators are approved for use in `calculate` properties across both environments.

### 2.1 Base Operators (Universal Support)

These operators are part of the standard JSON Logic specification (jsonlogic.com) and are implemented by `json-logic-js` on the Vue side.

> **The Rust column was measured, not assumed, by the [operator-parity spike](../../projects/spikes/01.%20JFSS%20Operator%20Parity.md) (#31) on 2026-08-21.** Earlier versions of this table named `json-logic-rs`, which **does not exist on crates.io**; the marks below were never checked against anything. Every ✅ now records a result from `datalogic-rs` 5.2.0, which is the spike's recommendation but **is not yet adopted** — decision **D-10** is open on which evaluator Kelir takes.
>
> The Go column is gone as of v1.4.0 (decision **D-11**). It was never measured, and no Go backend exists.

| Operator | Description | Vue (`json-logic-js`) | Rust (pending **D-10**) |
| :--- | :--- | :---: | :---: |
| `var` | Access data by key (supports dot-notation for arrays) | ✅ | ✅ |
| `+` | Addition | ✅ | ✅ |
| `-` | Subtraction | ✅ | ✅ |
| `*` | Multiplication | ✅ | ✅ |
| `/` | Division | ✅ | ✅ |
| `%` | Modulo | ✅ | ✅ |
| `min` | Minimum of array | ✅ | ✅ |
| `max` | Maximum of array | ✅ | ✅ |
| `map` | Transform each element of an array | ✅ | ✅* |
| `filter` | Filter array elements | ✅ | ✅* |
| `reduce` | Reduce array to single value | ✅ | ✅* |
| `all` | Check if all elements match condition | ✅ | ✅* |
| `some` | Check if any element matches condition | ✅ | ✅* |

`none` is likewise a standard JSON Logic operator and falls in this tier if a use case arises.

**Status:** ✅ **APPROVED FOR USE**

\* **CI Parity Requirement:** `map`, `filter`, `reduce`, `all`, and `some` are standard JSON Logic operators, but the exact behaviour of the Rust library (argument evaluation, lambda scoping, edge cases such as empty arrays) **must be verified in CI parity tests** before an expression using them ships. Vue and Rust must return identical results for identical inputs.

That verification is **done for `datalogic-rs` 5.2.0**: all eleven array cases in the spike corpus — including empty arrays, a missing source array, and `none` — return identical results to `json-logic-js` 2.0.5. The corpus lives in [`spikes/jfss-operator-parity/`](../../spikes/jfss-operator-parity/) and is **not yet a CI gate**; promoting it is Sprint 7 work and depends on D-10.

### 2.2 Extended Operators (Conditional Support)

These operators are **NOT** part of the standard JSON Logic specification. They are extensions added by specific library implementations.

| Operator | Description | Vue (`json-logic-js`) | Rust (pending **D-10**) |
| :--- | :--- | :---: | :---: |
| `sum` | Sum all numbers in an array | ❌ **NOT SUPPORTED** | ❌ **NOT SUPPORTED** |

`sum` is non-standard and requires a custom implementation in **both** environments — including `json-logic-js`, which does not ship it natively.

Confirmed by the spike: neither candidate Rust crate ships `sum`, and a custom implementation is about ten lines. What the spike also found is that **not every library can accept one** — `jsonlogic-rs` has no registration API at all, and silently returns an unregistered operator's expression instead of failing (§4.1).

**Status:** ⚠️ **REQUIRES CUSTOM IMPLEMENTATION** (See Section 4)

### 2.3 Forbidden Operators

These operators are **strictly forbidden** in `calculate` properties because they cannot be safely evaluated on the backend.

| Operator | Reason for Ban |
| :--- | :--- |
| `if` / `?:` | Conditional logic belongs in the `conditional` property, not `calculate`. |
| `==`, `!=`, `>`, `<` | Comparison operators return booleans, not numeric values. |
| `and`, `or`, `!` | Logical operators return booleans, not numeric values. |
| `cat` | String concatenation is not a numeric calculation. |
| `in` | Array membership check returns boolean. |
| `log` | Side-effect operator, not a pure calculation. |

**Status:** ❌ **FORBIDDEN**

### 2.4 Generated (Non-Deterministic) Operators

A fourth tier exists for **non-deterministic, server-side operators**. These are permitted **only** in fields declared with `calculateMode: "generated"` (JFSS v2.0.1, S4.2.3 Case C) and are documented in Section 3.3.

**Status:** 🔒 **SERVER-SIDE ONLY, `calculateMode: "generated"` ONLY**

---

## 3. Approved Operator Catalog

### 3.1 Base Operators (Ready to Use)

#### `var` (Variable Access)
Accesses data from the form payload. Supports dot-notation for nested objects and array indices.

**Syntax:**
```json
{ "var": "key" }
{ "var": "array.0" }
{ "var": "array.0.nestedProperty" }
```

**Example:**
```json
{ "var": "items.0.unit_price" }
```

**Implementation Notes:**
- **Vue:** Native support via `json-logic-js`
- **Rust:** Native support, verified against `datalogic-rs` 5.2.0 (dot-notation, array indices, nested keys, missing keys, and the `{"var": [key, default]}` form)

---

#### `*` (Multiplication)
Multiplies two or more numbers.

**Syntax:**
```json
{ "*": [value1, value2, ...] }
```

**Example (Line Item Total):**
```json
{
  "calculate": {
    "*": [
      { "var": "unit_price" },
      { "var": "quantity" }
    ]
  }
}
```

**Implementation Notes:**
- **All Environments:** Native support
- **Null Safety:** If any operand is `null` or missing, the result is `0` (not `NaN`). This is a JFSS-mandated normalization applied by the wrapper described in Section 7.3, not native library behaviour.

> ⚠️ **This rule is ambiguous, and the ambiguity is load-bearing.** "The result is `0`" can mean *the null operand is treated as `0`* or *the whole expression collapses to `0`*. For `*` the two readings agree, which is why the example above has never exposed the difference — but for `+` they do not: `{"+": [null, 5]}` is `5` under the operand reading and `0` under the expression reading, and the [operator-parity spike](../../projects/spikes/01.%20JFSS%20Operator%20Parity.md) §2.4 measured `json-logic-js` and `datalogic-rs` landing on opposite sides of it. `json-logic-js` is not even self-consistent: it takes the expression reading for `+` and `*` and the operand reading for `-`. **Which reading is normative is unresolved and blocks nothing before the Phase 4 renderer**; it must be settled with D-10, with worked examples for `+`, `-`, `*`, and `/` rather than one for `*`.

---

#### `+` (Addition)
Adds two or more numbers.

**Syntax:**
```json
{ "+": [value1, value2, ...] }
```

**Example (Weighted Sum):**
```json
{
  "calculate": {
    "+": [
      { "*": [{ "var": "a.0" }, 1] },
      { "*": [{ "var": "a.1" }, 2] },
      { "*": [{ "var": "a.2" }, 3] }
    ]
  }
}
```

---

#### `/` (Division)
Divides the first number by the second.

**Syntax:**
```json
{ "/": [numerator, denominator] }
```

**Example (Average):**
```json
{
  "calculate": {
    "/": [
      { "var": "total_score" },
      { "var": "score_count" }
    ]
  }
}
```

> ⚠️ **JS property accessors are FORBIDDEN in JSON Logic paths.** A path such as `{ "var": "scores.length" }` only works by accident in JavaScript (it resolves the `.length` property of the array object) and **breaks polyglot parity** — Rust resolves dot-notation strictly as object keys and array indices, and no `length`/`count` operator is registered. Model counts as explicit data fields (as in `score_count` above) instead.

**Implementation Notes:**
- **Division by Zero:** If the denominator is `0`, the result is `0` (not `Infinity` or `NaN`). This is a **JFSS-mandated normalization**, not native library behaviour: `json-logic-js` natively returns `Infinity` for `x / 0`. Both environments must apply a normalization wrapper — the Vue-side wrapper (Section 7.3) must have an equivalent Rust implementation.

> ⚠️ **Corrected in v1.3.0.** Versions up to 1.2.0 gave that wrapper as `Number(result) || 0` and claimed it implemented this rule. **It does not.** `Number(Infinity) || 0` is `Infinity`, because `Infinity` is truthy — the old wrapper normalized `NaN` and left division by zero untouched, so the field rendered `Infinity` and `JSON.stringify` submitted `null`. Use the finiteness test in Section 7.3. Measured in [operator-parity spike](../../projects/spikes/01.%20JFSS%20Operator%20Parity.md) §2.3.

---

#### `min` / `max` (Minimum / Maximum)
Returns the smallest or largest value from an array or list of arguments.

**Syntax:**
```json
{ "min": [value1, value2, ...] }
{ "max": [value1, value2, ...] }
```

**Example (Discount Cap):**
```json
{
  "calculate": {
    "min": [
      { "*": [{ "var": "subtotal" }, 0.2] },
      100
    ]
  }
}
```

> ⚠️ **A missing operand turns a cap into a zero.** `Math.min(null, 100)` is `0`, so under `json-logic-js` this expression yields `0` — not a capped discount, but no discount — for as long as `subtotal` is empty. The "result is `0`" rule is satisfied and the business answer is wrong. The fix is not a wrapper: a calculation whose inputs are absent should not be evaluated at all, which the renderer's dependency graph (JFSS S12.2, already required for cycle detection) is the right place to enforce. See [operator-parity spike](../../projects/spikes/01.%20JFSS%20Operator%20Parity.md) §2.5.

---

#### `map` (Array Transformation)
Applies an expression to each element of an array, returning a new array.

**Syntax:**
```json
{ "map": [array, expression] }
```

**Example (Calculate Line Totals):**
```json
{
  "calculate": {
    "map": [
      { "var": "items" },
      { "*": [{ "var": "unit_price" }, { "var": "quantity" }] }
    ]
  }
}
```

**How it works:**
1. `{ "var": "items" }` → Fetches the array
2. `{ "map": [...] }` → For each item, calculates `unit_price * quantity`
3. Returns a new array of line totals

**Implementation Notes:**
- **Vue:** Native support via `json-logic-js`
- **Rust:** Standard operator, verified against `datalogic-rs` 5.2.0 — lambda scoping, empty arrays, and a missing source array all match `json-logic-js`
- **Variable Scope:** Inside the `map` expression, `{ "var": "unit_price" }` refers to `items[n].unit_price`, not the root data

---

### 3.2 Extended Operators (Requires Custom Implementation)

#### `sum` (Array Summation)
Sums all numeric values in an array.

**Syntax:**
```json
{ "sum": [array] }
```

**Example (Grand Total):**
```json
{
  "calculate": {
    "sum": [{ "var": "line_totals" }]
  }
}
```

**Implementation Notes:**
- **Vue:** ❌ **NOT SUPPORTED natively** — Must register custom operator via `jsonLogic.add_operation`
- **Rust:** ❌ **NOT SUPPORTED natively** — Must register custom operator (see Section 4.1)
- **Empty Array:** Returns `0` (not `null`)
- **Before choosing a library, check that it can accept a custom operator at all.** `jsonlogic-rs` 0.5.0 cannot, and returns `{"sum": …}` unevaluated rather than failing — see Section 4.1.

---

### 3.3 Generated (Non-Deterministic) Operators

This tier contains operators whose result is **not a pure function of the form payload** — they consult server-side state (sequences, clocks, external services). Because they can never satisfy the recalculate-and-compare parity guarantee, they are governed by strict rules:

1. Generated operators are allowed **only** in fields declared `calculateMode: "generated"` (JFSS v2.0.1, S4.2.3 Case C).
2. They are **server-side only**. The client MUST NOT evaluate them; the Vue renderer treats the field as an empty read-only placeholder until the server populates it.
3. The server evaluates the expression **exactly once**, only when the Existing Payload value is null or absent. A persisted generated value is never recomputed or overwritten.
4. **Generated operators are FORBIDDEN in `derived` calculations.** A `calculateMode: "derived"` (or unspecified) expression containing a generated operator is an invalid schema and MUST be rejected.

#### `generateInvoiceId` (Sequential Document Number)
Generates the next sequential document number for a given numbering rule. The sequence lives in the backend database; concurrency (two submissions racing for the same number) is the server's responsibility.

**Params Schema:**
```json
{ "generateInvoiceId": ["string (numbering-rule identifier, e.g. 'invoice_default')"] }
```

The single argument is a static string naming a server-side numbering rule, which defines the prefix, padding, and reset period (e.g. `INV-2026-00042`).

**Example:**
```json
{
  "id": "field_invoice_number",
  "role": "data",
  "type": "textfield",
  "key": "invoice_number",
  "label": "Invoice #",
  "readOnly": true,
  "validation": { "required": false },
  "calculateMode": "generated",
  "calculate": { "generateInvoiceId": ["invoice_default"] }
}
```

**Implementation Notes:**
- **Vue:** Never evaluated. Renders the persisted value if present, otherwise an empty read-only placeholder.
- **Rust:** Evaluated once at first persistence, inside the same transaction that inserts the document, against an allow-list of known numbering rules. Registering the operator on the **server** engine and not on the client one is what makes rule 2 above enforceable by the engine rather than by convention.

---

## 4. Backend Custom Operator Implementation

Because `sum` is not natively supported by any candidate library, you **must** implement it as a custom operator in **both** environments — `jsonLogic.add_operation` on the Vue side, and the Rust engine's registration API on the backend.

> ⚠️ **Check that the library accepts a custom operator at all, and how it hands over arguments.** The two are separate questions and both have bitten this registry. For `map`-style operators the lambda argument must arrive **unevaluated** (as a raw JSON Logic expression) so it can be applied per-element; a registration API that pre-evaluates every argument silently breaks lambda operators. Confirm both with parity tests before relying on the code below.
>
> Measured for `datalogic-rs` 5.2.0: custom-operator arguments arrive **pre-evaluated**. That is fine for `sum`, which takes a value, and would rule the API out for a custom lambda operator — which is moot, since `map`, `filter`, `reduce`, `all`, `some` and `none` are all built in and verified.

### 4.1 Implementing `sum` in Rust

> **Rewritten in v1.3.0 after the [operator-parity spike](../../projects/spikes/01.%20JFSS%20Operator%20Parity.md) (#31); renumbered from 4.3 in v1.4.0 when the Go sections were removed.** Versions up to 1.2.0 carried an illustrative pseudocode block against a `json_logic_rs::add_operator` API. That API does not exist, and neither does the crate: **`json-logic-rs` is not published to crates.io.** The repository of that name publishes as `jsonlogic-rs`, which has no custom-operator registration at all — its operator table is a compile-time static map.

**Do not use `jsonlogic-rs` 0.5.0.** It does not reject an operator it does not know; it returns the expression object unevaluated, wrapped in `Ok`:

```text
{"sum":[[1,2]]}  ->  Ok(Object {"sum": Array [Array [Number(1), Number(2)]]})
```

That breaks JFSS S8.1.1, which requires a `400 Bad Request` for an unknown operator, and it breaks the Tamper-Proof Pattern in the worst available direction. Driving the Section 6.1 invoice pattern through it, the backend "recalculates" a grand total, gets an object back, launders it to `0` through the Section 7.3 wrapper, and persists `0` in place of `42` — with nothing logged and nothing rejected.

**The working shape**, verified against `datalogic-rs` 5.2.0, is the `CustomOperator` trait. Arguments arrive pre-evaluated; the result is allocated into the evaluation arena:

```rust
use bumpalo::Bump;
use datalogic_rs::{operator::EvalContext, CustomOperator, DataValue, Engine};

struct Sum;

impl CustomOperator for Sum {
    fn evaluate<'a>(
        &self,
        args: &[&'a DataValue<'a>],
        _ctx: &mut EvalContext<'_, 'a>,
        arena: &'a Bump,
    ) -> datalogic_rs::Result<&'a DataValue<'a>> {
        let total = args
            .first()
            .and_then(|value| value.as_array())
            .map(|items| items.iter().filter_map(DataValue::as_f64).sum::<f64>())
            .unwrap_or(0.0);
        Ok(arena.alloc(DataValue::from_f64(total)))
    }
}

let engine = Engine::builder().add_operator("sum", Sum).build();
```

The empty-array case falls out of `unwrap_or(0.0)` and of summing an empty iterator, both of which yield `0` as Section 3.2 requires.

**This is not yet an adoption.** Decision **D-10** is open on which evaluator Kelir takes; the code above records what the spike verified, not a committed dependency. The runnable version is in [`spikes/jfss-operator-parity/`](../../spikes/jfss-operator-parity/).

### 4.2 Implementing `sum` in Vue

The Vue side needs the same operator and, critically, the **same edge-case behaviour** — an empty array yields `0`, and a non-numeric element contributes `0` rather than poisoning the sum with `NaN`:

```typescript
import jsonLogic from 'json-logic-js';

jsonLogic.add_operation('sum', (arr: unknown) =>
  Array.isArray(arr) ? arr.reduce<number>((total, v) => total + (Number(v) || 0), 0) : 0,
);
```

The two implementations above are the ones the spike ran against each other; they agree on the corpus. Writing them independently is exactly the risk this registry exists to manage, which is why the parity corpus is the acceptance test for a custom operator and not the code review.

### 4.3 Do Not Reimplement Standard Operators

`map`, `filter`, `reduce`, `all`, `some` and `none` are **standard** JSON Logic operators. Use the library's built-in implementation on both sides. A hand-written replacement is a second place for the two runtimes to disagree, and the spike verified the built-ins agree on lambda scoping, empty arrays, and a missing source array.

If a parity test ever shows a built-in diverging, the fix is a registry entry recording the divergence and a decision — not a silent local override.

---

## 5. Extending the Registry (Standard Operating Procedure)

If a developer needs to use a new calculation operator (e.g., `average`, `count`, `round`), they must follow this workflow:

1. **Check the Matrix:** Verify the operator is not already in Section 2.
2. **Verify Library Support:** Check whether `json-logic-js` and the Rust library both support it natively.
3. **If Not Supported:**
   - Implement the operator as a custom operator in Rust (see Section 4.1)
   - Implement the operator as a custom operator in Vue (see Section 4.2)
   - Test parity: add the operator to the parity corpus and confirm Vue and Rust return identical results for the same input, edge cases included
4. **Update this Registry:** Add the operator to the appropriate section (2.1, 2.2, 2.3, or 2.4)
5. **Document Examples:** Add usage examples and implementation notes
6. **Code Review:** The PR must be reviewed by at least one frontend and one backend engineer to ensure parity

---

## 6. Common Calculation Patterns

### 6.1 Invoice Pattern (Line Items → Grand Total)

**Schema:**
```json
{
  "components": [
    {
      "id": "field_items",
      "role": "data",
      "type": "datagrid",
      "key": "items",
      "label": "Line Items",
      "validation": { "required": true },
      "components": [
        {
          "id": "field_unit_price",
          "role": "data",
          "type": "number",
          "key": "unit_price",
          "label": "Unit Price",
          "validation": { "required": true }
        },
        {
          "id": "field_quantity",
          "role": "data",
          "type": "number",
          "key": "quantity",
          "label": "Quantity",
          "validation": { "required": true }
        }
      ]
    },
    {
      "id": "field_grand_total",
      "role": "data",
      "type": "number",
      "key": "grand_total",
      "label": "Grand Total",
      "readOnly": true,
      "validation": { "required": false },
      "calculate": {
        "sum": [
          {
            "map": [
              { "var": "items" },
              { "*": [{ "var": "unit_price" }, { "var": "quantity" }] }
            ]
          }
        ]
      }
    }
  ]
}
```

**How it works:**
1. User adds items to the `items` array
2. For each item, Vue calculates `unit_price * quantity`
3. Vue sums all line totals and displays `grand_total`
4. On submission, the Rust backend recalculates the exact same expression
5. The backend overwrites the `grand_total` in the payload with the secure calculation
6. Database stores the tamper-proof value

---

### 6.2 Discount Pattern (Percentage with Cap)

**Schema:**
```json
{
  "calculate": {
    "min": [
      { "*": [{ "var": "subtotal" }, { "var": "discount_percent" }, 0.01] },
      100
    ]
  }
}
```

**How it works:**
- Calculates `subtotal * discount_percent * 0.01` (e.g., $1000 * 20% = $200)
- Caps the discount at $100 maximum
- Result: `min(200, 100) = 100`

---

## 7. Security Boundaries

### 7.1 Never Trust Frontend Calculations
The `calculate` property is evaluated on the frontend for UX, but the backend **must** recalculate it. A malicious user can:
- Modify the JSON schema in their browser
- Submit a tampered `grand_total` value
- Bypass frontend validation

**The backend is the source of truth.** Always overwrite calculated values before database insertion.

### 7.2 Prevent Circular Dependencies
Field A cannot calculate from Field B if Field B calculates from Field A. This creates an infinite loop.

**CI/CD Enforcement:** Add a script to your pipeline that:
1. Parses all `calculate` properties
2. Builds a dependency graph
3. Fails the build if a cycle is detected

### 7.3 Handle Missing Data Gracefully
If `unit_price` is `null` while the user is typing `quantity`, the calculation `null * 5` should return `0`, not `NaN` or crash. Likewise, division by zero must yield `0` (Section 3.1).

This "result is 0" rule is a **JFSS-mandated normalization, not native library behaviour** — `json-logic-js` natively returns `Infinity` for `x / 0`, and other non-finite or non-numeric results are possible. Both environments must therefore wrap raw evaluation results in an identical normalization step. The Vue wrapper below **must have an equivalent Rust implementation**, or parity breaks on the edge cases.

**Implementation (Vue):**
```typescript
// Vue: useCalculator.ts
const result = jsonLogic.apply(component.calculate, newData);
const value = Number(result);
formData.value[component.key] = Number.isFinite(value) ? value : 0;
```

> ⚠️ **Corrected in v1.3.0.** Versions up to 1.2.0 gave this wrapper as `Number(result) || 0`, which **does not normalize division by zero**: `Infinity` is truthy, so `Number(Infinity) || 0` is `Infinity`. Only `NaN` was ever normalized. The field then rendered `Infinity` and `JSON.stringify` submitted `null`, so the backend received a null for a field the user saw as a number. The finiteness test above is the predicate every environment must use — and note that `0` for a divide-by-zero is a **display and storage** decision, not a claim that the calculation was meaningful. Measured in [operator-parity spike](../../projects/spikes/01.%20JFSS%20Operator%20Parity.md) §2.3.

---

## 8. Critical Reminders

1. **The Registry is the Contract:** If an operator is not in this registry, do not use it. If you need a new operator, follow the SOP in Section 5.

2. **Test Parity Religiously:** Before deploying a calculation, test it in Vue and Rust with the same input data. Both must return identical results, and the edge cases are where they will not — empty arrays, null operands, division by zero.

3. **Prefer Base Operators:** Use `+`, `-`, `*`, `/` whenever possible. They are universally supported and require no custom implementation.

4. **Document Your Choices:** If you implement a custom operator, document it in this registry with examples and implementation notes for both environments.

5. **The Backend Overwrites:** Never, ever trust the frontend's calculated value. The backend must recalculate and overwrite before saving to the database. This is only safe if the recalculation can **fail loudly** — an evaluator that returns an unknown operator instead of rejecting it turns "overwrite" into silent corruption (Section 4.3).

6. **This registry governs `calculate` only.** `conditional.logic` (JFSS §7.1) is also JSON Logic, is also re-evaluated server-side before persistence (JFSS S10.2), and needs precisely the comparison and boolean operators Section 2.3 forbids here — so the operator surface a backend must implement is larger than the matrix above, and **no document currently bounds it**. The [operator-parity spike](../../projects/spikes/01.%20JFSS%20Operator%20Parity.md) §2.6 found all fourteen conditional cases agreeing across every evaluator tested, so this is a documentation gap rather than a live defect. A conditional tier is owed before the Phase 4 renderer.

---

## 9. Changelog

- **1.4.0 (2026-08-21):** **Removed Go.** Decision **D-11** settles what the [operator-parity spike](../../projects/spikes/01.%20JFSS%20Operator%20Parity.md) §2.8 raised: Kelir's backend is Rust, no Go service exists or is planned, and this registry had been carrying a Go column and two Go implementations for it. Dropped the Go column from the Section 2.1 and 2.2 matrices, deleted the Go `sum` and `map` sections (old 4.1 and 4.2), renumbered the Rust `sum` section from 4.3 to **4.1**, and added **4.2** (the Vue `sum` it must agree with, which was previously only implied) and **4.3** (do not reimplement standard operators). Restated the Section 1.1 polyglot contract, the Section 5 extension SOP, and Sections 6.1, 7.3 and 8 for two runtimes. **This is not a relaxation:** the parity problem lives at the language boundary, and one JavaScript runtime plus one Rust runtime still has one. Nothing about the approved operator set, the mandated normalizations, or the Tamper-Proof Pattern changes.
- **1.3.0 (2026-08-21):** Applied the findings of the [JFSS operator-parity spike](../../projects/spikes/01.%20JFSS%20Operator%20Parity.md) (#31). **Corrections to statements the spike disproved:** the Rust column named `json-logic-rs`, which is not a crates.io package, and its ✅ marks had never been checked — the column is now headed "pending **D-10**" and every mark records a measured `datalogic-rs` 5.2.0 result; Section 4.3's illustrative pseudocode targeted an API that does not exist and is replaced with the verified `CustomOperator` shape plus the reason `jsonlogic-rs` is disqualified (it returns unknown operators instead of rejecting them, inverting the Tamper-Proof Pattern); the Section 7.3 wrapper `Number(result) || 0` did not implement the Section 3.1 division-by-zero rule it was cited for and is replaced with a finiteness test. **Gaps flagged, not yet resolved:** the "result is 0" rule is ambiguous between the operand and whole-expression readings, which `json-logic-js` and `datalogic-rs` answer differently for `+` (Section 3.1); `min`/`max` with a missing operand turns the Section 6.2 discount cap into a zero; this registry governs `calculate` only, while `conditional.logic` needs the operators Section 2.3 forbids and is bounded by nothing (Section 8). Recorded the Section 2.1 CI parity requirement as **satisfied for Rust** by the spike corpus, which is not yet a CI gate.
- **1.2.0 (2026-08-05):** Reclassified the standard JSON Logic array operators (`map`, `filter`, `reduce`, `all`, `some`) from Extended to Base, subject to CI parity verification for Go/Rust; added the Generated (Non-Deterministic) Operators tier (Sections 2.4, 3.3) with `generateInvoiceId` as its first registered operator; fixed the Section 3.1 average example (removed the JS-only `.length` accessor) and added the property-accessor prohibition; documented the division-by-zero and null-operand "result is 0" rule as a JFSS-mandated normalization requiring wrappers in every environment; marked the Rust sample as illustrative pseudocode and added custom-operator semantics caveats; added `validation` objects to the Section 6.1 example; editorial cleanup of conversational framing.
- **1.1.0:** Changes not recorded.
- **1.0.0:** Initial release.
