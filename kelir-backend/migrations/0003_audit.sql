-- 0003_audit.sql — the formal audit record.
--
-- **Arriving earlier than the schema document maps it.** Database Schema §10
-- places audit_events in the activity/audit migration alongside activity_events
-- and snapshots, which is Phase 6 work. FR-AUTH-008 ("record login activity in
-- audit log") is Phase 2 scope, so the table is needed now, and SQLx applies
-- migrations in version order — a later phase cannot insert a lower-numbered
-- file. The remainder of §10 lands with its own migration in Phase 6.
--
-- Columns referencing tables that do not exist yet (documents,
-- workflow_instances) are bare UUIDs here; the migrations that create those
-- tables add the constraints, matching how §3 handles party_id.

CREATE TABLE audit_events (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    document_id     UUID,                           -- FK added by the document migration
    workflow_instance_id UUID,                      -- FK added by the workflow migration
    entity_type     TEXT,                           -- 'SUPPLIER' for master-data changes
    entity_id       UUID,
    event_type      TEXT        NOT NULL,           -- 'Security.SignedIn'
    action          TEXT        NOT NULL,           -- 'LOGIN', 'UPDATE', 'PERMISSION_CHANGE'
    object_type     TEXT        NOT NULL,           -- 'USER', 'ROLE', 'DOCUMENT', 'CONFIG'
    object_id       UUID        NOT NULL,
    old_value_json  JSONB,
    new_value_json  JSONB,
    reason          TEXT,
    actor_user_id   UUID        REFERENCES users (id),
    actor_role_id   UUID        REFERENCES roles (id),
    ip_address      TEXT,
    digital_signature_ref TEXT,
    previous_hash   TEXT        NOT NULL,           -- 'sha256:...' of the previous row in the tenant chain
    current_hash    TEXT        NOT NULL,           -- hash over row content + previous_hash
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Append-only (FR-AUD-003): no updated_at, no deleted_at, and nothing in the
-- application updates or deletes a row. The chain in previous_hash/current_hash
-- makes tampering detectable; enforcing it in the database with a rule or
-- trigger lands with the tamper-evidence work in Phase 6.
CREATE INDEX idx_audit_events_tenant_id_created_at ON audit_events (tenant_id, created_at);
CREATE INDEX idx_audit_events_object ON audit_events (object_type, object_id, created_at);
CREATE INDEX idx_audit_events_document_id ON audit_events (document_id, created_at);
CREATE INDEX idx_audit_events_actor_user_id ON audit_events (actor_user_id, created_at);
