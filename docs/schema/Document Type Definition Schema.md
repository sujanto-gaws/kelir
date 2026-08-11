# Document Type Definition Schema Specification (DTDS)
**Version:** 1.0.0
**Status:** Draft Standard
**Target Stack:** Rust (Document Type Service), Vue.js (Document Type Builder)
**Last updated:** 2026-08-11

---

## 1. Introduction

The Document Type Definition Schema Specification (DTDS) defines the aggregate JSON document that describes one Kelir **document type**: its identity, form and list bindings, numbering rule, workflow selection rules, attachment requirements, lifecycle hook registrations, and master-data change configuration ([architectures/01](../architectures/01.%20Basic%20Framework%20Concept%20and%20Architecture.md) §10.4, §12.4.1).

Like the Party aggregate (architectures/05), the DTDS document is an **exchange and authoring shape, not a storage shape**: the document type builder edits it, import/export and the `POST /document-types` API carry it, and the platform normalizes it into `document_types` and its child tables ([Database Schema](../design/02.%20Database%20Schema.md) §6.2–6.5, §6.11). `GET /document-types/{id}` re-projects the aggregate from those tables.

### 1.1 Core Philosophy

- **Configuration Is the Application:** A complete business application (form + list + numbering + workflow + attachments + hooks) is one DTDS document. Creating a new document-based application means authoring one of these, not writing code.
- **Bind by Key, Pin at Use:** The aggregate references forms, lists, and workflows by their stable keys. Revision pinning happens at runtime — a document pins its form revision at creation; workflow selection resolves to the latest `ACTIVE` workflow revision at submit time.
- **One Condition Language:** All conditions (`workflowSelectionRules[].condition`, `attachmentRules[].requiredIf`) are JSON Logic restricted to the [JFSS Calculation Rule Registry](JFSS%20Calculation%20Rule%20Registry.md), evaluated against the same context as JWSS conditions. String expressions in older examples are superseded.
- **Hooks, Not Switches:** Behavior beyond configuration attaches through Hook Registration Entries ([Lifecycle Hook Contract](Lifecycle%20Hook%20Contract.md) §3) in the document-type priority band.

### 1.2 Definitions

| Term | Definition |
| :--- | :--- |
| **Document Type** | A configured class of business documents (`purchase_requisition`), the unit of RAD application delivery. |
| **Selection Rule** | A prioritized, conditioned mapping from this document type to a workflow key, evaluated at submit by `before_workflow_select`. |
| **Fallback Rule** | The single selection rule without a `condition`; matches when no conditioned rule does. |
| **Numbering Template** | The token string producing business numbers (`PR-{year}-{sequence}`). |
| **Master-Data Change Type** | A document type whose completion writes to a master data registry (concepts/03). |

### 1.3 Conformance

The key words **MUST**, **MUST NOT**, **REQUIRED**, **SHALL**, **SHOULD**, **MAY**, and **OPTIONAL** are to be interpreted as described in [RFC 2119](https://www.rfc-editor.org/rfc/rfc2119).

An implementation is **conformant** if it:

1. Accepts every aggregate that validates against the DTDS Meta-Schema for its declared `version`, and rejects every one that does not.
2. Enforces every rule of Section 8 before activating a document type.
3. Evaluates selection and attachment conditions with registry operators only, producing identical outcomes to any other conformant implementation.
4. Round-trips the aggregate: normalizing to tables and re-projecting yields a semantically identical document.

Where this document and the Meta-Schema disagree, **the Meta-Schema is normative**.

---

## 2. Root Schema Structure

| Property | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `documentTypeKey` | `string` | Yes | Unique, immutable key. Pattern `^[a-z][a-z0-9_]*$`, max 60. Normalizes to `document_types.type_code` (upper-cased). |
| `version` | `string` | Yes | The DTDS specification version (`"1.0.0"`). Not a revision of this document type. |
| `name` | `string` | Yes | Display name. |
| `description` | `string` | No | — |
| `category` | `string` | No | Grouping label (`PROCUREMENT`, `HR`, `MASTER_DATA`). |
| `formKey` | `string` | Yes | JFSS `formId` of the bound form. The referenced form MUST have a `PUBLISHED` revision (S2). |
| `listKey` | `string` | No | `rad_lists.list_key` of the bound list; omitted = platform default document list. |
| `numbering` | `object` | Yes | Numbering Rule (Section 3). |
| `workflowSelectionRules` | `array` | Yes | Selection Rules (Section 4). Minimum 1. |
| `attachmentRules` | `array` | No (default `[]`) | Attachment Rules (Section 5). |
| `hooks` | `array` | No (default `[]`) | Hook Registration Entries (Section 6). |
| `masterData` | `object` | No | Master-Data Change configuration (Section 7). Present only for master-data change types. |
| `defaultSecurityLevel` | `string` | No (default `INTERNAL`) | Enum: `PUBLIC`, `INTERNAL`, `CONFIDENTIAL`, `RESTRICTED`. |
| `defaultPriority` | `string` | No (default `NORMAL`) | Enum: `LOW`, `NORMAL`, `HIGH`, `URGENT`. |
| `retentionPolicyCode` | `string` | No | `retention_policies.policy_code`; omitted = tenant default. |

---

## 3. Numbering Rule

| Property | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `template` | `string` | Yes | Token string, e.g. `"PR-{year}-{sequence}"`. Grammar in §3.1. |
| `sequenceScope` | `string` | No (default `YEAR`) | Enum: `GLOBAL`, `YEAR`, `MONTH`, `DEPARTMENT_YEAR` — when the sequence resets. |
| `sequencePadding` | `integer` | No (default `6`) | Zero-padding width of `{sequence}`. 1–12. |

### 3.1 Template Grammar

A template is literal text plus tokens in braces. Recognized tokens:

| Token | Expands to |
| :--- | :--- |
| `{year}` | 4-digit year at assignment time |
| `{month}` | 2-digit month |
| `{sequence}` | Next sequence in the rule's scope, zero-padded |
| `{department}` | Department code of `requested_for_department_id` (REQUIRED in the template when `sequenceScope` is `DEPARTMENT_YEAR`) |
| `{typeCode}` | The document type's `type_code` |

A template MUST contain `{sequence}` exactly once (S5). Unknown tokens are an ERROR. Numbers are assigned once at Submit, between the `before_document_number_assign` and `after_document_number_assign` hooks, and never reassigned (numbering state: `document_type_numbering_rules`, Database Schema §6.3).

---

## 4. Workflow Selection Rules

Evaluated at submit by the `before_workflow_select` stage: ascending `priority`, first rule whose `condition` is `true` wins; the fallback rule is evaluated last regardless of priority.

| Property | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `workflowKey` | `string` | Yes | JWSS `workflowKey`. Resolves to the latest `ACTIVE` revision at submit time. |
| `condition` | `object` | No | JSON Logic over the condition context (JWSS §6.1). Absent = fallback rule. |
| `priority` | `integer` | No (default `100`) | Lower evaluated first. |

Exactly one rule MUST omit `condition` (S3) — every document type always has a selectable workflow, so `before_workflow_select` can only fail by hook veto, never by exhaustion.

```json
"workflowSelectionRules": [
  { "workflowKey": "purchase_requisition_standard",
    "condition": { "<=": [ { "var": "document.amount" }, 10000000 ] }, "priority": 10 },
  { "workflowKey": "purchase_requisition_high_value",
    "condition": { ">":  [ { "var": "document.amount" }, 10000000 ] }, "priority": 20 },
  { "workflowKey": "purchase_requisition_standard" }
]
```

---

## 5. Attachment Rules

Enforced by the core `before_document_submit` handler `core:require_attachment`; normalized into `document_type_attachment_rules` (Database Schema §6.5).

| Property | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `category` | `string` | Yes | `attachment_categories.category_code` (`QUOTATION`). MUST exist (S4). |
| `requiredIf` | `object` | No | JSON Logic; absent = always required. |
| `maxFileSize` | `integer` | No | Bytes; omitted = platform default. |
| `allowedMimeTypes` | `array` | No (default `[]` = all permitted types) | e.g. `["application/pdf"]`. |

```json
"attachmentRules": [
  { "category": "QUOTATION",
    "requiredIf": { ">": [ { "var": "document.amount" }, 10000000 ] },
    "allowedMimeTypes": ["application/pdf"] }
]
```

---

## 6. Hooks

Each entry is a **Hook Registration Entry** per the [Lifecycle Hook Contract](Lifecycle%20Hook%20Contract.md) §3 with `hook` REQUIRED, normalized into `document_lifecycle_hooks` scoped to this document type. `priority` defaults to 100 and SHOULD lie in the document-type band 100–299 (architectures/01 §12.4.3).

```json
"hooks": [
  { "hook": "before_document_approve", "handler": "core:check_approval_limit", "priority": 200 },
  { "hook": "after_document_complete",
    "handler": "plugin:erp-connector:post_purchase_requisition", "priority": 500 }
]
```

---

## 7. Master-Data Change Configuration

Present only when completing a document of this type writes to a master data registry (concepts/03 §5).

| Property | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `entityType` | `string` | Yes | Enum: `PARTY`, `SUPPLIER`, `CUSTOMER`, `EMPLOYEE`, `FACILITY`, `PRODUCT`, `SERVICE`. |
| `allowedActions` | `array` | Yes | Non-empty subset of `CREATE`, `UPDATE`, `DEACTIVATE`. |

The document's `entity_type` / `entity_id` / `target_action` columns (Database Schema §6.6) are constrained by this configuration; the write itself happens in the `before_document_post` / `after_document_post` stage via the core master-data handler, stamping `created_by_document_id` / `last_updated_by_document_id`.

---

## 8. Activation Validation Rules

A document type failing any ERROR rule MUST remain `DRAFT`.

| Rule | Level | Requirement |
| :--- | :--- | :--- |
| **S1** | ERROR | Aggregate validates against the DTDS Meta-Schema; `version` major is supported. |
| **S2** | ERROR | `formKey` references a form with a `PUBLISHED` revision; `listKey` (if present) references an `ACTIVE` list. |
| **S3** | ERROR | Every `workflowSelectionRules[].workflowKey` references a workflow with an `ACTIVE` revision; exactly one rule omits `condition`. |
| **S4** | ERROR | Every `attachmentRules[].category` and `retentionPolicyCode` (if present) exists. |
| **S5** | ERROR | Numbering template: `{sequence}` exactly once, all tokens recognized, `{department}` present iff `sequenceScope` is `DEPARTMENT_YEAR`. |
| **S6** | ERROR | All `condition` / `requiredIf` logic uses registry operators only, and every `{"var": ...}` path resolves within the condition context or the bound form's data keys. |
| **S7** | ERROR | Every `hooks[].hook` is in the lifecycle hook catalogue; handler references resolve (unknown plugin = ERROR, disabled = WARNING). |
| **S8** | ERROR | `masterData` present iff the type writes master data; `category: "MASTER_DATA"` without a `masterData` block is an ERROR. |
| **S9** | WARNING | `hooks[].priority` outside the document-type band 100–299. |
| **S10** | WARNING | Changing `numbering.template` or `sequenceScope` on an active type (already-issued numbers are never reassigned; sequences continue per §3.1). |

---

## 9. Example

```json
{
  "documentTypeKey": "purchase_requisition",
  "version": "1.0.0",
  "name": "Purchase Requisition",
  "description": "Request to purchase goods or services",
  "category": "PROCUREMENT",
  "formKey": "purchase_requisition_form",
  "listKey": "purchase_requisition_list",
  "numbering": { "template": "PR-{year}-{sequence}", "sequenceScope": "YEAR", "sequencePadding": 6 },
  "workflowSelectionRules": [
    { "workflowKey": "purchase_requisition_standard",
      "condition": { "<=": [ { "var": "document.amount" }, 10000000 ] }, "priority": 10 },
    { "workflowKey": "purchase_requisition_high_value",
      "condition": { ">":  [ { "var": "document.amount" }, 10000000 ] }, "priority": 20 },
    { "workflowKey": "purchase_requisition_standard" }
  ],
  "attachmentRules": [
    { "category": "QUOTATION",
      "requiredIf": { ">": [ { "var": "document.amount" }, 10000000 ] },
      "allowedMimeTypes": ["application/pdf"] }
  ],
  "hooks": [
    { "hook": "before_document_submit", "handler": "core:check_budget", "priority": 150,
      "config": { "budgetSource": "cost_center" } },
    { "hook": "after_document_complete",
      "handler": "plugin:erp-connector:post_purchase_requisition", "priority": 500 }
  ],
  "defaultSecurityLevel": "INTERNAL",
  "retentionPolicyCode": "FINANCE_10Y"
}
```

---

## 10. Companion Documents

| Document | Role |
| :--- | :--- |
| [JSON Form Schema (JFSS)](JSON%20Form%20Schema.md) | The bound form definition (`formKey`) |
| [JSON Workflow Schema (JWSS)](JSON%20Workflow%20Schema.md) | The selectable workflows and the shared condition context |
| [Lifecycle Hook Contract (LHCS)](Lifecycle%20Hook%20Contract.md) | `hooks` entry shape |
| [JFSS Calculation Rule Registry](JFSS%20Calculation%20Rule%20Registry.md) | Permitted condition operators |
| [architectures/01 §10.4, §12](../architectures/01.%20Basic%20Framework%20Concept%20and%20Architecture.md) | Document type concept and lifecycle |
| [Database Schema §6](../design/02.%20Database%20Schema.md) | Normalized storage tables |

---

# DTDS v1.0.0 Meta-Schema

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "https://kelir.dev/schemas/dtds-meta-v1.0.0.json",
  "title": "DTDS Document Type Definition",
  "type": "object",
  "required": ["documentTypeKey", "version", "name", "formKey", "numbering", "workflowSelectionRules"],
  "additionalProperties": false,
  "properties": {
    "documentTypeKey": { "type": "string", "pattern": "^[a-z][a-z0-9_]*$", "maxLength": 60 },
    "version": { "type": "string", "pattern": "^1\\.[0-9]+\\.[0-9]+$" },
    "name": { "type": "string", "minLength": 1 },
    "description": { "type": "string" },
    "category": { "type": "string", "pattern": "^[A-Z][A-Z0-9_]*$" },
    "formKey": { "type": "string", "minLength": 1 },
    "listKey": { "type": "string" },
    "numbering": {
      "type": "object",
      "required": ["template"],
      "additionalProperties": false,
      "properties": {
        "template": { "type": "string", "pattern": "^(?=(?:[^{]*\\{sequence\\}){1}(?!.*\\{sequence\\}.*\\{sequence\\}))[A-Za-z0-9._/-]*(\\{(year|month|sequence|department|typeCode)\\}[A-Za-z0-9._/-]*)+$" },
        "sequenceScope": { "enum": ["GLOBAL", "YEAR", "MONTH", "DEPARTMENT_YEAR"], "default": "YEAR" },
        "sequencePadding": { "type": "integer", "minimum": 1, "maximum": 12, "default": 6 }
      }
    },
    "workflowSelectionRules": {
      "type": "array",
      "minItems": 1,
      "items": {
        "type": "object",
        "required": ["workflowKey"],
        "additionalProperties": false,
        "properties": {
          "workflowKey": { "type": "string", "pattern": "^[a-z][a-z0-9_]*$" },
          "condition": { "type": "object" },
          "priority": { "type": "integer", "minimum": 0, "default": 100 }
        }
      }
    },
    "attachmentRules": {
      "type": "array",
      "default": [],
      "items": {
        "type": "object",
        "required": ["category"],
        "additionalProperties": false,
        "properties": {
          "category": { "type": "string", "pattern": "^[A-Z][A-Z0-9_]*$" },
          "requiredIf": { "type": "object" },
          "maxFileSize": { "type": "integer", "exclusiveMinimum": 0 },
          "allowedMimeTypes": { "type": "array", "default": [],
                                "items": { "type": "string", "pattern": "^[a-z-]+/[A-Za-z0-9.+*-]+$" } }
        }
      }
    },
    "hooks": {
      "type": "array",
      "default": [],
      "items": {
        "allOf": [
          { "$ref": "https://kelir.dev/schemas/lhcs-meta-v1.0.0.json#/$defs/hookRegistrationEntry" },
          { "required": ["hook", "handler"] }
        ]
      }
    },
    "masterData": {
      "type": "object",
      "required": ["entityType", "allowedActions"],
      "additionalProperties": false,
      "properties": {
        "entityType": { "enum": ["PARTY", "SUPPLIER", "CUSTOMER", "EMPLOYEE",
                                 "FACILITY", "PRODUCT", "SERVICE"] },
        "allowedActions": { "type": "array", "minItems": 1, "uniqueItems": true,
                            "items": { "enum": ["CREATE", "UPDATE", "DEACTIVATE"] } }
      }
    },
    "defaultSecurityLevel": { "enum": ["PUBLIC", "INTERNAL", "CONFIDENTIAL", "RESTRICTED"],
                              "default": "INTERNAL" },
    "defaultPriority": { "enum": ["LOW", "NORMAL", "HIGH", "URGENT"], "default": "NORMAL" },
    "retentionPolicyCode": { "type": "string", "pattern": "^[A-Z][A-Z0-9_]*$" }
  }
}
```

The meta-schema will be extracted to `docs/schema/dtds-meta-v1.0.0.json` when the activation validator is implemented; until then, this block is the normative artifact. Reference resolution (S2–S4, S7), the single-fallback rule (S3), operator and var-path checks (S6), and master-data consistency (S8) require registry state and MUST be enforced by the activation validator in code. The `template` pattern approximates §3.1; the validator remains authoritative for token grammar.
