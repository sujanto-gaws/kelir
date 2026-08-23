-- 0011_record_status_permissions.sql — the permission a lifecycle transition needs.
--
-- No new tables. record_status has been a column on mdm_parties,
-- mdm_facilities, mdm_products and mdm_services since 0008_master_data.sql;
-- what #99 adds is the transition that moves it, and this is the one row that
-- transition checks.
--
-- Takes 0011 because that is the next free number after 0010, not because a
-- phase reserved it (naming convention §4.3). The migrations planned after it
-- shift down by one in the Database Schema mapping table, which is the only
-- place the sequence lives; merged migrations are never renumbered.
--
-- Continues the id-block convention of 0002_identity.sql: permissions take the
-- 0001 block, which stands at ...0020 after 0010.
--
-- ONE permission, and the resource segment is a governance action rather than
-- an entity.
--
-- The alternative shape was master-data:party:approve and
-- master-data:facility:approve, growing a row per entity for a control that
-- does not vary by entity — the per-endpoint permission shape decision D-6
-- rejected for the catalogue. mdm_products and mdm_services will take the same
-- row when Sprint 7 gives them endpoints, which is the property this shape has
-- and the other does not.
--
-- It is deliberately NOT master-data:party:update. Correcting a supplier's
-- address and taking the supplier out of service are different authorities:
-- one fixes a record, the other changes what the business may do with it. A
-- transition also carries its own audit action (RECORD_STATUS_CHANGE), which
-- would be indistinguishable from an ordinary edit if it shared the edit's
-- permission.

INSERT INTO permissions (id, tenant_id, permission_code, module, description) VALUES
    ('00000000-0000-0000-0001-000000000021', '00000000-0000-0000-0000-000000000001',
     'master-data:record-status:transition', 'master-data',
     'Move a master-data record through its governance lifecycle');

-- ROLE-ADMIN holds every permission in the catalogue (0002_identity.sql); grant
-- only the new row rather than re-inserting the ones already granted.
INSERT INTO role_permissions (id, tenant_id, role_id, permission_id)
SELECT
    gen_random_uuid(),
    '00000000-0000-0000-0000-000000000001',
    '00000000-0000-0000-0002-000000000001',
    id
FROM permissions
WHERE permission_code = 'master-data:record-status:transition';
