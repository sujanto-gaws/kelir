-- 0015_document.sql — document types, documents and the lifecycle tables.
--
-- The whole of Database Schema §6 in one file, as its mapping table assigns it.
-- Only the document *type* has an endpoint in Sprint 7 (#157); documents
-- themselves are Sprint 9 under decision **D-16**. The tables are created
-- together for the reason 0008 and 0014 give: sqlx applies migrations in
-- version order, so splitting the group would consume a version number and buy
-- nothing.
--
-- **It applies after 0014_rad.sql, and that is not an accident of numbering.**
-- `document_types.form_id` references `rad_forms` and `list_id` references
-- `rad_lists`, both created by 0014. Migrations apply in version order, so this
-- file cannot precede it — which is the ordering constraint decision **D-2**
-- rests on, and the reason RAD's storage layer moved into Phase 4 at all.
--
-- Takes 0015 because that is the next free number after 0014 (naming
-- convention §4.3). The migrations planned after it shift down by one in the
-- Database Schema mapping table, which is the only place the sequence lives;
-- merged migrations are never renumbered.
--
-- Base columns and conventions per §1. Bounded string columns take a length
-- from §1.3.1 rather than TEXT, as 0008 (deviation #15) and 0014 (deviation
-- #17) did, and for the same reason: §6's DDL wrote `document_ref TEXT` beside
-- `status VARCHAR(40)`, and several of the affected columns sit inside unique
-- indexes, which is the failure 0004_string_lengths.sql exists to fix. Recorded
-- as deviation #18.
--
-- ---------------------------------------------------------------------------
-- Three forward references, and why each is shaped the way it is
-- ---------------------------------------------------------------------------
--
--   * **`document_type_workflows.workflow_definition_id` carries no foreign
--     key.** It points at `workflow_definitions`, which `0016_workflow.sql`
--     creates in Phase 5. The alternatives were both worse: creating that table
--     early would put it outside the phase that owns it and outside the design
--     that specifies it, and a deferred constraint is a constraint nothing
--     enforces while reading as though something does. The column exists, is
--     `NOT NULL`, and the constraint arrives with the workflow migration —
--     which is where §6.4 says it arrives.
--
--     **It stays NOT NULL, which differs from #157's acceptance criterion 3.**
--     That criterion asked for nullable; §6.4 specifies `NOT NULL`, and where a
--     plan and a `docs/` document disagree the document wins (projects/README).
--     The substance is unaffected either way, and NOT NULL is also the right
--     answer on its own terms: `document_type_workflows` is the join between a
--     type and a workflow, so a row naming no workflow is a binding that binds
--     nothing.
--
--   * **`document_type_attachment_rules.category_id` carries no foreign key**,
--     for the same reason: `attachment_categories` arrives with
--     `0017_attachment.sql` in Phase 6.
--
--   * **`documents.process_instance_id` carries no foreign key**, pointing at
--     `workflow_instances` in `0016_workflow.sql`.
--
-- `documents.current_version_id` is different in kind — it is circular with
-- `document_versions`, so its constraint is added by an ALTER at the foot of
-- this file rather than deferred to another migration. Both tables exist here.

-- ---------------------------------------------------------------------------
-- Retention (§6.1)
-- ---------------------------------------------------------------------------

CREATE TABLE retention_policies (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID        REFERENCES users (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID        REFERENCES users (id),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    policy_code     VARCHAR(64) NOT NULL,
    name            VARCHAR(200) NOT NULL,
    retention_years INTEGER     NOT NULL,
    action_on_expiry VARCHAR(40) NOT NULL DEFAULT 'PURGE_PAYLOAD'
                    CHECK (action_on_expiry IN ('PURGE_PAYLOAD', 'DELETE', 'REVIEW')),
    legal_hold_exempt BOOLEAN   NOT NULL DEFAULT false,
    description     TEXT,
    CONSTRAINT ck_retention_policies_years CHECK (retention_years >= 0)
);
CREATE UNIQUE INDEX uq_retention_policies_tenant_id_policy_code
    ON retention_policies (tenant_id, policy_code) WHERE deleted_at IS NULL;

-- ---------------------------------------------------------------------------
-- Document types and their bindings (§6.2-§6.5) — FR-DTYPE-001, 002, 003
-- ---------------------------------------------------------------------------
--
-- These tables plus the type-scoped rows of §6.11 are the normalized storage of
-- the Document Type Definition Schema aggregate.

CREATE TABLE document_types (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID        REFERENCES users (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID        REFERENCES users (id),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    type_code       VARCHAR(64) NOT NULL,           -- 'PURCHASE_REQUISITION', 'SUPPLIER_UPDATE'
    name            VARCHAR(200) NOT NULL,
    description     TEXT,
    category        VARCHAR(64),                    -- 'PROCUREMENT', 'HR', 'MASTER_DATA'
    -- The published JFSS form this type renders. Nullable: a type is
    -- configured before its form exists as often as after, and a type with no
    -- form is incomplete rather than invalid.
    form_id         UUID        REFERENCES rad_forms (id),
    list_id         UUID        REFERENCES rad_lists (id),
    default_security_level VARCHAR(40) NOT NULL DEFAULT 'INTERNAL'
                    CHECK (default_security_level IN ('PUBLIC', 'INTERNAL', 'CONFIDENTIAL', 'RESTRICTED')),
    retention_policy_id UUID    REFERENCES retention_policies (id),
    target_entity_type  VARCHAR(64),                -- set for master-data change document types
    status          VARCHAR(40) NOT NULL DEFAULT 'ACTIVE'
                    CHECK (status IN ('DRAFT', 'ACTIVE', 'DEPRECATED'))
);
CREATE UNIQUE INDEX uq_document_types_tenant_id_type_code
    ON document_types (tenant_id, type_code) WHERE deleted_at IS NULL;
-- The read the binding check makes: is this form still live, and is it mine.
CREATE INDEX idx_document_types_form_id ON document_types (form_id);

CREATE TABLE document_type_numbering_rules (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID        REFERENCES users (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID        REFERENCES users (id),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    document_type_id UUID       NOT NULL REFERENCES document_types (id) ON DELETE CASCADE,
    rule_template   VARCHAR(200) NOT NULL,          -- 'PR-{year}-{sequence}'
    sequence_scope  VARCHAR(40) NOT NULL DEFAULT 'YEAR'
                    CHECK (sequence_scope IN ('GLOBAL', 'YEAR', 'MONTH', 'DEPARTMENT_YEAR')),
    sequence_padding INTEGER    NOT NULL DEFAULT 6,
    next_sequence   BIGINT      NOT NULL DEFAULT 1,
    sequence_key    VARCHAR(64) NOT NULL DEFAULT '',    -- current scope bucket, e.g. '2026'
    is_active       BOOLEAN     NOT NULL DEFAULT true,
    -- A padding of 0 renders no number and a huge one renders a number no
    -- column holds; a sequence counts up from 1.
    CONSTRAINT ck_document_type_numbering_rules_padding CHECK (sequence_padding BETWEEN 1 AND 20),
    CONSTRAINT ck_document_type_numbering_rules_sequence CHECK (next_sequence >= 1)
);
CREATE UNIQUE INDEX uq_document_type_numbering_rules_active
    ON document_type_numbering_rules (document_type_id) WHERE is_active AND deleted_at IS NULL;

CREATE TABLE document_type_workflows (
    id                      UUID    PRIMARY KEY,
    tenant_id               UUID    NOT NULL REFERENCES tenants (id),
    created_by              UUID    REFERENCES users (id),
    created_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by              UUID    REFERENCES users (id),
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at              TIMESTAMPTZ,
    document_type_id        UUID    NOT NULL REFERENCES document_types (id) ON DELETE CASCADE,
    -- No REFERENCES: `workflow_definitions` arrives with 0016_workflow.sql in
    -- Phase 5. See the header — this is a decision, not an omission.
    workflow_definition_id  UUID    NOT NULL,
    condition_expression    TEXT,                   -- 'amount <= 10000000'; null = default
    priority                INTEGER NOT NULL DEFAULT 1, -- lower evaluated first, first match wins
    valid_from              DATE,
    valid_to                DATE,
    status                  VARCHAR(40) NOT NULL DEFAULT 'ACTIVE'
                            CHECK (status IN ('ACTIVE', 'INACTIVE')),
    -- A window that closes before it opens selects no workflow ever, and would
    -- be found by whoever wondered why a document never routed.
    CONSTRAINT ck_document_type_workflows_window
        CHECK (valid_from IS NULL OR valid_to IS NULL OR valid_from <= valid_to)
);
CREATE INDEX idx_document_type_workflows_document_type_id
    ON document_type_workflows (document_type_id, priority);

CREATE TABLE document_type_attachment_rules (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID        REFERENCES users (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID        REFERENCES users (id),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    document_type_id UUID       NOT NULL REFERENCES document_types (id) ON DELETE CASCADE,
    -- No REFERENCES: `attachment_categories` arrives with 0017_attachment.sql.
    category_id     UUID        NOT NULL,
    required_if_expression TEXT,                    -- 'amount > 10000000'; null = always required
    max_file_size   BIGINT,                         -- bytes; null = platform default
    allowed_mime_types_json JSONB NOT NULL DEFAULT '[]',
    is_enabled      BOOLEAN     NOT NULL DEFAULT true
);
CREATE INDEX idx_document_type_attachment_rules_document_type_id
    ON document_type_attachment_rules (document_type_id) WHERE deleted_at IS NULL;

-- ---------------------------------------------------------------------------
-- Documents (§6.6-§6.10) — no endpoint until Sprint 9 (D-16)
-- ---------------------------------------------------------------------------

CREATE TABLE documents (
    id                  UUID    PRIMARY KEY,
    tenant_id           UUID    NOT NULL REFERENCES tenants (id),
    created_by          UUID    REFERENCES users (id),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by          UUID    REFERENCES users (id),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at          TIMESTAMPTZ,
    document_ref        VARCHAR(64) NOT NULL,       -- DOC-2026-000123 (internal reference)
    document_number     VARCHAR(64),                -- PR-2026-000123; null until Submit assigns it
    document_type_id    UUID    NOT NULL REFERENCES document_types (id),
    -- The exact form revision, pinned at creation. This is what makes a
    -- published revision immutable *matter*: an old document re-renders
    -- against the definition it was filled in with.
    form_id             UUID    REFERENCES rad_forms (id),
    title               VARCHAR(200) NOT NULL,
    status              VARCHAR(40) NOT NULL DEFAULT 'DRAFT'
                        CHECK (status IN ('DRAFT', 'SUBMITTED', 'IN_REVIEW', 'PENDING_APPROVAL',
                                          'APPROVED', 'REJECTED', 'RETURNED', 'COMPLETED',
                                          'ARCHIVED', 'CANCELLED')),
    form_data_json      JSONB   NOT NULL DEFAULT '{}',
    current_version_id  UUID,                       -- FK added at the foot of this file
    -- No REFERENCES: `workflow_instances` arrives with 0016_workflow.sql.
    process_instance_id UUID,
    -- master-data change request context (concepts/03 §5.1)
    entity_type         VARCHAR(64),                -- 'SUPPLIER', 'EMPLOYEE', 'FACILITY', ...
    entity_id           UUID,
    target_action       VARCHAR(40) CHECK (target_action IN ('CREATE', 'UPDATE', 'DEACTIVATE')),
    -- request context
    priority            VARCHAR(40) NOT NULL DEFAULT 'NORMAL'
                        CHECK (priority IN ('LOW', 'NORMAL', 'HIGH', 'URGENT')),
    security_level      VARCHAR(40) NOT NULL DEFAULT 'INTERNAL'
                        CHECK (security_level IN ('PUBLIC', 'INTERNAL', 'CONFIDENTIAL', 'RESTRICTED')),
    requested_by        UUID    REFERENCES users (id),
    requested_for_department_id UUID REFERENCES departments (id),
    requested_for_facility_id   UUID REFERENCES mdm_facilities (id),
    amount              NUMERIC(18,2),              -- promoted from form data for workflow conditions
    currency_uom        VARCHAR(40),
    retention_policy_id UUID    REFERENCES retention_policies (id),
    submitted_at        TIMESTAMPTZ,
    completed_at        TIMESTAMPTZ,
    archived_at         TIMESTAMPTZ,
    purged_at           TIMESTAMPTZ                 -- payload removed by lifecycle Purge; audit rows remain
);
CREATE UNIQUE INDEX uq_documents_tenant_id_document_ref
    ON documents (tenant_id, document_ref) WHERE deleted_at IS NULL;
CREATE UNIQUE INDEX uq_documents_tenant_id_document_number
    ON documents (tenant_id, document_number) WHERE document_number IS NOT NULL AND deleted_at IS NULL;
CREATE INDEX idx_documents_tenant_id_status ON documents (tenant_id, status);
CREATE INDEX idx_documents_tenant_id_document_type_id_status
    ON documents (tenant_id, document_type_id, status);
CREATE INDEX idx_documents_form_data_json ON documents USING GIN (form_data_json);

CREATE TABLE document_versions (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID        REFERENCES users (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID        REFERENCES users (id),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    document_id     UUID        NOT NULL REFERENCES documents (id) ON DELETE CASCADE,
    version_number  INTEGER     NOT NULL,
    form_data_json  JSONB       NOT NULL,
    rendered_file_reference VARCHAR(2048),          -- storage path of generated PDF, if any
    change_reason   TEXT
);
CREATE UNIQUE INDEX uq_document_versions_document_id_version_number
    ON document_versions (document_id, version_number);

-- Circular with `documents`, so it is an ALTER rather than a column
-- constraint: neither table can reference the other at CREATE time.
ALTER TABLE documents ADD CONSTRAINT fk_documents_current_version_id
    FOREIGN KEY (current_version_id) REFERENCES document_versions (id);

CREATE TABLE document_metadata (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID        REFERENCES users (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID        REFERENCES users (id),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    document_id     UUID        NOT NULL REFERENCES documents (id) ON DELETE CASCADE,
    metadata_key    VARCHAR(64) NOT NULL,           -- 'supplier_id', 'cost_center'
    metadata_value  TEXT        NOT NULL,
    data_type       VARCHAR(40) NOT NULL DEFAULT 'STRING'
                    CHECK (data_type IN ('STRING', 'NUMBER', 'BOOLEAN', 'DATE'))
);
CREATE UNIQUE INDEX uq_document_metadata_document_id_metadata_key
    ON document_metadata (document_id, metadata_key) WHERE deleted_at IS NULL;
CREATE INDEX idx_document_metadata_tenant_id_key_value
    ON document_metadata (tenant_id, metadata_key, metadata_value);

-- Append-only: no updated_by / updated_at / deleted_at, per §1.2.
CREATE TABLE document_status_history (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    document_id     UUID        NOT NULL REFERENCES documents (id) ON DELETE CASCADE,
    old_status      VARCHAR(40),
    new_status      VARCHAR(40) NOT NULL,
    changed_by      UUID        REFERENCES users (id),   -- null for engine/system transitions
    reason          TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_document_status_history_document_id
    ON document_status_history (document_id, created_at);

CREATE TABLE document_relations (
    id                  UUID    PRIMARY KEY,
    tenant_id           UUID    NOT NULL REFERENCES tenants (id),
    created_by          UUID    REFERENCES users (id),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by          UUID    REFERENCES users (id),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at          TIMESTAMPTZ,
    source_document_id  UUID    NOT NULL REFERENCES documents (id) ON DELETE CASCADE,
    target_document_id  UUID    NOT NULL REFERENCES documents (id) ON DELETE CASCADE,
    relation_type       VARCHAR(40) NOT NULL
                        CHECK (relation_type IN ('RELATED_TO', 'SUPPORTS', 'BASED_ON', 'SUPERSEDES',
                                                 'PARENT_OF', 'CHILD_OF', 'REFERENCE', 'REQUIRED_FOR')),
    remarks             TEXT,
    CONSTRAINT ck_document_relations_not_self CHECK (source_document_id <> target_document_id)
);
CREATE UNIQUE INDEX uq_document_relations_source_target_type
    ON document_relations (source_document_id, target_document_id, relation_type)
    WHERE deleted_at IS NULL;
CREATE INDEX idx_document_relations_target ON document_relations (target_document_id);

-- ---------------------------------------------------------------------------
-- Lifecycle hooks (§6.11-§6.12)
-- ---------------------------------------------------------------------------
--
-- The core hook registry. Workflow-declared guards and actions are *not* rows
-- here — they live in `workflow_definitions.definition_json` and version with
-- it; plugin handlers live in `plugin_hooks`. The resolver merges the sources.

CREATE TABLE document_lifecycle_hooks (
    id                  UUID    PRIMARY KEY,
    tenant_id           UUID    NOT NULL REFERENCES tenants (id),
    created_by          UUID    REFERENCES users (id),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by          UUID    REFERENCES users (id),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at          TIMESTAMPTZ,
    document_type_id    UUID    REFERENCES document_types (id) ON DELETE CASCADE,  -- null = all types
    hook_name           VARCHAR(64) NOT NULL,       -- 'before_document_submit'
    handler_reference   VARCHAR(200) NOT NULL,      -- 'core:require_attachment' | 'plugin:<id>:<handler>'
    priority            INTEGER NOT NULL DEFAULT 100,   -- bands per architectures/01 §12.4.3
    config_json         JSONB   NOT NULL DEFAULT '{}',
    is_enabled          BOOLEAN NOT NULL DEFAULT true
);
CREATE INDEX idx_document_lifecycle_hooks_tenant_id_hook_name
    ON document_lifecycle_hooks (tenant_id, hook_name, priority) WHERE deleted_at IS NULL;

-- Append-only execution log for every handler run, whatever its source.
CREATE TABLE document_hook_executions (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    source          VARCHAR(40) NOT NULL
                    CHECK (source IN ('CORE', 'DOCUMENT_TYPE', 'WORKFLOW', 'PLUGIN')),
    hook_id         UUID        REFERENCES document_lifecycle_hooks (id),  -- null when source = WORKFLOW
    workflow_transition_ref VARCHAR(200),           -- '<workflowKey>@<revision>:<from>-><to>'
    document_id     UUID        NOT NULL REFERENCES documents (id) ON DELETE CASCADE,
    hook_name       VARCHAR(64) NOT NULL,
    handler_reference VARCHAR(200) NOT NULL,
    result          VARCHAR(40) NOT NULL
                    CHECK (result IN ('CONTINUE', 'MODIFY', 'REJECT', 'ERROR')),
    duration_ms     INTEGER,
    error_message   TEXT,
    executed_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_document_hook_executions_document_id
    ON document_hook_executions (document_id, executed_at);

-- ---------------------------------------------------------------------------
-- Deferred foreign keys from 0002 and 0008 (§6.13)
-- ---------------------------------------------------------------------------
--
-- These columns have been waiting for `document_types` and `documents` since
-- the migrations that created them said so. Adding the constraints here is what
-- closes those comments; leaving them would be four tables' worth of untyped
-- UUIDs pointing at real rows with nothing checking.

ALTER TABLE delegations ADD CONSTRAINT fk_delegations_document_type_id
    FOREIGN KEY (document_type_id) REFERENCES document_types (id);

ALTER TABLE mdm_parties ADD CONSTRAINT fk_mdm_parties_created_by_document_id
    FOREIGN KEY (created_by_document_id) REFERENCES documents (id);
ALTER TABLE mdm_parties ADD CONSTRAINT fk_mdm_parties_last_updated_by_document_id
    FOREIGN KEY (last_updated_by_document_id) REFERENCES documents (id);

ALTER TABLE mdm_facilities ADD CONSTRAINT fk_mdm_facilities_created_by_document_id
    FOREIGN KEY (created_by_document_id) REFERENCES documents (id);
ALTER TABLE mdm_facilities ADD CONSTRAINT fk_mdm_facilities_last_updated_by_document_id
    FOREIGN KEY (last_updated_by_document_id) REFERENCES documents (id);

ALTER TABLE mdm_products ADD CONSTRAINT fk_mdm_products_created_by_document_id
    FOREIGN KEY (created_by_document_id) REFERENCES documents (id);
ALTER TABLE mdm_products ADD CONSTRAINT fk_mdm_products_last_updated_by_document_id
    FOREIGN KEY (last_updated_by_document_id) REFERENCES documents (id);

ALTER TABLE mdm_services ADD CONSTRAINT fk_mdm_services_created_by_document_id
    FOREIGN KEY (created_by_document_id) REFERENCES documents (id);
ALTER TABLE mdm_services ADD CONSTRAINT fk_mdm_services_last_updated_by_document_id
    FOREIGN KEY (last_updated_by_document_id) REFERENCES documents (id);

-- ---------------------------------------------------------------------------
-- Permission catalogue rows for the document-type surface (#157)
-- ---------------------------------------------------------------------------
--
-- Four, for the four things a document type is: created, read, edited and
-- retired. `document-type` is its own module (SDD §5 lists `document_type`
-- beside `document`), and it manages one resource, so the resource segment is
-- omitted (naming convention §6).
--
-- Nothing is seeded for documents, versions, metadata, relations or hooks:
-- those have no endpoint until Sprint 9, and a permission row no route checks
-- reads as a control that exists (`0010_facility_permissions.sql`'s rule).
--
-- Continues the id-block convention of 0002_identity.sql: permissions take the
-- 0001 block, which stands at ...0031 after 0014.

INSERT INTO permissions (id, tenant_id, permission_code, module, description) VALUES
    ('00000000-0000-0000-0001-000000000032', '00000000-0000-0000-0000-000000000001',
     'document-type:create', 'document-type', 'Create a document type'),
    ('00000000-0000-0000-0001-000000000033', '00000000-0000-0000-0000-000000000001',
     'document-type:read',   'document-type', 'View document types'),
    ('00000000-0000-0000-0001-000000000034', '00000000-0000-0000-0000-000000000001',
     'document-type:update', 'document-type', 'Edit a document type, including the form and workflow it binds'),
    ('00000000-0000-0000-0001-000000000035', '00000000-0000-0000-0000-000000000001',
     'document-type:delete', 'document-type', 'Retire a document type');

-- ROLE-ADMIN holds every permission in the catalogue (0002_identity.sql); grant
-- only the four new rows rather than re-inserting the ones already granted.
INSERT INTO role_permissions (id, tenant_id, role_id, permission_id)
SELECT
    gen_random_uuid(),
    '00000000-0000-0000-0000-000000000001',
    '00000000-0000-0000-0002-000000000001',
    id
FROM permissions
WHERE permission_code IN (
    'document-type:create',
    'document-type:read',
    'document-type:update',
    'document-type:delete'
);
