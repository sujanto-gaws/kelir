-- 0027_workflow_history.sql — how a document got here, as its own record
-- (FR-WF-012, #181).
--
-- **A new table rather than a column on an old one, and the reason is
-- `workflow_task_history`.** That table (§7.7) looks like the obvious home and
-- is not one, on two counts that both matter:
--
--   * Its `task_id` is `NOT NULL`. A transition does not need a task — the
--     instance's first state is entered by the submit, and FR-WF-005's system
--     tasks and JWSS's `AUTO` transitions will move a process with no task at
--     all. Making that column nullable would widen a table whose every existing
--     row is about a task, so that some rows are not.
--   * Its `old_status`/`new_status` hold **task** statuses — `CREATED`,
--     `ASSIGNED`, `COMPLETED`. A workflow state is a different vocabulary from
--     a different document, and putting both in one column pair is the
--     two-meanings-one-column failure this schema has refused before (deviation
--     #11 on enum casing, §6.3 on the numbering counter).
--
-- The two tables answer different questions and are read by different screens:
-- `workflow_task_history` is one task's own progress, this is the process's.
--
-- **It is also not the audit trail, and that is the distinction #181 exists to
-- draw.** `audit_events` answers *was this tampered with* — hash-chained, behind
-- `master-data:audit:read`, read by somebody investigating. This answers *how
-- did this document get here* — shown in the document workspace to the approver
-- deciding it, behind `workflow:instance:read`. Two records of one event with no
-- stated relationship is the problem #178 settled for status one layer over, so
-- it is stated here: **neither is derived from the other.** The engine writes
-- this row; `modules::audit` writes its own. An audit row a user-facing screen
-- depended on would become an audit row nobody can change.
--
-- **Append-only, and shaped so that it cannot be otherwise.** No `deleted_at`,
-- no `updated_at`, no `updated_by` — §1.2's base columns are deliberately not
-- all here, exactly as `workflow_task_history` and `document_status_history`
-- omit them. A history that can be soft-deleted is a history with a hole in it
-- and no way to see that there is one. #181 AC6 asks for no route that edits or
-- deletes; the absence of the columns is what makes that true by construction
-- rather than by nobody having written the route yet.
--
-- `from_state` is nullable and `to_state` is not: the first row of every
-- instance is the state it started in, which came from nowhere. That row is
-- what makes the list an account of the whole process rather than of everything
-- after the first decision.
--
-- `comment` is here and is written by nothing yet. FR-TASK-006 (#182) is what
-- fills it — the decision's own reason, captured with the decision — and the
-- column is created now so that #182 is a value being passed rather than a
-- migration. It is the same choice `0025` made for `workflow_tasks.comment`,
-- and this file names the issue so the emptiness reads as scheduled rather than
-- as an oversight.
--
-- **N−1 compatibility — schema half.** One new table, nothing altered, nothing
-- dropped. The previous release's binary neither reads nor writes it and starts
-- against this schema unchanged; a table it does not know about is a table it
-- cannot be broken by.

CREATE TABLE workflow_history (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    workflow_instance_id UUID   NOT NULL REFERENCES workflow_instances (id) ON DELETE CASCADE,
    document_id     UUID        NOT NULL REFERENCES documents (id),
    from_state      VARCHAR(64),                    -- null on the instance's first row
    to_state        VARCHAR(64) NOT NULL,
    action          VARCHAR(40),                    -- null when no action moved it
    task_id         UUID        REFERENCES workflow_tasks (id),  -- the task the decision came from
    comment         TEXT,                           -- FR-TASK-006 (#182) fills this
    actor_user_id   UUID        REFERENCES users (id),   -- null for engine actions
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT ck_workflow_history_moved CHECK (from_state IS DISTINCT FROM to_state)
);

-- The read this table exists for: one document's history, oldest first. The
-- document rather than the instance, because that is what the workspace has in
-- its hand — and a document that was returned and resubmitted may one day have
-- had more than one process.
CREATE INDEX idx_workflow_history_document_id
    ON workflow_history (document_id, created_at);
CREATE INDEX idx_workflow_history_instance_id
    ON workflow_history (workflow_instance_id, created_at);
