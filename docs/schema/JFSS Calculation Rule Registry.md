# JFSS Calculation Rule Registry
**Version:** 1.2.0
**Status:** Active Standard
**Last updated:** 2026-08-05
**Pairs with:** JFSS v2.0.1
**Maintainers:** Full-Stack Engineering Team

## 1. Purpose & The Polyglot Contract

The JFSS Calculation Rule Registry defines the standardized JSON Logic operators that can be utilized within the `calculate` property of any component.

The registry exists to prevent the "Polyglot Parity Problem" from breaking the tamper-proof calculation pattern. Without it, a frontend developer could use an operator in a calculation that the Vue frontend renders perfectly but the Go/Rust backend cannot recalculate, causing either a 500 error or a silent security bypass where the backend trusts the frontend's calculated value.

### 1.1 The Polyglot Contract
Because this application utilizes a Vue frontend and Golang/Rust backends, using a calculation operator is a **binding architectural commitment**.

Before using an operator in a `calculate` property, the engineering team must ensure:
1. **Frontend Parity:** The operator is supported by `json-logic-js` (Vue).
2. **Backend Parity:** The operator is supported by the Go/Rust JSON Logic library OR has been explicitly implemented as a custom operator.
3. **Tamper-Proof Guarantee:** The backend can recalculate the exact same expression to overwrite the frontend's submitted value.

**If an operator is not in this registry, it is FORBIDDEN from use in the `calculate` property.**

---

## 2. Operator Support Matrix

This matrix defines which JSON Logic operators are approved for use in `calculate` properties across all three environments.

### 2.1 Base Operators (Universal Support)

These operators are part of the standard JSON Logic specification (jsonlogic.com) and are implemented by `json-logic-js` and by `diegoholiveira/jsonlogic` v3.

| Operator | Description | Vue (`json-logic-js`) | Go (`diegoholiveira/jsonlogic`) | Rust (`json-logic-rs`) |
| :--- | :--- | :---: | :---: | :---: |
| `var` | Access data by key (supports dot-notation for arrays) | ✅ | ✅ | ✅ |
| `+` | Addition | ✅ | ✅ | ✅ |
| `-` | Subtraction | ✅ | ✅ | ✅ |
| `*` | Multiplication | ✅ | ✅ | ✅ |
| `/` | Division | ✅ | ✅ | ✅ |
| `%` | Modulo | ✅ | ✅ | ✅ |
| `min` | Minimum of array | ✅ | ✅ | ✅ |
| `max` | Maximum of array | ✅ | ✅ | ✅ |
| `map` | Transform each element of an array | ✅ | ✅* | ✅* |
| `filter` | Filter array elements | ✅ | ✅* | ✅* |
| `reduce` | Reduce array to single value | ✅ | ✅* | ✅* |
| `all` | Check if all elements match condition | ✅ | ✅* | ✅* |
| `some` | Check if any element matches condition | ✅ | ✅* | ✅* |

`none` is likewise a standard JSON Logic operator and falls in this tier if a use case arises.

**Status:** ✅ **APPROVED FOR USE**

\* **CI Parity Requirement:** `map`, `filter`, `reduce`, `all`, and `some` are standard JSON Logic operators, but the exact behaviour of the Go and Rust libraries (argument evaluation, lambda scoping, edge cases such as empty arrays) **must be verified in CI parity tests** before an expression using them ships. Vue, Go, and Rust must return identical results for identical inputs.

### 2.2 Extended Operators (Conditional Support)

These operators are **NOT** part of the standard JSON Logic specification. They are extensions added by specific library implementations.

| Operator | Description | Vue (`json-logic-js`) | Go (`diegoholiveira/jsonlogic`) | Rust (`json-logic-rs`) |
| :--- | :--- | :---: | :---: | :---: |
| `sum` | Sum all numbers in an array | ❌ **NOT SUPPORTED** | ❌ **NOT SUPPORTED** | ❌ **NOT SUPPORTED** |

`sum` is non-standard and requires a custom implementation in **all three** environments (including `json-logic-js`, which does not ship it natively).

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
- **Go:** Native support via `diegoholiveira/jsonlogic`
- **Rust:** Native support via `json-logic-rs`

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

> ⚠️ **JS property accessors are FORBIDDEN in JSON Logic paths.** A path such as `{ "var": "scores.length" }` only works by accident in JavaScript (it resolves the `.length` property of the array object) and **breaks polyglot parity** — Go and Rust resolve dot-notation strictly as object keys and array indices, and no `length`/`count` operator is registered. Model counts as explicit data fields (as in `score_count` above) instead.

**Implementation Notes:**
- **Division by Zero:** If the denominator is `0`, the result is `0` (not `Infinity` or `NaN`). This is a **JFSS-mandated normalization**, not native library behaviour: `json-logic-js` natively returns `Infinity` for `x / 0`. Every environment must apply a normalization wrapper — the Vue-side `Number(result) || 0` wrapper (Section 7.3) must have equivalent Go and Rust implementations.

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
- **Go:** Standard operator, implemented by `diegoholiveira/jsonlogic` v3 — verify behaviour in CI parity tests (Section 2.1)
- **Rust:** Standard operator — verify library support in CI parity tests (Section 2.1)
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
- **Go:** ❌ **NOT SUPPORTED natively** — Must register custom operator (see Section 4.1)
- **Rust:** ❌ **NOT SUPPORTED natively** — Must register custom operator (see Section 4.3)
- **Empty Array:** Returns `0` (not `null`)

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
- **Go/Rust:** Evaluated once at first persistence, inside the same transaction that inserts the document, against an allow-list of known numbering rules.

---

## 4. Backend Custom Operator Implementation

Because `sum` is not natively supported by any of the three libraries, you **must** implement it as a custom operator in each environment. The `map` implementation below is retained as a reference/fallback, but see the note in Section 4.2.

> ⚠️ **Library-version caveat:** The argument-evaluation and scoping semantics of custom operators **must be verified against the library version in use**. In particular, for `map`-style operators the lambda argument must arrive **unevaluated** (as a raw JSON Logic expression) so it can be applied per-element; some registration APIs pre-evaluate all arguments, which silently breaks lambda operators. Confirm this behaviour with parity tests before relying on the code below.

### 4.1 Implementing `sum` in Go

```go
package jsonlogic_ext

import (
    "github.com/diegoholiveira/jsonlogic/v3"
)

func init() {
    jsonlogic.AddOperator("sum", func(values interface{}, data interface{}) interface{} {
        // values is an array containing one element: the array to sum
        valuesList, ok := values.([]interface{})
        if !ok || len(valuesList) == 0 {
            return float64(0)
        }
        
        // Extract the actual array
        arr, ok := valuesList[0].([]interface{})
        if !ok {
            return float64(0)
        }
        
        // Sum all numeric values
        var sum float64
        for _, v := range arr {
            switch val := v.(type) {
            case float64:
                sum += val
            case int:
                sum += float64(val)
            case int64:
                sum += float64(val)
            }
        }
        
        return sum
    })
}
```

### 4.2 Implementing `map` in Go (Fallback Only)

`map` is a **standard** JSON Logic operator implemented natively by `diegoholiveira/jsonlogic` v3 — prefer the library's built-in and use the code below only if CI parity tests reveal a divergence that must be papered over.

```go
package jsonlogic_ext

import (
    "bytes"
    "encoding/json"
    "github.com/diegoholiveira/jsonlogic/v3"
)

func init() {
    jsonlogic.AddOperator("map", func(values interface{}, data interface{}) interface{} {
        valuesList, ok := values.([]interface{})
        if !ok || len(valuesList) != 2 {
            return []interface{}{}
        }
        
        // Extract array and expression
        arr, ok := valuesList[0].([]interface{})
        if !ok {
            return []interface{}{}
        }
        
        expression := valuesList[1]
        
        // Apply expression to each element
        var result []interface{}
        for _, item := range arr {
            // Create a temporary data context with the current item
            tempData := map[string]interface{}{
                "item": item,
            }
            
            // Merge item properties into tempData for direct access
            if itemMap, ok := item.(map[string]interface{}); ok {
                for k, v := range itemMap {
                    tempData[k] = v
                }
            }
            
            // Apply the expression
            var buf bytes.Buffer
            exprJSON, _ := json.Marshal(expression)
            dataJSON, _ := json.Marshal(tempData)
            
            err := jsonlogic.Apply(
                bytes.NewBuffer(exprJSON),
                bytes.NewBuffer(dataJSON),
                &buf,
            )
            
            if err == nil {
                var val interface{}
                json.Unmarshal(buf.Bytes(), &val)
                result = append(result, val)
            }
        }
        
        return result
    })
}
```

### 4.3 Implementing `sum` in Rust

> ⚠️ **ILLUSTRATIVE PSEUDOCODE.** The block below sketches the intent only — the `json-logic-rs` crate's actual custom-operator API differs from what is shown here. The real registration API (types, error handling, evaluation hooks) must be confirmed against the crate version during implementation.

```rust
// ILLUSTRATIVE PSEUDOCODE — not the real json-logic-rs API.
use json_logic_rs::{Value, LogicError};

pub fn register_custom_operators() {
    // Register 'sum' operator
    json_logic_rs::add_operator("sum", |values: &[Value], _data: &Value| -> Result<Value, LogicError> {
        if values.is_empty() {
            return Ok(Value::Number(0.0));
        }
        
        if let Value::Array(arr) = &values[0] {
            let sum: f64 = arr.iter().filter_map(|v| {
                if let Value::Number(n) = v {
                    Some(*n)
                } else {
                    None
                }
            }).sum();
            
            Ok(Value::Number(sum))
        } else {
            Ok(Value::Number(0.0))
        }
    });
}
```

`map` is a standard JSON Logic operator: use the Rust library's built-in implementation if available rather than registering a custom one, and fall back to a custom operator only if parity tests show the built-in diverges.

---

## 5. Extending the Registry (Standard Operating Procedure)

If a developer needs to use a new calculation operator (e.g., `average`, `count`, `round`), they must follow this workflow:

1. **Check the Matrix:** Verify the operator is not already in Section 2.
2. **Verify Library Support:** Check if `json-logic-js`, the Go library, and the Rust library all support it natively.
3. **If Not Supported:**
   - Implement the operator as a custom operator in Go (see Section 4)
   - Implement the operator as a custom operator in Rust
   - Test parity: Ensure Vue, Go, and Rust all return identical results for the same input
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
4. On submission, Go backend recalculates the exact same expression
5. Go overwrites the `grand_total` in the payload with the secure calculation
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

This "result is 0" rule is a **JFSS-mandated normalization, not native library behaviour** — `json-logic-js` natively returns `Infinity` for `x / 0`, and other non-finite or non-numeric results are possible. Every environment must therefore wrap raw evaluation results in an identical normalization step. The Vue wrapper below **must have equivalent Go and Rust implementations**, or parity breaks on the edge cases.

**Implementation (Vue):**
```typescript
// Vue: useCalculator.ts
const result = jsonLogic.apply(component.calculate, newData);
formData.value[component.key] = Number(result) || 0;
```

---

## 8. Critical Reminders

1. **The Registry is the Contract:** If an operator is not in this registry, do not use it. If you need a new operator, follow the SOP in Section 5.

2. **Test Parity Religiously:** Before deploying a calculation, test it in Vue, Go, and Rust with the same input data. All three must return identical results.

3. **Prefer Base Operators:** Use `+`, `-`, `*`, `/` whenever possible. They are universally supported and require no custom implementation.

4. **Document Your Choices:** If you implement a custom operator, document it in this registry with examples and implementation notes for all three environments.

5. **The Backend Overwrites:** Never, ever trust the frontend's calculated value. The backend must recalculate and overwrite before saving to the database.

---

## 9. Changelog

- **1.2.0 (2026-08-05):** Reclassified the standard JSON Logic array operators (`map`, `filter`, `reduce`, `all`, `some`) from Extended to Base, subject to CI parity verification for Go/Rust; added the Generated (Non-Deterministic) Operators tier (Sections 2.4, 3.3) with `generateInvoiceId` as its first registered operator; fixed the Section 3.1 average example (removed the JS-only `.length` accessor) and added the property-accessor prohibition; documented the division-by-zero and null-operand "result is 0" rule as a JFSS-mandated normalization requiring wrappers in every environment; marked the Rust sample as illustrative pseudocode and added custom-operator semantics caveats; added `validation` objects to the Section 6.1 example; editorial cleanup of conversational framing.
- **1.1.0:** Changes not recorded.
- **1.0.0:** Initial release.
