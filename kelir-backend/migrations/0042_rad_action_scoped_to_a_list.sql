-- A configured action can belong to one list (#348).
--
-- `rad_actions` has carried a `context` and no `list_id` since `0014_rad.sql`,
-- so a `LIST` action belongs to the tenant and is offered on **every** list in
-- it. #340 gave the table its first reader and met this on the way in:
-- `rad::domain::action`'s module doc states the limitation where a reader will
-- meet it, and this column is what closes it.
--
-- **The shape §5.7 and §5.8 already use.** `rad_list_columns` and
-- `rad_list_filters` both carry `list_id`, precisely because a column and a
-- filter belong to a list. An action did not, and that was the whole defect.
--
-- **Nullable, and the null means something.** An action with no `list_id` is
-- offered on every list of its context — which is exactly today's behaviour, so
-- this migration changes nothing until somebody sets the column. That is what
-- makes it N−1 compatible (release process §6): the `v0.7.0` image reads the
-- column, and the release before it never selected it, so both run against this
-- schema. It also avoids a data migration that would have to guess which list
-- an existing tenant-wide action was meant for — and the honest answer is that
-- nobody knows, because until now the question could not be asked.
--
-- **`ON DELETE` is deliberately absent**, which means `NO ACTION`: deleting a
-- list that an action is scoped to is refused rather than cascading. A cascade
-- would delete configuration nobody asked to delete, and `SET NULL` is worse —
-- it would silently widen the action back to every list in the tenant, which is
-- the state this column exists to end.
--
-- **Only `LIST` is closed here.** A `DETAIL`, `DOCUMENT` or `TASK` action has
-- the same problem one table over — nothing scopes it to a document type or a
-- task definition either — and #348 says plainly that doing lists alone is the
-- narrower and more honest first step. The `CHECK` below is what stops the
-- column being quietly reused for the others without that decision being taken.

-- **The reference target the key below needs, and it bounds no value.**
-- `rad_lists.id` is already unique; this constraint exists only so the foreign
-- key can carry `tenant_id` with it -- exactly as `uq_rad_forms_id_tenant_id`
-- does under `0021` and `uq_roles_id_tenant_id` under `0017`, and for the
-- reason the Database Schema's deviation #20 gives: a row filed under the wrong
-- tenant should be **unwritable** rather than merely unwritten.
ALTER TABLE rad_lists
    ADD CONSTRAINT uq_rad_lists_id_tenant_id UNIQUE (id, tenant_id);

-- **A composite key rather than `REFERENCES rad_lists (id)`.** The plain form
-- would let one tenant's action name another tenant's list. Nothing could read
-- it back -- the catalogue filters `rad_actions.tenant_id`, so the row would
-- simply never match a list the caller could pass -- but "unreadable" is a
-- weaker guarantee than "unwritable", and this project has settled on the
-- second three times already.
--
-- **`MATCH SIMPLE` is what makes the nullable column work**, and it is the
-- default rather than a choice being smuggled in: a key with any column null is
-- not enforced, so a tenant-wide action (`list_id IS NULL`) references nothing
-- and is unaffected by the constraint.
ALTER TABLE rad_actions
    ADD COLUMN list_id UUID,
    ADD CONSTRAINT fk_rad_actions_list_id_tenant_id
        FOREIGN KEY (list_id, tenant_id) REFERENCES rad_lists (id, tenant_id);

COMMENT ON COLUMN rad_actions.list_id IS
    'The list this action belongs to; NULL means every list of its context. '
    'Only a LIST action may carry one — see ck_rad_actions_list_id_is_a_list_action.';

-- A `list_id` on a `DETAIL` action would be read by nothing and would look, to
-- anybody configuring it, like a scoping that had been applied. Refusing it is
-- cheaper than the surprise, and it is the constraint that has to be widened
-- when the other three contexts get their own scoping.
ALTER TABLE rad_actions
    ADD CONSTRAINT ck_rad_actions_list_id_is_a_list_action
    CHECK (list_id IS NULL OR context = 'LIST');

-- The reader's predicate is `list_id IS NULL OR list_id = $3`, so the rows it
-- scans for one list are the tenant-wide ones plus that list's own.
CREATE INDEX idx_rad_actions_list_id ON rad_actions (list_id) WHERE deleted_at IS NULL;
