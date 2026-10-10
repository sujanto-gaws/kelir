-- 0051_workflow_definition_deprecate_permission.sql — the permission an
-- administrator deprecates a published workflow revision under (FR-WF-001;
-- #573; D-101 B; D-108).
--
-- `POST /api/v1/workflow/definitions/{id}/deprecation` moves an `ACTIVE`
-- revision to `DEPRECATED`. No existing code fits. `workflow:definition:update`
-- is *Edit a draft workflow revision* and the service refuses it on anything
-- but a `DRAFT`; `workflow:definition:publish` fixes a revision, the opposite
-- act; and `workflow:definition:delete` removes a row. Deprecating changes
-- which revisions new documents route to without removing or editing one, so
-- D-108 gave it a permission of its own. Database Schema §7.12 carries the row.
--
-- No schema change. `workflow_definitions.status` already admits `DEPRECATED`
-- (0025_workflow.sql); until the route there was only SQL to write it.
--
-- The id continues the catalogue block after `0050`'s `...0077`.
--
-- # The delete permission's text
--
-- `0025` seeded `workflow:definition:delete` as *Retire a workflow definition*.
-- D-101 uses *retire* for what this route does, and `delete_definition` does
-- something else: it soft-deletes one revision, and refuses while approvals
-- still run on it. Two codes whose descriptions both say "retire" invite
-- granting the wrong one, so the text is reworded below to what the route does.
-- The code itself is unchanged. `0013` and `0047` are the precedents for
-- correcting a catalogue description; migrations are never edited once applied
-- (SQLx verifies checksums), so `0025` keeps its text and this is the way.
--
-- # Who holds it
--
-- The system tenant's `ROLE-ADMIN`, as every permission migration since `0010`
-- has granted, and as `0025_workflow.sql` granted the other workflow definition
-- permissions. **A tenant provisioned after this runs is granted it too**:
-- `organization::service` gives a provisioned tenant's `ROLE-ADMIN` the whole
-- catalogue except the families it withholds, `organization:tenant:*` and
-- `integration:credential:*`, and `workflow:definition:*` is neither. A tenant
-- provisioned by an older image keeps the grants it was provisioned with; its
-- administrator can grant this code to a role.
--
-- # N−1 compatibility
--
-- One catalogue row, one grant, and one description rewrite. Nothing is dropped
-- and no column, constraint or index is touched. No code reads a description,
-- and the previous image names neither the new code nor the route, so it starts
-- against this schema unchanged (release process §6).

INSERT INTO permissions (id, tenant_id, permission_code, module, description) VALUES
    ('00000000-0000-0000-0001-000000000078', '00000000-0000-0000-0000-000000000001',
     'workflow:definition:deprecate', 'workflow',
     'Deprecate a published workflow revision, so new documents stop routing to it');

INSERT INTO role_permissions (id, tenant_id, role_id, permission_id)
SELECT
    gen_random_uuid(),
    '00000000-0000-0000-0000-000000000001',
    '00000000-0000-0000-0002-000000000001',
    id
FROM permissions
WHERE permission_code = 'workflow:definition:deprecate';

UPDATE permissions
SET description = 'Delete a workflow revision that no running approval uses',
    updated_at = now()
WHERE permission_code = 'workflow:definition:delete';
