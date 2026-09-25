-- 0046_integration.sql — the external system registry and the rest of §12
-- (FR-INT-001; #520; Database Schema §12; architectures/03 §4).
--
-- # Eight tables, and three of them have a reader
--
-- Database Schema §12 declares nine integration tables. `outbox_events` left
-- first, in `0045_outbox.sql`, because its first reader was the hook chain's
-- after half; **this creates the other eight**. Only three are read or written
-- by anything in this release — `external_systems`, `integration_endpoints` and
-- `integration_credentials`, behind `/api/v1/integration/external-systems` —
-- and the other five (`integration_mappings`, `integration_logs`,
-- `webhook_subscriptions`, `webhook_events`, `inbox_events`) are DDL whose
-- writers are FR-INT-002 to FR-INT-006 (#520 AC-3). They land here because the
-- Database Schema puts all of §12 in one migration and splitting a second time
-- would be a decision about the plan, not about this row.
--
-- # A secret is never a value here
--
-- `integration_credentials.secret_reference` and
-- `webhook_subscriptions.secret_reference` hold **where** a secret lives —
-- `vault://kelir/erp/api-key`, `env://KELIR_ERP_API_KEY` — and never the
-- secret. Nothing in this schema can hold a resolved one, so no route can
-- return one. What the API checks about the shape of a reference is
-- `integration::domain::credential`'s to say, and it says it there.
--
-- # Deviations from §12's DDL
--
-- 1. **Every reference to `external_systems` carries `tenant_id` with it**, and
--    `webhook_events`' reference to `webhook_subscriptions` does too. §12 writes
--    them as `REFERENCES external_systems (id)`, which lets one tenant's
--    endpoint, credential or log row name another tenant's system. Nothing
--    would read it back — every query filters `tenant_id` — but *unreadable* is
--    the weaker guarantee, and this project has settled on *unwritable* four
--    times (`0017`, `0020`, `0021`, `0042`; Database Schema §14 #19 and #20).
--    So `external_systems` and `webhook_subscriptions` each gain
--    `UNIQUE (id, tenant_id)`, which bounds no value and exists only to be the
--    reference target, and the keys are composite. **`MATCH SIMPLE` is the
--    default and it is what keeps the nullable references working**: a log,
--    subscription or inbox row with no `external_system_id` references nothing.
-- 2. **`fk_master_data_source_references_external_system_id` is composite for
--    the same reason**, under the name §12.1 gives it. It is the key
--    `0008_master_data.sql` has deferred since Sprint 5 (its comment says
--    `0016`, the number planned then). **Nothing has ever written
--    `master_data_source_references`**, so the table is empty on every
--    deployment and adding a validated key cannot fail.
-- 3. The foreign keys are **named** (`fk_<table>_<column>_tenant_id`, the
--    `0017` form), where §12's inline `REFERENCES` would have taken
--    PostgreSQL's generated names — naming convention §4.3.
-- 4. **`integration_credentials` is indexed by `(tenant_id,
--    external_system_id)`, not §12.3's `(external_system_id) WHERE is_active`.**
--    Every query the credential routes run — the list, its count and the
--    single read — filters `tenant_id` and `external_system_id` and **none
--    filters `is_active`**, because an administrator is shown inactive
--    references too. A partial index on `is_active` serves none of them, so
--    all three were sequential scans of every tenant's credentials; found by
--    `migration-author` at review. The replacement leads with the tenant, as
--    every query does, and also serves an active-only lookup, which is only a
--    narrower filter over the same prefix. It is non-partial for the same
--    reason `deleted_at` is left out of it: a key a query filters on is an
--    index column, and a predicate the index does not share is a scan.
--
-- Column types, defaults, `CHECK`s, `ON DELETE CASCADE` on the pure child rows
-- and every other index are §12's as written. Two things §12 leaves open are left open
-- here rather than decided in DDL: `external_systems.system_type` has a
-- vocabulary in a comment and no `CHECK` (the API holds it to that list), and
-- several bounded values sit in `TEXT` columns (`system_name`, `base_url`,
-- `path`, `secret_reference`) — the API bounds them, see
-- `integration::domain`.
--
-- # Eight permissions, not eleven (#520, the product owner's answers)
--
-- * `integration:external-system:create`, `:read`, `:update`, `:deactivate` —
--   **deactivate, and no delete.** A delete would have to decide what becomes
--   of the `master_data_source_references` rows the key above points at the
--   system, which is its own decision. `:deactivate` is gated apart from
--   `:update`, on `workflow:definition:publish`'s precedent (`0025`).
-- * **Endpoints have no permissions of their own.** An endpoint is part of a
--   system's configuration: `:read` shows them and `:update` changes them.
-- * `integration:credential:create`, `:read`, `:update`, `:delete` — **a
--   reference is readable, under its own permission**, so that seeing a system
--   does not mean seeing where its secrets live.
--
-- Ids continue `0002_identity.sql`'s permission block, which stood at `...0066`
-- after `0043`.
--
-- # N−1 compatibility
--
-- Eight new tables, two `UNIQUE (id, tenant_id)` targets on tables this
-- migration creates, one foreign key on a table nothing writes, eight
-- permission rows and their grants. Nothing existing is altered in a way a
-- reader can observe, and nothing is dropped. The `v0.8.0` image, and a
-- 0.9-dev image at `0045`, name none of it in any statement and start against
-- this schema unchanged (release process §6). A tenant created by an older
-- image before this runs keeps the grants it was provisioned with; the new
-- rows are granted to the system tenant's `ROLE-ADMIN`, as every permission
-- migration since `0010` has done.

-- ---------------------------------------------------------------------------
-- §12.1 external_systems
-- ---------------------------------------------------------------------------

CREATE TABLE external_systems (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID        REFERENCES users (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID        REFERENCES users (id),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    system_code     VARCHAR(64) NOT NULL,           -- 'SAP_ERP'
    system_name     TEXT        NOT NULL,
    system_type     TEXT,                           -- ERP | CRM | HRIS | E_SIGNATURE | EMAIL_PROVIDER
                                                    -- | SSO_PROVIDER | PAYMENT_GATEWAY | TAX_SYSTEM
                                                    -- | BANK_SYSTEM | BI_SYSTEM | DOCUMENT_ARCHIVE
    base_url        TEXT,
    auth_type       TEXT        CHECK (auth_type IN ('API_KEY', 'BASIC_AUTH', 'BEARER_TOKEN',
                                                     'OAUTH2_CLIENT_CREDENTIALS', 'JWT', 'HMAC_SECRET',
                                                     'CERTIFICATE', 'SFTP_PASSWORD')),
    timeout_seconds INTEGER     NOT NULL DEFAULT 30,
    retry_policy_json JSONB     NOT NULL DEFAULT '{}',  -- maxRetries, initialDelaySeconds, backoffMultiplier,
                                                        -- deadLetterAfterAttempts
    description     TEXT,
    status          VARCHAR(40) NOT NULL DEFAULT 'ACTIVE'
                    CHECK (status IN ('ACTIVE', 'INACTIVE', 'MAINTENANCE')),
    -- Deviation 1: the target every tenant-carrying key below references.
    CONSTRAINT uq_external_systems_id_tenant_id UNIQUE (id, tenant_id)
);

CREATE UNIQUE INDEX uq_external_systems_tenant_id_system_code
    ON external_systems (tenant_id, system_code) WHERE deleted_at IS NULL;

COMMENT ON TABLE external_systems IS
    'The registry of systems Kelir integrates with (FR-INT-001). Deactivated through POST /external-systems/{id}/deactivate, never deleted: master_data_source_references points at these rows.';

-- Deviation 2: §12.1's key, composite, on a table nothing has written.
ALTER TABLE master_data_source_references
    ADD CONSTRAINT fk_master_data_source_references_external_system_id
    FOREIGN KEY (external_system_id, tenant_id) REFERENCES external_systems (id, tenant_id);

-- ---------------------------------------------------------------------------
-- §12.2 integration_endpoints
-- ---------------------------------------------------------------------------

CREATE TABLE integration_endpoints (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID        REFERENCES users (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID        REFERENCES users (id),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    external_system_id UUID     NOT NULL,
    endpoint_code   VARCHAR(64) NOT NULL,           -- 'CREATE_PURCHASE_ORDER'
    name            VARCHAR(200) NOT NULL,
    method          TEXT        NOT NULL CHECK (method IN ('GET', 'POST', 'PUT', 'PATCH', 'DELETE')),
    path            TEXT        NOT NULL,
    description     TEXT,
    status          VARCHAR(40) NOT NULL DEFAULT 'ACTIVE'
                    CHECK (status IN ('ACTIVE', 'INACTIVE')),
    CONSTRAINT fk_integration_endpoints_external_system_id_tenant_id
        FOREIGN KEY (external_system_id, tenant_id)
        REFERENCES external_systems (id, tenant_id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX uq_integration_endpoints_system_endpoint_code
    ON integration_endpoints (external_system_id, endpoint_code) WHERE deleted_at IS NULL;

-- ---------------------------------------------------------------------------
-- §12.3 integration_credentials
-- ---------------------------------------------------------------------------

CREATE TABLE integration_credentials (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID        REFERENCES users (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID        REFERENCES users (id),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    external_system_id UUID     NOT NULL,
    credential_type TEXT        NOT NULL CHECK (credential_type IN ('API_KEY', 'BASIC_AUTH', 'BEARER_TOKEN',
                                                'OAUTH2_CLIENT_CREDENTIALS', 'JWT', 'HMAC_SECRET',
                                                'CERTIFICATE', 'SFTP_PASSWORD')),
    secret_reference TEXT       NOT NULL,           -- 'vault://kelir/erp/api-key'
    valid_from      DATE,
    valid_to        DATE,
    is_active       BOOLEAN     NOT NULL DEFAULT true,
    CONSTRAINT fk_integration_credentials_external_system_id_tenant_id
        FOREIGN KEY (external_system_id, tenant_id)
        REFERENCES external_systems (id, tenant_id) ON DELETE CASCADE
);

-- Deviation 4: the credential routes' predicate, not §12.3's `WHERE is_active`.
CREATE INDEX idx_integration_credentials_tenant_id_external_system_id
    ON integration_credentials (tenant_id, external_system_id);

COMMENT ON COLUMN integration_credentials.secret_reference IS
    'Where the secret lives (vault://..., env://...), never the secret. Readable under integration:credential:read.';

-- ---------------------------------------------------------------------------
-- §12.4 integration_mappings — DDL only; no writer in this release
-- ---------------------------------------------------------------------------

CREATE TABLE integration_mappings (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID        REFERENCES users (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID        REFERENCES users (id),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    mapping_code    VARCHAR(64) NOT NULL,
    external_system_id UUID     NOT NULL,
    source_entity   TEXT        NOT NULL,           -- 'purchase_requisition'
    target_object   TEXT        NOT NULL,           -- 'PurchaseOrder'
    direction       TEXT        NOT NULL DEFAULT 'OUTBOUND'
                    CHECK (direction IN ('INBOUND', 'OUTBOUND', 'BIDIRECTIONAL')),
    field_mappings_json JSONB   NOT NULL DEFAULT '[]',  -- [{sourceField, targetField, transform}]
    value_maps_json JSONB       NOT NULL DEFAULT '{}',
    is_enabled      BOOLEAN     NOT NULL DEFAULT true,
    CONSTRAINT fk_integration_mappings_external_system_id_tenant_id
        FOREIGN KEY (external_system_id, tenant_id)
        REFERENCES external_systems (id, tenant_id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX uq_integration_mappings_tenant_id_mapping_code
    ON integration_mappings (tenant_id, mapping_code) WHERE deleted_at IS NULL;

-- ---------------------------------------------------------------------------
-- §12.5 integration_logs — append-only, masked payloads; FR-INT-006 writes it
-- ---------------------------------------------------------------------------

CREATE TABLE integration_logs (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    external_system_id UUID,
    direction       TEXT        NOT NULL CHECK (direction IN ('INBOUND', 'OUTBOUND')),
    integration_type TEXT,                          -- REST | SOAP | FILE | WEBHOOK | QUEUE
    endpoint        TEXT,
    method          TEXT,
    correlation_id  TEXT        NOT NULL,
    document_id     UUID        REFERENCES documents (id),
    entity_type     VARCHAR(64),
    entity_id       UUID,
    request_payload_json  JSONB,                    -- masked
    response_payload_json JSONB,                    -- masked
    status_code     INTEGER,
    status          VARCHAR(40) NOT NULL
                    CHECK (status IN ('SUCCESS', 'FAILED', 'PENDING', 'RETRYING', 'DEAD_LETTER')),
    error_message   TEXT,
    started_at      TIMESTAMPTZ NOT NULL,
    completed_at    TIMESTAMPTZ,
    duration_ms     INTEGER,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT fk_integration_logs_external_system_id_tenant_id
        FOREIGN KEY (external_system_id, tenant_id)
        REFERENCES external_systems (id, tenant_id)
);

CREATE INDEX idx_integration_logs_correlation_id ON integration_logs (correlation_id);
CREATE INDEX idx_integration_logs_tenant_system_started
    ON integration_logs (tenant_id, external_system_id, started_at);

COMMENT ON TABLE integration_logs IS
    'Append-only. request_payload_json and response_payload_json are stored masked (architectures/03 governance rules).';

-- ---------------------------------------------------------------------------
-- §12.6 webhook_subscriptions — DDL only; FR-INT-005 writes it
-- ---------------------------------------------------------------------------

CREATE TABLE webhook_subscriptions (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID        REFERENCES users (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID        REFERENCES users (id),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    external_system_id UUID,
    target_url      TEXT        NOT NULL,
    secret_reference TEXT       NOT NULL,           -- HMAC signing secret reference
    event_types_json JSONB      NOT NULL DEFAULT '[]',  -- ['Document.Approved', 'Workflow.TaskCompleted']
    auth_type       TEXT        NOT NULL DEFAULT 'HMAC_SIGNATURE',
    status          VARCHAR(40) NOT NULL DEFAULT 'ACTIVE'
                    CHECK (status IN ('ACTIVE', 'PAUSED', 'DISABLED')),
    CONSTRAINT fk_webhook_subscriptions_external_system_id_tenant_id
        FOREIGN KEY (external_system_id, tenant_id)
        REFERENCES external_systems (id, tenant_id),
    -- Deviation 1: the target webhook_events' tenant-carrying key references.
    CONSTRAINT uq_webhook_subscriptions_id_tenant_id UNIQUE (id, tenant_id)
);

CREATE INDEX idx_webhook_subscriptions_tenant_id ON webhook_subscriptions (tenant_id)
    WHERE deleted_at IS NULL;

-- ---------------------------------------------------------------------------
-- §12.7 webhook_events — outbound delivery queue and log; FR-INT-005 writes it
-- ---------------------------------------------------------------------------

CREATE TABLE webhook_events (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    subscription_id UUID        NOT NULL,
    event_type      VARCHAR(64) NOT NULL,
    payload_json    JSONB       NOT NULL,
    correlation_id  TEXT,
    status          VARCHAR(40) NOT NULL DEFAULT 'PENDING'
                    CHECK (status IN ('PENDING', 'DELIVERED', 'RETRYING', 'FAILED', 'DEAD_LETTER')),
    attempt_count   INTEGER     NOT NULL DEFAULT 0,
    next_attempt_at TIMESTAMPTZ,
    delivered_at    TIMESTAMPTZ,
    last_status_code INTEGER,
    last_error      TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT fk_webhook_events_subscription_id_tenant_id
        FOREIGN KEY (subscription_id, tenant_id)
        REFERENCES webhook_subscriptions (id, tenant_id) ON DELETE CASCADE
);

CREATE INDEX idx_webhook_events_pending
    ON webhook_events (next_attempt_at) WHERE status IN ('PENDING', 'RETRYING');

-- ---------------------------------------------------------------------------
-- §12.9 inbox_events — inbound, idempotent on the source's event id;
-- FR-INT-003 and FR-INT-004 write it
-- ---------------------------------------------------------------------------

CREATE TABLE inbox_events (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    external_system_id UUID,
    external_event_id TEXT      NOT NULL,           -- idempotency key from the source system
    event_type      VARCHAR(64) NOT NULL,
    payload_json    JSONB       NOT NULL,
    signature_valid BOOLEAN,
    status          VARCHAR(40) NOT NULL DEFAULT 'RECEIVED'
                    CHECK (status IN ('RECEIVED', 'PROCESSING', 'PROCESSED', 'FAILED', 'IGNORED')),
    processed_at    TIMESTAMPTZ,
    last_error      TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT fk_inbox_events_external_system_id_tenant_id
        FOREIGN KEY (external_system_id, tenant_id)
        REFERENCES external_systems (id, tenant_id)
);

CREATE UNIQUE INDEX uq_inbox_events_source_external_event_id
    ON inbox_events (external_system_id, external_event_id);

-- ---------------------------------------------------------------------------
-- Permissions (#520 AC-4, as the product owner's answers amend it)
-- ---------------------------------------------------------------------------

INSERT INTO permissions (id, tenant_id, permission_code, module, description) VALUES
    ('00000000-0000-0000-0001-000000000067', '00000000-0000-0000-0000-000000000001',
     'integration:external-system:create', 'integration', 'Register an external system'),
    ('00000000-0000-0000-0001-000000000068', '00000000-0000-0000-0000-000000000001',
     'integration:external-system:read', 'integration', 'View external systems and their endpoints'),
    ('00000000-0000-0000-0001-000000000069', '00000000-0000-0000-0000-000000000001',
     'integration:external-system:update', 'integration', 'Edit an external system and manage its endpoints'),
    ('00000000-0000-0000-0001-000000000070', '00000000-0000-0000-0000-000000000001',
     'integration:external-system:deactivate', 'integration', 'Activate or deactivate an external system'),
    ('00000000-0000-0000-0001-000000000071', '00000000-0000-0000-0000-000000000001',
     'integration:credential:create', 'integration', 'Add a credential reference to an external system'),
    ('00000000-0000-0000-0001-000000000072', '00000000-0000-0000-0000-000000000001',
     'integration:credential:read', 'integration', 'View credential references — where a system''s secrets live, never the secrets'),
    ('00000000-0000-0000-0001-000000000073', '00000000-0000-0000-0000-000000000001',
     'integration:credential:update', 'integration', 'Edit a credential reference'),
    ('00000000-0000-0000-0001-000000000074', '00000000-0000-0000-0000-000000000001',
     'integration:credential:delete', 'integration', 'Remove a credential reference');

-- ROLE-ADMIN holds every permission in the catalogue (0002_identity.sql); grant
-- only the eight new rows rather than re-inserting the ones already granted.
INSERT INTO role_permissions (id, tenant_id, role_id, permission_id)
SELECT
    gen_random_uuid(),
    '00000000-0000-0000-0000-000000000001',
    '00000000-0000-0000-0002-000000000001',
    id
FROM permissions
WHERE permission_code IN (
    'integration:external-system:create',
    'integration:external-system:read',
    'integration:external-system:update',
    'integration:external-system:deactivate',
    'integration:credential:create',
    'integration:credential:read',
    'integration:credential:update',
    'integration:credential:delete'
);
