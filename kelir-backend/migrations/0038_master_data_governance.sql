-- 0038_master_data_governance.sql — the row that says a master-data record has
-- a change awaiting approval (FR-MDM-010; #255, decision D-55, recorded as
-- ADR-0033).
--
-- # One table, because two columns already exist and were waiting for this
--
-- **`document_types.target_entity_type` is the configuration** (#255 AC3), and
-- `0015_document.sql` created it in Sprint 4 with the comment *set for
-- master-data change document types*. Nothing has read it since. A type that
-- carries it **and** a workflow binding is a governed-change type; that is the
-- FR-RAD-009 shape the acceptance criterion asks for, and it needs no column of
-- its own.
--
-- **`documents.entity_type` / `entity_id` are the link** (FR-DOC-011, #170).
-- A change document already names the record it is about, and the write-time
-- existence check in `document::repository::link` already holds that row while
-- the document is written.
--
-- So what is missing is not configuration and not a link. It is the **open
-- change** itself: which document is currently proposing a change to this
-- record, and what the record's status was before it was proposed.
--
-- # `previous_record_status`, and why the row exists at all
--
-- **D-55: raising a governed change moves the record to `PENDING_APPROVAL`.**
-- `master_data::domain::record_status` reserved that value for this item in as
-- many words — *it is the workflow's state (FR-MDM-010, Phase 5+) and nothing
-- today can approve anything* — and `RecordStatus::may_move_to` is where it
-- said the change would land when the workflow arrived.
--
-- A refused change has to put the record back, and back is **not always
-- `DRAFT`**: an `ACTIVE` supplier whose address change is rejected is still an
-- active supplier. `PENDING_APPROVAL` on its own cannot say which, so this row
-- remembers, and the resolution reads it rather than guessing.
--
-- # One open change per record, held by the database
--
-- `uq_mdm_change_requests_open_per_record` is a partial unique index over
-- `(entity_type, entity_id) WHERE resolved_at IS NULL` —
-- `uq_workflow_tasks_open_per_instance`'s shape, for the same reason: two
-- approvals in flight over one record would apply in an order nobody chose, and
-- the second would apply to a record the first had already changed. The service
-- refuses the second raise; this index is what makes the refusal true under
-- concurrency rather than usually.
--
-- # No new permission, deliberately
--
-- Raising a change is creating and submitting a **document**, which
-- `document:create` and `document:submit` already govern, against a type an
-- administrator configured. Approving is the workflow's own permission on the
-- task. Applying the change is done by the process rather than by a caller, so
-- there is nobody to check a permission against.
--
-- A `master-data:change-request:*` code would therefore be a row nothing checks,
-- which is the `delegations` situation **D-13** spent two decisions undoing. The
-- one thing that *is* newly refused — a direct `PUT` on a record with a change
-- in flight — is refused by the record's own status, under the permission that
-- already governs that write.
--
-- # N−1 compatibility
--
-- One new table the previous release names in no statement, and no `ALTER` to
-- anything. `v0.6.0`'s binary reads `record_status` through
-- `RecordStatus::from_db`, which already knows `PENDING_APPROVAL` — the value
-- has been in the `CHECK` since `0008` and in the enum since #99 — so a record
-- this release parks there is a record the previous binary renders correctly and
-- refuses to transition, because `may_move_to` is the same table in both.

CREATE TABLE mdm_change_requests (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID        REFERENCES users (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID        REFERENCES users (id),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    document_id     UUID        NOT NULL REFERENCES documents (id) ON DELETE CASCADE,
    entity_type     VARCHAR(64) NOT NULL,
    entity_id       UUID        NOT NULL,
    previous_record_status VARCHAR(40) NOT NULL
                    CHECK (previous_record_status IN ('DRAFT', 'PENDING_APPROVAL', 'ACTIVE',
                                                      'SUSPENDED', 'INACTIVE', 'ARCHIVED')),
    outcome         VARCHAR(40)
                    CHECK (outcome IN ('APPLIED', 'REFUSED')),
    resolved_at     TIMESTAMPTZ
);

CREATE UNIQUE INDEX uq_mdm_change_requests_document_id
    ON mdm_change_requests (document_id);

CREATE UNIQUE INDEX uq_mdm_change_requests_open_per_record
    ON mdm_change_requests (entity_type, entity_id) WHERE resolved_at IS NULL;

CREATE INDEX idx_mdm_change_requests_entity
    ON mdm_change_requests (entity_type, entity_id, created_at);

COMMENT ON TABLE mdm_change_requests IS
    'One row per master-data change raised through a document (FR-MDM-010, #255). Open while resolved_at is null, and at most one may be open per record — the partial unique index, not the service, is what makes that true under concurrency.';

COMMENT ON COLUMN mdm_change_requests.previous_record_status IS
    'Where the record was before the change was raised, so a refusal puts it back rather than guessing DRAFT. An ACTIVE supplier whose change is rejected is still an active supplier.';

COMMENT ON COLUMN mdm_change_requests.outcome IS
    'Null while open. APPLIED when the approval wrote the change, REFUSED when the process ended any other way. The row is kept either way: the attempt is part of the record''s history (#255 AC4).';

COMMENT ON COLUMN document_types.target_entity_type IS
    'PARTY or FACILITY on a governed-change type (FR-MDM-010, #255 AC3), null on an ordinary one. Created by 0015 and read by nothing until #255. A type carrying this and a workflow binding routes changes to that entity through approval; a value this build does not know governs nothing, which is EntityType::from_db refusing rather than guessing.';
