-- `activity:read` leaves the catalogue, one release after the check that used
-- it (FR-ACT-005, #301, **D-47**).
--
-- # What this deletes and why it is a migration rather than an edit
--
-- `0033_activity.sql` seeded the row and granted it to `ROLE-ADMIN`. **D-47**
-- (#250 AC2) then removed `caller.require("activity:read")` from
-- `activity::service::list_activity`: a document's timeline reads through the
-- document's own permission and nothing else, which the four-record table in
-- `modules::activity`'s header, `0033`'s own `COMMENT ON TABLE` and Database
-- Schema §10 had all said from the start.
--
-- What that left behind is **a permission row nothing checks**, which is the
-- `delegations` shape **D-13** spent two decisions undoing and which
-- `modules::attachment` and `modules::comment` both cite as the reason they
-- declare no `delete` permission. A row that exists reads as a control that
-- exists: a tenant granting it believes they have opened something, and a
-- tenant withholding it believes they have closed something. Neither is true.
--
-- # N−1 compatibility, which is the whole reason this waited a release
--
-- The [release process](../../docs/standards/04.%20Release%20Process.md) §6
-- schema half: *don't drop or rename columns/tables in the same release that
-- stops using them — deprecate in release N, remove in N+1.* `v0.5.0`'s binary
-- calls `caller.require("activity:read")`, so deleting the row beside the check
-- would have 403'd every timeline read on the release still serving traffic
-- through a rolling deploy, and left a rollback with a check that can never
-- pass.
--
-- **`v0.6.0` is the N−1 for this migration and it does not check the
-- permission.** Its `list_activity` asks `document::service::get_document` and
-- nothing else, so a row this migration removes is a row that binary reads in no
-- statement. Rehearsed rather than reasoned about (§6): the rehearsal is
-- recorded on #301.
--
-- Nothing else references the row. `permissions` is read by
-- `identity::repository::list_permissions` as a catalogue and joined through
-- `role_permissions`; there is no foreign key into it from anywhere a tenant's
-- data lives, so the grants and the row are the whole of it.

-- The grants first: `role_permissions.permission_id` references `permissions`,
-- and a delete in the other order refuses.
DELETE FROM role_permissions
WHERE permission_id IN (
    SELECT id FROM permissions WHERE permission_code = 'activity:read'
);

-- **By code rather than by the id `0033` wrote.** The seed inserted one row for
-- the system tenant; a deployment that copied the catalogue into a second tenant
-- holds the same code under a different id, and this removes the permission
-- rather than one row of it.
DELETE FROM permissions
WHERE permission_code = 'activity:read';
