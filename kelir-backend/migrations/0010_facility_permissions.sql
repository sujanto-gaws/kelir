-- 0010_facility_permissions.sql — permission catalogue rows for facilities.
--
-- No new tables: mdm_facilities, its unique index on (tenant_id, facility_code)
-- and its parent index were all created by 0008_master_data.sql. This adds only
-- what #98's endpoints enforce, which 0008 deliberately did not seed — a
-- permission row that no route checks reads as a control that exists.
--
-- Takes 0010 because that is the next free number after 0009, not because a
-- phase reserved it (naming convention §4.3). The migrations planned after it
-- shift down by one in the Database Schema mapping table, which is the only
-- place the sequence lives; merged migrations are never renumbered.
--
-- Continues the id-block convention of 0002_identity.sql: permissions take the
-- 0001 block, which stands at ...0016 after 0009.
--
-- Four actions, unlike party roles' three. A facility is an entity with a
-- lifetime, not a relationship: it is created, read, edited in place and
-- retired, which is the same shape as the party core's four
-- (master-data:party:create/read/update/delete). The resource segment is
-- required because this module manages several resources (naming convention
-- §6).
--
-- `read` gates the list and the single read together. There is no second,
-- narrower permission for the address or the owner: unlike a supplier profile,
-- nothing on a facility is more sensitive than the fact of the facility, so a
-- second gate would be a control with nothing behind it.

INSERT INTO permissions (id, tenant_id, permission_code, module, description) VALUES
    ('00000000-0000-0000-0001-000000000017', '00000000-0000-0000-0000-000000000001',
     'master-data:facility:create', 'master-data', 'Create a facility'),
    ('00000000-0000-0000-0001-000000000018', '00000000-0000-0000-0000-000000000001',
     'master-data:facility:read',   'master-data', 'View facilities'),
    ('00000000-0000-0000-0001-000000000019', '00000000-0000-0000-0000-000000000001',
     'master-data:facility:update', 'master-data', 'Edit a facility'),
    ('00000000-0000-0000-0001-000000000020', '00000000-0000-0000-0000-000000000001',
     'master-data:facility:delete', 'master-data', 'Retire a facility');

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
    'master-data:facility:create',
    'master-data:facility:read',
    'master-data:facility:update',
    'master-data:facility:delete'
);
