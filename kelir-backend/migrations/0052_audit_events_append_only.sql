-- 0052_audit_events_append_only.sql — a stored audit row refuses a change
-- (FR-AUD-003, NFR-SEC-006; #833; D-139).
--
-- **This supersedes 0003_audit.sql lines 37-40**, which said nothing in the
-- application updates or deletes a row and that enforcing it in the database
-- would land with the tamper-evidence work in Phase 6. That file is applied and
-- is not edited (release process §6), which is why the correction is here, as
-- `0019` did for the same file's column comment.
--
-- **The trigger is per statement, and it names the three verbs.** A `TRUNCATE`
-- has no per-row form, and a per-row trigger would let a write matching no row
-- through without a word. `INSERT` and `SELECT` are untouched, and they are all
-- the audit module does, so a binary of the previous release runs against this
-- schema unchanged (release process §6's N−1 rule).
--
-- **Nothing legitimately changes a row.** No table references `audit_events`,
-- its own foreign keys have no cascade, nothing hard-deletes a tenant, and
-- retention reaches documents and attachments only. There is deliberately no
-- session setting that turns this off: a setting the application could flip is
-- no control.
--
-- **What this stops**: an `UPDATE`, `DELETE` or `TRUNCATE` from the application's
-- connection, a write matching no row included.
--
-- **What this does not stop**:
--   - **anything the table's owner or a superuser can do.** The application
--     connects as the owner today, so a stolen application credential defeats
--     this (#842 is the separate roles). `DISABLE TRIGGER`, `DROP TRIGGER`,
--     `DROP TABLE` and a column-type rewrite, as `0004` did, are the plain
--     cases. **A check that the trigger is enabled proves nothing**: replacing
--     this function, or `ALTER TABLE audit_events INHERIT` and then writing
--     through the parent, changes rows with `tgenabled` still `O`. A superuser
--     also has `session_replication_role = replica` and
--     `pg_restore --disable-triggers`;
--   - access to the disk, a backup or the host;
--   - a restore from an altered dump;
--   - a forged **appended** row, because `INSERT` stays allowed.
--
-- **A restore still works.** A full `pg_restore` emits triggers after the data,
-- and `COPY` is an insert.
--
-- **A future migration that rewrites rows must disable this trigger itself**: an
-- `UPDATE` or `DELETE` on this table is refused for a migration as it is for
-- anybody else. Such a file runs `ALTER TABLE audit_events DISABLE TRIGGER
-- trg_audit_events_append_only` before its write and `ENABLE TRIGGER` after it,
-- and says why. A rewritten row no longer recomputes to its `current_hash`, so
-- it also owes the chain an answer, as `0019` said.

CREATE FUNCTION audit_events_refuse_change() RETURNS trigger
    LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'audit_events is append-only: % is refused', TG_OP;
END
$$;

CREATE TRIGGER trg_audit_events_append_only
    BEFORE UPDATE OR DELETE OR TRUNCATE ON audit_events
    FOR EACH STATEMENT
    EXECUTE FUNCTION audit_events_refuse_change();

COMMENT ON TRIGGER trg_audit_events_append_only ON audit_events IS
    'Refuses UPDATE, DELETE and TRUNCATE unless it is disabled. It does not stop the table owner or a superuser; INSERT stays allowed. FR-AUD-003, #833.';
