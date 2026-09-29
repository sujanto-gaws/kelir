-- 0049_integration_endpoint_call_permission.sql — the permission an
-- administrator makes a test call to an integration endpoint under
-- (FR-INT-002; #547; ADR-0043).
--
-- `POST /api/v1/integration/external-systems/{id}/endpoints/{endpointId}/test-call`
-- resolves the system's credential reference in process, sends one real
-- request to the endpoint and answers with what came back. That is a different
-- question from reading a system (`integration:external-system:read`), editing
-- it (`:update`) or seeing where its secrets are kept
-- (`integration:credential:read`), so it has a permission of its own (#547
-- AC1). Database Schema §12.10 carries the row.
--
-- No schema change. The call writes `integration_logs`, which `0046` created
-- with every column it uses, and the credential types it attaches
-- (`BEARER_TOKEN`, `BASIC_AUTH`) are already in the `credential_type` CHECK. The
-- product owner's answer 3 (2026-09-29) keeps `API_KEY` waiting for a
-- header-name column, so this row has no DDL beyond the permission.
--
-- The id continues the catalogue block after `0048`'s `...0075`.
--
-- # Who holds it
--
-- The system tenant's `ROLE-ADMIN`, as every permission migration since `0010`
-- has granted. **A tenant provisioned after this runs is granted it too**:
-- `organization::service` gives a provisioned tenant's `ROLE-ADMIN` the whole
-- catalogue except the families it withholds — `organization:tenant:*` and,
-- since #551, `integration:credential:*` — and `integration:endpoint:*` is
-- neither. A new tenant's administrator can call an endpoint, and cannot
-- attach a credential to one until somebody grants `integration:credential:create`
-- deliberately, which is the line #551 drew. A tenant provisioned by an older
-- image keeps the grants it was provisioned with; its administrator can grant
-- this code to a role.
--
-- # N−1 compatibility
--
-- One catalogue row and one grant. Nothing existing is altered and nothing is
-- dropped. The previous image names neither the code nor the route, and starts
-- against this schema unchanged (release process §6).

INSERT INTO permissions (id, tenant_id, permission_code, module, description) VALUES
    ('00000000-0000-0000-0001-000000000076', '00000000-0000-0000-0000-000000000001',
     'integration:endpoint:call', 'integration',
     'Make a test call to an integration endpoint, with its system''s credential attached');

INSERT INTO role_permissions (id, tenant_id, role_id, permission_id)
SELECT
    gen_random_uuid(),
    '00000000-0000-0000-0000-000000000001',
    '00000000-0000-0000-0002-000000000001',
    id
FROM permissions
WHERE permission_code = 'integration:endpoint:call';
