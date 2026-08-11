# Plugin Manifest Schema Specification (PMS)
**Version:** 1.0.0
**Status:** Draft Standard
**Target Stack:** Rust (Plugin Loader / Registry), Plugin Authors
**Last updated:** 2026-08-11

---

## 1. Introduction

The Plugin Manifest Schema Specification (PMS) defines the structure of `plugin.json`, the manifest every Kelir plugin ships at its package root ([architectures/04](../architectures/04.%20Kelir%20Plugin%20and%20Extension%20Management%20Concept.md) §6–7). The manifest is the **complete declaration of what a plugin is and may do**: identity, compatibility, entrypoints, requested permissions, extension point registrations, lifecycle hook handlers, event subscriptions, settings schema, dependencies, and integrity data.

The plugin loader validates the manifest against this specification during discovery — a plugin whose manifest does not validate MUST NOT progress past the `DISCOVERED` state to `VALIDATED` (architectures/04 §4.3). The accepted manifest is stored verbatim in `plugin_versions.manifest_json` ([Database Schema](../design/02.%20Database%20Schema.md) §13.2); on enable, its `hooks` and `events` entries are materialized into `plugin_hooks` and `plugin_events`.

### 1.1 Core Philosophy

- **Declare Everything, Grant Explicitly:** A plugin can only do what its manifest declares, and only after an administrator grants the declared permissions. Undeclared capability use is a runtime security error.
- **One Hook Contract:** Hook handler declarations use the Hook Registration Entry of the [Lifecycle Hook Contract](Lifecycle%20Hook%20Contract.md) — the same shape as document type configuration and JWSS `guards`/`actions`, so a handler moves between surfaces without rewriting.
- **Immutable per Version:** A manifest describes exactly one plugin version. Changing anything means publishing a new version with a new checksum.
- **Settings as Schema:** Plugins never define settings tables; they declare a `settingsSchema` and the platform stores, validates, and encrypts values (`plugin_settings`, architectures/04 §4.7).

### 1.2 Definitions

| Term | Definition |
| :--- | :--- |
| **Manifest** | The `plugin.json` document at the plugin package root. |
| **Plugin Id** | Kebab-case unique identifier (`contract-management`); doubles as the `plugin_code` registry key. |
| **Pseudo-Plugin** | A built-in core capability addressable as a dependency (`document-core`, `workflow-core`). Never installed; always version-matched against the platform. |
| **Extension Point** | A named registration slot (backend `register_*`, frontend kebab-case slot) from the catalogue of architectures/04 §4.6 / §5.3. |
| **Entrypoint** | The executable artifact reference per side. v1 backend entrypoints reference compiled-in Rust crates; Phase 3+ MAY reference `.wasm` artifacts. |

### 1.3 Conformance

The key words **MUST**, **MUST NOT**, **REQUIRED**, **SHALL**, **SHOULD**, **MAY**, and **OPTIONAL** are to be interpreted as described in [RFC 2119](https://www.rfc-editor.org/rfc/rfc2119).

An implementation is **conformant** if it:

1. Accepts every manifest that validates against the PMS Meta-Schema for its declared `manifestVersion`, and rejects every one that does not.
2. Enforces every install-time rule of Section 8 before the plugin reaches `VALIDATED`.
3. Registers at enable time exactly the hooks, events, and extension points the manifest declares — no more, no fewer.
4. Refuses at runtime any capability use not covered by a declared **and granted** permission.

Where this document and the Meta-Schema disagree, **the Meta-Schema is normative**.

---

## 2. Root Schema Structure

| Property | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `manifestVersion` | `string` | Yes | The PMS specification version this manifest conforms to (`"1.0.0"`). A validator for the 1.x line MUST reject a different major version. Distinct from `version`, which is the plugin's own version. |
| `pluginId` | `string` | Yes | Kebab-case, globally unique (`contract-management`). Pattern `^[a-z][a-z0-9-]*$`, max 60. |
| `name` | `string` | Yes | Display name. |
| `version` | `string` | Yes | The plugin's own version, strict semver (`1.0.0`). |
| `publisher` | `string` | Yes | Publisher display name. |
| `category` | `string` | Yes | Enum: `FEATURE`, `DOCUMENT_TYPE`, `WORKFLOW`, `MASTER_DATA`, `INTEGRATION`, `UI`, `FORM_FIELD`, `REPORT`, `NOTIFICATION`, `AUTH`, `STORAGE`, `COMPLIANCE`, `THEME`, `LOCALIZATION`. |
| `description` | `string` | No | — |
| `kelirVersion` | `string` | Yes | Semver range of compatible platform versions (`">=1.0.0"`). |
| `entrypoints` | `object` | Yes | Section 3. At least one of `backend` / `frontend`. |
| `permissions` | `array` | No (default `[]`) | Requested permission codes (Section 4). |
| `extensionPoints` | `array` | No (default `[]`) | Extension point names the plugin registers into (Section 5). |
| `hooks` | `array` | No (default `[]`) | Lifecycle hook handler declarations (Section 6.1). |
| `events` | `array` | No (default `[]`) | Platform event subscriptions (Section 6.2). |
| `settingsSchema` | `object` | No (default `{}`) | Map of setting key → Setting Declaration (Section 7). |
| `dependencies` | `array` | No (default `[]`) | `{ pluginId, version }` pairs; `version` is a semver range. May reference pseudo-plugins. |
| `migrations` | `array` | No (default `[]`) | Ordered migration identifiers (`NNNN_snake_case`) shipped under `migrations/` in the package. |
| `assets` | `object` | No | `{ "icon": path, "logo": path }` — package-relative paths. |
| `checksum` | `string` | Yes | `sha256:`-prefixed digest of the package content (manifest excluded). |
| `signature` | `string` | No | Publisher signature over the checksum. REQUIRED for non-official plugins on installations that enforce signing. |

---

## 3. Entrypoints

| Property | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `backend` | `string` | No | v1: compiled-in Rust crate reference (`backend/contract_management`). Phase 3+: package-relative `.wasm` path. |
| `frontend` | `string` | No | Package-relative JS module path (`frontend/contract-management.js`), loaded as the plugin's frontend bundle. |

At least one entrypoint MUST be present. A manifest declaring `hooks`, backend `register_*` extension points, or `migrations` MUST declare a `backend` entrypoint; one declaring frontend extension points MUST declare a `frontend` entrypoint (S6).

---

## 4. Permissions

Two vocabularies may appear, distinguished by prefix:

| Kind | Pattern | Examples | Governs |
| :--- | :--- | :--- | :--- |
| Plugin-runtime | `plugin:<kebab-verb-phrase>` | `plugin:send-http-request`, `plugin:read-secrets`, `plugin:register-job` | What the plugin runtime itself may do |
| Core | `module[:resource]:action` (naming convention §6) | `document:read`, `workflow:task:execute`, `master-data:party:update` | What the plugin may do to platform data |

Rules:

- Every permission the plugin ever exercises MUST be listed; the runtime denies undeclared use even if an administrator would have granted it.
- Declared permissions are written to `plugin_permissions` with `is_granted = false`; handlers execute with granted permissions only (architectures/04 §4.5, architectures/01 §12.5).
- Wildcards are not permitted in manifests.

---

## 5. Extension Points

`extensionPoints` entries MUST come from the published catalogue:

- **Backend** (`snake_case`): `register_document_type`, `register_master_data_entity`, `register_form_field`, `register_list_column`, `register_menu`, `register_dashboard_widget`, `register_document_tab`, `register_task_action`, `register_workflow_handler`, `register_workflow_condition`, `register_numbering_rule`, `register_integration_connector`, `register_master_data_sync`, `register_notification_channel`, `register_storage_driver`, `register_auth_provider`, `register_report`.
- **Frontend** (`kebab-case`): `sidebar-menu`, `dashboard-widget`, `document-tab`, `document-action-button`, `master-data-tab`, `task-action-button`, `admin-page`, `settings-page`, `report-widget`, `form-field`, `table-column`, `notification-panel`, `search-result-item`.

Unknown names are an install-time ERROR — the catalogue is versioned with the platform, and `kelirVersion` gates which names exist.

---

## 6. Hooks and Events

### 6.1 Lifecycle Hook Declarations

Each `hooks` entry is a **Hook Registration Entry** per the [Lifecycle Hook Contract](Lifecycle%20Hook%20Contract.md) §3, with two manifest-specific constraints:

1. `hook` is REQUIRED (there is no implied hook name, unlike JWSS `guards`/`actions`).
2. `priority` defaults to 500 and SHOULD lie in the plugin band (≥ 500, architectures/01 §12.4.3).

The `handler` reference MUST use the manifest's own plugin id: `plugin:<pluginId>:<handler_name>`. On enable, entries are materialized into `plugin_hooks`; on disable they are deactivated.

```json
{
  "hooks": [
    { "hook": "after_document_approve",
      "handler": "plugin:contract-management:register_contract",
      "priority": 510,
      "config": { "documentTypes": ["CONTRACT_APPROVAL"] } }
  ]
}
```

### 6.2 Event Subscriptions

Each `events` entry subscribes a handler to a platform event (dotted `PascalCase`, naming convention §7). Delivery is asynchronous via the outbox; handlers MUST be idempotent.

| Property | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `event` | `string` | Yes | Event name (`Document.Approved`) or prefix wildcard (`Document.*`). |
| `handler` | `string` | Yes | Handler Reference (`plugin:<pluginId>:<handler_name>`). |
| `config` | `object` | No (default `{}`) | Passed to the handler with each delivery. |

The difference from §6.1: hooks participate in the document lifecycle chain (and `before_*` hooks can veto); event subscriptions are pure reactions to the event stream and can never affect the originating action.

---

## 7. Settings Schema

`settingsSchema` maps setting keys (pattern `^[a-z][a-z0-9_]*$`) to Setting Declarations:

| Property | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `type` | `string` | Yes | Enum: `string`, `number`, `integer`, `boolean`, `enum`, `json`, `secret`. |
| `label` | `string` | No | Display label for the settings UI. |
| `description` | `string` | No | — |
| `default` | any | No | MUST match `type`. Forbidden for `secret`. |
| `required` | `boolean` | No (default `false`) | Enabling the plugin fails while a required setting has no value. |
| `options` | `array` | Conditional | REQUIRED for `type: "enum"`: `[{ "label", "value" }]`. |
| `tenantScoped` | `boolean` | No (default `true`) | `false` = one global value across tenants. |

`secret` values are stored encrypted with `plugin_settings.is_secret = true` and are readable at runtime only when the plugin holds the `plugin:read-secrets` permission.

---

## 8. Install-Time Validation Rules

A manifest failing any ERROR rule MUST NOT reach `VALIDATED`.

| Rule | Level | Requirement |
| :--- | :--- | :--- |
| **S1** | ERROR | Manifest validates against the PMS Meta-Schema; `manifestVersion` major is supported. |
| **S2** | ERROR | `pluginId` matches the registry entry being installed into (no id changes across versions). |
| **S3** | ERROR | `version` is strict semver and greater than any previously registered version of this plugin. |
| **S4** | ERROR | The running platform version satisfies `kelirVersion`. |
| **S5** | ERROR | `checksum` matches the package content; if a `signature` is present or required, it verifies. |
| **S6** | ERROR | Entrypoint coverage: backend-side declarations require `backend`; frontend extension points require `frontend` (§3). |
| **S7** | ERROR | Every `hooks[].hook` is in the lifecycle hook catalogue; every `extensionPoints[]` name is in the extension point catalogue. |
| **S8** | ERROR | Every `hooks[].handler` and `events[].handler` references this manifest's own `pluginId`. |
| **S9** | ERROR | All `dependencies` resolve: pseudo-plugins against the platform version, real plugins against installed versions satisfying the range. Dependency cycles are an ERROR. |
| **S10** | ERROR | `settingsSchema` defaults match their declared types; `enum` declarations carry `options`; `secret` declarations carry no `default`. |
| **S11** | WARNING | `hooks[].priority` below 500 (outside the plugin band). |
| **S12** | WARNING | Requested permission not recognized by this platform version (kept for forward compatibility, never grantable). |

---

## 9. Example

```json
{
  "manifestVersion": "1.0.0",
  "pluginId": "contract-management",
  "name": "Contract Management",
  "version": "1.2.0",
  "publisher": "Kelir Official",
  "category": "FEATURE",
  "description": "Adds contract lifecycle management capabilities.",
  "kelirVersion": ">=1.0.0",
  "entrypoints": {
    "backend": "backend/contract_management",
    "frontend": "frontend/contract-management.js"
  },
  "permissions": [
    "document:read",
    "document:write",
    "attachment:read",
    "workflow:task:execute",
    "notification:send",
    "plugin:register-job"
  ],
  "extensionPoints": [
    "document-tab",
    "dashboard-widget",
    "register_workflow_handler",
    "register_report"
  ],
  "hooks": [
    { "hook": "after_document_approve",
      "handler": "plugin:contract-management:register_contract",
      "priority": 510,
      "config": { "documentTypes": ["CONTRACT_APPROVAL"] } }
  ],
  "events": [
    { "event": "Document.Completed",
      "handler": "plugin:contract-management:index_contract" }
  ],
  "settingsSchema": {
    "contract_retention_years": {
      "type": "integer", "label": "Contract retention (years)",
      "default": 7, "required": true
    },
    "enable_contract_renewal_reminder": {
      "type": "boolean", "default": true
    },
    "esign_api_token": {
      "type": "secret", "label": "E-signature API token", "required": false
    }
  },
  "dependencies": [
    { "pluginId": "document-core", "version": ">=1.0.0" },
    { "pluginId": "workflow-core", "version": ">=1.0.0" }
  ],
  "migrations": ["0001_contract_register"],
  "assets": { "icon": "assets/icon.svg", "logo": "assets/logo.svg" },
  "checksum": "sha256:9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
  "signature": "..."
}
```

---

## 10. Companion Documents

| Document | Role |
| :--- | :--- |
| [architectures/04](../architectures/04.%20Kelir%20Plugin%20and%20Extension%20Management%20Concept.md) | Plugin platform: lifecycle states, permission model, catalogue authority |
| [Lifecycle Hook Contract](Lifecycle%20Hook%20Contract.md) | Registration entry shape reused by `hooks` |
| [JSON Workflow Schema](JSON%20Workflow%20Schema.md) | Sibling standard; plugin workflow handlers are referenced from JWSS `guards`/`actions` |
| [Database Schema §13](../design/02.%20Database%20Schema.md) | `plugin_versions.manifest_json`, `plugin_hooks`, `plugin_events`, `plugin_permissions`, `plugin_settings` |

---

# PMS v1.0.0 Meta-Schema

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "https://kelir.dev/schemas/pms-meta-v1.0.0.json",
  "title": "PMS Plugin Manifest",
  "type": "object",
  "required": ["manifestVersion", "pluginId", "name", "version", "publisher",
               "category", "kelirVersion", "entrypoints", "checksum"],
  "additionalProperties": false,
  "properties": {
    "manifestVersion": { "type": "string", "pattern": "^1\\.[0-9]+\\.[0-9]+$" },
    "pluginId": { "type": "string", "pattern": "^[a-z][a-z0-9-]*$", "maxLength": 60 },
    "name": { "type": "string", "minLength": 1 },
    "version": { "$ref": "#/$defs/semver" },
    "publisher": { "type": "string", "minLength": 1 },
    "category": { "enum": ["FEATURE", "DOCUMENT_TYPE", "WORKFLOW", "MASTER_DATA", "INTEGRATION",
                           "UI", "FORM_FIELD", "REPORT", "NOTIFICATION", "AUTH", "STORAGE",
                           "COMPLIANCE", "THEME", "LOCALIZATION"] },
    "description": { "type": "string" },
    "kelirVersion": { "$ref": "#/$defs/semverRange" },
    "entrypoints": {
      "type": "object",
      "additionalProperties": false,
      "minProperties": 1,
      "properties": {
        "backend": { "type": "string", "minLength": 1 },
        "frontend": { "type": "string", "minLength": 1 }
      }
    },
    "permissions": {
      "type": "array", "default": [], "uniqueItems": true,
      "items": { "type": "string",
                 "pattern": "^(plugin:[a-z][a-z0-9-]*|[a-z][a-z0-9-]*(:[a-z][a-z0-9-]*){1,2})$" }
    },
    "extensionPoints": {
      "type": "array", "default": [], "uniqueItems": true,
      "items": { "type": "string", "pattern": "^([a-z][a-z0-9_]*|[a-z][a-z0-9-]*)$" }
    },
    "hooks": {
      "type": "array", "default": [],
      "items": {
        "allOf": [
          { "$ref": "https://kelir.dev/schemas/lhcs-meta-v1.0.0.json#/$defs/hookRegistrationEntry" },
          { "required": ["hook", "handler"] }
        ]
      }
    },
    "events": {
      "type": "array", "default": [],
      "items": {
        "type": "object",
        "required": ["event", "handler"],
        "additionalProperties": false,
        "properties": {
          "event": { "type": "string",
                     "pattern": "^[A-Z][A-Za-z0-9]*\\.([A-Z][A-Za-z0-9]*|\\*)$" },
          "handler": { "$ref": "https://kelir.dev/schemas/lhcs-meta-v1.0.0.json#/$defs/handlerReference" },
          "config": { "type": "object", "default": {} }
        }
      }
    },
    "settingsSchema": {
      "type": "object", "default": {},
      "propertyNames": { "pattern": "^[a-z][a-z0-9_]*$" },
      "additionalProperties": { "$ref": "#/$defs/settingDeclaration" }
    },
    "dependencies": {
      "type": "array", "default": [],
      "items": {
        "type": "object",
        "required": ["pluginId", "version"],
        "additionalProperties": false,
        "properties": {
          "pluginId": { "type": "string", "pattern": "^[a-z][a-z0-9-]*$" },
          "version": { "$ref": "#/$defs/semverRange" }
        }
      }
    },
    "migrations": {
      "type": "array", "default": [],
      "items": { "type": "string", "pattern": "^[0-9]{4}_[a-z][a-z0-9_]*$" }
    },
    "assets": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "icon": { "type": "string" },
        "logo": { "type": "string" }
      }
    },
    "checksum": { "type": "string", "pattern": "^sha256:[0-9a-f]{64}$" },
    "signature": { "type": "string" }
  },
  "$defs": {
    "semver": { "type": "string",
                "pattern": "^(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)$" },
    "semverRange": { "type": "string", "minLength": 1 },
    "settingDeclaration": {
      "type": "object",
      "required": ["type"],
      "additionalProperties": false,
      "properties": {
        "type": { "enum": ["string", "number", "integer", "boolean", "enum", "json", "secret"] },
        "label": { "type": "string" },
        "description": { "type": "string" },
        "default": {},
        "required": { "type": "boolean", "default": false },
        "options": {
          "type": "array", "minItems": 1,
          "items": {
            "type": "object",
            "required": ["label", "value"],
            "additionalProperties": false,
            "properties": { "label": { "type": "string" }, "value": {} }
          }
        },
        "tenantScoped": { "type": "boolean", "default": true }
      },
      "allOf": [
        { "if": { "properties": { "type": { "const": "enum" } } },
          "then": { "required": ["type", "options"] } },
        { "if": { "properties": { "type": { "const": "secret" } } },
          "then": { "not": { "required": ["default"] } } }
      ]
    }
  }
}
```

The meta-schema will be extracted to `docs/schema/pms-meta-v1.0.0.json` when the install validator is implemented; until then, this block is the normative artifact. Checksum verification (S5), catalogue membership (S7), self-reference of handlers (S8), dependency resolution (S9), and default-type agreement (S10) require package and registry state and MUST be enforced by the install validator in code.
