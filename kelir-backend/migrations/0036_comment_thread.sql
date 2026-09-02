-- 0036_comment_thread.sql — the columns the comment epic's tail fills, and the
-- two permissions it checks (FR-CMT-002, FR-CMT-003, FR-CMT-004; #253,
-- decisions D-50 and D-51, recorded as ADR-0029 and ADR-0030).
--
-- `0032_comment.sql` created `parent_comment_id` and said which sprint would
-- fill it. This is that sprint, and the file is small for the reason that one
-- is large: the shape was decided then, so what is left is one column, one
-- constraint and two permission rows.
--
-- # Two FR numbers `0032`'s header got wrong, corrected where they can be
--
-- That file calls resolving a thread FR-CMT-004 and a file on a comment
-- FR-CMT-003. **SRS §4.11 has FR-CMT-003 edit, FR-CMT-004 delete, FR-CMT-005
-- resolve, FR-CMT-006 mention** — so the two columns this file's permissions
-- serve are the ones `0032` filed under other numbers. **`0032` is not edited**:
-- a merged migration's bytes are its checksum in every deployment that has
-- applied it, and rewriting one turns a comment into a failed startup. The
-- correction is in [Database Schema §9](../../docs/design/02.%20Database%20Schema.md),
-- which governs the table, and in this paragraph.
--
-- # `edited_at`, next to an `updated_at` that already exists
--
-- Every table in this schema carries `updated_at`, and it moves for **any**
-- write to the row — including the soft delete below, which is not an edit and
-- must not read as one. **#253 AC3 asks that an edit be visible as an edit**,
-- which is a fact about the *body* and not about the row: a comment whose text
-- changed with nothing saying so is a conversation somebody can rewrite after
-- the fact.
--
-- So the two are separate columns and mean different things. `updated_at`
-- answers *when was this row last written*; `edited_at` answers *when did what
-- this comment says last change*, and it is null on a comment nobody has
-- edited — which is the state the screen renders as silence rather than as a
-- timestamp equal to `created_at`.
--
-- # One level of threading, and the constraint is not what holds it
--
-- **D-50 (ADR-0029): a reply is to a root comment, and a reply to a reply is
-- refused.** The argument is in that record and in `modules::comment`; what
-- belongs here is which half of it the database holds. `ck_comments_not_its_own_parent` is
-- `0026_form_section_not_its_own_parent.sql`'s constraint on this table's
-- self-reference: it refuses the one hop a row-level `CHECK` can see, and it is
-- free for ever after.
--
-- **It does not enforce the depth.** *This row's parent has no parent* is a
-- property of another row, which no `CHECK` can read — the same reason
-- `0026` gives for a ring of three. The depth is enforced in
-- `comment::service::add_comment`, in the transaction that writes the reply,
-- because that is where the parent is loaded anyway; the constraint is the half
-- that needs no caller.
--
-- # Two permissions, and no third one for a moderator
--
-- `comment:update` and `comment:delete`, checked **with** authorship: the
-- permission says whether an account edits or deletes comments at all, and
-- `created_by` says whose. Both questions have to be yes.
--
-- **There is deliberately no moderator permission** — no code that lets one
-- account delete another's comment. #253 AC2 asks that editing be permitted to
-- the author, and a permission granting more than that would be a permission
-- nothing in this release checks, which is the `delegations` situation **D-13**
-- spent two decisions undoing. When a deployment needs it, it arrives with the
-- surface that uses it.
--
-- # N−1 compatibility
--
-- One nullable column, one constraint over a column the previous release never
-- writes, and two permission rows. Nothing is altered, dropped or renamed, and
-- every statement the previous release holds still type-checks: it names
-- `parent_comment_id` in no statement — `0032`'s own header says so — and so
-- cannot write a row the constraint would reject. Adding a permission is the
-- safe direction; the one that bites is dropping a code the previous release
-- still checks.

ALTER TABLE comments
    ADD COLUMN edited_at TIMESTAMPTZ;

ALTER TABLE comments
    ADD CONSTRAINT ck_comments_not_its_own_parent
    CHECK (parent_comment_id IS DISTINCT FROM id);

COMMENT ON COLUMN comments.edited_at IS
    'When the body last changed (FR-CMT-003). Null on a comment nobody has edited. Distinct from updated_at, which moves for any write to the row including the soft delete.';

COMMENT ON COLUMN comments.parent_comment_id IS
    'The root comment this one replies to (FR-CMT-002, #253). One level: a reply names a comment whose own parent_comment_id is null, refused in comment::service::add_comment because no row-level CHECK can read another row (D-50).';

COMMENT ON COLUMN comments.deleted_at IS
    'When the author deleted this comment (FR-CMT-004). The row and its body stay; the read boundary withholds the body and serves the comment as a tombstone while it still has undeleted replies, so deleting a root does not take a conversation with it (D-51).';

INSERT INTO permissions (id, tenant_id, permission_code, module, description) VALUES
    ('00000000-0000-0000-0001-000000000058', '00000000-0000-0000-0000-000000000001',
     'comment:update', 'comment',
     'Edit a comment (FR-CMT-003). Held with authorship, never instead of it: this grants editing one''s own comments, and no permission in this release lets an account edit somebody else''s.'),
    ('00000000-0000-0000-0001-000000000059', '00000000-0000-0000-0000-000000000001',
     'comment:delete', 'comment',
     'Delete a comment (FR-CMT-004). Held with authorship, as comment:update is. The delete is soft, and a deleted comment that has replies stays in the conversation as a tombstone so the thread''s shape survives it.');

-- ROLE-ADMIN holds every permission in the catalogue (0002_identity.sql); grant
-- only the two new rows rather than re-inserting the ones already granted.
INSERT INTO role_permissions (id, tenant_id, role_id, permission_id)
SELECT
    gen_random_uuid(),
    '00000000-0000-0000-0000-000000000001',
    '00000000-0000-0000-0002-000000000001',
    id
FROM permissions
WHERE permission_code IN ('comment:update', 'comment:delete');
