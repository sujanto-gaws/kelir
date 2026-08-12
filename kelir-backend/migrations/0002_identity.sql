-- 0002_identity.sql — users, roles, permissions, departments, delegation.
--
-- Table definitions per docs/design/02. Database Schema.md §3; base columns and
-- conventions per §1. Foreign keys to tables that do not exist yet (mdm_parties,
-- document_types) are deliberately left as bare UUID columns and constrained by
-- the migration that creates the target, as the schema document specifies.

CREATE TABLE departments (
    id                  UUID        PRIMARY KEY,
    tenant_id           UUID        NOT NULL REFERENCES tenants (id),
    created_by          UUID,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by          UUID,
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at          TIMESTAMPTZ,
    department_code     TEXT        NOT NULL,           -- DEPT-PROC
    name                TEXT        NOT NULL,
    parent_department_id UUID       REFERENCES departments (id),
    manager_party_id    UUID,                           -- FK to mdm_parties added in the master-data migration
    status              TEXT        NOT NULL DEFAULT 'ACTIVE'
                        CHECK (status IN ('ACTIVE', 'INACTIVE'))
);
CREATE UNIQUE INDEX uq_departments_tenant_id_department_code
    ON departments (tenant_id, department_code) WHERE deleted_at IS NULL;
CREATE INDEX idx_departments_tenant_id_parent_department_id
    ON departments (tenant_id, parent_department_id);

CREATE TABLE users (
    id                  UUID        PRIMARY KEY,
    tenant_id           UUID        NOT NULL REFERENCES tenants (id),
    created_by          UUID,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by          UUID,
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at          TIMESTAMPTZ,
    username            TEXT        NOT NULL,           -- user.john
    email               TEXT        NOT NULL,
    password_hash       TEXT        NOT NULL,           -- Argon2id PHC string
    display_name        TEXT        NOT NULL,
    department_id       UUID        REFERENCES departments (id),
    party_id            UUID,                           -- FK to mdm_parties added in the master-data migration
    status              TEXT        NOT NULL DEFAULT 'ACTIVE'
                        CHECK (status IN ('ACTIVE', 'INACTIVE', 'LOCKED', 'PENDING_ACTIVATION')),
    last_login_at       TIMESTAMPTZ,
    failed_login_count  INTEGER     NOT NULL DEFAULT 0,
    must_change_password BOOLEAN    NOT NULL DEFAULT false
);
CREATE UNIQUE INDEX uq_users_tenant_id_username
    ON users (tenant_id, username) WHERE deleted_at IS NULL;
CREATE UNIQUE INDEX uq_users_tenant_id_email
    ON users (tenant_id, email) WHERE deleted_at IS NULL;

-- Rotating refresh tokens (architecture 01 §18.1). Append-only: revocation sets
-- revoked_at rather than deleting, so a reused token can still be recognised.
CREATE TABLE refresh_tokens (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    user_id         UUID        NOT NULL REFERENCES users (id),
    token_hash      TEXT        NOT NULL,               -- SHA-256 of the opaque token
    issued_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at      TIMESTAMPTZ NOT NULL,
    rotated_from_id UUID        REFERENCES refresh_tokens (id),
    revoked_at      TIMESTAMPTZ,
    revoked_reason  TEXT
);
CREATE UNIQUE INDEX uq_refresh_tokens_token_hash ON refresh_tokens (token_hash);
CREATE INDEX idx_refresh_tokens_user_id_expires_at ON refresh_tokens (user_id, expires_at);

CREATE TABLE roles (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID,
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    role_code       TEXT        NOT NULL,               -- ROLE-PROC-STAFF
    name            TEXT        NOT NULL,
    description     TEXT,
    is_system       BOOLEAN     NOT NULL DEFAULT false  -- seeded roles, not deletable
);
CREATE UNIQUE INDEX uq_roles_tenant_id_role_code
    ON roles (tenant_id, role_code) WHERE deleted_at IS NULL;

-- Global catalogue owned by the system tenant, seeded here and extended by
-- plugins from Phase 9.
CREATE TABLE permissions (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID,
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    permission_code TEXT        NOT NULL,               -- module:resource:action
    description     TEXT,
    module          TEXT        NOT NULL,               -- first segment, denormalized for grouping
    source          TEXT        NOT NULL DEFAULT 'CORE'
                    CHECK (source IN ('CORE', 'PLUGIN'))
);
CREATE UNIQUE INDEX uq_permissions_tenant_id_permission_code
    ON permissions (tenant_id, permission_code) WHERE deleted_at IS NULL;

CREATE TABLE role_permissions (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID,
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    role_id         UUID        NOT NULL REFERENCES roles (id) ON DELETE CASCADE,
    permission_id   UUID        NOT NULL REFERENCES permissions (id)
);
CREATE UNIQUE INDEX uq_role_permissions_role_id_permission_id
    ON role_permissions (role_id, permission_id) WHERE deleted_at IS NULL;

CREATE TABLE user_roles (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID,
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    user_id         UUID        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    role_id         UUID        NOT NULL REFERENCES roles (id),
    department_id   UUID        REFERENCES departments (id),   -- optional department-scoped grant
    valid_from      DATE,
    valid_to        DATE
);
CREATE UNIQUE INDEX uq_user_roles_user_id_role_id_department_id
    ON user_roles (user_id, role_id, COALESCE(department_id, '00000000-0000-0000-0000-000000000000'))
    WHERE deleted_at IS NULL;
CREATE INDEX idx_user_roles_tenant_id_role_id ON user_roles (tenant_id, role_id);

-- Task and approval delegation windows; consumed by the workflow assignment
-- resolver from Phase 5.
CREATE TABLE delegations (
    id                  UUID        PRIMARY KEY,
    tenant_id           UUID        NOT NULL REFERENCES tenants (id),
    created_by          UUID,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by          UUID,
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at          TIMESTAMPTZ,
    delegator_user_id   UUID        NOT NULL REFERENCES users (id),
    delegate_user_id    UUID        NOT NULL REFERENCES users (id),
    scope               TEXT        NOT NULL DEFAULT 'ALL'
                        CHECK (scope IN ('ALL', 'DOCUMENT_TYPE', 'ROLE')),
    document_type_id    UUID,                           -- FK added by the document migration
    role_id             UUID        REFERENCES roles (id),
    starts_at           TIMESTAMPTZ NOT NULL,
    ends_at             TIMESTAMPTZ NOT NULL,
    reason              TEXT,
    is_active           BOOLEAN     NOT NULL DEFAULT true,
    CONSTRAINT ck_delegations_window CHECK (ends_at > starts_at),
    CONSTRAINT ck_delegations_not_self CHECK (delegate_user_id <> delegator_user_id)
);
CREATE INDEX idx_delegations_tenant_id_delegator_user_id
    ON delegations (tenant_id, delegator_user_id, starts_at, ends_at);

-- The users referenced by created_by / updated_by across every table exist now,
-- so 0001_core.sql's forward references can be constrained.
ALTER TABLE system_settings
    ADD CONSTRAINT fk_system_settings_created_by FOREIGN KEY (created_by) REFERENCES users (id),
    ADD CONSTRAINT fk_system_settings_updated_by FOREIGN KEY (updated_by) REFERENCES users (id);

-- Permission catalogue for the modules that exist in Phase 2. Later phases seed
-- their own; the format is module:resource:action (naming convention §6).
INSERT INTO permissions (id, tenant_id, permission_code, module, description) VALUES
    ('00000000-0000-0000-0001-000000000001', '00000000-0000-0000-0000-000000000001', 'identity:user:create',       'identity',     'Create users'),
    ('00000000-0000-0000-0001-000000000002', '00000000-0000-0000-0000-000000000001', 'identity:user:read',         'identity',     'View users'),
    ('00000000-0000-0000-0001-000000000003', '00000000-0000-0000-0000-000000000001', 'identity:user:update',       'identity',     'Modify users'),
    ('00000000-0000-0000-0001-000000000004', '00000000-0000-0000-0000-000000000001', 'identity:user:delete',       'identity',     'Deactivate users'),
    ('00000000-0000-0000-0001-000000000005', '00000000-0000-0000-0000-000000000001', 'identity:role:create',       'identity',     'Create roles'),
    ('00000000-0000-0000-0001-000000000006', '00000000-0000-0000-0000-000000000001', 'identity:role:read',         'identity',     'View roles and permissions'),
    ('00000000-0000-0000-0001-000000000007', '00000000-0000-0000-0000-000000000001', 'identity:role:update',       'identity',     'Modify roles and their permissions'),
    ('00000000-0000-0000-0001-000000000008', '00000000-0000-0000-0000-000000000001', 'identity:role:delete',       'identity',     'Delete roles'),
    ('00000000-0000-0000-0001-000000000009', '00000000-0000-0000-0000-000000000001', 'organization:department:read',   'organization', 'View departments'),
    ('00000000-0000-0000-0001-00000000000a', '00000000-0000-0000-0000-000000000001', 'organization:department:manage', 'organization', 'Create and modify departments');

-- ROLE-ADMIN is a system role: it cannot be deleted, and it holds every
-- permission in the catalogue. Bootstrapping needs one account that can grant
-- the others.
INSERT INTO roles (id, tenant_id, role_code, name, description, is_system) VALUES
    ('00000000-0000-0000-0002-000000000001', '00000000-0000-0000-0000-000000000001',
     'ROLE-ADMIN', 'Administrator', 'Full access to identity and organization management', true);

INSERT INTO role_permissions (id, tenant_id, role_id, permission_id)
SELECT
    gen_random_uuid(),
    '00000000-0000-0000-0000-000000000001',
    '00000000-0000-0000-0002-000000000001',
    id
FROM permissions;
