-- 0028_delegation.sql — who decided, and on whose behalf (FR-IDM-006,
-- FR-WF-009, FR-TASK-008; #184).
--
-- **One column, and it is the whole schema half of this item.** `delegations`
-- has existed since `0002_identity.sql` with a window check and a not-self
-- check the database already enforces, and `workflow_tasks.delegated_from_user_id`
-- has existed since `0025_workflow.sql`. **D-13** unscheduled #24 because that
-- first table had a writer and no reader; **D-17** scheduled its consumer here
-- so the window would land beside the actions that honour it. What was missing
-- was never storage for the *window* — it was somewhere to record that a
-- decision was taken by one person exercising another's authority.
--
-- **Why the workflow history and not `approval_decisions`.** §7.8 is the formal
-- record of *what was decided about this document*, and its `approver_user_id`
-- is the person who decided: that is the signature, and a delegated approval is
-- signed by the delegate rather than jointly. §7.11 answers *how did this
-- document get here*, which is the question accountability is asked in — #184
-- AC4's own sentence is that "a history that shows only the delegate is a
-- history that loses the accountability delegation was supposed to preserve".
-- So the pair of names goes on the row a person reads, and the formal record
-- keeps naming one approver.
--
-- **Nullable, and null on almost every row.** A transition that nobody was
-- standing in for has no second party, and writing the actor into both columns
-- to avoid a null would make "acting for themselves" indistinguishable from
-- "acting for somebody who happens to be them".
--
-- **`workflow_tasks.status` gains no new value and loses none.** `DELEGATED` is
-- in §7.6's `CHECK` and this item still does not write it: a delegated task is
-- an open task that somebody else now holds, so its status stays `ASSIGNED` and
-- `delegated_from_user_id` carries the hand-off. Writing `DELEGATED` would take
-- the row out of `uq_workflow_tasks_open_per_instance` and out of the inbox's
-- open filter, leaving a running process with no open task and the delegate
-- unable to see the work they had just been given — a status answering *who
-- holds this* rather than *where has this got to* is the two-meanings-one-column
-- failure this schema has refused before. It is the same standing as `STARTED`
-- on `workflow_instances`: the value was specified, and the product does not
-- produce it.
--
-- **No new index.** The routing read is *given this delegator, is a window open
-- now* — `idx_delegations_tenant_id_delegator_user_id (tenant_id,
-- delegator_user_id, starts_at, ends_at)` from `0002` is exactly that read, and
-- it was created four sprints before anything performed it.
--
-- **N−1 compatibility — schema half.** One nullable column added, nothing
-- altered and nothing dropped. The previous release's binary names its columns
-- explicitly in both the insert and the select on this table, so it neither
-- writes nor reads this one and starts against this schema unchanged.

ALTER TABLE workflow_history
    ADD COLUMN on_behalf_of_user_id UUID REFERENCES users (id);

COMMENT ON COLUMN workflow_history.on_behalf_of_user_id IS
    'Whose authority the actor was exercising, when a delegation put this task in their hands (#184 AC4). Null where the actor was acting as themselves.';
