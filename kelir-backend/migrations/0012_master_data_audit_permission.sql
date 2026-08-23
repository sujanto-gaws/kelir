-- 0012_master_data_audit_permission.sql — the permission the change-history
-- surface checks.
--
-- No new tables. audit_events was created by 0003_audit.sql and the master-data
-- write path has been filling it since #80's first endpoint; what #100 adds is
-- the surface that reads it back, and this is the one row that surface checks.
--
-- Takes 0012 because that is the next free number after 0011 (naming convention
-- §4.3). The migrations planned after it shift down by one in the Database
-- Schema mapping table, which is the only place the sequence lives.
--
-- Continues the id-block convention of 0002_identity.sql: permissions take the
-- 0001 block, which stands at ...0021 after 0011.
--
-- A master-data permission rather than the audit module's own `audit:read`.
-- That module has no endpoints yet, and minting its permission here would seed
-- a catalogue row the audit module then has to honour — a control defined by
-- the first caller that needed it rather than by the module that owns it. When
-- FR-AUD-004 lands, `audit:read` is its to define, and whether this row folds
-- into it is a decision that surface can make with its own requirements in
-- front of it.
--
-- It does not carry the role records on its own. A role assignment's audit
-- record names the role type, and #81 keeps a party's roles from a caller
-- without master-data:party-role:read — so the service includes those rows only
-- for a caller who holds both, and counts the page the same way.

INSERT INTO permissions (id, tenant_id, permission_code, module, description) VALUES
    ('00000000-0000-0000-0001-000000000022', '00000000-0000-0000-0000-000000000001',
     'master-data:audit:read', 'master-data',
     'Read the change history of a master-data record');

-- ROLE-ADMIN holds every permission in the catalogue (0002_identity.sql); grant
-- only the new row rather than re-inserting the ones already granted.
INSERT INTO role_permissions (id, tenant_id, role_id, permission_id)
SELECT
    gen_random_uuid(),
    '00000000-0000-0000-0000-000000000001',
    '00000000-0000-0000-0002-000000000001',
    id
FROM permissions
WHERE permission_code = 'master-data:audit:read';
