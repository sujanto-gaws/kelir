-- 0013_master_data_audit_permission_scope.sql — the change-history permission
-- says what it grants, now that it no longer grants it alone.
--
-- No new tables and no new rows. `0012_master_data_audit_permission.sql` seeded
-- `master-data:audit:read` with a description written when it was sufficient on
-- its own: "Read the change history of a master-data record". Decision D-12
-- (#136) made the record's own read permission required alongside it, because a
-- record's `oldValue` and `newValue` are the record's own field values — so the
-- catalogue row now describes a permission that opens the question rather than
-- the content, which is what an administrator granting it is choosing.
--
-- Fix-forward rather than an edit to 0012: migrations are immutable once merged
-- (coding standard §2.5), and the checksum in `_sqlx_migrations` is what
-- enforces it — `an_edited_migration_is_still_refused` asserts as much.
--
-- Takes 0013 because that is the next free number after 0012 (naming convention
-- §4.3). The migrations planned after it shift down by one in the Database
-- Schema mapping table, which is the only place the sequence lives.
--
-- Scoped to the row rather than to a tenant: the catalogue is system-defined
-- seed data (D-6), and this description is wrong wherever the row exists.

UPDATE permissions
SET description = 'Ask what changed on a master-data record; the record''s own read permission is required with it'
WHERE permission_code = 'master-data:audit:read';
