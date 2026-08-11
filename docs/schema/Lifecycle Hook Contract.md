# Lifecycle Hook Contract Specification (LHCS)
**Version:** 1.0.0
**Status:** Draft Standard
**Target Stack:** Rust (Hook Resolver / Engine), Plugin Runtimes
**Last updated:** 2026-08-11

---

## 1. Introduction

The Lifecycle Hook Contract Specification (LHCS) defines the three JSON shapes shared by every participant in the document lifecycle hook system of [architectures/01 §12](../architectures/01.%20Basic%20Framework%20Concept%20and%20Architecture.md):

1. the **Hook Registration Entry** — how a handler is attached to a hook, identical across all four authoring surfaces (core, document type configuration, workflow definition `guards`/`actions`, plugin manifest);
2. the **Hook Invocation Payload** — the context object every handler receives;
3. the **Hook Result** — what a `before_*` handler returns (`CONTINUE` / `MODIFY` / `REJECT`).

This contract is the **plugin ABI** of the lifecycle: a handler written against it runs unchanged whether registered by a plugin, a document type, or a workflow transition. Breaking changes to these shapes are major-version changes to this specification.

### 1.1 Definitions

| Term | Definition |
| :--- | :--- |
| **Hook** | A named extension point at a lifecycle stage (`before_document_submit`). The catalogue is architectures/01 §12.3. |
| **Handler** | The unit of code invoked for one registration entry. Identified by a Handler Reference (§2). |
| **Chain** | All handlers registered for one hook name, merged across sources, ordered by priority. |
| **Before Hook** | Synchronous, inside the caller's transaction; may veto or mutate. Names start `before_`. |
| **After Hook** | Asynchronous, post-commit, dispatched via the outbox; fire-and-forget with retries. Names start `after_`. |
| **Source** | Where a registration came from: `CORE`, `DOCUMENT_TYPE`, `WORKFLOW`, `PLUGIN`. |

### 1.2 Conformance

The key words **MUST**, **MUST NOT**, **REQUIRED**, **SHALL**, **SHOULD**, **MAY**, and **OPTIONAL** are to be interpreted as described in [RFC 2119](https://www.rfc-editor.org/rfc/rfc2119).

An implementation is **conformant** if it:

1. Validates every registration entry against the LHCS Meta-Schema before accepting it (publish, plugin enable, or configuration save).
2. Delivers to every handler an invocation payload valid against §4, and interprets results exactly per §5.
3. Executes chains with the ordering, timeout, isolation, and logging rules of architectures/01 §12.5 — those execution semantics are normative for this contract even though they are specified there.
4. Records every execution in `document_hook_executions` regardless of source.

Where this document and the Meta-Schema disagree, **the Meta-Schema is normative**.

---

## 2. Handler Reference Grammar

A handler reference is a string naming executable code:

```text
core:<handler_name>                     e.g. core:require_attachment
plugin:<pluginId>:<handler_name>        e.g. plugin:erp-connector:reserve_budget
```

- `<pluginId>` is the kebab-case plugin id (the `plugin.json` `pluginId` field, stored as `plugins.plugin_code`).
- `<handler_name>` is `snake_case`.
- Pattern: `^(core:[a-z][a-z0-9_]*|plugin:[a-z][a-z0-9-]*:[a-z][a-z0-9_]*)$`
- A reference MUST resolve at registration time: unknown `core:` handlers are an ERROR; `plugin:` handlers of an unknown plugin are an ERROR, of a disabled plugin a WARNING (the entry stays registered but inert until the plugin is enabled).

---

## 3. Hook Registration Entry

The single shape used by document type configuration (`document_lifecycle_hooks`), workflow `guards`/`actions` (JWSS §7), and plugin manifests (`plugin_hooks`).

| Property | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `hook` | `string` | Yes* | Hook name from the catalogue (architectures/01 §12.3). *In JWSS `guards`/`actions` the hook name is implied (`before_workflow_transition` / `after_workflow_transition`) and MAY be omitted; if present it MUST match the implied name. |
| `handler` | `string` | Yes | Handler Reference (§2). |
| `priority` | `integer` | No | Execution order, ascending. Defaults to the floor of the source's band (§3.1). |
| `config` | `object` | No (default `{}`) | Handler-specific configuration, passed verbatim in the invocation payload. |
| `isEnabled` | `boolean` | No (default `true`) | Disabled entries stay registered but are skipped by the resolver. |

### 3.1 Priority Bands

| Band | Source |
| :--- | :--- |
| 0 – 99 | `CORE` |
| 100 – 299 | `DOCUMENT_TYPE` |
| 300 – 499 | `WORKFLOW` |
| 500 + | `PLUGIN` |

Lower runs first; ties resolve by registration order. Entries outside their source's band are accepted with a WARNING — the bands are a convention that keeps merged chains predictable, not a hard constraint.

### 3.2 Kind Constraint

A `before_*` hook name MUST NOT be registered with an after-only semantic expectation and vice versa; the validator rejects a registration whose hook name is not in the catalogue for the entry's position (e.g. an `after_*` name inside JWSS `guards`).

---

## 4. Hook Invocation Payload

Every handler receives one JSON object. Field availability varies by stage; fields not applicable to the stage are `null`, never absent — handlers can rely on the full shape.

| Property | Type | Description |
| :--- | :--- | :--- |
| `hookName` | `string` | The hook being executed. |
| `stage` | `string` | Lifecycle stage enum: `CREATE`, `UPDATE`, `VALIDATE`, `ATTACHMENT`, `COMMENT`, `VERSION`, `SUBMIT`, `NUMBERING`, `WORKFLOW_SELECT`, `TRANSITION`, `TASK_ASSIGN`, `DECIDE`, `TASK_COMPLETE`, `DELEGATE`, `ESCALATE`, `POST`, `COMPLETE`, `CANCEL`, `ARCHIVE`, `PURGE`. |
| `source` | `string` | Registration source of this handler: `CORE`, `DOCUMENT_TYPE`, `WORKFLOW`, `PLUGIN`. |
| `tenantId` | `string` | — |
| `documentId` | `string` | — |
| `documentTypeKey` | `string` | — |
| `currentStatus` | `string` \| `null` | Platform status before the action. |
| `targetStatus` | `string` \| `null` | Platform status after the action, when the action changes status. |
| `actorUserId` | `string` \| `null` | `null` for system/scheduler-initiated stages (e.g. `PURGE`). |
| `formData` | `object` | Current form data payload (JFSS payload shape). |
| `metadata` | `object` | Promoted document metadata key–values. |
| `workflowContext` | `object` \| `null` | `{ "workflowKey", "workflowRevision", "instanceId", "state", "transition": { "from", "to", "action" } \| null, "taskId": string \| null }`. `null` before an instance exists and after it completes. |
| `subject` | `object` \| `null` | Stage-specific subject: the attachment (`ATTACHMENT`), comment (`COMMENT`), task (`TASK_*`, `DECIDE`), or version (`VERSION`) being acted on. `null` elsewhere. |
| `config` | `object` | The registration entry's `config`, verbatim. |
| `correlationId` | `string` | Propagated to logs, outbox events, and integration calls. |
| `invokedAt` | `string` (date-time) | — |

The payload is **read-only** except through the `MODIFY` result (§5). Handlers MUST NOT write `documents.status` or other engine-owned state directly (architectures/01 §12.5).

---

## 5. Hook Result

### 5.1 Before Hooks

A `before_*` handler MUST return:

| Property | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `result` | `string` | Yes | Enum: `CONTINUE`, `MODIFY`, `REJECT`. |
| `formData` | `object` | Conditional | REQUIRED for `MODIFY` when mutating form data; the engine replaces the payload's `formData` for the remainder of the chain and the action. MUST be omitted otherwise. |
| `metadata` | `object` | No | With `MODIFY`: replacement promoted metadata. |
| `rejectCode` | `string` | Conditional | REQUIRED for `REJECT`. Machine-readable, `SCREAMING_SNAKE_CASE` (e.g. `BUDGET_EXCEEDED`). |
| `rejectMessage` | `string` | Conditional | REQUIRED for `REJECT`. Human-readable. |
| `details` | `array` | No | With `REJECT`: `[{ "field", "message" }]` entries for field-level surfacing. |

Semantics:

- `CONTINUE` — chain proceeds unchanged.
- `MODIFY` — chain proceeds with the returned data. Later handlers see the modified payload. Modifications MUST still validate against the document's form schema; the engine re-validates after the chain.
- `REJECT` — chain stops, the transaction rolls back, the API returns the error of §6. A timeout is treated as `REJECT` with `rejectCode: "HOOK_TIMEOUT"`.

### 5.2 After Hooks

An `after_*` handler returns nothing meaningful to the chain — the action is already committed. It MUST be **idempotent** (delivery is at-least-once via the outbox) and reports only success/failure; failure triggers the outbox retry schedule and, on repeated failure, the circuit breaker (architectures/01 §12.5).

---

## 6. Rejection Error Mapping

A `REJECT` from any before-handler surfaces as the standard error envelope (SDD §12.3) with the hook's code and message nested:

```json
{
  "success": false,
  "error": {
    "code": "HOOK_REJECTED",
    "message": "Budget exceeded for cost center CC-4400",
    "details": [
      { "field": "amount", "message": "Remaining budget is 8,500,000" },
      { "field": "_hook",  "message": "BUDGET_EXCEEDED by core:check_budget (before_document_submit)" }
    ]
  }
}
```

- `error.code` is always `HOOK_REJECTED`; the handler's `rejectCode` appears in the `_hook` detail entry together with the handler reference and hook name.
- `error.message` is the handler's `rejectMessage`.
- Handler-supplied `details` entries pass through ahead of the `_hook` entry.

---

## 7. Execution Log Shape

Every execution — any source, any result — is recorded in `document_hook_executions` ([Database Schema](../design/02.%20Database%20Schema.md) §6.12) with the handler's `source`, `hook_name`, `handler_reference`, `result` (`CONTINUE` / `MODIFY` / `REJECT` / `ERROR`), `duration_ms`, and for workflow-sourced handlers the `workflow_transition_ref` (`<workflowKey>@<revision>:<from>-><to>`). `GET /documents/{id}/hooks/resolved` exposes the merged chain for debugging (architectures/01 §12.7).

---

## 8. Companion Documents

| Document | Role |
| :--- | :--- |
| [architectures/01 §12](../architectures/01.%20Basic%20Framework%20Concept%20and%20Architecture.md) | Hook catalogue, lifecycle stages, execution rules (normative for execution semantics) |
| [JSON Workflow Schema](JSON%20Workflow%20Schema.md) | `guards`/`actions` authoring surface embedding the Registration Entry |
| [architectures/04 §4.6](../architectures/04.%20Kelir%20Plugin%20and%20Extension%20Management%20Concept.md) | Plugin hook subscription and sandboxing |
| [Database Schema §6.11–6.12](../design/02.%20Database%20Schema.md) | Registry and execution log tables |

---

# LHCS v1.0.0 Meta-Schema

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "https://kelir.dev/schemas/lhcs-meta-v1.0.0.json",
  "title": "LHCS Lifecycle Hook Contract",
  "$defs": {
    "handlerReference": {
      "type": "string",
      "pattern": "^(core:[a-z][a-z0-9_]*|plugin:[a-z][a-z0-9-]*:[a-z][a-z0-9_]*)$"
    },
    "hookName": {
      "type": "string",
      "pattern": "^(before|after)_[a-z][a-z0-9_]*$"
    },
    "hookRegistrationEntry": {
      "type": "object",
      "required": ["handler"],
      "additionalProperties": false,
      "properties": {
        "hook": { "$ref": "#/$defs/hookName" },
        "handler": { "$ref": "#/$defs/handlerReference" },
        "priority": { "type": "integer", "minimum": 0, "maximum": 32767 },
        "config": { "type": "object", "default": {} },
        "isEnabled": { "type": "boolean", "default": true }
      }
    },
    "invocationPayload": {
      "type": "object",
      "required": ["hookName", "stage", "source", "tenantId", "documentId", "documentTypeKey",
                   "currentStatus", "targetStatus", "actorUserId", "formData", "metadata",
                   "workflowContext", "subject", "config", "correlationId", "invokedAt"],
      "additionalProperties": false,
      "properties": {
        "hookName": { "$ref": "#/$defs/hookName" },
        "stage": { "enum": ["CREATE", "UPDATE", "VALIDATE", "ATTACHMENT", "COMMENT", "VERSION",
                            "SUBMIT", "NUMBERING", "WORKFLOW_SELECT", "TRANSITION", "TASK_ASSIGN",
                            "DECIDE", "TASK_COMPLETE", "DELEGATE", "ESCALATE", "POST", "COMPLETE",
                            "CANCEL", "ARCHIVE", "PURGE"] },
        "source": { "enum": ["CORE", "DOCUMENT_TYPE", "WORKFLOW", "PLUGIN"] },
        "tenantId": { "type": "string" },
        "documentId": { "type": "string" },
        "documentTypeKey": { "type": "string" },
        "currentStatus": { "type": ["string", "null"] },
        "targetStatus": { "type": ["string", "null"] },
        "actorUserId": { "type": ["string", "null"] },
        "formData": { "type": "object" },
        "metadata": { "type": "object" },
        "workflowContext": {
          "type": ["object", "null"],
          "required": ["workflowKey", "workflowRevision", "instanceId", "state", "transition", "taskId"],
          "additionalProperties": false,
          "properties": {
            "workflowKey": { "type": "string" },
            "workflowRevision": { "type": "integer" },
            "instanceId": { "type": "string" },
            "state": { "type": "string" },
            "transition": {
              "type": ["object", "null"],
              "required": ["from", "to", "action"],
              "additionalProperties": false,
              "properties": {
                "from": { "type": "string" },
                "to": { "type": "string" },
                "action": { "type": "string" }
              }
            },
            "taskId": { "type": ["string", "null"] }
          }
        },
        "subject": { "type": ["object", "null"] },
        "config": { "type": "object" },
        "correlationId": { "type": "string" },
        "invokedAt": { "type": "string", "format": "date-time" }
      }
    },
    "beforeHookResult": {
      "type": "object",
      "required": ["result"],
      "additionalProperties": false,
      "properties": {
        "result": { "enum": ["CONTINUE", "MODIFY", "REJECT"] },
        "formData": { "type": "object" },
        "metadata": { "type": "object" },
        "rejectCode": { "type": "string", "pattern": "^[A-Z][A-Z0-9_]*$" },
        "rejectMessage": { "type": "string", "minLength": 1 },
        "details": {
          "type": "array",
          "items": {
            "type": "object",
            "required": ["field", "message"],
            "additionalProperties": false,
            "properties": {
              "field": { "type": "string" },
              "message": { "type": "string" }
            }
          }
        }
      },
      "allOf": [
        { "if": { "properties": { "result": { "const": "REJECT" } } },
          "then": { "required": ["result", "rejectCode", "rejectMessage"] } },
        { "if": { "properties": { "result": { "const": "CONTINUE" } } },
          "then": { "not": { "anyOf": [ { "required": ["formData"] }, { "required": ["metadata"] } ] } } }
      ]
    }
  }
}
```

The meta-schema will be extracted to `docs/schema/lhcs-meta-v1.0.0.json` when the registration validator is implemented; until then, this block is the normative artifact. Handler-reference resolution (§2), band warnings (§3.1), and kind constraints (§3.2) require registry state and MUST be enforced by the registration validator in code.
