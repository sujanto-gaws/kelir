# JSON Workflow Schema Specification (JWSS)
**Version:** 1.0.0
**Status:** Draft Standard
**Target Stack:** Rust (Workflow Engine), Vue.js (Workflow Designer)
**Last updated:** 2026-08-11

---

## 1. Introduction

The JSON Workflow Schema Specification (JWSS) defines the structure of a Kelir **workflow definition**: the states a document-bound process moves through, the transitions between them, who may act, under which conditions, and which lifecycle hook handlers guard or react to each transition. It is the **single source of truth** consumed by the backend workflow engine for execution and by the frontend workflow designer for editing and visualization.

A JWSS document is stored whole in `workflow_definitions.definition_json` ([Database Schema](../design/02.%20Database%20Schema.md) §7.1). The `workflow_states` and `workflow_transitions` tables are projections regenerated from it on publish; the JSON is authoritative.

### 1.1 Core Philosophy

- **State-Transition Model:** A workflow is a finite state machine, not BPMN. States are named business steps; transitions are the only way state changes.
- **Fixed Platform Spine:** Workflow states are free-form, but every state MUST map onto the fixed platform `documents.status` enum, so the document lifecycle stays uniform across all workflow shapes (architectures/02 §3.12).
- **Hooks over Hardcoding:** Transition-scoped business rules are declared as `guards` and `actions` — lifecycle hook handler entries per the [Lifecycle Hook Contract](Lifecycle%20Hook%20Contract.md) — resolved into the same hook chain as document-type and plugin handlers (architectures/01 §12.4.2).
- **JSON Logic Conditions:** All conditions use JSON Logic restricted to the operators of the [JFSS Calculation Rule Registry](JFSS%20Calculation%20Rule%20Registry.md), guaranteeing polyglot parity with form evaluation. String expressions in older documents (`"amount > 10000000"`) are superseded.
- **Published Means Immutable:** A published definition is never edited; changes create the next revision. Running instances execute the revision they started with.

### 1.2 Definitions

| Term | Definition |
| :--- | :--- |
| **State** | A named step of the process (`MANAGER_APPROVAL`). Holds task creation and assignment configuration. |
| **Transition** | A directed edge between two states, triggered by an action, optionally conditioned and guarded. |
| **Action** | The verb a caller invokes to fire a transition (`APPROVE`, `RETURN`). `AUTO` transitions fire without a caller. |
| **Guard** | A `before_workflow_transition` hook handler scoped to one transition. May veto. |
| **Transition Action (hook)** | An `after_workflow_transition` hook handler scoped to one transition. Runs after commit. |
| **Assignment Rule** | The object resolving who may act on a task or transition (user, role, department scope, expression). |
| **Revision** | The stored version of a workflow definition (`workflow_definitions.version`). Distinct from the JWSS spec `version` field. |

### 1.3 Conformance

The key words **MUST**, **MUST NOT**, **REQUIRED**, **SHALL**, **SHOULD**, **MAY**, and **OPTIONAL** are to be interpreted as described in [RFC 2119](https://www.rfc-editor.org/rfc/rfc2119).

An implementation is **conformant** if it:

1. Accepts every workflow definition that validates against the JWSS Meta-Schema for its declared `version`, and rejects every one that does not.
2. Enforces every structural rule of Section 8 at publish time — a definition violating any S-rule MUST NOT reach `ACTIVE` status.
3. Executes guards and actions through the lifecycle hook chain with the semantics of the [Lifecycle Hook Contract](Lifecycle%20Hook%20Contract.md), never through a private mechanism.
4. Pins running instances to the definition revision they started with.

Where this document and the Meta-Schema disagree, **the Meta-Schema is normative**.

---

## 2. Root Schema Structure

| Property | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `workflowKey` | `string` | Yes | Unique, immutable key of the workflow (`purchase_requisition_standard`). Pattern `^[a-z][a-z0-9_]*$`. |
| `version` | `string` | Yes | The JWSS specification version this document conforms to (`"1.0.0"`). A validator for the 1.x line MUST reject a different major version. This is **not** the definition revision — that is `workflow_definitions.version`. |
| `name` | `string` | Yes | Human-readable name. |
| `description` | `string` | No | — |
| `initialState` | `string` | Yes | Code of the state a new instance starts in. MUST reference a declared state (S1). |
| `states` | `array` | Yes | Ordered array of State Objects (Section 3). Minimum 2. |
| `transitions` | `array` | Yes | Array of Transition Objects (Section 4). Minimum 1. |
| `variables` | `array` | No | Declared workflow variables (Section 6). |
| `settings` | `object` | No | Engine options (e.g. `cancelableBy`, default due hours). |

---

## 3. State Object

| Property | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `code` | `string` | Yes | Unique within the definition, `SCREAMING_SNAKE_CASE` (`FINANCE_APPROVAL`). |
| `name` | `string` | Yes | Display name. |
| `mapsToDocumentStatus` | `string` | Yes | One of the fixed platform statuses: `DRAFT`, `SUBMITTED`, `IN_REVIEW`, `PENDING_APPROVAL`, `APPROVED`, `REJECTED`, `RETURNED`, `COMPLETED`, `ARCHIVED`, `CANCELLED`. |
| `isFinal` | `boolean` | No (default `false`) | Final states end the instance; no outgoing transitions allowed (S4). |
| `task` | `object` | No | Task Specification (§3.1). Present when entering this state creates a human task; absent for pass-through states. |

### 3.1 Task Specification

| Property | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `taskDefinitionKey` | `string` | Yes | Stable key (`finance_approval`) recorded on `workflow_tasks.task_definition_key`. |
| `taskName` | `string` | Yes | Display name. |
| `taskType` | `string` | No (default `APPROVAL_TASK`) | Enum: `USER_TASK`, `APPROVAL_TASK`, `REVIEW_TASK`, `SERVICE_TASK`, `SIGNATURE_TASK`, `DATA_ENTRY_TASK`. |
| `assignment` | `object` | Yes | Assignment Rule (Section 5). |
| `dueInHours` | `number` | No | Relative due time; sets `workflow_tasks.due_at`. |
| `escalation` | `object` | No | `{ "afterHours": number, "assignment": AssignmentRule }` — consumed by the escalation scheduler. |
| `priority` | `string` | No (default `NORMAL`) | Enum: `LOW`, `NORMAL`, `HIGH`, `URGENT`. |

---

## 4. Transition Object

| Property | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `from` | `string` | Yes | Source state code. MUST be declared (S2). |
| `to` | `string` | Yes | Target state code. MUST be declared (S2). |
| `action` | `string` | Yes | Enum: `SUBMIT`, `APPROVE`, `REJECT`, `RETURN`, `RESUBMIT`, `DELEGATE`, `ESCALATE`, `CANCEL`, `COMPLETE`, `AUTO`. |
| `allowedBy` | `string` \| `object` | Conditional | Assignment Rule (Section 5) or shorthand string (§5.2). REQUIRED unless `action` is `AUTO` (S5). |
| `condition` | `object` | No | JSON Logic expression over the condition context (Section 6). The transition is eligible only when it evaluates `true`. |
| `guards` | `array` | No | Hook Registration Entries (Lifecycle Hook Contract §3) merged into the `before_workflow_transition` chain, scoped to this transition. |
| `actions` | `array` | No | Hook Registration Entries merged into the `after_workflow_transition` chain, scoped to this transition. |

Multiple transitions MAY share `from` and `action` with disjoint `condition`s; the engine evaluates them in document order and fires the first eligible one (S7).

---

## 5. Assignment Rule Object

Used by `transitions[].allowedBy`, `task.assignment`, and `task.escalation.assignment`.

### 5.1 Structure

| Property | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `assigneeType` | `string` | Yes | Enum: `USER`, `ROLE`, `DEPARTMENT_ROLE`, `OWNER`, `MANAGER_OF_OWNER`, `EXPRESSION`. |
| `userId` | `string` | Conditional | REQUIRED when `assigneeType` is `USER`. |
| `roleCode` | `string` | Conditional | REQUIRED when `assigneeType` is `ROLE` or `DEPARTMENT_ROLE`. |
| `departmentScope` | `string` | No | `REQUESTED_DEPARTMENT`, `OWNER_DEPARTMENT`, or a department code. Only meaningful with `DEPARTMENT_ROLE`. |
| `expression` | `object` | Conditional | JSON Logic resolving to a user id or role code. REQUIRED when `assigneeType` is `EXPRESSION`. |

Delegation windows (`delegations`, Database Schema §3.8) are applied by the assignment resolver after the rule resolves; they are not part of the rule.

### 5.2 Shorthand Strings

For compactness, `allowedBy` MAY be a string, normalized as:

| Shorthand | Normalized rule |
| :--- | :--- |
| `"OWNER"` | `{ "assigneeType": "OWNER" }` |
| `"ROLE:X"` | `{ "assigneeType": "ROLE", "roleCode": "X" }` |
| `"USER:X"` | `{ "assigneeType": "USER", "userId": "X" }` |

Validators MUST accept both forms; the engine normalizes to the object form before evaluation.

---

### 5.3 What Kelir resolves, and what it refuses

**Informative — an implementation note, not a change to the vocabulary above.** JWSS declares six `assigneeType` values. Kelir resolves four and **refuses the other two at save time**, naming what would have to exist first; a definition using one of them cannot be stored, let alone published. Recorded here because a specification that lists six and an implementation that serves four is exactly the gap a workflow author discovers at the worst moment.

| `assigneeType` | Kelir | Why |
| :--- | :--- | :--- |
| `USER` | resolves | `users` |
| `ROLE` | resolves | `roles.role_code` — an unclaimed role task is claimed by whoever takes it |
| `DEPARTMENT_ROLE` | resolves | `roles` plus `user_roles.department_id`, which has carried a department-scoped grant since `0002` |
| `OWNER` | resolves | `documents.created_by` |
| `MANAGER_OF_OWNER` | **refused** | There is no user-to-manager edge in the schema. `departments.manager_party_id` names a **party**, and party-to-user is not a resolvable relation — FR-ORG-002's reporting line is unbuilt. Resolving it by guesswork would route an approval to somebody chosen by a coincidence of data |
| `EXPRESSION` | **refused** | The evaluator exists; the context does not. §6.1's condition context is document, formData, variables and actor, and an expression resolving to a *principal* needs a directory nothing reads |

Refused at **save** rather than at run time, for the reason JFSS gives about a stored definition generally: a definition is written once and executed many times, and the execution path has no good failure. A workflow that publishes cleanly and then cannot assign its first task is a stalled instance nobody is told about.

The same applies to `guards` and `actions`: they are **stored and not executed** as of `v0.5.0`, because there is no hook chain to merge them into (architectures/01 §12.4.2 is unbuilt). They are accepted rather than refused so that definitions authored now do not have to be rewritten when the chain lands, and the workflow engine states in one place that it does not invoke them — a stored handler must not be read as evidence that it runs.

---

## 6. Conditions and Variables

### 6.1 Condition Context

`condition` expressions evaluate against a context object:

```json
{
  "document": { "status": "...", "amount": 45000000, "documentTypeKey": "..." },
  "formData": { },
  "variables": { },
  "actor":    { "userId": "...", "roles": [], "departmentId": "..." }
}
```

### 6.2 Operators

Conditions MUST use only operators registered in the [JFSS Calculation Rule Registry](JFSS%20Calculation%20Rule%20Registry.md). Custom operators require registration there first — the same polyglot-parity rule as form calculations (JFSS S8.1.1).

### 6.3 Variable Declaration

| Property | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `key` | `string` | Yes | Pattern `^[a-z][a-z0-9_]*$`; stored in `workflow_variables`. |
| `dataType` | `string` | Yes | Enum: `STRING`, `NUMBER`, `BOOLEAN`, `DATE`, `JSON`. |
| `source` | `object` | No | JSON Logic computing the initial value from the condition context at instance start. |

---

## 7. Guards and Actions

`guards` and `actions` entries follow the Hook Registration Entry of the [Lifecycle Hook Contract](Lifecycle%20Hook%20Contract.md) §3, with two JWSS-specific constraints:

1. Priorities SHOULD lie in the workflow band **300–499** (architectures/01 §12.4.3). Values outside the band are accepted but WARN at publish.
2. A guard result of `REJECT` aborts the transition and surfaces the standard `HOOK_REJECTED` error (Lifecycle Hook Contract §6); the instance stays in `from` state.

Guards run inside the transition's transaction via `before_workflow_transition`; actions run post-commit via `after_workflow_transition`. Handlers declared here run **only** for their own transition, in the definition revision the instance started with.

---

## 8. Structural Validation Rules

Publish-time rules. A definition failing any rule MUST remain `DRAFT`.

| Rule | Requirement |
| :--- | :--- |
| **S1** | `initialState` references a declared state, and that state has `isFinal: false`. |
| **S2** | Every `transitions[].from` / `.to` references a declared state. |
| **S3** | State `code`s are unique; `(from, action, to)` triples are unique. |
| **S4** | No transition originates from a state with `isFinal: true`. |
| **S5** | Every non-`AUTO` transition declares `allowedBy`; `AUTO` transitions MUST NOT declare it. |
| **S6** | Every state is reachable from `initialState`, and at least one final state is reachable from every non-final state (no dead ends, no orphans). |
| **S7** | Transitions sharing `from` + `action`: at most one of them MAY omit `condition` (the fallback), and it is evaluated last regardless of document order. |
| **S8** | Every state that is the target of a non-`AUTO` transition and is not final SHOULD declare a `task`; a stateless wait is a publish WARNING. |
| **S9** | Every `mapsToDocumentStatus` value is a member of the platform enum. At least one state maps to `COMPLETED` or `CANCELLED`. |
| **S10** | All `condition`, `expression`, and variable `source` logic uses registered operators only (§6.2). |
| **S11** | Guard/action handler references resolve at publish time: `core:*` handlers exist; `plugin:*` handlers belong to an installed plugin (a disabled plugin is a WARNING, an unknown one an ERROR). |

---

## 9. Storage and Revisioning

- The document is stored in `workflow_definitions.definition_json`; `workflow_key`, `initial_state`, and the projections `workflow_states` / `workflow_transitions` are extracted from it on publish.
- `workflow_definitions.jwss_version` records the declared spec `version`.
- Publishing sets `status = 'ACTIVE'` and freezes the row. Editing an `ACTIVE` definition creates a new row with `version + 1` in `DRAFT`.
- Running instances reference `workflow_definition_id` (a specific revision) — a newly published revision affects only instances started after it.

---

## 10. Example

```json
{
  "workflowKey": "purchase_requisition_standard",
  "version": "1.0.0",
  "name": "Standard Purchase Requisition Workflow",
  "initialState": "DRAFT",
  "states": [
    { "code": "DRAFT", "name": "Draft", "mapsToDocumentStatus": "DRAFT" },
    { "code": "MANAGER_APPROVAL", "name": "Manager Approval",
      "mapsToDocumentStatus": "PENDING_APPROVAL",
      "task": {
        "taskDefinitionKey": "manager_approval",
        "taskName": "Manager Approval",
        "assignment": { "assigneeType": "MANAGER_OF_OWNER" },
        "dueInHours": 48,
        "escalation": { "afterHours": 72,
                        "assignment": { "assigneeType": "ROLE", "roleCode": "DEPT_HEAD" } }
      }
    },
    { "code": "FINANCE_APPROVAL", "name": "Finance Approval",
      "mapsToDocumentStatus": "PENDING_APPROVAL",
      "task": {
        "taskDefinitionKey": "finance_approval",
        "taskName": "Finance Approval",
        "assignment": { "assigneeType": "DEPARTMENT_ROLE", "roleCode": "FINANCE_APPROVER",
                        "departmentScope": "REQUESTED_DEPARTMENT" }
      }
    },
    { "code": "COMPLETED", "name": "Completed", "mapsToDocumentStatus": "COMPLETED", "isFinal": true },
    { "code": "REJECTED",  "name": "Rejected",  "mapsToDocumentStatus": "REJECTED",  "isFinal": true },
    { "code": "RETURNED",  "name": "Returned",  "mapsToDocumentStatus": "RETURNED" },
    { "code": "CANCELLED", "name": "Cancelled", "mapsToDocumentStatus": "CANCELLED", "isFinal": true }
  ],
  "transitions": [
    { "from": "DRAFT", "to": "MANAGER_APPROVAL", "action": "SUBMIT", "allowedBy": "OWNER" },
    { "from": "MANAGER_APPROVAL", "to": "FINANCE_APPROVAL", "action": "APPROVE",
      "allowedBy": { "assigneeType": "MANAGER_OF_OWNER" },
      "guards":  [ { "hook": "before_workflow_transition",
                     "handler": "core:check_approval_limit", "priority": 300,
                     "config": { "limitField": "amount" } } ],
      "actions": [ { "hook": "after_workflow_transition",
                     "handler": "plugin:erp-connector:reserve_budget", "priority": 320 } ]
    },
    { "from": "MANAGER_APPROVAL", "to": "RETURNED", "action": "RETURN",
      "allowedBy": { "assigneeType": "MANAGER_OF_OWNER" } },
    { "from": "FINANCE_APPROVAL", "to": "COMPLETED", "action": "APPROVE",
      "allowedBy": "ROLE:FINANCE_APPROVER",
      "condition": { "<=": [ { "var": "document.amount" }, 10000000 ] } },
    { "from": "FINANCE_APPROVAL", "to": "REJECTED", "action": "REJECT",
      "allowedBy": "ROLE:FINANCE_APPROVER" },
    { "from": "RETURNED", "to": "MANAGER_APPROVAL", "action": "RESUBMIT", "allowedBy": "OWNER" },
    { "from": "DRAFT",    "to": "CANCELLED", "action": "CANCEL", "allowedBy": "OWNER" },
    { "from": "RETURNED", "to": "CANCELLED", "action": "CANCEL", "allowedBy": "OWNER" }
  ]
}
```

---

## 11. Companion Documents

| Document | Role |
| :--- | :--- |
| [Lifecycle Hook Contract](Lifecycle%20Hook%20Contract.md) | Guard/action entry shape, invocation payload, result contract |
| [JFSS Calculation Rule Registry](JFSS%20Calculation%20Rule%20Registry.md) | The only operators permitted in conditions and expressions |
| [architectures/01 §12](../architectures/01.%20Basic%20Framework%20Concept%20and%20Architecture.md) | Document lifecycle and hook execution semantics |
| [Database Schema §7](../design/02.%20Database%20Schema.md) | Storage tables and projections |

---

# JWSS v1.0.0 Meta-Schema

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "https://kelir.dev/schemas/jwss-meta-v1.0.0.json",
  "title": "JWSS Workflow Definition",
  "type": "object",
  "required": ["workflowKey", "version", "name", "initialState", "states", "transitions"],
  "additionalProperties": false,
  "properties": {
    "workflowKey": { "type": "string", "pattern": "^[a-z][a-z0-9_]*$", "maxLength": 100 },
    "version": { "type": "string", "pattern": "^1\\.[0-9]+\\.[0-9]+$" },
    "name": { "type": "string", "minLength": 1 },
    "description": { "type": "string" },
    "initialState": { "$ref": "#/$defs/stateCode" },
    "states": { "type": "array", "minItems": 2, "items": { "$ref": "#/$defs/state" } },
    "transitions": { "type": "array", "minItems": 1, "items": { "$ref": "#/$defs/transition" } },
    "variables": { "type": "array", "items": { "$ref": "#/$defs/variable" } },
    "settings": { "type": "object" }
  },
  "$defs": {
    "stateCode": { "type": "string", "pattern": "^[A-Z][A-Z0-9_]*$", "maxLength": 60 },
    "documentStatus": { "enum": ["DRAFT", "SUBMITTED", "IN_REVIEW", "PENDING_APPROVAL", "APPROVED",
                                 "REJECTED", "RETURNED", "COMPLETED", "ARCHIVED", "CANCELLED"] },
    "jsonLogic": { "type": "object" },
    "hookRegistrationEntry": {
      "type": "object",
      "required": ["handler"],
      "additionalProperties": false,
      "properties": {
        "hook": { "type": "string" },
        "handler": { "type": "string", "minLength": 1 },
        "priority": { "type": "integer" },
        "config": { "type": "object" },
        "isEnabled": { "type": "boolean" }
      }
    },
    "state": {
      "type": "object",
      "required": ["code", "name", "mapsToDocumentStatus"],
      "additionalProperties": false,
      "properties": {
        "code": { "$ref": "#/$defs/stateCode" },
        "name": { "type": "string", "minLength": 1 },
        "mapsToDocumentStatus": { "$ref": "#/$defs/documentStatus" },
        "isFinal": { "type": "boolean", "default": false },
        "task": { "$ref": "#/$defs/taskSpec" }
      }
    },
    "taskSpec": {
      "type": "object",
      "required": ["taskDefinitionKey", "taskName", "assignment"],
      "additionalProperties": false,
      "properties": {
        "taskDefinitionKey": { "type": "string", "pattern": "^[a-z][a-z0-9_]*$" },
        "taskName": { "type": "string", "minLength": 1 },
        "taskType": { "enum": ["USER_TASK", "APPROVAL_TASK", "REVIEW_TASK", "SERVICE_TASK",
                               "SIGNATURE_TASK", "DATA_ENTRY_TASK"], "default": "APPROVAL_TASK" },
        "assignment": { "$ref": "#/$defs/assignmentRule" },
        "dueInHours": { "type": "number", "exclusiveMinimum": 0 },
        "escalation": {
          "type": "object",
          "required": ["afterHours", "assignment"],
          "additionalProperties": false,
          "properties": {
            "afterHours": { "type": "number", "exclusiveMinimum": 0 },
            "assignment": { "$ref": "#/$defs/assignmentRule" }
          }
        },
        "priority": { "enum": ["LOW", "NORMAL", "HIGH", "URGENT"], "default": "NORMAL" }
      }
    },
    "transition": {
      "type": "object",
      "required": ["from", "to", "action"],
      "additionalProperties": false,
      "properties": {
        "from": { "$ref": "#/$defs/stateCode" },
        "to": { "$ref": "#/$defs/stateCode" },
        "action": { "enum": ["SUBMIT", "APPROVE", "REJECT", "RETURN", "RESUBMIT", "DELEGATE",
                             "ESCALATE", "CANCEL", "COMPLETE", "AUTO"] },
        "allowedBy": {
          "oneOf": [
            { "type": "string", "pattern": "^(OWNER|(ROLE|USER):[A-Za-z0-9._-]+)$" },
            { "$ref": "#/$defs/assignmentRule" }
          ]
        },
        "condition": { "$ref": "#/$defs/jsonLogic" },
        "guards": { "type": "array", "items": { "$ref": "#/$defs/hookRegistrationEntry" } },
        "actions": { "type": "array", "items": { "$ref": "#/$defs/hookRegistrationEntry" } }
      },
      "if": { "properties": { "action": { "const": "AUTO" } } },
      "then": { "not": { "required": ["allowedBy"] } },
      "else": { "required": ["allowedBy"] }
    },
    "assignmentRule": {
      "type": "object",
      "required": ["assigneeType"],
      "additionalProperties": false,
      "properties": {
        "assigneeType": { "enum": ["USER", "ROLE", "DEPARTMENT_ROLE", "OWNER",
                                   "MANAGER_OF_OWNER", "EXPRESSION"] },
        "userId": { "type": "string" },
        "roleCode": { "type": "string" },
        "departmentScope": { "type": "string" },
        "expression": { "$ref": "#/$defs/jsonLogic" }
      },
      "allOf": [
        { "if": { "properties": { "assigneeType": { "const": "USER" } } },
          "then": { "required": ["userId"] } },
        { "if": { "properties": { "assigneeType": { "enum": ["ROLE", "DEPARTMENT_ROLE"] } } },
          "then": { "required": ["roleCode"] } },
        { "if": { "properties": { "assigneeType": { "const": "EXPRESSION" } } },
          "then": { "required": ["expression"] } }
      ]
    },
    "variable": {
      "type": "object",
      "required": ["key", "dataType"],
      "additionalProperties": false,
      "properties": {
        "key": { "type": "string", "pattern": "^[a-z][a-z0-9_]*$" },
        "dataType": { "enum": ["STRING", "NUMBER", "BOOLEAN", "DATE", "JSON"] },
        "source": { "$ref": "#/$defs/jsonLogic" }
      }
    }
  }
}
```

**Extracted to [`jwss-meta-v1.0.0.json`](jwss-meta-v1.0.0.json) on 2026-08-28**, when the validator was implemented ([#174](https://github.com/sujanto-gaws/kelir/issues/174)). That file and this block are the same document, and `tests/workflow_jwss_meta_schema.rs` compares them — a duplicate nothing checks would drift silently, with the validator enforcing one version while the specification described another.

**The Hook Registration Entry is defined locally rather than referenced across documents.** The block above used to `$ref` `lhcs-meta-v1.0.0.json`, and there is no such file — the [Lifecycle Hook Contract](Lifecycle%20Hook%20Contract.md) §3 defines the shape in prose and has never been extracted. A `$ref` to a schema that does not exist is not resolvable by any validator, so extracting this one had to resolve it: `$defs/hookRegistrationEntry` is §3's table, transcribed, with `hook` optional because §3 makes it optional in this position. When an LHCS meta-schema is extracted, this definition is what it must agree with.

Reachability (S6), fallback ordering (S7), and handler resolution (S11) are beyond JSON Schema expressiveness and MUST be enforced by the publish validator in code. **S11 is not enforced by Kelir as of `v0.5.0`**, and §5.3 says what that means.
