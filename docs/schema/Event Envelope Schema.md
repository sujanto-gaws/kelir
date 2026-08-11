# Event Envelope Schema Specification (EES)
**Version:** 1.0.0
**Status:** Draft Standard
**Target Stack:** Rust (Outbox Worker / Webhook Dispatcher), External Consumers, Plugin Runtimes
**Last updated:** 2026-08-11

---

## 1. Introduction

The Event Envelope Schema Specification (EES) defines the single JSON shape in which every Kelir platform event travels once it leaves the transaction that produced it: through the transactional outbox to in-process consumers (notifications, search indexing, `after_*` lifecycle hooks), to plugin event subscriptions (PMS §6.2), and over HTTP to external webhook consumers (architectures/03 §2.5–2.6).

One envelope, three transports. A consumer written against this contract can move between transports without change, and an event recorded in `outbox_events` is byte-for-byte the event a webhook consumer receives as the request body.

### 1.1 Core Philosophy

- **Envelope / Payload Separation:** The envelope (identity, type, aggregate, actor, causality, time) is fixed by this specification; the `payload` varies per event type under the additivity rule (§4.3).
- **At-Least-Once, Idempotent by Id:** Delivery on every transport is at-least-once. `eventId` is the deduplication key; consumers MUST treat a repeated `eventId` as a duplicate, not a new occurrence.
- **Causality over Ordering:** Cross-aggregate ordering is not guaranteed. `correlationId` groups events of one business operation; `causationId` chains cause to effect; `sequence` orders events within one aggregate.
- **Dotted Event Names:** Event types use the dotted `PascalCase` vocabulary of the [naming convention](../standards/02.%20Naming%20Convention.md) §7 (`Document.Approved`). The undotted names in older integration examples (`DocumentApproved`) are superseded.

### 1.2 Definitions

| Term | Definition |
| :--- | :--- |
| **Envelope** | The outer object defined in Section 2. |
| **Payload** | The event-type-specific `payload` object (Section 4). |
| **Aggregate** | The business object the event is about (`DOCUMENT`, `TASK`, `PARTY`, …). |
| **Correlation Id** | Identifier shared by every event, log line, and integration call of one business operation. |
| **Causation Id** | The `eventId` of the event whose processing produced this event; `null` for user-initiated origins. |
| **Delivery** | One attempt to hand one envelope to one consumer (webhook POST, plugin handler call, worker dispatch). |

### 1.3 Conformance

The key words **MUST**, **MUST NOT**, **REQUIRED**, **SHALL**, **SHOULD**, **MAY**, and **OPTIONAL** are to be interpreted as described in [RFC 2119](https://www.rfc-editor.org/rfc/rfc2119).

A producer is **conformant** if every published envelope validates against the EES Meta-Schema and the payload rules of §4. A consumer is **conformant** if it deduplicates on `eventId`, tolerates unknown payload fields, and does not depend on cross-aggregate ordering. A webhook dispatcher is additionally conformant if it implements the transport binding of Section 5.

Where this document and the Meta-Schema disagree, **the Meta-Schema is normative**.

---

## 2. Envelope Structure

| Property | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `eventId` | `string` (UUID) | Yes | Unique per occurrence; the deduplication key. Stable across redeliveries. |
| `version` | `string` | Yes | The EES specification version of this envelope (`"1.0.0"`). A consumer for the 1.x line MUST reject a different major version. |
| `eventType` | `string` | Yes | Dotted `PascalCase` name (`Document.Approved`). |
| `occurredAt` | `string` (date-time) | Yes | When the event happened (transaction commit time), not when it was delivered. |
| `tenantId` | `string` | Yes | — |
| `aggregateType` | `string` | Yes | Enum: `DOCUMENT`, `TASK`, `WORKFLOW_INSTANCE`, `ATTACHMENT`, `COMMENT`, `PARTY`, `FACILITY`, `PRODUCT`, `SERVICE`, `USER`, `PLUGIN`, `SYSTEM`. |
| `aggregateId` | `string` | Yes | Id of the aggregate row. |
| `sequence` | `integer` \| `null` | Yes | Monotonic per (`aggregateType`, `aggregateId`); `null` when the producer cannot sequence (e.g. `SYSTEM` events). |
| `correlationId` | `string` | Yes | Propagated from the originating request; also present in `integration_logs` and `document_hook_executions`. |
| `causationId` | `string` \| `null` | Yes | `eventId` of the causing event, `null` at the origin. |
| `actor` | `object` | Yes | `{ "actorType": "USER" \| "SYSTEM" \| "WORKFLOW_ENGINE" \| "INTEGRATION" \| "SCHEDULER" \| "PLUGIN", "actorId": string \| null }`. |
| `payload` | `object` | Yes | Event-type-specific body (Section 4). MAY be `{}` but MUST be present. |

The envelope is exactly what is stored in `outbox_events.payload_json` at publish time and what a webhook consumer receives as the raw HTTP body — producers MUST NOT re-serialize differently per transport (the webhook signature is computed over these bytes, §5.2).

---

## 3. Event Naming and Catalogue

- Names follow naming convention §7: `<Entity>.<PastTenseAction>`, entity is the business subject (`Supplier.Created` even though storage is party-based).
- A prefix wildcard `<Entity>.*` is valid **only in subscriptions** (webhook `event_types_json`, PMS `events[].event`), never in an envelope.
- The authoritative catalogue is the activity event vocabulary (concepts/02 §15) plus the lifecycle events of architectures/01 §12.3. Core families: `Document.*`, `Attachment.*`, `Comment.*`, `Workflow.*`, `Security.*`, master-data entities (`Supplier.*`, `Customer.*`, `Employee.*`, `Facility.*`, `Product.*`, `Service.*`), `Plugin.*`.
- New event types MAY be introduced by minor platform versions and by plugins (namespaced by their subject entity). Consumers MUST ignore event types they do not know.

---

## 4. Payload Profiles

Payloads are defined per event family as **profiles**: a minimum guaranteed field set. Producers MAY add fields; the additivity rule (§4.3) governs evolution.

### 4.1 Core Profiles

**`Document.*`**

| Field | Type | Notes |
| :--- | :--- | :--- |
| `documentId` | `string` | — |
| `documentRef` | `string` | `DOC-2026-000123` |
| `documentNumber` | `string` \| `null` | `null` before numbering |
| `documentTypeKey` | `string` | — |
| `status` | `string` | Platform status after the event |
| `previousStatus` | `string` \| `null` | — |
| `title` | `string` | — |

**`Workflow.*`**

| Field | Type | Notes |
| :--- | :--- | :--- |
| `instanceId` | `string` | — |
| `workflowKey` | `string` | — |
| `workflowRevision` | `integer` | — |
| `documentId` | `string` | — |
| `state` | `string` | Current workflow state |
| `taskId` | `string` \| `null` | Set for `Workflow.Task*` events |
| `assigneeUserId` | `string` \| `null` | Set for assignment/delegation events |
| `action` | `string` \| `null` | Set for completion events |

**`Attachment.*`** — `attachmentId`, `documentId`, `fileName`, `category` (`string|null`), `fileSize`.

**`Comment.*`** — `commentId`, `documentId`, `commentType`, `mentionedUserIds` (`string[]`).

**Master data (`Supplier.*`, `Customer.*`, `Employee.*`, `Facility.*`, `Product.*`, `Service.*`)** — `entityType`, `entityId`, `entityCode` (`string|null`), `recordStatus`, `changedByDocumentId` (`string|null`).

**`Security.*`** — `objectType`, `objectId`, `outcome` (`"ALLOWED" | "DENIED"`).

### 4.2 Sensitive Data Rule

Payloads MUST NOT carry form data, attachments content, credentials, or personal data beyond the profile fields. Consumers needing more MUST call back through the API with their own authorization — the envelope is a notification, not a data feed. (This mirrors the masked-logging governance of architectures/03.)

### 4.3 Additivity Rule

Within a major EES version, payload profiles only gain optional fields; fields are never removed, renamed, or retyped. Consumers MUST ignore unknown fields. Breaking a profile requires a major version bump of this specification.

---

## 5. Transport Bindings

### 5.1 Outbox and Plugin Delivery

The outbox worker dispatches envelopes to in-process consumers and plugin event handlers (`plugin_events` rows). The plugin handler receives the envelope as its argument together with the subscription's `config`. Failures follow the outbox retry schedule; exhausted retries dead-letter the delivery, never the event.

### 5.2 Webhook HTTP Binding

One delivery is one `POST` to the subscription's `target_url`:

| Header | Value |
| :--- | :--- |
| `Content-Type` | `application/json` |
| `X-Kelir-Event` | The `eventType` |
| `X-Kelir-Event-Id` | The `eventId` |
| `X-Kelir-Delivery` | Unique id of this delivery attempt (`webhook_events.id`) |
| `X-Kelir-Timestamp` | Unix seconds at send time |
| `X-Kelir-Signature` | `sha256=<hex>` — HMAC-SHA256 over `<timestamp>.<raw body>` using the subscription secret |

Consumer obligations:

1. Verify the signature against the **raw** body before parsing; reject on mismatch.
2. Reject deliveries whose `X-Kelir-Timestamp` is outside a tolerance window (RECOMMENDED: 300 seconds) to prevent replay.
3. Respond `2xx` within the dispatcher timeout to acknowledge. Any other response or a timeout schedules a retry per the subscription's retry policy; exhausted retries mark the delivery `DEAD_LETTER` (`webhook_events`, Database Schema §12.7).
4. Deduplicate on `eventId` — retries and redeliveries reuse it (`X-Kelir-Delivery` changes, `X-Kelir-Event-Id` does not).

---

## 6. Validation Rules

| Rule | Level | Requirement |
| :--- | :--- | :--- |
| **S1** | ERROR | Envelope validates against the EES Meta-Schema; `version` major is supported. |
| **S2** | ERROR | `eventType` matches the dotted pattern and contains no wildcard. |
| **S3** | ERROR | `payload` carries at least the profile fields of its event family (§4.1). |
| **S4** | ERROR | Envelope bytes published to a transport are identical to the stored `outbox_events.payload_json`. |
| **S5** | ERROR | Payload contains none of the prohibited content classes of §4.2 (enforced by producer review, spot-checked by the dispatcher's masking filter). |
| **S6** | WARNING | `sequence` is `null` for a sequenceable aggregate type. |

---

## 7. Example

```json
{
  "eventId": "018f4c2e-9b7a-7c3d-8e5f-2a1b3c4d5e6f",
  "version": "1.0.0",
  "eventType": "Document.Approved",
  "occurredAt": "2026-08-11T10:30:00Z",
  "tenantId": "TNT-001",
  "aggregateType": "DOCUMENT",
  "aggregateId": "3f2b8a10-4d5e-7f80-9a1b-c2d3e4f50617",
  "sequence": 7,
  "correlationId": "req-9d81c9e2",
  "causationId": null,
  "actor": { "actorType": "USER", "actorId": "user.sara" },
  "payload": {
    "documentId": "3f2b8a10-4d5e-7f80-9a1b-c2d3e4f50617",
    "documentRef": "DOC-2026-000123",
    "documentNumber": "PR-2026-000123",
    "documentTypeKey": "purchase_requisition",
    "status": "APPROVED",
    "previousStatus": "PENDING_APPROVAL",
    "title": "Purchase Laptop for Finance Team",
    "decision": "APPROVE",
    "decisionLevel": "FINANCE"
  }
}
```

(`decision` and `decisionLevel` are producer additions on top of the `Document.*` profile — legal under §4.3.)

---

## 8. Companion Documents

| Document | Role |
| :--- | :--- |
| [Naming Convention §7](../standards/02.%20Naming%20Convention.md) | Event name vocabulary |
| [Plugin Manifest Schema §6.2](Plugin%20Manifest%20Schema.md) | Plugin event subscriptions consuming this envelope |
| [Lifecycle Hook Contract](Lifecycle%20Hook%20Contract.md) | `after_*` hooks dispatched from the same outbox |
| [architectures/03 §2.5–2.6](../architectures/03.%20Kelir%20Modules%20for%20Interfacing%20with%20External%20Systems.md) | Webhook and event bus modules |
| [Database Schema §12.7–12.9](../design/02.%20Database%20Schema.md) | `webhook_events`, `outbox_events`, `inbox_events` storage |

---

# EES v1.0.0 Meta-Schema

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "https://kelir.dev/schemas/ees-meta-v1.0.0.json",
  "title": "EES Event Envelope",
  "type": "object",
  "required": ["eventId", "version", "eventType", "occurredAt", "tenantId", "aggregateType",
               "aggregateId", "sequence", "correlationId", "causationId", "actor", "payload"],
  "additionalProperties": false,
  "properties": {
    "eventId": { "type": "string", "format": "uuid" },
    "version": { "type": "string", "pattern": "^1\\.[0-9]+\\.[0-9]+$" },
    "eventType": { "type": "string", "pattern": "^[A-Z][A-Za-z0-9]*\\.[A-Z][A-Za-z0-9]*$" },
    "occurredAt": { "type": "string", "format": "date-time" },
    "tenantId": { "type": "string", "minLength": 1 },
    "aggregateType": { "enum": ["DOCUMENT", "TASK", "WORKFLOW_INSTANCE", "ATTACHMENT", "COMMENT",
                                "PARTY", "FACILITY", "PRODUCT", "SERVICE", "USER", "PLUGIN", "SYSTEM"] },
    "aggregateId": { "type": "string", "minLength": 1 },
    "sequence": { "type": ["integer", "null"], "minimum": 1 },
    "correlationId": { "type": "string", "minLength": 1 },
    "causationId": { "type": ["string", "null"] },
    "actor": {
      "type": "object",
      "required": ["actorType", "actorId"],
      "additionalProperties": false,
      "properties": {
        "actorType": { "enum": ["USER", "SYSTEM", "WORKFLOW_ENGINE", "INTEGRATION",
                                "SCHEDULER", "PLUGIN"] },
        "actorId": { "type": ["string", "null"] }
      }
    },
    "payload": { "type": "object" }
  }
}
```

The meta-schema will be extracted to `docs/schema/ees-meta-v1.0.0.json` when the dispatcher is implemented; until then, this block is the normative artifact. Profile field guarantees (S3), byte-identity across transports (S4), and the sensitive-data rule (S5) are beyond JSON Schema expressiveness and MUST be enforced by the producer and dispatcher in code.
