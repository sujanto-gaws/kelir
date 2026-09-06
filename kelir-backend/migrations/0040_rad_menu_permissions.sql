-- The four permissions the menu builder's routes check (FR-RAD-004, #341).
--
-- `rad_menus` has been in the schema since `0014_rad.sql` and had no reader, so
-- `0014` deliberately seeded no permission for it: Database Schema §5.13 states
-- the rule it was following — *entities, fields, menus, actions and lookups have
-- no endpoint yet, and a permission row that no route checks reads as a control
-- that exists*. `/api/v1/rad/menus` is now four routes, so the rows arrive with
-- the thing that checks them.
--
-- **Four rather than one**, matching forms and lists: reading the navigation a
-- deployment has configured and rewriting it are different questions, and a
-- tenant that wants them to be the same person grants both. There is no
-- `publish` counterpart — a menu has no revision anything pins, which is the
-- same reason `rad:list:*` has none.

INSERT INTO permissions (id, tenant_id, permission_code, module, description) VALUES
    ('00000000-0000-0000-0001-000000000062', '00000000-0000-0000-0000-000000000001',
     'rad:menu:create', 'rad', 'Add an entry to the configured navigation'),
    ('00000000-0000-0000-0001-000000000063', '00000000-0000-0000-0000-000000000001',
     'rad:menu:read',   'rad', 'View the configured navigation'),
    ('00000000-0000-0000-0001-000000000064', '00000000-0000-0000-0000-000000000001',
     'rad:menu:update', 'rad', 'Edit a configured navigation entry'),
    ('00000000-0000-0000-0001-000000000065', '00000000-0000-0000-0000-000000000001',
     'rad:menu:delete', 'rad', 'Remove a configured navigation entry');

-- ROLE-ADMIN holds every permission in the catalogue (0002_identity.sql); grant
-- only the four new rows rather than re-inserting the ones already granted.
INSERT INTO role_permissions (id, tenant_id, role_id, permission_id)
SELECT
    gen_random_uuid(),
    '00000000-0000-0000-0000-000000000001',
    '00000000-0000-0000-0002-000000000001',
    id
FROM permissions
WHERE permission_code IN (
    'rad:menu:create',
    'rad:menu:read',
    'rad:menu:update',
    'rad:menu:delete'
);
