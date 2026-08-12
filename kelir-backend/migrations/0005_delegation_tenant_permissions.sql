-- 0005_delegation_tenant_permissions.sql — permission catalogue rows for
-- Sprint 4's new surfaces: delegation management and tenant administration.
--
-- Continues the id-block convention started in 0002_identity.sql (permissions
-- use the 0001 block, roles the 0002 block) without colliding with the ten
-- rows seeded there. `department` permissions already exist and are already
-- granted to ROLE-ADMIN; no rows are added for them here.
--
-- Per naming convention §6, actions are short verbs. A delegation is created
-- or ended, not edited in place, so there is no identity:delegation:update —
-- matching how the department module has no per-department delete, only the
-- two-action read/manage pair.

INSERT INTO permissions (id, tenant_id, permission_code, module, description) VALUES
    ('00000000-0000-0000-0001-00000000000b', '00000000-0000-0000-0000-000000000001', 'identity:delegation:create',     'identity',     'Create delegations'),
    ('00000000-0000-0000-0001-00000000000c', '00000000-0000-0000-0000-000000000001', 'identity:delegation:read',       'identity',     'View delegations'),
    ('00000000-0000-0000-0001-00000000000d', '00000000-0000-0000-0000-000000000001', 'identity:delegation:delete',     'identity',     'End delegations'),
    ('00000000-0000-0000-0001-00000000000e', '00000000-0000-0000-0000-000000000001', 'organization:tenant:read',       'organization', 'View tenants'),
    ('00000000-0000-0000-0001-00000000000f', '00000000-0000-0000-0000-000000000001', 'organization:tenant:manage',     'organization', 'Create and modify tenants');

-- ROLE-ADMIN holds every permission in the catalogue (0002_identity.sql); grant
-- only the five new rows rather than re-inserting the ones already granted.
INSERT INTO role_permissions (id, tenant_id, role_id, permission_id)
SELECT
    gen_random_uuid(),
    '00000000-0000-0000-0000-000000000001',
    '00000000-0000-0000-0002-000000000001',
    id
FROM permissions
WHERE permission_code IN (
    'identity:delegation:create',
    'identity:delegation:read',
    'identity:delegation:delete',
    'organization:tenant:read',
    'organization:tenant:manage'
);
