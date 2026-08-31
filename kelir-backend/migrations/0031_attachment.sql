-- 0031_attachment.sql — a document carries the files it is about (FR-ATT-001,
-- FR-ATT-003; #244).
--
-- **Three tables, because they are one area and one of them closes a debt.**
-- `attachment_categories` is what `0015_document.sql` said would arrive with
-- "the attachment migration" — its `document_type_attachment_rules.category_id`
-- has carried the comment *"No REFERENCES: `attachment_categories` arrives with
-- 0017_attachment.sql"* since 2026-08-25, and the number in that comment is two
-- shifts out of date. Read it the way [Database Schema](../../docs/design/02.%20Database%20Schema.md)
-- §1 says to read every such comment: by what the migration creates, not by its
-- number. This is that file.
--
-- **The number.** `0031` and not `0028`, which the schema's mapping table
-- reserved for this file until 2026-08-31. `0028`, `0029` and `0030` were taken
-- by `0028_delegation.sql`, `0029_workflow_routing.sql` and
-- `0030_workflow_self_transition.sql` — Sprints 10 and 11's own work, each
-- taking the next free number as naming convention §4.3 requires. The table has
-- been corrected and the three rows below this one shifted with it.
--
-- # What this migration does not do, stated so the gaps are decisions
--
-- **`attachment_versions` is created and nothing writes it.** Versioning is
-- FR-ATT-007, a `Could`, and Sprint 13 stretch. It is created here rather than
-- in that sprint because `attachments.current_version_number` already implies
-- it and a table implied by a column is a table somebody will assume exists.
-- `0027_workflow_history.sql` set this precedent by saying which issue would
-- fill its `comment` column; this says which sprint fills a table.
--
-- **Nothing sets `virus_scan_status` to anything but its default.** The scan is
-- #246 and the gate is its item. An attachment that reads `CLEAN` because
-- nothing scanned it is worse than one that reads `PENDING` for ever, so the
-- `CHECK` admits the four values §8.2 declares and this sprint's item 1 writes
-- exactly one of them.
--
-- **`attachment:delete` is not seeded.** Soft-delete is FR-ATT-006 and Sprint
-- 13; a permission row nothing checks is the `delegations` situation **D-13**
-- spent two decisions undoing — a table with a writer and no reader, or here a
-- grant with no gate. The two rows seeded are the two this sprint's items 1 and
-- 2 actually require.
--
-- # There is no constraint tying a row to an object, and that is the design
--
-- An object lives in object storage and a row lives here, and no transaction
-- spans both. #244 AC2 requires the failure that is possible to be the
-- recoverable one, and the service writes **the object first, then the row** —
-- so an interrupted upload can leave an object with no row, which costs storage
-- and reaches nobody, and can never leave a row whose `storage_reference`
-- points at nothing, which would be a download that 500s for somebody who did
-- nothing wrong. `storage_reference` is generated from the document and a fresh
-- id and is never taken from the request (#244 AC6): a caller-supplied path is a
-- caller-chosen destination.
--
-- # N−1 compatibility
--
-- **Three new tables the previous release does not know about**, and one
-- `ALTER TABLE ... ADD CONSTRAINT` on `document_type_attachment_rules`. That
-- table has existed since `0015` and **nothing in any release has ever written
-- to it** — `grep` over `kelir-backend/src/` finds no statement naming it — so
-- it is empty in every deployment, the constraint validates against no rows, and
-- `v0.4.0`'s binary neither reads nor writes it. The previous release starts
-- against this schema unchanged.

CREATE TABLE attachment_categories (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID        REFERENCES users (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID        REFERENCES users (id),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    category_code   VARCHAR(64) NOT NULL,
    name            VARCHAR(200) NOT NULL,
    description     TEXT,
    is_system       BOOLEAN     NOT NULL DEFAULT false
);

CREATE UNIQUE INDEX uq_attachment_categories_tenant_id_category_code
    ON attachment_categories (tenant_id, category_code) WHERE deleted_at IS NULL;

-- The debt `0015_document.sql` named. Its column has been `NOT NULL` with no
-- referent for four sprints; nothing has written the table, so nothing can have
-- put an unmatched value in it.
ALTER TABLE document_type_attachment_rules
    ADD CONSTRAINT fk_document_type_attachment_rules_category_id
    FOREIGN KEY (category_id) REFERENCES attachment_categories (id);

CREATE TABLE attachments (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID        REFERENCES users (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID        REFERENCES users (id),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    document_id     UUID        NOT NULL REFERENCES documents (id) ON DELETE CASCADE,
    document_version_id UUID    REFERENCES document_versions (id),
    task_id         UUID        REFERENCES workflow_tasks (id),
    file_name       TEXT        NOT NULL,
    original_file_name TEXT     NOT NULL,
    mime_type       TEXT        NOT NULL,
    file_size       BIGINT      NOT NULL,
    checksum        TEXT        NOT NULL,
    storage_reference TEXT      NOT NULL,
    category_id     UUID        REFERENCES attachment_categories (id),
    description     TEXT,
    security_level  TEXT        NOT NULL DEFAULT 'INTERNAL'
                    CHECK (security_level IN ('PUBLIC', 'INTERNAL', 'CONFIDENTIAL', 'RESTRICTED')),
    virus_scan_status TEXT      NOT NULL DEFAULT 'PENDING'
                    CHECK (virus_scan_status IN ('PENDING', 'CLEAN', 'INFECTED', 'FAILED')),
    retention_policy_id UUID    REFERENCES retention_policies (id),
    current_version_number INTEGER NOT NULL DEFAULT 1
);

CREATE INDEX idx_attachments_document_id
    ON attachments (document_id) WHERE deleted_at IS NULL;

COMMENT ON COLUMN attachments.storage_reference IS
    'Where the bytes are, generated from the document and a fresh id and never taken from the request (#244 AC6). The object is written before this row, so an object with no row is possible and a row with no object is not.';

COMMENT ON COLUMN attachments.virus_scan_status IS
    'PENDING until something scans it (#246). Download is refused unless CLEAN, and FAILED is a refusal rather than a pass: a scan that could not run has cleared nothing. Item 1 writes only the default.';

COMMENT ON COLUMN attachments.original_file_name IS
    'The name as uploaded, kept because it is what the person recognises. `file_name` is the stored name and the two differ whenever the first is unsafe to use as the second.';

CREATE TABLE attachment_versions (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID        REFERENCES users (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID        REFERENCES users (id),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    attachment_id   UUID        NOT NULL REFERENCES attachments (id) ON DELETE CASCADE,
    version_number  INTEGER     NOT NULL,
    storage_reference TEXT      NOT NULL,
    file_size       BIGINT      NOT NULL,
    checksum        TEXT        NOT NULL,
    change_reason   TEXT
);

CREATE UNIQUE INDEX uq_attachment_versions_attachment_id_version_number
    ON attachment_versions (attachment_id, version_number);

COMMENT ON TABLE attachment_versions IS
    'Created by 0031 and written by nothing. Attachment versioning is FR-ATT-007, a Could, and Sprint 13 stretch; the table exists here because attachments.current_version_number already implies it.';

INSERT INTO permissions (id, tenant_id, permission_code, module, description) VALUES
    ('00000000-0000-0000-0001-000000000051', '00000000-0000-0000-0000-000000000001',
     'attachment:create',  'attachment', 'Attach a file to a document'),
    ('00000000-0000-0000-0001-000000000052', '00000000-0000-0000-0000-000000000001',
     'attachment:read',    'attachment', 'List and download a document''s attachments');

-- ROLE-ADMIN holds every permission in the catalogue (0002_identity.sql); grant
-- only the two new rows rather than re-inserting the ones already granted.
INSERT INTO role_permissions (id, tenant_id, role_id, permission_id)
SELECT
    gen_random_uuid(),
    '00000000-0000-0000-0000-000000000001',
    '00000000-0000-0000-0002-000000000001',
    id
FROM permissions
WHERE permission_code IN ('attachment:create', 'attachment:read');
