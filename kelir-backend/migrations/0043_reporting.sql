-- 0043_reporting.sql — the dashboard gets a permission and the one index its
-- summary reads through (FR-RPT-001; #431).
--
-- **This migration creates no table, and that is the point worth stating
-- first.** `modules::reporting` reads `workflow_tasks` and `documents`, both of
-- which belong to other modules and neither of which is copied here. A
-- reporting table would be a third record of work that two records already
-- hold, and `modules::activity`'s own four-record table is this project's
-- longest argument about what that costs. **The dashboard is a read over
-- existing rows.**
--
-- # Why there is a permission at all
--
-- [ADR-0039](../../docs/architectures/adr/0039.%20A%20Dashboard%20Widget%20Is%20a%20Purpose-Built%20Endpoint.md)
-- (**D-78**) makes every FR-RPT widget a purpose-built endpoint rather than a
-- RAD list definition, so the dashboard is a **surface** in its own right and
-- has to be grantable like one. Without a row here the only way to gate it
-- would be to require the permissions of the records underneath — and that is
-- the shape the next paragraph exists to refuse.
--
-- # And why it is not a second permission over somebody else's data
--
-- **`0041_activity_read_dropped.sql` is the cost of getting this wrong**, and
-- it is one migration away. `activity:read` gated a timeline whose every fact
-- was already guarded by the document's own read; **D-45** found it serving an
-- attachment's file name to a caller holding neither `attachment:read` nor
-- `comment:read`, **D-47** took the second permission out of the check, and
-- #301 took the row out of the catalogue one release later. A permission whose
-- only job is to duplicate one already checked outlives the check and then
-- guards nothing.
--
-- **`reporting:dashboard:read` avoids that by what the summary counts, not by
-- assertion.** Every number it returns is about rows the viewer already has a
-- first-hand relationship with: tasks assigned to them or offered to a role
-- they hold, and documents they raised themselves. **No number on it describes
-- another person's work or another surface's data**, so there is no second
-- permission it could be duplicating — and none it is quietly standing in for.
-- What it does gate is the surface: a deployment that does not want a person on
-- the dashboard has one grant to withhold.
--
-- # The index, and why `status` is the third column
--
-- The summary's document half counts the caller's own drafts, so the predicate
-- is `(tenant_id, created_by, status)` and all three are equalities.
-- `idx_documents_tenant_id_status` (`0015`) leads with the wrong column for it
-- — a tenant's whole `DRAFT` population, filtered to one author afterwards —
-- which is the scan that grows with the deployment rather than with the
-- person's own work (NFR-PERF-002). **A dashboard is the one screen every
-- signed-in person loads**, so the read it does on every sign-in is the wrong
-- one to leave to a filter.
--
-- `created_by` is nullable (`0015`), and a null author matches no caller: rows
-- the system raised are in nobody's dashboard, which is correct and is the
-- reason the count is not `count(*)` over a looser predicate.
--
-- # N−1 compatibility
--
-- One permission row, one grant, one index. Nothing is altered and nothing is
-- dropped, so the `v0.7.0` image — which names neither the permission nor the
-- index in any statement — starts against this schema unchanged (release
-- process §6). In the other direction a `v0.8.0` image against the `v0.7.0`
-- schema would find the permission missing and refuse the dashboard with a 403,
-- which is a surface being unavailable rather than a failure.

-- The caller's own drafts, counted on every dashboard load.
CREATE INDEX idx_documents_tenant_id_created_by_status
    ON documents (tenant_id, created_by, status);

COMMENT ON INDEX idx_documents_tenant_id_created_by_status IS
    'The dashboard summary counts the caller''s own drafts (FR-RPT-001, #431). idx_documents_tenant_id_status leads with the tenant''s whole status population and filters to one author afterwards, which grows with the deployment rather than with the person''s work.';

INSERT INTO permissions (id, tenant_id, permission_code, module, description) VALUES
    ('00000000-0000-0000-0001-000000000066', '00000000-0000-0000-0000-000000000001',
     'reporting:dashboard:read', 'reporting',
     'Read the dashboard summary — the caller''s own waiting work, not anybody else''s');

-- ROLE-ADMIN holds every permission in the catalogue (0002_identity.sql); grant
-- only the new row rather than re-inserting the ones already granted.
INSERT INTO role_permissions (id, tenant_id, role_id, permission_id)
SELECT
    gen_random_uuid(),
    '00000000-0000-0000-0000-000000000001',
    '00000000-0000-0000-0002-000000000001',
    id
FROM permissions
WHERE permission_code = 'reporting:dashboard:read';
