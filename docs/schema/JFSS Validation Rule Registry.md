# JFSS Validation Rule Registry
**Version:** 1.5.1  
**Status:** Active Standard  
**Last updated:** 2026-09-16  
**Pairs with:** JFSS v2.0.1  
**Maintainers:** Full-Stack Engineering Team

## 1. Purpose & The Polyglot Contract
The JFSS Validation Rule Registry defines the standardized, advanced validation rules that can be utilized within the `rules` array of any `role: "data"` component. 

### 1.1 The Polyglot Contract
Kelir runs a Vue frontend and a **Rust** backend, so adding a new rule to this registry is a **binding architectural commitment** across exactly two runtimes. Two, not three: earlier versions named Go alongside Rust, for a backend that does not exist and is not planned (decision **D-11**).

Before adding a new rule to this document, the engineering team must ensure:
1. **Frontend Parity:** The rule can be evaluated using Vue/Zod/Yup.
2. **Backend Parity:** The rule can be evaluated natively in Rust without relying on an embedded JavaScript engine.
3. **Semantic Parity:** For a rule scoped `both`, the two implementations agree on the **edge cases**, not just the happy path — a rule that both sides evaluate but decide differently is worse than one only the server enforces, because nothing surfaces the disagreement. See the `regex` warning below for a live example.
4. **Security Boundary:** The rule's `scope` correctly reflects whether it is a UX enhancement (`client`), a strict security boundary (`server`), or a shared data-integrity check (`both`).

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
  * **Rust:** Compare `payload[current_key] == payload[&params.target]` on `serde_json::Value`, whose `PartialEq` is structural — note that a missing key and an explicit `null` are both `Value::Null` and therefore compare equal, which is the correct outcome here only because S10.1 requires every data `key` to be submitted.

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
> **Decided 2026-09-09 as decision D-15, and the guidance below is now a rule the server enforces — which closes the loud half and the commonest silent case, not every silent one.** Of the two candidates — constrain the params schema and check it at save, or adopt a backtracking engine (`fancy-regex`) — **the first is taken and the second is rejected on the NFR rather than on the capability**: a pattern is written by a tenant's configuration author, so backtracking makes catastrophic backtracking a denial of service reachable from a stored form definition. A linear-time guarantee is worth more than lookahead in a rule a browser also has to agree with.
>
> **~~Not every divergence is closed (1.4.1).~~ ~~Closed in 1.5.0~~ Narrowed in 1.5.0, not closed (1.5.1)** ([#413](https://github.com/sujanto-gaws/kelir/issues/413), [#465](https://github.com/sujanto-gaws/kelir/issues/465)). The server refused a bare `\d`, `\w` or `\s` and **did not** refuse `\b`, a POSIX bracket expression such as `[[:digit:]]`, or a Unicode property such as `\p{Nd}` — constructs the `regex` crate compiles and ECMA-262 reads differently or not at all. **All three are now refused at the same seam**, and the table below carries them.
>
> **What 1.5.0 did not close, measured 2026-09-16** ([record 16](../../projects/verifications/16.%20Sprint%2018%20Independent%20Pass.md) finding 2, node v24.15.0 against `regex` 1.13.1). **The refusal is a list of constructs, not a definition of agreement**, so a construct the list does not name is still stored and still divergent. Each of these is saved with a `201` today and decides at least one input differently on the two sides:
>
> | Construct | Example | Where they part |
> |---|---|---|
> | `.` | `^.{1,3}$` | A character outside the BMP is two units in the browser and one here; `\r` and U+2028 match here and not there |
> | A literal astral character in a class | `^[😀]$` | Two units in the browser, one here |
> | Nested classes and set operations | `[[a-c]]`, `[a-z&&[^aeiou]]`, `[a-z--b]` | Crate syntax; an ordinary class in the browser |
> | Braced and 8-digit escapes | `\x{41}`, `\u{41}`, `\U00000041` | Crate syntax; literal characters in the browser |
> | Crate-only anchors and `\a` | `\A`, `\z`, `\<`, `\>`, `\a` | Anchors here; identity escapes there, and `\a` the other way |
> | Inline flags and named groups | `(?i)abc`, `(?P<x>a)`, `(?x) a b` | Compile here; a `SyntaxError` in the browser, **so the browser fails every value** |
> | Case folding under `i` | `^k$` against U+212A | The crate folds more code points |
> | `$` under `m` | `^a$` against `a\r\nb` | ECMA-262 treats `\r` as a line terminator |
>
> **Until the list is extended or the dialect is redefined, write a `both`-scoped pattern from ASCII literals, written-out classes, `^`, `$` without `m`, and the ordinary quantifiers.** What the refusal promises is narrower than *the two sides agree*: **a pattern the server refuses would certainly have diverged; a pattern it accepts may still diverge.** [ADR-0038](../architectures/adr/0038.%20Kelir%20Patterns%20Are%20the%20Linear-Time%20Subset.md) §2 records which dialect Kelir means.
>
> **What Kelir refuses when a definition is written** ([#391](https://github.com/sujanto-gaws/kelir/issues/391), through the S10.3 envelope, at the same seam that already refuses an unregistered rule name):
>
> | Refused | Code | Why |
> |---|---|---|
> | A pattern this backend cannot compile — lookahead, backreferences, a syntax error | `PATTERN_NOT_COMPILABLE` | Stored, it **rejects every value**: the evaluator maps a compile error to *no match*, which is correct at submit (a rule that could not be applied has not been satisfied) and is a field nobody can fill |
> | A bare `\d`, `\w` or `\s`, in either case | `PATTERN_CLASS_NOT_PINNED` | It compiles on both sides and **means different things**, so the two runtimes decide one input opposite ways with nothing raised on either side — §1's Semantic Parity requirement, failing silently |
> | A POSIX bracket expression — `[[:digit:]]`, `[[:^alpha:]]` and the rest | `PATTERN_CLASS_NOT_PINNED` | **ECMA-262 has no such syntax.** Where this crate reads a named class, the browser reads an ordinary class containing `[`, `:` and the letters of the name — a different set rather than a wider or narrower one. Write the class out |
> | A Unicode property — `\p{…}` or `\P{…}` | `PATTERN_CLASS_NOT_PINNED` | **ECMA-262 reads these only under the `u` flag**, which the renderer passes only if the rule's `params.flags` asks for it; without it `\p` is an identity escape and the browser reads a literal `p`. Write the class out |
> | `\b` or `\B` | `PATTERN_CONSTRUCT_NOT_PORTABLE` | ECMA-262 defines the boundary over `[A-Za-z0-9_]` and this crate over Unicode word characters, so `caf\b` matches `café` in the browser and not here. **A separate code because the remedy is different in kind**: a class can be written out, a boundary has to be re-expressed with the characters around it, such as `(^\|[^A-Za-z0-9_])` |
>
> Both apply to `validation.pattern` as well as to this rule, because one evaluator decides both.
>
> **What the check covers, and what it does not** — the edge an absence of findings needs to mean anything ([coding standard](../standards/01.%20Coding%20Standard.md) §2.9):
>
> - **Covered:** `\d \D \w \W \s \S` anywhere in the pattern; `\p{…}` and `\P{…}` anywhere; POSIX bracket expressions inside a bracket expression; `\b` and `\B` **outside** one.
> - **Not covered, deliberately:** `\b` *inside* a bracket expression, which is a backspace escape on both sides and agrees.
> - **Not covered, and a limit rather than a decision:** a divergence neither engine expresses as syntax. Case folding under the `i` flag differs on a handful of code points and nothing detects it.
>
> **The capability limit, stated where you meet it: Kelir patterns carry no lookahead and no backreferences.** So **server-side password complexity is several rules rather than one** — a `minLength` and one pattern per class, or a single pinned pattern per requirement. **`passwordStrength` does not cover this**: its own Rust note says the backend ignores it and relies on `validation.minLength` and `validation.pattern`, which is the keyword this decision constrains. That is a real narrowing, taken deliberately.

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
  * **Rust:** Ignored. The backend relies on the base `validation.minLength` and `validation.pattern` for actual password security.

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
*These rules contain sensitive business logic or require database access. They are evaluated exclusively by the Rust backend. The frontend will only display the error message if the backend returns a `400 Bad Request` upon form submission.*

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
  * **Rust:** Execute a parameterized `SELECT COUNT(*)`. *Never* interpolate the `table` or `column` strings into the SQL; map them to an allow-list of known tables and columns first. The [coding standard](../standards/01.%20Coding%20Standard.md) §2.5 requires compile-time-verified queries, and `sqlx::query!` cannot take a runtime table name at all — so the allow-list is not merely advice here, it is the only shape that compiles: match the pair to a fixed `sqlx::query_scalar!` per known target.

#### `exists` (Foreign Key Validation)
Verifies that the submitted value corresponds to a valid primary key in a related database table.
* **Use Case:** Ensuring a submitted `department_id` or `category_id` actually exists in the database.
* **Params Schema:** `{ "table": "string", "column": "string" }`
* **Example:**
  ```json
  { "rule": "exists", "scope": "server", "params": { "table": "departments", "column": "id" }, "message": "The selected department does not exist." }
  ```
* **Implementation Notes:**
  * **Rust:** Execute a parameterized `SELECT COUNT(*)`. *Never* interpolate the `table` or `column` strings into the SQL; map them to an allow-list of known tables and columns first. The [coding standard](../standards/01.%20Coding%20Standard.md) §2.5 requires compile-time-verified queries, and `sqlx::query!` cannot take a runtime table name at all — so the allow-list is not merely advice here, it is the only shape that compiles: match the pair to a fixed `sqlx::query_scalar!` per known target.

#### `authorized` (RBAC / Permission Check)
Verifies that the currently authenticated user session has the required permissions to submit the specific value for this field.
* **Use Case:** Preventing a standard user from tampering with the payload to set `role: "admin"`.
* **Params Schema:**
  ```json
  { "requiredPermission": "string", "allowedValues": ["array", "of", "values"] }
  ```
* **Implementation Notes:**
  * **Rust:** Take the caller's permissions from the request's authenticated claims — the same `Authenticated` extractor every protected route uses, never a value read out of the payload. If `payload[current_key]` is not in `allowedValues`, or the caller lacks `requiredPermission`, reject the payload.

---

## 4. Extending the Registry (Standard Operating Procedure)

If a developer needs to introduce a new validation rule (e.g., `validateCryptoAddress`), they must follow this workflow:

1. **Draft the Rule:** Define the `rule` name, `scope`, and `params` schema.
2. **Update this Registry:** Add the rule to the appropriate scope section in this document.
3. **Implement in Vue:** Add the logic to the `zodBuilder.ts` (or equivalent) switch statement.
4. **Implement in Rust:** Add the logic to the backend's server-rule evaluator (`evaluate_server_rules` or equivalent) as a new `match` arm. An unrecognised rule name MUST be an error, not a skipped arm — a rule the backend silently ignores is a `server`-scoped check that does not run.
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

- **1.4.1 (2026-09-11):** **1.4.0 called the `regex` question resolved, and it is not wholly.** The save-time refusal closes lookahead, backreferences and a bare `\d`, `\w` or `\s`; `\b`, POSIX bracket expressions and `\p{…}` still compile on the server and diverge silently in the browser ([#413](https://github.com/sujanto-gaws/kelir/issues/413), the [Sprint 16 independent pass](../../projects/verifications/15.%20Sprint%2016%20Independent%20Pass.md) finding 1). The warning under `regex` now says which. Found again by the `v0.7.0` pre-flight schema check; 1.4.0's entry below is left as written. No rule is added, removed, or re-scoped.
- **1.5.1 (2026-09-16):** **1.5.0 said *Closed*, and it was not.** [#465](https://github.com/sujanto-gaws/kelir/issues/465), [record 16](../../projects/verifications/16.%20Sprint%2018%20Independent%20Pass.md) finding 2: about twenty constructs outside the refusal's list are still stored and still decide inputs differently on the two sides. `.` against a character outside the BMP is the ordinary one, and inline flags such as `(?i)` are the worst, because the browser cannot build them and fails every value. **The warning under `regex` now tabulates them, states what the refusal does and does not promise, and gives the subset an author can rely on.** No rule, code or refusal changed; this is a correction to a claim.
- **1.5.0 (2026-09-13):** **Closed the three divergences 1.4.1 recorded as open** ([#413](https://github.com/sujanto-gaws/kelir/issues/413), Sprint 18 item 1). `\b`/`\B`, POSIX bracket expressions and `\p{…}`/`\P{…}` are refused where a definition is written, joining the bare classes. **`\b` earns its own code**, `PATTERN_CONSTRUCT_NOT_PORTABLE`, because it has no portable spelling — a class can be transcribed and a boundary has to be re-expressed. **The check's boundary is now stated** rather than left to be inferred. **And the refusal's explanation is corrected**: one message had served `\d`, `\w` and `\s`, saying *ECMA-262's classes are ASCII and this crate's are Unicode* and ending *write `[0-9]`* whatever the pattern was. **For `\s` both halves were wrong** — both sides read it as Unicode whitespace, differing at exactly U+0085 and U+FEFF in opposite directions, and a digit class is not a remedy for a whitespace one. Each construct now carries its own reason.
- **1.4.0 (2026-09-09):** **Resolved the `regex` rule's open question as decision D-15.** The "ECMA 262 regex" params schema is constrained to what the Rust `regex` crate honours, checked when a form definition is written rather than when it is filled in ([#391](https://github.com/sujanto-gaws/kelir/issues/391)): an uncompilable pattern is `PATTERN_NOT_COMPILABLE` and a bare `\d`, `\w` or `\s` is `PATTERN_CLASS_NOT_PINNED`, both through the S10.3 envelope and both applying to `validation.pattern` too. **`fancy-regex` was the rejected alternative**, on the ReDoS exposure a tenant-authored pattern would open rather than on what it can express. The interim guidance under the `regex` warning becomes the rule; the capability limit — no lookahead, no backreferences, so server-side password complexity is several rules — is stated there. No rule is added, removed, or re-scoped.
- **1.3.0 (2026-08-21):** **Removed Go.** Decision **D-11**: Kelir's backend is Rust, and this registry had been naming Go alongside it throughout. Restated §1.1 for two runtimes and added a **Semantic Parity** requirement — for a `scope: "both"` rule the two implementations must agree on the edge cases, which is what the `regex` entry had been quietly failing. Converted every `Go/Rust` implementation note to Rust and made them concrete rather than generic: `matchesField` gains the `serde_json::Value` equality caveat; `unique` and `exists` state why the allow-list is the only shape that compiles under `sqlx::query!`; `authorized` names the authenticated claims rather than "the JWT/Session context"; §4 step 4 requires an unrecognised rule name to be an error rather than a skipped `match` arm. No rule is added, removed, or re-scoped.
- **1.2.0 (2026-08-21):** Recorded the [operator-parity spike](../../projects/spikes/01.%20JFSS%20Operator%20Parity.md) (#31) finding against the `regex` rule: the "ECMA 262 regex" params schema is not honourable by the Rust `regex` crate — lookahead and backreferences are rejected at compile time, and `\d` diverges silently between the ASCII ECMA-262 class and Rust's Unicode `Nd`, which for a `scope: "both"` rule means the two sides reach opposite verdicts on the same input. Added Rust implementation notes and interim guidance; the two candidate resolutions are open.
- **1.1.0 (2026-08-05):** Aligned the Section 5 error-response contract with JFSS v2.0.1 Section 10.3 (`path` with dot-notation, plus `rule`, `code`, `message`); added the document header and title; added examples for `notMatchesField`, `oneOf`, `notOneOf`, and `exists` (with the SQL-injection allow-list warning); split `oneOf`/`notOneOf` into separate entries; clarified `oneOf`/`notOneOf` vs. `validation.enum`; defined the `async` rule's request/response contract and Zod async-parse note; fixed the stale `jfss-meta.json` filename reference.
- **1.0.0:** Initial release.

---
