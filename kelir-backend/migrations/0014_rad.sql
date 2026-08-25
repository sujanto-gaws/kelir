-- 0014_rad.sql — the RAD metadata layer: entities, fields, forms, lists, menus,
-- actions, validation rules and lookup definitions.
--
-- The whole of Database Schema §5 in one file, as its mapping table assigns it.
-- Most of these tables have no endpoint in Sprint 7 — only forms and lists get
-- one (#156) — and are created here anyway for the reason 0008_master_data.sql
-- gives: sqlx applies migrations in version order, so splitting the group would
-- consume a second version number and buy nothing.
--
-- **This migration must apply before the document migration, and that ordering
-- is the part of decision D-2 a renumbering can never disturb.**
-- `document_types.form_id` references `rad_forms` (§6.2). Migrations apply in
-- version order, so if the document migration landed first the reference could
-- not resolve and would need a deferred constraint — which is how a foreign key
-- ends up permanently missing. D-2 moved RAD's storage layer into Phase 4 for
-- exactly this.
--
-- Takes 0014 because that is the next free number after 0013, not because a
-- phase reserved it (naming convention §4.3). The migrations planned after it
-- shift down by one in the Database Schema mapping table, which is the only
-- place the sequence lives; merged migrations are never renumbered.
--
-- Base columns and conventions per §1. Three things are decisions rather than
-- transcriptions of §5, and each is stated here:
--
--   * **Bounded string columns take a length from §1.3.1 rather than TEXT.**
--     §5's DDL wrote `form_key TEXT` beside `status VARCHAR(40)`, and eleven of
--     the affected columns sit inside unique indexes — the failure
--     0004_string_lengths.sql exists to fix, since a btree entry cannot exceed
--     ~8 KB and index entries are never TOASTed, so an oversized value is
--     accepted or rejected depending on how compressible it happens to be.
--     0008_master_data.sql made the same correction for §4 and recorded it as
--     deviation #15; this is deviation #17. The rule applied is the same:
--     VARCHAR(40) where a CHECK bounds the vocabulary, VARCHAR(64) for open
--     code columns and business identifiers, VARCHAR(200) for human names and
--     titles, VARCHAR(2048) for routes and paths, TEXT for genuine prose.
--     `definition_json` is JSONB and unaffected.
--
--   * **ON DELETE CASCADE on the child rows is what §5 specifies**, and it is
--     not the practice coding standard §4 forbids. `rad_form_sections` and
--     `rad_form_fields` are projections regenerated on every save of
--     `definition_json` — they have no independent life, and lifecycle for the
--     tables that do is governed by soft delete. The cascade is a referential-
--     integrity backstop for an administrative hard delete, not a request path.
--
--   * **`rad_forms.published_by` carries its REFERENCES users (id)**, as do
--     every `created_by` and `updated_by` here. `users` exists since 0002, so
--     nothing needs forward-declaring.
--
-- **Two self-references in here can be made cyclic, and nothing yet stops
-- them.** `rad_menus.parent_menu_id` and `rad_form_sections.parent_section_id`
-- are the shape #141 had to fix on `mdm_facilities`, where a hierarchy could be
-- made a ring two different ways and any traversal then looped. No guard is
-- added here because no route writes either column in Sprint 7 — a check
-- nothing calls is a check nobody maintains — and because the facility fix is
-- application-level (a locked ancestor walk in the service), not a constraint
-- this file could carry. Whoever builds the menu or the form-builder surface
-- inherits that obligation; #191 records it.

-- ---------------------------------------------------------------------------
-- Entities and their fields (§5.1-5.2) — FR-RAD-001, FR-RAD-005
-- ---------------------------------------------------------------------------

CREATE TABLE rad_entities (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID        REFERENCES users (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID        REFERENCES users (id),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    entity_key      VARCHAR(64) NOT NULL,           -- 'supplier'
    label           VARCHAR(200) NOT NULL,
    table_name      VARCHAR(64),                    -- backing table for core-backed entities
    module          VARCHAR(64),                    -- 'master_data', 'document'
    description     TEXT,
    is_system       BOOLEAN     NOT NULL DEFAULT false,
    status          VARCHAR(40) NOT NULL DEFAULT 'ACTIVE'
                    CHECK (status IN ('DRAFT', 'ACTIVE', 'DEPRECATED'))
);
CREATE UNIQUE INDEX uq_rad_entities_tenant_id_entity_key
    ON rad_entities (tenant_id, entity_key) WHERE deleted_at IS NULL;

CREATE TABLE rad_fields (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID        REFERENCES users (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID        REFERENCES users (id),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    entity_id       UUID        NOT NULL REFERENCES rad_entities (id) ON DELETE CASCADE,
    field_key       VARCHAR(64) NOT NULL,           -- 'supplier_code'
    label           VARCHAR(200) NOT NULL,
    -- string | number | integer | boolean | date | datetime | enum | lookup
    -- | textarea | array | object. Left unbounded by a CHECK: the vocabulary is
    -- JFSS's and grows with the spec, so a CHECK here would make a schema
    -- migration the price of a new component type.
    field_type      VARCHAR(40) NOT NULL,
    is_required     BOOLEAN     NOT NULL DEFAULT false,
    options_json    JSONB,                          -- enum options, lookup config
    validation_json JSONB,                          -- JFSS validation object
    sort_order      INTEGER     NOT NULL DEFAULT 0
);
CREATE UNIQUE INDEX uq_rad_fields_entity_id_field_key
    ON rad_fields (entity_id, field_key) WHERE deleted_at IS NULL;

-- ---------------------------------------------------------------------------
-- Forms and their projections (§5.3-5.5) — FR-RAD-002
-- ---------------------------------------------------------------------------
--
-- The JFSS document in `definition_json` is the authority; the two projection
-- tables are extracted from it on save so the builder and reporting can query
-- fields relationally without parsing JSON. JFSS has no revision concept of its
-- own — its `version` is the spec version — so revisioning lives here.

CREATE TABLE rad_forms (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID        REFERENCES users (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID        REFERENCES users (id),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    form_key        VARCHAR(64) NOT NULL,           -- JFSS formId, immutable
    title           VARCHAR(200) NOT NULL,
    revision        INTEGER     NOT NULL DEFAULT 1,
    jfss_version    VARCHAR(40) NOT NULL,           -- '2.0.1', the spec version of definition_json
    definition_json JSONB       NOT NULL,           -- the complete JFSS document
    entity_id       UUID        REFERENCES rad_entities (id),
    status          VARCHAR(40) NOT NULL DEFAULT 'DRAFT'
                    CHECK (status IN ('DRAFT', 'PUBLISHED', 'DEPRECATED')),
    published_at    TIMESTAMPTZ,
    published_by    UUID        REFERENCES users (id),
    -- A published form has a publication; a draft has none. Stated as a
    -- constraint rather than left to the service, because `published_at` is
    -- what the immutability rule keys on: a PUBLISHED row with a null stamp
    -- would be editable by any code that asks "was this published?" the
    -- obvious way.
    CONSTRAINT ck_rad_forms_published_stamp CHECK (
        (status = 'PUBLISHED') = (published_at IS NOT NULL)
    ),
    CONSTRAINT ck_rad_forms_revision_positive CHECK (revision >= 1)
);
CREATE UNIQUE INDEX uq_rad_forms_tenant_id_form_key_revision
    ON rad_forms (tenant_id, form_key, revision) WHERE deleted_at IS NULL;
-- The read every renderer makes: the current revision of a form by key.
CREATE INDEX idx_rad_forms_tenant_id_form_key_status
    ON rad_forms (tenant_id, form_key, status) WHERE deleted_at IS NULL;

CREATE TABLE rad_form_sections (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID        REFERENCES users (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID        REFERENCES users (id),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    form_id         UUID        NOT NULL REFERENCES rad_forms (id) ON DELETE CASCADE,
    section_key     VARCHAR(64) NOT NULL,           -- JFSS component id
    title           VARCHAR(200),
    parent_section_id UUID      REFERENCES rad_form_sections (id),
    sort_order      INTEGER     NOT NULL DEFAULT 0
);
CREATE UNIQUE INDEX uq_rad_form_sections_form_id_section_key
    ON rad_form_sections (form_id, section_key) WHERE deleted_at IS NULL;

CREATE TABLE rad_form_fields (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID        REFERENCES users (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID        REFERENCES users (id),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    form_id         UUID        NOT NULL REFERENCES rad_forms (id) ON DELETE CASCADE,
    section_id      UUID        REFERENCES rad_form_sections (id) ON DELETE CASCADE,
    component_id    VARCHAR(64) NOT NULL,           -- JFSS component id
    field_key       VARCHAR(64) NOT NULL,           -- JFSS data key (payload property)
    label           VARCHAR(200) NOT NULL,
    component_type  VARCHAR(40) NOT NULL,           -- textfield | select | datagrid | ...
    is_required     BOOLEAN     NOT NULL DEFAULT false,
    is_calculated   BOOLEAN     NOT NULL DEFAULT false,
    sort_order      INTEGER     NOT NULL DEFAULT 0
);
CREATE UNIQUE INDEX uq_rad_form_fields_form_id_component_id
    ON rad_form_fields (form_id, component_id) WHERE deleted_at IS NULL;
CREATE INDEX idx_rad_form_fields_form_id_field_key ON rad_form_fields (form_id, field_key);

-- ---------------------------------------------------------------------------
-- Lists (§5.6-5.8) — FR-RAD-003
-- ---------------------------------------------------------------------------

CREATE TABLE rad_lists (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID        REFERENCES users (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID        REFERENCES users (id),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    list_key        VARCHAR(64) NOT NULL,           -- 'purchase_requisition_list'
    title           VARCHAR(200) NOT NULL,
    entity_id       UUID        REFERENCES rad_entities (id),
    default_sort_json JSONB,                        -- [{"key":"created_at","dir":"desc"}]
    page_size       INTEGER     NOT NULL DEFAULT 20,
    status          VARCHAR(40) NOT NULL DEFAULT 'ACTIVE'
                    CHECK (status IN ('DRAFT', 'ACTIVE', 'DEPRECATED')),
    -- A page size of 0 pages forever and a negative one is nonsense; the upper
    -- bound matches the pagination cap the API already enforces (FR-API-006).
    CONSTRAINT ck_rad_lists_page_size CHECK (page_size BETWEEN 1 AND 100)
);
CREATE UNIQUE INDEX uq_rad_lists_tenant_id_list_key
    ON rad_lists (tenant_id, list_key) WHERE deleted_at IS NULL;

CREATE TABLE rad_list_columns (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID        REFERENCES users (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID        REFERENCES users (id),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    list_id         UUID        NOT NULL REFERENCES rad_lists (id) ON DELETE CASCADE,
    -- 'document_number', 'form_data.amount' — a dotted path into the payload,
    -- which is why it is a code column rather than an identifier.
    column_key      VARCHAR(64) NOT NULL,
    label           VARCHAR(200) NOT NULL,
    data_type       VARCHAR(40),                    -- STRING | NUMBER | DATE | ENUM | LOOKUP
    format          VARCHAR(40),                    -- e.g. 'currency', 'date-short'
    is_sortable     BOOLEAN     NOT NULL DEFAULT true,
    width           VARCHAR(40),
    sort_order      INTEGER     NOT NULL DEFAULT 0
);
CREATE UNIQUE INDEX uq_rad_list_columns_list_id_column_key
    ON rad_list_columns (list_id, column_key) WHERE deleted_at IS NULL;

CREATE TABLE rad_list_filters (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID        REFERENCES users (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID        REFERENCES users (id),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    list_id         UUID        NOT NULL REFERENCES rad_lists (id) ON DELETE CASCADE,
    filter_key      VARCHAR(64) NOT NULL,
    label           VARCHAR(200) NOT NULL,
    filter_type     VARCHAR(40) NOT NULL
                    CHECK (filter_type IN ('TEXT', 'ENUM', 'LOOKUP', 'DATE_RANGE',
                                           'NUMBER_RANGE', 'BOOLEAN')),
    options_json    JSONB,
    is_default      BOOLEAN     NOT NULL DEFAULT false,
    sort_order      INTEGER     NOT NULL DEFAULT 0
);
CREATE UNIQUE INDEX uq_rad_list_filters_list_id_filter_key
    ON rad_list_filters (list_id, filter_key) WHERE deleted_at IS NULL;

-- ---------------------------------------------------------------------------
-- Menus and actions (§5.9-5.10)
-- ---------------------------------------------------------------------------

CREATE TABLE rad_menus (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID        REFERENCES users (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID        REFERENCES users (id),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    menu_key        VARCHAR(64) NOT NULL,
    label           VARCHAR(200) NOT NULL,
    icon            VARCHAR(64),                    -- Lucide icon name
    parent_menu_id  UUID        REFERENCES rad_menus (id),
    route_path      VARCHAR(2048),                  -- '/documents', '/master-data/parties'
    required_permission VARCHAR(64),                -- hide when the user lacks it
    source          VARCHAR(40) NOT NULL DEFAULT 'CORE'
                    CHECK (source IN ('CORE', 'CONFIG', 'PLUGIN')),
    sort_order      INTEGER     NOT NULL DEFAULT 0,
    is_enabled      BOOLEAN     NOT NULL DEFAULT true,
    -- The one-hop half of the cycle problem, which a constraint can express.
    -- The rest of it — a ring of three — needs an ancestor walk in the service,
    -- as #141 built for facilities. See the header note and #191.
    CONSTRAINT ck_rad_menus_not_its_own_parent CHECK (parent_menu_id IS DISTINCT FROM id)
);
CREATE UNIQUE INDEX uq_rad_menus_tenant_id_menu_key
    ON rad_menus (tenant_id, menu_key) WHERE deleted_at IS NULL;

CREATE TABLE rad_actions (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID        REFERENCES users (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID        REFERENCES users (id),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    action_key      VARCHAR(64) NOT NULL,
    label           VARCHAR(200) NOT NULL,
    context         VARCHAR(40) NOT NULL
                    CHECK (context IN ('LIST', 'DETAIL', 'DOCUMENT', 'TASK')),
    action_type     VARCHAR(40) NOT NULL
                    CHECK (action_type IN ('NAVIGATE', 'API_CALL', 'WORKFLOW_ACTION', 'PLUGIN')),
    config_json     JSONB       NOT NULL DEFAULT '{}',
    required_permission VARCHAR(64),
    sort_order      INTEGER     NOT NULL DEFAULT 0,
    is_enabled      BOOLEAN     NOT NULL DEFAULT true
);
CREATE UNIQUE INDEX uq_rad_actions_tenant_id_action_key
    ON rad_actions (tenant_id, action_key) WHERE deleted_at IS NULL;

-- ---------------------------------------------------------------------------
-- Validation rules and lookups (§5.11-5.12)
-- ---------------------------------------------------------------------------
--
-- Cross-field and server-side rules beyond the per-component JFSS `validation`
-- object; `rule` values come from the JFSS Validation Rule Registry.

CREATE TABLE rad_validation_rules (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID        REFERENCES users (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID        REFERENCES users (id),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    form_id         UUID        REFERENCES rad_forms (id) ON DELETE CASCADE,
    entity_id       UUID        REFERENCES rad_entities (id) ON DELETE CASCADE,
    rule            VARCHAR(64) NOT NULL,           -- registry rule name
    scope           VARCHAR(40) NOT NULL DEFAULT 'BOTH'
                    CHECK (scope IN ('CLIENT', 'SERVER', 'BOTH')),
    params_json     JSONB       NOT NULL DEFAULT '{}',
    message         TEXT        NOT NULL,
    is_enabled      BOOLEAN     NOT NULL DEFAULT true,
    CONSTRAINT ck_rad_validation_rules_target
        CHECK (form_id IS NOT NULL OR entity_id IS NOT NULL)
);
CREATE INDEX idx_rad_validation_rules_form_id ON rad_validation_rules (form_id);
CREATE INDEX idx_rad_validation_rules_entity_id ON rad_validation_rules (entity_id);

CREATE TABLE rad_lookup_definitions (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID        REFERENCES users (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID        REFERENCES users (id),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    lookup_key      VARCHAR(64) NOT NULL,           -- 'supplier', 'cost_center'
    label           VARCHAR(200) NOT NULL,
    source_type     VARCHAR(40) NOT NULL
                    CHECK (source_type IN ('ENTITY', 'ENUM', 'API', 'STATIC')),
    source_config_json JSONB    NOT NULL DEFAULT '{}',  -- entity key + display/value fields, static items, endpoint
    filter_json     JSONB,                          -- e.g. only ACTIVE suppliers
    is_enabled      BOOLEAN     NOT NULL DEFAULT true
);
CREATE UNIQUE INDEX uq_rad_lookup_definitions_tenant_id_lookup_key
    ON rad_lookup_definitions (tenant_id, lookup_key) WHERE deleted_at IS NULL;

-- ---------------------------------------------------------------------------
-- Permission catalogue rows for the storage APIs (#156)
-- ---------------------------------------------------------------------------
--
-- Seeded here rather than in a later migration, as #155 AC4 requires, and
-- seeded only for what #156 actually checks. Entities, fields, menus, actions
-- and lookups get no permissions yet, for the reason 0010_facility_permissions
-- gives: a permission row that no route checks reads as a control that exists.
--
-- Continues the id-block convention of 0002_identity.sql: permissions take the
-- 0001 block, which stands at ...0022 after 0012.
--
-- `publish` is a separate action on forms and has no counterpart on lists. It
-- is not "update with a different value": publishing fixes a revision forever
-- — documents pin the exact `rad_forms.id` they were created against, so a
-- published revision is what an old document re-renders with years later. Who
-- may draft a form and who may make a draft binding are different questions,
-- and a deployment that wants them to be the same person can grant both. A
-- list has no such moment: its status moves between ACTIVE and DEPRECATED and
-- back, and nothing pins it.

INSERT INTO permissions (id, tenant_id, permission_code, module, description) VALUES
    ('00000000-0000-0000-0001-000000000023', '00000000-0000-0000-0000-000000000001',
     'rad:form:create',  'rad', 'Create a form definition'),
    ('00000000-0000-0000-0001-000000000024', '00000000-0000-0000-0000-000000000001',
     'rad:form:read',    'rad', 'View form definitions'),
    ('00000000-0000-0000-0001-000000000025', '00000000-0000-0000-0000-000000000001',
     'rad:form:update',  'rad', 'Edit a draft form revision'),
    ('00000000-0000-0000-0001-000000000026', '00000000-0000-0000-0000-000000000001',
     'rad:form:publish', 'rad', 'Publish a form revision, fixing it for every document that pins it'),
    ('00000000-0000-0000-0001-000000000027', '00000000-0000-0000-0000-000000000001',
     'rad:form:delete',  'rad', 'Retire a form definition'),
    ('00000000-0000-0000-0001-000000000028', '00000000-0000-0000-0000-000000000001',
     'rad:list:create',  'rad', 'Create a list definition'),
    ('00000000-0000-0000-0001-000000000029', '00000000-0000-0000-0000-000000000001',
     'rad:list:read',    'rad', 'View list definitions'),
    ('00000000-0000-0000-0001-000000000030', '00000000-0000-0000-0000-000000000001',
     'rad:list:update',  'rad', 'Edit a list definition'),
    ('00000000-0000-0000-0001-000000000031', '00000000-0000-0000-0000-000000000001',
     'rad:list:delete',  'rad', 'Retire a list definition');

-- ROLE-ADMIN holds every permission in the catalogue (0002_identity.sql); grant
-- only the nine new rows rather than re-inserting the ones already granted.
INSERT INTO role_permissions (id, tenant_id, role_id, permission_id)
SELECT
    gen_random_uuid(),
    '00000000-0000-0000-0000-000000000001',
    '00000000-0000-0000-0002-000000000001',
    id
FROM permissions
WHERE permission_code IN (
    'rad:form:create',
    'rad:form:read',
    'rad:form:update',
    'rad:form:publish',
    'rad:form:delete',
    'rad:list:create',
    'rad:list:read',
    'rad:list:update',
    'rad:list:delete'
);
