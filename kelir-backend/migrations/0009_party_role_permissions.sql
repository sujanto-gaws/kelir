-- 0009_party_role_permissions.sql — permission catalogue rows for party roles.
--
-- No new tables: mdm_role_types, mdm_party_roles and the four role profiles
-- were created by 0008_master_data.sql. This adds only what #81's endpoints
-- enforce, which 0008 deliberately did not seed — a permission row that no
-- route checks reads as a control that exists.
--
-- Takes 0009 because that is the next free number, not because a phase reserved
-- it (naming convention §4.3). The migrations planned after it shift down by
-- one in the Database Schema mapping table, which is the only place the
-- sequence lives; merged migrations are never renumbered, so 0008's forward
-- references still name the numbers that were planned when it was written.
--
-- Continues the id-block convention of 0002_identity.sql: permissions take the
-- 0001 block, which stands at ...0013 after 0008.
--
-- Three actions rather than the party core's four. A role is assigned or
-- removed, never edited in place — changing which role a party holds means
-- ending one and starting another, and the history is the point (§4.5 carries
-- starts_at and ends_at). That mirrors identity's delegations, which have
-- create/read/delete and no update for the same reason.
--
-- `read` is a real gate, not a courtesy. A supplier profile carries a bank
-- account number and a customer profile a credit limit, so seeing that a party
-- exists and seeing what it is worth are different permissions: the party
-- aggregate omits its `roles` and `profiles` entirely for a caller who holds
-- master-data:party:read alone.

INSERT INTO permissions (id, tenant_id, permission_code, module, description) VALUES
    ('00000000-0000-0000-0001-000000000014', '00000000-0000-0000-0000-000000000001',
     'master-data:party-role:assign', 'master-data', 'Assign a role and its profile to a party'),
    ('00000000-0000-0000-0001-000000000015', '00000000-0000-0000-0000-000000000001',
     'master-data:party-role:remove', 'master-data', 'Remove a role from a party'),
    ('00000000-0000-0000-0001-000000000016', '00000000-0000-0000-0000-000000000001',
     'master-data:party-role:read',   'master-data', 'View the roles and role profiles of a party');

-- ROLE-ADMIN holds every permission in the catalogue (0002_identity.sql); grant
-- only the three new rows rather than re-inserting the ones already granted.
INSERT INTO role_permissions (id, tenant_id, role_id, permission_id)
SELECT
    gen_random_uuid(),
    '00000000-0000-0000-0000-000000000001',
    '00000000-0000-0000-0002-000000000001',
    id
FROM permissions
WHERE permission_code IN (
    'master-data:party-role:assign',
    'master-data:party-role:remove',
    'master-data:party-role:read'
);
