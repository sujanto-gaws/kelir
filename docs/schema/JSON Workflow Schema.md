# JSON Workflow Schema Specification (JWSS)
**Version:** 1.0.0
**Status:** Draft Standard
**Target Stack:** Rust (Workflow Engine), Vue.js (Workflow Designer)
**Last updated:** 2026-08-30

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

Every string property this document declares that an engine stores in a column carries a `maxLength` equal to that column's, and §9.1 collects them. A property that is bounded only by the storage is a property whose definition publishes and then fails while somebody is holding the task ([#259](https://github.com/sujanto-gaws/kelir/issues/259)).

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

`dueInHours` is **relative and not absolute** because a definition outlives every instance that runs it: an absolute date in one would be wrong for every instance after the first. An engine MUST stamp `workflow_tasks.due_at` when the task is generated and MUST NOT recompute it afterwards — a deadline that moved when the definition was revised is a deadline nobody agreed to. Kelir computes the stamp in the database, so it shares a clock with every later comparison against it ([#185](https://github.com/sujanto-gaws/kelir/issues/185)).

`escalation` is **stored and not executed**, for the reason §7's `guards` and `actions` are: FR-WF-010 is unscheduled and there is no scheduler. A definition may declare one and nothing will act on it. Lateness is made *visible* — `workflow_tasks.due_at` and the inbox's overdue indicator — and nothing acts on it automatically.
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
| `requiresComment` | `boolean` | No | Default `false`. When `true`, the caller MUST supply a comment with the decision that fires this transition; an engine MUST refuse the transition otherwise (S12). MUST NOT be `true` on an `AUTO` transition. |
| `guards` | `array` | No | Hook Registration Entries (Lifecycle Hook Contract §3) merged into the `before_workflow_transition` chain, scoped to this transition. |
| `actions` | `array` | No | Hook Registration Entries merged into the `after_workflow_transition` chain, scoped to this transition. |

Multiple transitions MAY share `from` and `action` with disjoint `condition`s; the engine evaluates them in document order and fires the first eligible one (S7).

### 4.1 `requiresComment` is per edge, not per workflow

A comment is mandatory on the **transition**, because that is the granularity at which the answer differs: an approval is self-explaining and a refusal is not, so `REJECT` and `RETURN` are the edges a workflow author usually marks and `APPROVE` is the one they usually do not. A workflow-level setting could not express that, and a hard-coded rule — *rejections always need a reason* — would decide for every deployment a question that belongs to whoever writes the workflow.

The comment itself is stored on the decision record and on the history row (Database Schema §7.6, §7.8, §7.11), which is what makes the requirement worth stating: a reason captured where the decision is not visible would not be read.

**`AUTO` is excluded** for the reason §5.3 refuses two assignee types at save: there is no caller on an `AUTO` transition, so `requiresComment: true` on one would be an edge that can never fire — a stalled instance nobody is told about, produced by a definition that published cleanly.

### 4.2 `from` and `to` MAY name the same state

A transition whose `from` equals its `to` is legal, and it means *send it round again*: the state is re-entered, its `task` generates a fresh task, and the document does not move. Nothing in this section requires the two to differ, and the rules that constrain such an edge are the ones that constrain every other — S3 keeps the `(from, action, to)` triples unique, S4 keeps it off a final state, and **S6 is what stops a self-edge becoming a process that cannot end**, because a final state must still be reachable and a self-edge alone does not reach one.

An engine MUST record it in the history like any other transition. That row is the one a reader most needs and the one an engine is most tempted to drop: somebody decided something, at a time, with a reason, and the document stayed where it was. A history missing it has a gap exactly where a person acted, and nothing on the screen says the gap is there.

Kelir enforced the opposite until [#259](https://github.com/sujanto-gaws/kelir/issues/259) — a `CHECK` constraint on the history table required the two states to differ — so a definition using a self-edge saved, published, and then failed on the decision that fired it. The constraint was dropped rather than the construct: this specification is the normative document, and an implementation detail is not a reason to narrow it.

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

A window applies **only where the rule resolves to a person** — `USER` or `OWNER`. A `ROLE` or `DEPARTMENT_ROLE` assignment produces a task with no assignee, offered to everybody who holds the role, and there is no one person's work in it to redirect; redirecting it would turn a queue item into somebody's task at the moment it was created.

A window also does not reach back: a task already assigned when a window opens stays where it is. Handing over work already in somebody's hands is a separate action taken on the task itself (`POST /api/v1/workflow/tasks/{id}/delegation`), which changes who the open task is for and **does not fire a transition**. `DELEGATE` remains in §4's `action` vocabulary and is fired by nothing, for the reason `AUTO` is: a definition may declare such an edge, and no caller reaches it.

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

**`allowedBy` is enforced**, and this sentence exists because for one release it was not. It was parsed, validated here at save, projected to `workflow_transitions.allowed_by_json` and read by nothing ([#226](https://github.com/sujanto-gaws/kelir/issues/226)) — a control that looked like the one beside it and was the one above it. The engine now checks the chosen transition's rule against the actor before it moves the instance, using the same resolver a task's `assignment` uses, so the four resolvable assignee types mean the same thing on an edge as on a task.

**It authorizes; it does not select.** Where a state offers two transitions for one action, `condition` chooses between them (S7, fallback last) and `allowedBy` is then applied to the one chosen. A caller who may not take that edge is refused rather than routed down the next — an approver silently taking a branch the definition did not point them at is a worse outcome than the refusal, because a rejection routed as a return reads as their own decision.

**A task's `assignment` and a transition's `allowedBy` are two controls and both apply.** They coincide in the common shape and §8's example has them differ: a `RESUBMIT` out of `RETURNED`, a state that declares no task at all.

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

An engine MUST evaluate a condition with the **same evaluator it evaluates form logic with**. A routing condition is the same kind of expression against different data, and a second evaluator here would lose polyglot parity on the surface that decides who approves a document.

### 6.3 Variable Declaration

| Property | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `key` | `string` | Yes | Pattern `^[a-z][a-z0-9_]*$`; stored in `workflow_variables`. |
| `dataType` | `string` | Yes | Enum: `STRING`, `NUMBER`, `BOOLEAN`, `DATE`, `JSON`. |
| `source` | `object` | No | JSON Logic computing the initial value from the condition context at instance start. |

### 6.4 Evaluation Failure

A condition that **cannot be evaluated** — an operator that throws, a division by zero, an argument of the wrong shape — MUST stop the transition. An engine MUST NOT treat the failure as `false`.

The distinction matters because `false` is not a neutral answer. It sends the process down the fallback S7 declares, which is a *different branch chosen because the intended one broke*, and the instance then continues as though the routing rule had been consulted and had declined. Nothing in the record would say that it never ran. A workflow that routes wrongly on a bad expression is worse than one that refuses to move.

An expression that evaluates cleanly to a non-boolean is a different case: JSON Logic is truthy-typed, and `0` or `""` has told the engine something. Only a failure to evaluate at all is unknown.

Because the failure depends on the data, it cannot be caught by §8's S10 at save time: the expression may be entirely well formed and use only registered operators, and break on the third document it meets.

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
| **S7** | Transitions sharing `from` + `action`: at most one of them MAY omit `condition` (the fallback), and it is evaluated last regardless of document order. Where every condition is false and no fallback is declared, the instance MUST NOT move, and the refusal MUST distinguish that case from an action the state does not declare at all. |
| **S8** | Every state that is the target of a non-`AUTO` transition and is not final SHOULD declare a `task`; a stateless wait is a publish WARNING. |
| **S9** | Every `mapsToDocumentStatus` value is a member of the platform enum. At least one state maps to `COMPLETED` or `CANCELLED`. |
| **S10** | All `condition`, `expression`, and variable `source` logic uses registered operators only (§6.2). |
| **S11** | Guard/action handler references resolve at publish time: `core:*` handlers exist; `plugin:*` handlers belong to an installed plugin (a disabled plugin is a WARNING, an unknown one an ERROR). |
| **S12** | An `AUTO` transition MUST NOT declare `requiresComment: true`. There is no caller to supply one, so the edge could never fire (§4.1). |

---

## 9. Storage and Revisioning

- The document is stored in `workflow_definitions.definition_json`; `workflow_key`, `initial_state`, and the projections `workflow_states` / `workflow_transitions` are extracted from it on publish.
- `workflow_definitions.jwss_version` records the declared spec `version`.
- Publishing sets `status = 'ACTIVE'` and freezes the row. Editing an `ACTIVE` definition creates a new row with `version + 1` in `DRAFT`.
- Running instances reference `workflow_definition_id` (a specific revision) — a newly published revision affects only instances started after it.

### 9.1 A stored string is bounded where it is declared

Every string property an engine projects into a column MUST carry a `maxLength` in the meta-schema, and that bound MUST equal the column's. Kelir's:

| Property | Stored as | `maxLength` |
| :--- | :--- | :--- |
| `version` | `workflow_definitions.jwss_version VARCHAR(40)` | 40 |
| `initialState`, `states[].code`, `transitions[].from` / `.to` | `VARCHAR(64)` columns | 60, via `stateCode` |
| `states[].name` | `workflow_states.name VARCHAR(200)` | 200 |
| `states[].task.taskDefinitionKey` | `workflow_tasks.task_definition_key VARCHAR(64)` | 64 |
| `states[].task.taskName` | `workflow_tasks.task_name VARCHAR(200)` | 200 |
| `variables[].key` | `workflow_variables.variable_key VARCHAR(64)` | 64 |

**A `MUST`, because the failure it prevents is not an implementation's private business.** A definition bounded only by the storage passes every gate this specification defines and then fails at run time, in front of whoever is holding the task and nowhere near the definition that caused it. `stateCode` already carried its bound, which is what shows the omission was an oversight rather than a position: the bound was thought about for one property and not for its neighbours.

**The bound goes in the meta-schema and not into §8**, for the reason S5 and S12 are there rather than in prose: §1.3 makes the meta-schema normative, and a constraint expressible as a keyword is one an off-the-shelf validator enforces for every implementation rather than one each implementation reimplements.

`workflowKey` is deliberately absent from the table. Kelir takes the stored key from the create request rather than from the document, so the document's own `workflowKey` reaches no column; its `maxLength: 100` bounds the identifier this specification defines and is not a storage bound.

An engine whose columns are narrower than a bound above is **not** conforming, and the remedy is to widen the column rather than to refuse the document.

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
      "allowedBy": { "assigneeType": "MANAGER_OF_OWNER" }, "requiresComment": true },
    { "from": "FINANCE_APPROVAL", "to": "COMPLETED", "action": "APPROVE",
      "allowedBy": "ROLE:FINANCE_APPROVER",
      "condition": { "<=": [ { "var": "document.amount" }, 10000000 ] } },
    { "from": "FINANCE_APPROVAL", "to": "REJECTED", "action": "REJECT",
      "allowedBy": "ROLE:FINANCE_APPROVER", "requiresComment": true },
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

## 12. Revision History

This specification is a **`Draft Standard`** ([naming convention](../standards/02.%20Naming%20Convention.md) §10.1): *the shape may still change; implement at your own risk.* That status is what signals instability, so a change to the shape is recorded here rather than by moving the version — the version starts carrying that signal when the specification becomes an `Active Standard`, where §10.1 makes it the thing that means backwards compatibility. The `1.x` line is validated by one meta-schema, which §2 already states: *"a validator for the 1.x line MUST reject a different major version."*

| Revision | Date | Change |
| :--- | :--- | :--- |
| **R-1** | 2026-08-28 | **`transitions[].requiresComment` added** (§4, §4.1, S12), for FR-TASK-006 — [#182](https://github.com/sujanto-gaws/kelir/issues/182). A **strict widening**: the property is optional and defaults to `false`, so every document valid before this revision is valid after it and no stored definition needs rewriting. The §10 example marks its `REJECT` and `RETURN` edges, because those are the edges the property exists for. |
| **R-2** | 2026-08-29 | **§5.1 says what a delegation window does and does not do**, for FR-IDM-006, FR-WF-009 and FR-TASK-008 — [#184](https://github.com/sujanto-gaws/kelir/issues/184). **No change to the shape**: no property is added, removed or re-typed, and the meta-schema is untouched. What changes is the specification being explicit that a window applies only where the rule resolves to a person, that it does not reach back for tasks already assigned, and that the `DELEGATE` action in §4's vocabulary is still fired by nothing — a reader who knew delegation had been built could otherwise have concluded that such an edge now drives it. |
| **R-3** | 2026-08-29 | **§3.1 says what an engine must do with `dueInHours`, and what it must not do with `escalation`**, for FR-WF-011 and FR-TASK-007 — [#185](https://github.com/sujanto-gaws/kelir/issues/185). **No change to the shape**: `dueInHours` was already declared and already constrained by the meta-schema (`exclusiveMinimum: 0`), and nothing is added, removed or re-typed. What changes is the specification stating that the stamp happens at generation and is never recomputed — otherwise two engines could both claim conformance while one let a republished revision shorten a deadline somebody was working to — and that `escalation` is stored and not executed, which a reader could not tell from a table that describes it as *consumed by the escalation scheduler*. |
| **R-4** | 2026-08-29 | **§6.2 gains the one-evaluator rule, §6.4 says what an engine does when a condition cannot be evaluated, and S7 gains the no-match case**, for FR-WF-015 — [#186](https://github.com/sujanto-gaws/kelir/issues/186). **No change to the shape**: no property is added, removed or re-typed, and the meta-schema is untouched. §6.4 is new text rather than a clarification, and it settles a question two conforming engines could previously answer opposite ways — Kelir itself answered it the other way until this revision, treating an evaluation failure as `false` and falling through to the fallback. S7's addition is the matching obligation at the other end: a definition may leave a gap, and an engine must not paper over it silently. |
| **R-5** | 2026-08-30 | **§4.2 says a self-transition is legal, §9.1 requires every stored string to be bounded where it is declared, and the meta-schema gains five `maxLength` keywords** — [#259](https://github.com/sujanto-gaws/kelir/issues/259), finding 1 of the Sprint 11 independent pass. **A narrowing, and the first one on this line**: R-1 was a strict widening and R-2 to R-4 changed no shape at all. `version` (40), `states[].name` (200), `states[].task.taskDefinitionKey` (64), `states[].task.taskName` (200) and `variables[].key` (64) are bounds a document already had to respect to be storable, so **no document that could ever have run is refused by them** — what changes is that one which could not is refused at save instead of at run time. §4.2 settles the other half the opposite way: Kelir's own `CHECK` forbade a construct this specification permits, and the constraint was dropped rather than the construct. |

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
    "version": { "type": "string", "pattern": "^1\\.[0-9]+\\.[0-9]+$", "maxLength": 40 },
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
        "name": { "type": "string", "minLength": 1, "maxLength": 200 },
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
        "taskDefinitionKey": { "type": "string", "pattern": "^[a-z][a-z0-9_]*$", "maxLength": 64 },
        "taskName": { "type": "string", "minLength": 1, "maxLength": 200 },
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
        "requiresComment": { "type": "boolean", "default": false },
        "guards": { "type": "array", "items": { "$ref": "#/$defs/hookRegistrationEntry" } },
        "actions": { "type": "array", "items": { "$ref": "#/$defs/hookRegistrationEntry" } }
      },
      "if": { "properties": { "action": { "const": "AUTO" } } },
      "then": {
        "allOf": [
          { "not": { "required": ["allowedBy"] } },
          { "properties": { "requiresComment": { "const": false } } }
        ]
      },
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
        "key": { "type": "string", "pattern": "^[a-z][a-z0-9_]*$", "maxLength": 64 },
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
