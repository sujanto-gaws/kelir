-- 0050_integration_log_read_permission.sql — the permission an
-- administrator reads the integration log under (FR-INT-006; #548).
--
-- `GET /api/v1/integration/logs` and `GET /api/v1/integration/logs/{id}` list
-- and show `integration_logs` rows: the calls Kelir made, with their masked
-- request and response payloads. That is a different question from reading a
-- system (`integration:external-system:read`), seeing where its secrets are
-- kept (`integration:credential:read`) or making a call
-- (`integration:endpoint:call`), so it has a permission of its own (#548 AC2):
-- a log reader need not manage systems, and a system manager does not see
-- every call's payload by default. Database Schema §12.10 carries the row.
--
-- No schema change. `0046` created `integration_logs` with every column the
-- routes read, and `idx_integration_logs_tenant_system_started` is the index
-- the list uses.
--
-- The id continues the catalogue block after `0049`'s `...0076`.
--
-- # Who holds it
--
-- The system tenant's `ROLE-ADMIN`, as every permission migration since `0010`
-- has granted. **A tenant provisioned after this runs is granted it too**:
-- `organization::service` gives a provisioned tenant's `ROLE-ADMIN` the whole
-- catalogue except the families it withholds — `organization:tenant:*` and,
-- since #551, `integration:credential:*` — and `integration:log:*` is neither.
-- The payloads a log row holds were masked before they were stored, so reading
-- them shows no secret, which is why the family is not withheld. A tenant
-- provisioned by an older image keeps the grants it was provisioned with; its
-- administrator can grant this code to a role.
--
-- # N−1 compatibility
--
-- One catalogue row and one grant. Nothing existing is altered and nothing is
-- dropped. The previous image names neither the code nor the routes, and
-- starts against this schema unchanged (release process §6).

INSERT INTO permissions (id, tenant_id, permission_code, module, description) VALUES
    ('00000000-0000-0000-0001-000000000077', '00000000-0000-0000-0000-000000000001',
     'integration:log:read', 'integration',
     'Read the integration log: the calls Kelir made and their masked payloads');

INSERT INTO role_permissions (id, tenant_id, role_id, permission_id)
SELECT
    gen_random_uuid(),
    '00000000-0000-0000-0000-000000000001',
    '00000000-0000-0000-0002-000000000001',
    id
FROM permissions
WHERE permission_code = 'integration:log:read';
