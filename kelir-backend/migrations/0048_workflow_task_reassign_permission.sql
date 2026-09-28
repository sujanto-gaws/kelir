-- 0048_workflow_task_reassign_permission.sql — the permission an administrator
-- reassigns an open task under (FR-WF-017; #512; D-91; ADR-0042).
--
-- `POST /api/v1/workflow/tasks/{id}/reassign` moves an open task to a live
-- user or a live role. `workflow:task:execute` is for working one's own tasks,
-- claiming and deciding them, and which task is the caller's is answered
-- against the row. A reassign acts on a task somebody else holds, or nobody
-- does yet, so the permission is the whole of the check, and D-91 gave it one
-- of its own. Database Schema §7.12 carries the row.
--
-- No schema change. The reassign writes `workflow_tasks`' existing columns and
-- one `workflow_task_history` row with `action = 'REASSIGN'`; that column has no
-- `CHECK` (0025_workflow.sql), so the value needs nothing here.
--
-- # Who holds it
--
-- The system tenant's `ROLE-ADMIN`, as every permission migration since `0010`
-- has granted. A tenant provisioned after this runs is granted it too:
-- `organization::service` gives a provisioned tenant's `ROLE-ADMIN` the whole
-- catalogue except the families it withholds, and this is not one of them
-- (ADR-0042 §2). A tenant provisioned by an older image keeps the grants it was
-- provisioned with; its administrator can grant this code to a role.
--
-- # N−1 compatibility
--
-- One catalogue row and one grant. Nothing existing is altered and nothing is
-- dropped. The previous image names neither the code nor the route, and starts
-- against this schema unchanged (release process §6).

INSERT INTO permissions (id, tenant_id, permission_code, module, description) VALUES
    ('00000000-0000-0000-0001-000000000075', '00000000-0000-0000-0000-000000000001',
     'workflow:task:reassign', 'workflow', 'Reassign an open task to another user or role');

INSERT INTO role_permissions (id, tenant_id, role_id, permission_id)
SELECT
    gen_random_uuid(),
    '00000000-0000-0000-0000-000000000001',
    '00000000-0000-0000-0002-000000000001',
    id
FROM permissions
WHERE permission_code = 'workflow:task:reassign';
