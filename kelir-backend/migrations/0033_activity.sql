-- 0033_activity.sql — what happened to this document, for the people looking at
-- it (FR-ACT-001, FR-ACT-004; #247).
--
-- **This is not the audit trail, and that is the first thing this file says.**
-- `audit_events` shipped in `0003_audit.sql` in Sprint 3 — hash-chained,
-- unchangeable, behind `master-data:audit:read`, read by somebody investigating
-- whether a record was tampered with. `activity_events` is a **timeline**: shown
-- to whoever is already looking at a document, behind that document's own read
-- permission, and read by somebody catching up rather than somebody
-- investigating.
--
-- **Neither is derived from the other**, and nothing in this migration or the
-- module above it reads one to write the other. That claim was verified for the
-- three workflow records by tracing every `INSERT` site
-- ([record 09](../../projects/verifications/09.%20Sprint%2011%20Independent%20Pass.md)
-- §6.3), and it has to hold here the same way.
--
-- **D-44 is why the name is `0033_activity` and not `0033_activity_audit`.** The
-- schema's mapping table planned one migration for both, and the audit half had
-- already shipped four sprints earlier; the plan was corrected on 2026-08-31,
-- along with the number — `0030` was taken by `0030_workflow_self_transition.sql`.
--
-- # Append-only, and the columns are what say so
--
-- No `updated_by`, no `updated_at`, no `deleted_at` — §1.2's exception for
-- append-only tables. **An edit has nothing to stamp and a soft delete has
-- nowhere to write**, which is a stronger statement than "no route does that":
-- a route that does not exist today is one somebody adds tomorrow, and #247 AC4
-- asks for the property to be asserted over `information_schema` rather than
-- over the router. `0027_workflow_history.sql` set the shape.
--
-- # `actor_name` is denormalized, and it is not a shortcut
--
-- A timeline is read months later. `actor_user_id` still points at the user and
-- is the join for anything that needs the *current* person; `actor_name` is what
-- their name was **when this happened** (#247 AC5), so a renamed or removed
-- account does not silently rewrite the past. This is the opposite of
-- `comments`, which joins `users` for a live name — a conversation has current
-- participants, and a history has the people who were there.
--
-- # The three foreign keys that made this migration third
--
-- `attachment_id` and `comment_id` reference tables `0031` and `0032` create, so
-- this file cannot precede either. The Sprint 12 construction plan §6 reordered
-- the sprint's items around that: the three Phase 6 migrations have exactly one
-- legal order and this is the last of them.
--
-- # N−1 compatibility
--
-- One new table, nothing altered, nothing dropped. The previous release names it
-- in no statement and starts against this schema unchanged.

CREATE TABLE activity_events (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID        REFERENCES users (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    document_id     UUID        REFERENCES documents (id),
    workflow_instance_id UUID   REFERENCES workflow_instances (id),
    task_id         UUID        REFERENCES workflow_tasks (id),
    attachment_id   UUID        REFERENCES attachments (id),
    comment_id      UUID        REFERENCES comments (id),
    event_type      VARCHAR(64) NOT NULL,
    event_category  TEXT        NOT NULL
                    CHECK (event_category IN ('DOCUMENT', 'ATTACHMENT', 'COMMENT', 'WORKFLOW',
                                              'SECURITY', 'MASTER_DATA', 'NOTIFICATION')),
    actor_type      TEXT        NOT NULL DEFAULT 'USER'
                    CHECK (actor_type IN ('USER', 'SYSTEM', 'WORKFLOW_ENGINE', 'INTEGRATION',
                                          'SCHEDULER', 'PLUGIN')),
    actor_user_id   UUID        REFERENCES users (id),
    actor_name      TEXT,
    action_summary  TEXT        NOT NULL,
    details_json    JSONB       NOT NULL DEFAULT '{}',
    ip_address      VARCHAR(64),
    user_agent      TEXT,
    correlation_id  TEXT
);

CREATE INDEX idx_activity_events_document_id
    ON activity_events (document_id, created_at);
CREATE INDEX idx_activity_events_tenant_id_event_type
    ON activity_events (tenant_id, event_type, created_at);

COMMENT ON TABLE activity_events IS
    'The user-facing timeline of a document (FR-ACT-001), behind that document''s own read permission. Not audit_events, which is hash-chained, behind an audit permission, and answers whether a record was tampered with. Neither is derived from the other.';

COMMENT ON COLUMN activity_events.actor_name IS
    'The actor''s name as it was when this happened (#247 AC5), so a rename does not rewrite the past. actor_user_id is the join for anything that needs the current person.';

COMMENT ON COLUMN activity_events.ip_address IS
    'Null until FR-AUD-005 reaches the audit row it was written for (#248, D-44). A timeline does not show an address; this column exists because §10.1 declares it.';

INSERT INTO permissions (id, tenant_id, permission_code, module, description) VALUES
    ('00000000-0000-0000-0001-000000000055', '00000000-0000-0000-0000-000000000001',
     'activity:read', 'activity', 'Read a document''s activity timeline');

-- ROLE-ADMIN holds every permission in the catalogue (0002_identity.sql); grant
-- only the new row rather than re-inserting the ones already granted.
INSERT INTO role_permissions (id, tenant_id, role_id, permission_id)
SELECT
    gen_random_uuid(),
    '00000000-0000-0000-0000-000000000001',
    '00000000-0000-0000-0002-000000000001',
    id
FROM permissions
WHERE permission_code = 'activity:read';
