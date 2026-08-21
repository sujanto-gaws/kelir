-- 0008_master_data.sql — the Party model, facilities, products and services.
--
-- The whole of Database Schema §4 in one file, as its mapping table assigns it.
-- Facilities, products, services and source references have no endpoints until
-- Sprint 6 and are created here anyway: sqlx applies migrations in version
-- order, so splitting the group would consume a second version number and buy
-- nothing (#80). Party roles and the role profiles (§4.4-4.5, §4.12-4.15) are
-- likewise created here and used by #81, in the same sprint.
--
-- The Party model normalizes the PartyAggregate JSON Schema of
-- docs/architectures/05. The aggregate stays the API payload shape; these
-- tables are its storage. Free-string type ids in the aggregate (roleTypeId,
-- contactMechTypeId, partyIdentificationTypeId, ...) are code columns here,
-- seeded where the vocabulary is ours and extendable per tenant.
--
-- Base columns and conventions per §1. Three things about them are decisions
-- rather than transcriptions, and each is stated here:
--
--   * created_by / updated_by carry their REFERENCES users (id), as §1.2
--     specifies. The identity tables do not, because `users` did not exist when
--     0002 created them; 0002 constrained 0001's system_settings by ALTER for
--     exactly that reason. Nothing needs forward-declaring here.
--   * Bounded string columns take a length from §1.3.1 rather than TEXT. §4's
--     DDL was inconsistent on this — `status VARCHAR(40)` beside
--     `party_type TEXT` — and six of the affected columns sit inside unique
--     indexes, which is the failure 0004_string_lengths.sql exists to fix: a
--     btree entry cannot exceed ~8 KB and index entries are never TOASTed, so
--     an oversized value is accepted or rejected depending on how compressible
--     it happens to be. Doing it here costs nothing (the tables are new); doing
--     it later costs a second sweep migration against populated tables. §4 is
--     corrected to match in the same change, recorded as §14 deviation #15.
--     The rule applied: VARCHAR(40) where a CHECK bounds the vocabulary,
--     VARCHAR(64) for open code columns and business identifiers, VARCHAR(200)
--     for human names and titles, VARCHAR(255) for external references, TEXT
--     for genuine prose — descriptions, comments, one-line display projections.
--   * ON DELETE CASCADE on the party's child rows is what §4 specifies, and it
--     is not the practice coding standard §4 forbids. Lifecycle is governed by
--     soft delete: the application never issues a hard DELETE against a party,
--     so the cascade is unreachable from a request. It is a referential-
--     integrity backstop for an administrative hard delete, which would
--     otherwise leave a person row behind a party that no longer exists.
--
-- Master data governed by document workflows carries the lifecycle columns of
-- concepts/03 §5 (§4 preamble): record_status plus two document references. The
-- references stay bare UUID and gain their foreign keys in 0010_document.sql —
-- documents do not exist yet — and no workflow behaviour is implemented here;
-- FR-MDM-010 is Phase 5+. A direct edit by an authorized user leaves both null,
-- which is every edit the product can currently make.

-- ---------------------------------------------------------------------------
-- Party core (§4.1-4.3)
-- ---------------------------------------------------------------------------

CREATE TABLE mdm_parties (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID        REFERENCES users (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID        REFERENCES users (id),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    party_code      VARCHAR(64) NOT NULL,               -- aggregate partyId, 1..60 chars
    party_type      VARCHAR(40) NOT NULL
                    CHECK (party_type IN ('PERSON', 'PARTY_GROUP')),
    status          VARCHAR(40) NOT NULL DEFAULT 'PARTY_ENABLED'
                    CHECK (status IN ('PARTY_ENABLED', 'PARTY_DISABLED')),
    external_id     VARCHAR(255),
    description     TEXT,
    attributes_json JSONB       NOT NULL DEFAULT '{}',
    record_status   VARCHAR(40) NOT NULL DEFAULT 'DRAFT'
                    CHECK (record_status IN ('DRAFT', 'PENDING_APPROVAL', 'ACTIVE',
                                             'SUSPENDED', 'INACTIVE', 'ARCHIVED')),
    created_by_document_id      UUID,                   -- FK to documents added in 0010
    last_updated_by_document_id UUID,                   -- FK to documents added in 0010
    -- The aggregate bounds partyId at 60 characters while the column takes the
    -- standard 64, so the CHECK is what holds the contract: a value the schema
    -- would reject cannot be stored by a path that skipped validation.
    CONSTRAINT ck_mdm_parties_party_code_len CHECK (char_length(party_code) BETWEEN 1 AND 60)
);
CREATE UNIQUE INDEX uq_mdm_parties_tenant_id_party_code
    ON mdm_parties (tenant_id, party_code) WHERE deleted_at IS NULL;
CREATE INDEX idx_mdm_parties_tenant_id_party_type ON mdm_parties (tenant_id, party_type);

-- One-to-one extension of mdm_parties where party_type = 'PERSON'.
CREATE TABLE mdm_persons (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID        REFERENCES users (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID        REFERENCES users (id),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    party_id        UUID        NOT NULL REFERENCES mdm_parties (id) ON DELETE CASCADE,
    first_name      VARCHAR(200) NOT NULL,
    middle_name     VARCHAR(200),
    last_name       VARCHAR(200) NOT NULL,
    personal_title  VARCHAR(200),
    suffix          VARCHAR(200),
    gender          VARCHAR(40) CHECK (gender IN ('M', 'F', 'X')),
    birth_date      DATE,
    marital_status  VARCHAR(40),
    comments        TEXT
);
CREATE UNIQUE INDEX uq_mdm_persons_party_id ON mdm_persons (party_id) WHERE deleted_at IS NULL;

-- One-to-one extension of mdm_parties where party_type = 'PARTY_GROUP'
-- (companies, organizations, tenant parties).
CREATE TABLE mdm_party_groups (
    id                  UUID        PRIMARY KEY,
    tenant_id           UUID        NOT NULL REFERENCES tenants (id),
    created_by          UUID        REFERENCES users (id),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by          UUID        REFERENCES users (id),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at          TIMESTAMPTZ,
    party_id            UUID        NOT NULL REFERENCES mdm_parties (id) ON DELETE CASCADE,
    group_name          VARCHAR(200) NOT NULL,
    local_name          VARCHAR(200),
    office_site_name    VARCHAR(200),
    annual_revenue      NUMERIC(18,2),
    num_employees       INTEGER,
    ticker_symbol       VARCHAR(64),
    comments            TEXT
);
CREATE UNIQUE INDEX uq_mdm_party_groups_party_id
    ON mdm_party_groups (party_id) WHERE deleted_at IS NULL;

-- ---------------------------------------------------------------------------
-- Roles (§4.4-4.5) — tables only; the endpoints over them are #81
-- ---------------------------------------------------------------------------

CREATE TABLE mdm_role_types (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID        REFERENCES users (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID        REFERENCES users (id),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    role_type_code  VARCHAR(64) NOT NULL,
    name            VARCHAR(200) NOT NULL,
    description     TEXT,
    is_system       BOOLEAN     NOT NULL DEFAULT false
);
CREATE UNIQUE INDEX uq_mdm_role_types_tenant_id_role_type_code
    ON mdm_role_types (tenant_id, role_type_code) WHERE deleted_at IS NULL;

CREATE TABLE mdm_party_roles (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID        REFERENCES users (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID        REFERENCES users (id),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    party_id        UUID        NOT NULL REFERENCES mdm_parties (id) ON DELETE CASCADE,
    role_type_id    UUID        NOT NULL REFERENCES mdm_role_types (id),
    starts_at       TIMESTAMPTZ NOT NULL,               -- aggregate fromDate
    ends_at         TIMESTAMPTZ,                        -- aggregate thruDate
    status          VARCHAR(40) NOT NULL DEFAULT 'ACTIVE'
                    CHECK (status IN ('ACTIVE', 'INACTIVE')),
    comments        TEXT,
    attributes_json JSONB       NOT NULL DEFAULT '{}'
);
CREATE UNIQUE INDEX uq_mdm_party_roles_party_id_role_type_id_starts_at
    ON mdm_party_roles (party_id, role_type_id, starts_at) WHERE deleted_at IS NULL;
-- Powers the /suppliers, /customers and /employees role views (Sprint 6).
CREATE INDEX idx_mdm_party_roles_tenant_id_role_type_id
    ON mdm_party_roles (tenant_id, role_type_id);

-- ---------------------------------------------------------------------------
-- Party attributes (§4.6-4.9)
-- ---------------------------------------------------------------------------

CREATE TABLE mdm_party_identifications (
    id                  UUID        PRIMARY KEY,
    tenant_id           UUID        NOT NULL REFERENCES tenants (id),
    created_by          UUID        REFERENCES users (id),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by          UUID        REFERENCES users (id),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at          TIMESTAMPTZ,
    party_id            UUID        NOT NULL REFERENCES mdm_parties (id) ON DELETE CASCADE,
    identification_type VARCHAR(64) NOT NULL,   -- EMPLOYEE_NUMBER | CUSTOMER_NUMBER | SUPPLIER_NUMBER
                                                -- | TAX_ID | PASSPORT_NUMBER | COMPANY_REGISTRATION
                                                -- | TENANT_CODE
    id_value            VARCHAR(64) NOT NULL,
    issued_by           VARCHAR(200),
    issue_date          DATE,
    expire_date         DATE,
    attributes_json     JSONB       NOT NULL DEFAULT '{}'
);
CREATE UNIQUE INDEX uq_mdm_party_identifications_party_type_value
    ON mdm_party_identifications (party_id, identification_type, id_value) WHERE deleted_at IS NULL;
CREATE INDEX idx_mdm_party_identifications_tenant_id_type_value
    ON mdm_party_identifications (tenant_id, identification_type, id_value);

-- Append-only status history: no updated_by / updated_at / deleted_at (§1.2).
CREATE TABLE mdm_party_statuses (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    party_id        UUID        NOT NULL REFERENCES mdm_parties (id) ON DELETE CASCADE,
    status          VARCHAR(40) NOT NULL,
    status_at       TIMESTAMPTZ NOT NULL,
    changed_by      UUID        REFERENCES users (id),
    comments        TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_mdm_party_statuses_party_id_status_at ON mdm_party_statuses (party_id, status_at);

-- One table for both relationshipsFrom and relationshipsTo of the aggregate;
-- the API projects each direction from it.
CREATE TABLE mdm_party_relationships (
    id                  UUID        PRIMARY KEY,
    tenant_id           UUID        NOT NULL REFERENCES tenants (id),
    created_by          UUID        REFERENCES users (id),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by          UUID        REFERENCES users (id),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at          TIMESTAMPTZ,
    from_party_id       UUID        NOT NULL REFERENCES mdm_parties (id) ON DELETE CASCADE,
    to_party_id         UUID        NOT NULL REFERENCES mdm_parties (id) ON DELETE CASCADE,
    relationship_type   VARCHAR(64) NOT NULL,   -- EMPLOYMENT | CUSTOMER_ACCOUNT | SUPPLIER_ACCOUNT
                                                -- | CONTACT_REL | PARENT_TENANT | ORGANIZATION_ROLLUP
    from_role_type_id   UUID        REFERENCES mdm_role_types (id),
    to_role_type_id     UUID        REFERENCES mdm_role_types (id),
    starts_at           TIMESTAMPTZ NOT NULL,
    ends_at             TIMESTAMPTZ,
    status              VARCHAR(40),
    priority            INTEGER,
    comments            TEXT,
    attributes_json     JSONB       NOT NULL DEFAULT '{}'
);
CREATE INDEX idx_mdm_party_relationships_from
    ON mdm_party_relationships (from_party_id, relationship_type);
CREATE INDEX idx_mdm_party_relationships_to
    ON mdm_party_relationships (to_party_id, relationship_type);

CREATE TABLE mdm_party_classifications (
    id                  UUID        PRIMARY KEY,
    tenant_id           UUID        NOT NULL REFERENCES tenants (id),
    created_by          UUID        REFERENCES users (id),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by          UUID        REFERENCES users (id),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at          TIMESTAMPTZ,
    party_id            UUID        NOT NULL REFERENCES mdm_parties (id) ON DELETE CASCADE,
    class_type          VARCHAR(64) NOT NULL,           -- aggregate partyClassTypeId
    classification_code VARCHAR(64),                    -- aggregate partyClassificationId
    starts_at           TIMESTAMPTZ NOT NULL,
    ends_at             TIMESTAMPTZ,
    comments            TEXT
);
CREATE INDEX idx_mdm_party_classifications_party_id
    ON mdm_party_classifications (party_id, class_type);

-- ---------------------------------------------------------------------------
-- Contact mechanisms (§4.10-4.11)
-- ---------------------------------------------------------------------------

-- The aggregate's contactMechDetail (postal address, telecom number, email,
-- url) stays JSONB: the Party schema gives no normalization for it and the
-- shapes differ per type. display_value is the denormalized one-line projection
-- for lists and search, and stays TEXT — it is prose rather than a code, and no
-- index covers it.
CREATE TABLE mdm_contact_mechs (
    id                  UUID        PRIMARY KEY,
    tenant_id           UUID        NOT NULL REFERENCES tenants (id),
    created_by          UUID        REFERENCES users (id),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by          UUID        REFERENCES users (id),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at          TIMESTAMPTZ,
    contact_mech_type   VARCHAR(40) NOT NULL
                        CHECK (contact_mech_type IN ('EMAIL_ADDRESS', 'PHONE_NUMBER', 'MOBILE_NUMBER',
                                                     'POSTAL_ADDRESS', 'WEB_ADDRESS', 'OTHER')),
    display_value       TEXT        NOT NULL,   -- 'jane@acme.example', '+62 21 555 0100', one-line address
    detail_json         JSONB       NOT NULL DEFAULT '{}'  -- postalAddress | telecomNumber | emailAddress
                                                           -- | url | other
);
CREATE INDEX idx_mdm_contact_mechs_tenant_id_type ON mdm_contact_mechs (tenant_id, contact_mech_type);

CREATE TABLE mdm_party_contact_mechs (
    id                  UUID        PRIMARY KEY,
    tenant_id           UUID        NOT NULL REFERENCES tenants (id),
    created_by          UUID        REFERENCES users (id),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by          UUID        REFERENCES users (id),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at          TIMESTAMPTZ,
    party_id            UUID        NOT NULL REFERENCES mdm_parties (id) ON DELETE CASCADE,
    contact_mech_id     UUID        NOT NULL REFERENCES mdm_contact_mechs (id),
    purpose_type        VARCHAR(64),                    -- BILLING | SHIPPING | PRIMARY_OFFICE | ...
    starts_at           TIMESTAMPTZ NOT NULL,
    ends_at             TIMESTAMPTZ,
    is_primary          BOOLEAN     NOT NULL DEFAULT false,
    allow_solicitation  BOOLEAN     NOT NULL DEFAULT true,
    attributes_json     JSONB       NOT NULL DEFAULT '{}'
);
CREATE INDEX idx_mdm_party_contact_mechs_party_id ON mdm_party_contact_mechs (party_id);

-- ---------------------------------------------------------------------------
-- Role profiles (§4.12-4.15) — tables only; the endpoints over them are #81
-- ---------------------------------------------------------------------------

CREATE TABLE mdm_supplier_profiles (
    id                  UUID        PRIMARY KEY,
    tenant_id           UUID        NOT NULL REFERENCES tenants (id),
    created_by          UUID        REFERENCES users (id),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by          UUID        REFERENCES users (id),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at          TIMESTAMPTZ,
    party_id            UUID        NOT NULL REFERENCES mdm_parties (id) ON DELETE CASCADE,
    supplier_number     VARCHAR(64) NOT NULL,
    supplier_category   VARCHAR(64),                    -- e.g. IT
    payment_term_days   INTEGER,
    default_currency_uom VARCHAR(64),                   -- ISO 4217
    tax_number          VARCHAR(64),
    bank_name           VARCHAR(200),
    bank_account        VARCHAR(64),
    bank_account_name   VARCHAR(200),
    approval_status     VARCHAR(40) NOT NULL DEFAULT 'DRAFT'
                        CHECK (approval_status IN ('DRAFT', 'PENDING', 'APPROVED',
                                                   'REJECTED', 'BLACKLISTED')),
    status              VARCHAR(40),
    attributes_json     JSONB       NOT NULL DEFAULT '{}'
);
CREATE UNIQUE INDEX uq_mdm_supplier_profiles_party_id
    ON mdm_supplier_profiles (party_id) WHERE deleted_at IS NULL;
CREATE UNIQUE INDEX uq_mdm_supplier_profiles_tenant_id_supplier_number
    ON mdm_supplier_profiles (tenant_id, supplier_number) WHERE deleted_at IS NULL;

CREATE TABLE mdm_customer_profiles (
    id                  UUID        PRIMARY KEY,
    tenant_id           UUID        NOT NULL REFERENCES tenants (id),
    created_by          UUID        REFERENCES users (id),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by          UUID        REFERENCES users (id),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at          TIMESTAMPTZ,
    party_id            UUID        NOT NULL REFERENCES mdm_parties (id) ON DELETE CASCADE,
    customer_number     VARCHAR(64) NOT NULL,
    customer_category   VARCHAR(64),                    -- e.g. CORPORATE
    customer_since_date DATE,
    credit_limit        NUMERIC(18,2),
    payment_term_days   INTEGER,
    default_currency_uom VARCHAR(64),
    tax_number          VARCHAR(64),
    billing_party_id    UUID        REFERENCES mdm_parties (id),
    status              VARCHAR(40),
    attributes_json     JSONB       NOT NULL DEFAULT '{}'
);
CREATE UNIQUE INDEX uq_mdm_customer_profiles_party_id
    ON mdm_customer_profiles (party_id) WHERE deleted_at IS NULL;
CREATE UNIQUE INDEX uq_mdm_customer_profiles_tenant_id_customer_number
    ON mdm_customer_profiles (tenant_id, customer_number) WHERE deleted_at IS NULL;

-- The aggregate's departmentPartyId is realized as the departments link;
-- organization units modelled as parties use mdm_party_relationships
-- (ORGANIZATION_ROLLUP).
CREATE TABLE mdm_employee_profiles (
    id                  UUID        PRIMARY KEY,
    tenant_id           UUID        NOT NULL REFERENCES tenants (id),
    created_by          UUID        REFERENCES users (id),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by          UUID        REFERENCES users (id),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at          TIMESTAMPTZ,
    party_id            UUID        NOT NULL REFERENCES mdm_parties (id) ON DELETE CASCADE,
    employee_number     VARCHAR(64) NOT NULL,
    department_id       UUID        REFERENCES departments (id),
    manager_party_id    UUID        REFERENCES mdm_parties (id),
    position            VARCHAR(200),
    job_grade           VARCHAR(64),
    employment_type     VARCHAR(40) CHECK (employment_type IN ('FULL_TIME', 'PART_TIME', 'CONTRACT',
                                                               'INTERN', 'OUTSOURCED')),
    join_date           DATE,
    resign_date         DATE,
    status              VARCHAR(40),
    attributes_json     JSONB       NOT NULL DEFAULT '{}'
);
CREATE UNIQUE INDEX uq_mdm_employee_profiles_party_id
    ON mdm_employee_profiles (party_id) WHERE deleted_at IS NULL;
CREATE UNIQUE INDEX uq_mdm_employee_profiles_tenant_id_employee_number
    ON mdm_employee_profiles (tenant_id, employee_number) WHERE deleted_at IS NULL;

-- Tenant-level profile data (legal name, locale, currency — the aggregate's
-- tenantProfile) lives on tenants.settings_json (§2.1); there is no
-- mdm_tenant_profiles table in v1.
CREATE TABLE mdm_contact_profiles (
    id                          UUID        PRIMARY KEY,
    tenant_id                   UUID        NOT NULL REFERENCES tenants (id),
    created_by                  UUID        REFERENCES users (id),
    created_at                  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by                  UUID        REFERENCES users (id),
    updated_at                  TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at                  TIMESTAMPTZ,
    party_id                    UUID        NOT NULL REFERENCES mdm_parties (id) ON DELETE CASCADE,
    contact_type                VARCHAR(64),
    preferred_contact_mech_type VARCHAR(40),
    do_not_contact              BOOLEAN     NOT NULL DEFAULT false,
    assistant_party_id          UUID        REFERENCES mdm_parties (id),
    attributes_json             JSONB       NOT NULL DEFAULT '{}'
);
CREATE UNIQUE INDEX uq_mdm_contact_profiles_party_id
    ON mdm_contact_profiles (party_id) WHERE deleted_at IS NULL;

-- ---------------------------------------------------------------------------
-- Non-party master data (§4.16-4.19) — tables only; endpoints are Sprint 6
-- ---------------------------------------------------------------------------

-- Not party-based. Hierarchical: Building -> Floor -> Room (concepts/03 §4.1).
CREATE TABLE mdm_facilities (
    id                  UUID        PRIMARY KEY,
    tenant_id           UUID        NOT NULL REFERENCES tenants (id),
    created_by          UUID        REFERENCES users (id),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by          UUID        REFERENCES users (id),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at          TIMESTAMPTZ,
    facility_code       VARCHAR(64) NOT NULL,
    name                VARCHAR(200) NOT NULL,
    facility_type       VARCHAR(64),                    -- BUILDING | FLOOR | ROOM | WAREHOUSE | SITE
    parent_facility_id  UUID        REFERENCES mdm_facilities (id),
    owner_party_id      UUID        REFERENCES mdm_parties (id),
    address_json        JSONB       NOT NULL DEFAULT '{}',
    attributes_json     JSONB       NOT NULL DEFAULT '{}',
    record_status       VARCHAR(40) NOT NULL DEFAULT 'DRAFT'
                        CHECK (record_status IN ('DRAFT', 'PENDING_APPROVAL', 'ACTIVE',
                                                 'SUSPENDED', 'INACTIVE', 'ARCHIVED')),
    created_by_document_id      UUID,
    last_updated_by_document_id UUID
);
CREATE UNIQUE INDEX uq_mdm_facilities_tenant_id_facility_code
    ON mdm_facilities (tenant_id, facility_code) WHERE deleted_at IS NULL;
CREATE INDEX idx_mdm_facilities_tenant_id_parent ON mdm_facilities (tenant_id, parent_facility_id);

CREATE TABLE mdm_products (
    id                  UUID        PRIMARY KEY,
    tenant_id           UUID        NOT NULL REFERENCES tenants (id),
    created_by          UUID        REFERENCES users (id),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by          UUID        REFERENCES users (id),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at          TIMESTAMPTZ,
    product_code        VARCHAR(64) NOT NULL,
    name                VARCHAR(200) NOT NULL,
    description         TEXT,
    product_category    VARCHAR(64),
    uom                 VARCHAR(64),                    -- unit of measure
    list_price          NUMERIC(18,2),
    currency_uom        VARCHAR(64),
    attributes_json     JSONB       NOT NULL DEFAULT '{}',
    record_status       VARCHAR(40) NOT NULL DEFAULT 'DRAFT'
                        CHECK (record_status IN ('DRAFT', 'PENDING_APPROVAL', 'ACTIVE',
                                                 'SUSPENDED', 'INACTIVE', 'ARCHIVED')),
    created_by_document_id      UUID,
    last_updated_by_document_id UUID
);
CREATE UNIQUE INDEX uq_mdm_products_tenant_id_product_code
    ON mdm_products (tenant_id, product_code) WHERE deleted_at IS NULL;

CREATE TABLE mdm_services (
    id                  UUID        PRIMARY KEY,
    tenant_id           UUID        NOT NULL REFERENCES tenants (id),
    created_by          UUID        REFERENCES users (id),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by          UUID        REFERENCES users (id),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at          TIMESTAMPTZ,
    service_code        VARCHAR(64) NOT NULL,
    name                VARCHAR(200) NOT NULL,
    description         TEXT,
    service_category    VARCHAR(64),
    uom                 VARCHAR(64),                    -- e.g. HOUR, VISIT
    standard_rate       NUMERIC(18,2),
    currency_uom        VARCHAR(64),
    attributes_json     JSONB       NOT NULL DEFAULT '{}',
    record_status       VARCHAR(40) NOT NULL DEFAULT 'DRAFT'
                        CHECK (record_status IN ('DRAFT', 'PENDING_APPROVAL', 'ACTIVE',
                                                 'SUSPENDED', 'INACTIVE', 'ARCHIVED')),
    created_by_document_id      UUID,
    last_updated_by_document_id UUID
);
CREATE UNIQUE INDEX uq_mdm_services_tenant_id_service_code
    ON mdm_services (tenant_id, service_code) WHERE deleted_at IS NULL;

-- External-system provenance for synchronized master data (architectures/03
-- §4.5). external_system_id stays a bare UUID; its foreign key is added by
-- 0016_integration.sql, which creates external_systems.
CREATE TABLE master_data_source_references (
    id                  UUID        PRIMARY KEY,
    tenant_id           UUID        NOT NULL REFERENCES tenants (id),
    created_by          UUID        REFERENCES users (id),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by          UUID        REFERENCES users (id),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at          TIMESTAMPTZ,
    entity_type         VARCHAR(64) NOT NULL,           -- PARTY | SUPPLIER | CUSTOMER | EMPLOYEE
                                                        -- | FACILITY | PRODUCT | SERVICE
    kelir_entity_id     UUID        NOT NULL,
    external_system_id  UUID        NOT NULL,           -- FK added in 0016
    external_entity_id  VARCHAR(255) NOT NULL,
    external_entity_code VARCHAR(64),
    last_sync_at        TIMESTAMPTZ,
    sync_status         VARCHAR(40) NOT NULL DEFAULT 'PENDING'
                        CHECK (sync_status IN ('PENDING', 'SYNCED', 'FAILED', 'CONFLICT'))
);
CREATE UNIQUE INDEX uq_master_data_source_references_external_entity
    ON master_data_source_references (external_system_id, entity_type, external_entity_id)
    WHERE deleted_at IS NULL;
CREATE INDEX idx_master_data_source_references_kelir_entity
    ON master_data_source_references (entity_type, kelir_entity_id);

-- ---------------------------------------------------------------------------
-- Deferred foreign keys from 0002 (§4.20)
-- ---------------------------------------------------------------------------

-- 0002 left these as bare UUID columns because mdm_parties did not exist. They
-- are constrained here, by the migration that creates the target, exactly as
-- 0002 constrained 0001's system_settings.
ALTER TABLE departments ADD CONSTRAINT fk_departments_manager_party_id
    FOREIGN KEY (manager_party_id) REFERENCES mdm_parties (id);
ALTER TABLE users ADD CONSTRAINT fk_users_party_id
    FOREIGN KEY (party_id) REFERENCES mdm_parties (id);

-- ---------------------------------------------------------------------------
-- Seed data
-- ---------------------------------------------------------------------------

-- The role-type catalogue (§4.4), owned by the system tenant and marked
-- is_system so it cannot be removed out from under a party that holds the role.
-- A tenant adds its own role types as ordinary rows, with no migration (#81).
-- Ids continue the block convention of 0002_identity.sql — permissions take the
-- 0001 block, roles the 0002 block, and role types take 0003.
INSERT INTO mdm_role_types (id, tenant_id, role_type_code, name, description, is_system) VALUES
    ('00000000-0000-0000-0003-000000000001', '00000000-0000-0000-0000-000000000001',
     'TENANT', 'Tenant', 'The deployment''s own organization', true),
    ('00000000-0000-0000-0003-000000000002', '00000000-0000-0000-0000-000000000001',
     'EMPLOYEE', 'Employee', 'A person employed by the organization', true),
    ('00000000-0000-0000-0003-000000000003', '00000000-0000-0000-0000-000000000001',
     'CUSTOMER', 'Customer', 'A party goods or services are sold to', true),
    ('00000000-0000-0000-0003-000000000004', '00000000-0000-0000-0000-000000000001',
     'SUPPLIER', 'Supplier', 'A party goods or services are bought from', true),
    ('00000000-0000-0000-0003-000000000005', '00000000-0000-0000-0000-000000000001',
     'CONTACT', 'Contact', 'A person acting for another party', true),
    ('00000000-0000-0000-0003-000000000006', '00000000-0000-0000-0000-000000000001',
     'ORGANIZATION_UNIT', 'Organization Unit', 'A division or team modelled as a party', true);

-- Permission catalogue for the master-data module (naming convention §6). The
-- resource segment is required because the module manages several resources.
-- Only the four the party endpoints enforce are seeded; party roles bring their
-- own in #81. An unenforced permission row is worse than a missing one — it
-- reads as a control that exists.
INSERT INTO permissions (id, tenant_id, permission_code, module, description) VALUES
    ('00000000-0000-0000-0001-000000000010', '00000000-0000-0000-0000-000000000001',
     'master-data:party:create', 'master-data', 'Create parties'),
    ('00000000-0000-0000-0001-000000000011', '00000000-0000-0000-0000-000000000001',
     'master-data:party:read',   'master-data', 'View parties'),
    ('00000000-0000-0000-0001-000000000012', '00000000-0000-0000-0000-000000000001',
     'master-data:party:update', 'master-data', 'Modify parties'),
    ('00000000-0000-0000-0001-000000000013', '00000000-0000-0000-0000-000000000001',
     'master-data:party:delete', 'master-data', 'Delete parties');

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
    'master-data:party:create',
    'master-data:party:read',
    'master-data:party:update',
    'master-data:party:delete'
);
