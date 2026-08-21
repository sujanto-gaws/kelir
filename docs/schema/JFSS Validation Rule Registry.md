# JFSS Validation Rule Registry
**Version:** 1.2.0  
**Status:** Active Standard  
**Last updated:** 2026-08-21  
**Pairs with:** JFSS v2.0.1  
**Maintainers:** Full-Stack Engineering Team

## 1. Purpose & The Polyglot Contract
The JFSS Validation Rule Registry defines the standardized, advanced validation rules that can be utilized within the `rules` array of any `role: "data"` component. 

### 1.1 The Polyglot Contract
Because this application utilizes a Vue frontend and Golang/Rust backends, adding a new rule to this registry is a **binding architectural commitment**. 
Before adding a new rule to this document, the engineering team must ensure:
1. **Frontend Parity:** The rule can be evaluated using Vue/Zod/Yup.
2. **Backend Parity:** The rule can be evaluated natively in Golang and Rust without relying on external JavaScript engines.
3. **Security Boundary:** The rule's `scope` correctly reflects whether it is a UX enhancement (`client`), a strict security boundary (`server`), or a shared data-integrity check (`both`).

---

## 2. Rule Anatomy
Every rule in the schema must conform to this structure:
```json
{
  "rule": "string (Must match an identifier in this registry)",
  "scope": "client | server | both",
  "params": { /* Object defined by the specific rule below */ },
  "message": "string (Displayed to user on failure)"
}
```

---

## 3. Standard Rule Catalog

### 3.1 Scope: `both` (Shared Data Integrity)
*These rules enforce fundamental data relationships. They are evaluated in real-time by the frontend for immediate UX feedback, and strictly re-evaluated by the backend upon submission to prevent tampering.*

#### `matchesField`
Ensures the current field's value exactly matches the value of another data component.
* **Use Case:** Password confirmation, email confirmation.
* **Params Schema:**
  ```json
  { "target": "string (The `key` of the target data component)" }
  ```
* **Example:**
  ```json
  { "rule": "matchesField", "scope": "both", "params": { "target": "password" }, "message": "Passwords do not match." }
  ```
* **Implementation Notes:**
  * **Vue:** Use Zod's `superRefine` or Yup's `oneOf([Yup.ref('target')])` to access the global form context.
  * **Go/Rust:** Compare `payload[currentKey] == payload[params.target]`.

#### `notMatchesField`
Ensures the current field's value does *not* match another field.
* **Use Case:** Ensuring a new password is different from the old password.
* **Params Schema:** `{ "target": "string" }`
* **Example:**
  ```json
  { "rule": "notMatchesField", "scope": "both", "params": { "target": "old_password" }, "message": "New password must be different from the old password." }
  ```

#### `regex`
Applies a custom regular expression. (Use this when the base `validation.pattern` is insufficient, or to provide a highly specific, user-friendly error message for a complex pattern).
* **Use Case:** Complex string formatting (e.g., specific ID formats).
* **Params Schema:**
  ```json
  { "pattern": "string (ECMA 262 regex)", "flags": "string (e.g., 'i', 'g')" }
  ```
* **Implementation Notes:**
  * **Vue:** `new RegExp(params.pattern, params.flags).test(value)`
  * **Go:** `regexp.MustCompile(params.Pattern).MatchString(value)` *(Note: Go does not support all JS regex flags; ensure the pattern is cross-compatible).*
  * **Rust:** `regex::Regex::new(&params.pattern)?.is_match(value)` — **but see the warning below. The Rust `regex` crate cannot honour the full ECMA-262 params schema, and the divergences are not all loud.**

> ⚠️ **"ECMA 262 regex" is not a cross-language contract.** This rule is scoped `both`, so the frontend and the backend each decide it. Measured by the [operator-parity spike](../../projects/spikes/01.%20JFSS%20Operator%20Parity.md) §2.7 on 2026-08-21:
>
> | Pattern | ECMA-262 | Rust `regex` 1.x |
> |---|---|---|
> | `^[A-Z]{3}-\d{4}$` | matches | matches |
> | `(?i)^abc$` | matches | matches |
> | `^(?=.*[A-Z])(?=.*\d).{8,}$` — password complexity | matches | **refuses to compile** |
> | `^(\w+)-\1$` — backreference | matches | **refuses to compile** |
> | `^\d+$` against `٣٤٥` | **false** | **true** |
>
> Lookahead and backreferences fail loudly: the Rust `regex` crate rejects them by design, and a password-complexity pattern — the commonest custom `regex` rule there is — cannot be compiled at all. The character-class divergence fails **silently**: ECMA-262 `\d` is ASCII-only, Rust's `\d` is Unicode `Nd`, so the same rule rejects Arabic-Indic digits in the browser and accepts them on the server with no error on either side.
>
> Until this is resolved, prefer `validation.pattern` with a plainly ASCII, non-lookahead pattern, and pin digit classes explicitly (`[0-9]`, not `\d`). Two resolutions are open: constrain this rule's params schema to a cross-compatible subset and validate it when the schema is saved, or adopt a backtracking engine (`fancy-regex`) on the backend — which restores lookahead and backreferences but leaves the `\d` divergence needing an explicit pin either way.

#### `oneOf`
Ensures the value is strictly within a provided array.
* **Use Case:** Restricting input to a dynamic list of allowed codes.
* **Params Schema:** `{ "values": ["array", "of", "allowed", "values"] }`
* **Example:**
  ```json
  { "rule": "oneOf", "scope": "both", "params": { "values": ["standard", "express", "overnight"] }, "message": "Please select a valid shipping method." }
  ```

#### `notOneOf`
Ensures the value is strictly excluded from a provided array.
* **Use Case:** Blocking reserved or disallowed values (e.g., reserved usernames).
* **Params Schema:** `{ "values": ["array", "of", "disallowed", "values"] }`
* **Example:**
  ```json
  { "rule": "notOneOf", "scope": "both", "params": { "values": ["admin", "root", "system"] }, "message": "This username is reserved." }
  ```

**`oneOf`/`notOneOf` vs. `validation.enum`:** Use `validation.enum` for static value sets that are baked into the schema itself — the meta-schema can then validate them, and a `select` can auto-generate its `options` from them. Use the `oneOf` rule when the allowed set is resolved at validation time (e.g., from configuration or a lookup) or when you need a custom failure `message` or an explicit `scope`.

---

### 3.2 Scope: `client` (UX Enhancements)
*These rules are evaluated exclusively by the Vue frontend to improve the user experience. The backend completely ignores these rules during payload validation.*

#### `passwordStrength`
Evaluates the complexity of a password for a visual strength meter.
* **Use Case:** Real-time visual feedback (e.g., red/yellow/green bar) as the user types.
* **Params Schema:**
  ```json
  { "minScore": "integer (1-4)" }
  ```
* **Implementation Notes:**
  * **Vue:** Use a library like `zxcvbn` to calculate the score. If `score < params.minScore`, trigger the error message.
  * **Go/Rust:** Ignored. The backend relies on the base `validation.minLength` and `validation.pattern` for actual password security.

#### `async`
Triggers a debounced, read-only API call to provide real-time UX feedback.
* **Use Case:** Checking username availability or validating a promo code format while the user is typing.
* **Params Schema:**
  ```json
  { 
    "endpoint": "string (Relative API path)", 
    "method": "string (GET or POST)", 
    "debounce": "integer (milliseconds)" 
  }
  ```
* **Security Warning:** The frontend must only call endpoints explicitly allow-listed in the Vue router/API client. The backend must treat this endpoint as strictly read-only and rate-limited.
* **Request/Response Contract:** The endpoint is called via `POST` with the body:
  ```json
  { "key": "username", "value": "current field value", "formId": "user_registration_v1" }
  ```
  and must respond with:
  ```json
  { "valid": true }
  ```
  or, on failure, `{ "valid": false, "message": "Optional override for the rule's message" }`.
* **Implementation Notes:**
  * **Vue:** Wrap the fetch call in a Zod `refine` or Yup `test` that returns a Promise. Apply the debounce at the component level. Note that Zod integration requires the async parse path — `parseAsync`/`safeParseAsync` — because a synchronous `parse` throws on async refinements.

---

### 3.3 Scope: `server` (Security & Business Logic)
*These rules contain sensitive business logic or require database access. They are evaluated exclusively by the Golang/Rust backend. The frontend will only display the error message if the backend returns a `400 Bad Request` upon form submission.*

#### `unique`
Verifies that the submitted value does not already exist in a specific database table/column.
* **Use Case:** Ensuring usernames, email addresses, or slug identifiers are globally unique.
* **Params Schema:**
  ```json
  { "table": "string", "column": "string", "ignoreId": "string (Optional, for edit forms)" }
  ```
* **Example:**
  ```json
  { "rule": "unique", "scope": "server", "params": { "table": "users", "column": "email" }, "message": "This email is already registered." }
  ```
* **Implementation Notes:**
  * **Go/Rust:** Execute a parameterized `SELECT COUNT(*)` query. *Never* concatenate the `table` or `column` strings directly into the SQL query to prevent SQL injection; map them to an allow-list of known tables/columns first.

#### `exists` (Foreign Key Validation)
Verifies that the submitted value corresponds to a valid primary key in a related database table.
* **Use Case:** Ensuring a submitted `department_id` or `category_id` actually exists in the database.
* **Params Schema:** `{ "table": "string", "column": "string" }`
* **Example:**
  ```json
  { "rule": "exists", "scope": "server", "params": { "table": "departments", "column": "id" }, "message": "The selected department does not exist." }
  ```
* **Implementation Notes:**
  * **Go/Rust:** Execute a parameterized `SELECT COUNT(*)` query. *Never* concatenate the `table` or `column` strings directly into the SQL query to prevent SQL injection; map them to an allow-list of known tables/columns first.

#### `authorized` (RBAC / Permission Check)
Verifies that the currently authenticated user session has the required permissions to submit the specific value for this field.
* **Use Case:** Preventing a standard user from tampering with the payload to set `role: "admin"`.
* **Params Schema:**
  ```json
  { "requiredPermission": "string", "allowedValues": ["array", "of", "values"] }
  ```
* **Implementation Notes:**
  * **Go/Rust:** Extract the user's role/permissions from the JWT/Session context. If `payload[currentKey]` is not in `allowedValues` (or if the user lacks `requiredPermission`), reject the payload.

---

## 4. Extending the Registry (Standard Operating Procedure)

If a developer needs to introduce a new validation rule (e.g., `validateCryptoAddress`), they must follow this workflow:

1. **Draft the Rule:** Define the `rule` name, `scope`, and `params` schema.
2. **Update this Registry:** Add the rule to the appropriate scope section in this document.
3. **Implement in Vue:** Add the logic to the `zodBuilder.ts` (or equivalent) switch statement.
4. **Implement in Go/Rust:** Add the logic to the backend `EvaluateServerRules` (or equivalent) switch statement.
5. **Update Meta-Schema (Optional):** If the rule requires strict parameter validation, update the `advancedRule` definition in `jfss-meta-v2.0.1.json` to include an `if/then` block for the new `rule` string.
6. **Code Review:** The PR must be reviewed by at least one frontend and one backend engineer to ensure parity.

---

## 5. Error Code Mapping (Backend to Frontend)

When the backend rejects a payload due to a `server` or `both` scoped rule, it must return a standardized error response so the Vue frontend can map the error back to the correct field. This contract is defined normatively in JFSS v2.0.1, Section 10.3; the summary below must not diverge from it.

Each entry in `details` carries a `path` (a **Dot-Notation Path** — for array rows this is not a bare `key`, e.g. `line_items.2.product_sku`), the `rule` that failed, a stable machine-readable `code`, and a server-rendered fallback `message`.

**Standard Backend Error Response:**
```json
{
  "status": 400,
  "error": "VALIDATION_FAILED",
  "details": [
    {
      "path": "email",
      "rule": "unique",
      "code": "DUPLICATE_VALUE",
      "message": "This email is already registered."
    },
    {
      "path": "line_items.2.product_sku",
      "rule": "unique",
      "code": "DUPLICATE_VALUE",
      "message": "Duplicate SKU."
    }
  ]
}
```

**Frontend Handling:**
The Vue submission handler must catch the `400` response, iterate through the `details` array, and inject the `message` into the reactive `errors` state by resolving the `path` to the exact field (including the array row), triggering the UI to display the error beneath the correct field.

---

## 6. Changelog

- **1.2.0 (2026-08-21):** Recorded the [operator-parity spike](../../projects/spikes/01.%20JFSS%20Operator%20Parity.md) (#31) finding against the `regex` rule: the "ECMA 262 regex" params schema is not honourable by the Rust `regex` crate — lookahead and backreferences are rejected at compile time, and `\d` diverges silently between the ASCII ECMA-262 class and Rust's Unicode `Nd`, which for a `scope: "both"` rule means the two sides reach opposite verdicts on the same input. Added Rust implementation notes and interim guidance; the two candidate resolutions are open.
- **1.1.0 (2026-08-05):** Aligned the Section 5 error-response contract with JFSS v2.0.1 Section 10.3 (`path` with dot-notation, plus `rule`, `code`, `message`); added the document header and title; added examples for `notMatchesField`, `oneOf`, `notOneOf`, and `exists` (with the SQL-injection allow-list warning); split `oneOf`/`notOneOf` into separate entries; clarified `oneOf`/`notOneOf` vs. `validation.enum`; defined the `async` rule's request/response contract and Zod async-parse note; fixed the stale `jfss-meta.json` filename reference.
- **1.0.0:** Initial release.

---
