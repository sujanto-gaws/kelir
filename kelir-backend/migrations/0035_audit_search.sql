-- 0035_audit_search.sql — the permission FR-AUD-004's surface checks
-- (#252, decision D-49).
--
-- # One row, and the reasoning is why it is a new one
--
-- **D-12** required a master-data record's own read permission alongside
-- `master-data:audit:read` before a change history would hand back its field
-- values. #252 AC2 asks for that rule across every object type, and AC3 asks
-- which permission the general surface checks — *a permission named for one
-- module governing all of them is the kind of thing that reads as a mistake
-- later*.
--
-- So: **`audit:read` is a new code and `master-data:audit:read` is untouched.**
-- The old one opens `GET /parties/{id}/audit`; this one opens
-- `GET /api/v1/audit`. Two surfaces, two questions, two permissions — which is
-- the test **D-47** applied to `activity:read` and found it failing. This pair
-- passes it: neither is a second lock on the other's door.
--
-- **What this grants is the question, not the contents.** A row's `oldValue`
-- and `newValue` are the object's own field values and need that object's read
-- permission; without it the row is served with `valuesWithheld` and no
-- payloads. Hiding the row instead would teach an auditor the trail is shorter
-- than it is.
--
-- # No table, no column, no index
--
-- `audit_events` has been in `0003_audit.sql` since Sprint 3 (**D-44**), and
-- the three indexes this search needs are already on it:
-- `(actor_user_id, created_at)`, `(object_type, object_id, created_at)` and
-- `(tenant_id, created_at)`. The search was written against what exists rather
-- than the schema being widened to suit it.
--
-- # N−1 compatibility
--
-- One permission row and one grant. Nothing altered, nothing dropped, and
-- **adding a permission is the safe direction** — the rule that bites is
-- dropping one the previous release still checks, which is why D-47's removal
-- waits for #301. The previous release names `audit:read` in no statement.

INSERT INTO permissions (id, tenant_id, permission_code, module, description) VALUES
    ('00000000-0000-0000-0001-000000000057', '00000000-0000-0000-0000-000000000001',
     'audit:read', 'audit',
     'Search the audit trail across every module (FR-AUD-004). Grants the question — who did what to which object, and when — not the recorded values: a row''s old and new values are the object''s own field data and additionally require that object''s read permission (D-12, D-49), and are withheld rather than the row hidden.');

-- ROLE-ADMIN holds every permission in the catalogue (0002_identity.sql); grant
-- only the new row rather than re-inserting the ones already granted.
INSERT INTO role_permissions (id, tenant_id, role_id, permission_id)
SELECT
    gen_random_uuid(),
    '00000000-0000-0000-0000-000000000001',
    '00000000-0000-0000-0002-000000000001',
    id
FROM permissions
WHERE permission_code = 'audit:read';
