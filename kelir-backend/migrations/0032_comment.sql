-- 0032_comment.sql — the conversation about a document (FR-CMT-001; #249).
--
-- **This is not the decision comment, and the distinction is the first thing
-- this file says because the next reader will assume otherwise.** FR-TASK-006
-- shipped in Sprint 11 (#182) as three columns written in one transaction —
-- `workflow_tasks.comment`, `approval_decisions.comment` and
-- `workflow_history.comment`. That is a **record**: the reason an approver gave,
-- captured with the decision and immutable because the decision is. This table
-- is a **conversation**: something a person says about a document, which a later
-- sprint lets them reply to, edit and resolve.
--
-- Neither is derived from the other and neither can stand in for the other. An
-- approver's reason belongs to the decision and disappears from the record if it
-- is a comment somebody can edit; a colleague's question about a supplier
-- belongs to nobody's decision and would have to invent one to be stored as a
-- decision comment.
--
-- **The number.** `0032` and not `0029`, which the schema's mapping table
-- reserved for this file until 2026-08-31 — `0029` was taken by
-- `0029_workflow_routing.sql`. See `0031_attachment.sql`'s header.
--
-- **It follows the attachment migration and cannot precede it.**
-- `comments.attachment_id` references `attachments`, which `0031` creates. That
-- ordering is why the Sprint 12 construction plan runs item 6 second rather than
-- last: `activity_events` (§10.1) then references both, so the three migrations
-- have exactly one legal order.
--
-- # Three tables, and two of them are created empty
--
-- **`comment_mentions` and `comment_attachments` are created and written by
-- nothing.** Mentions are FR-CMT-005/006 and attachments on a comment are
-- FR-CMT-003, both **Sprint 13**. They are created here rather than there
-- because they are §9's shape and a migration per table for one area is a
-- migration for a `CREATE`. #249 AC1 asks that a table left unwritten say which
-- sprint fills it, which is what this paragraph is.
--
-- # Columns this item creates and does not write
--
-- **`parent_comment_id`** — threading, FR-CMT-002, Sprint 13. A column that
-- exists is not a feature that exists, and the reply that would fill it has no
-- API in this release.
--
-- **`status`, `resolved_by`, `resolved_at`** — resolving a comment thread,
-- FR-CMT-004, Sprint 13. Every row this item writes is `OPEN` and nothing moves
-- it.
--
-- **`comment_type` and `visibility`** take their defaults. The create surface
-- accepts a body and nothing else: a vocabulary the API lets a caller choose
-- from and no reader distinguishes is a vocabulary that will be wrong by the
-- time something reads it.
--
-- **`document_version_id` and `task_id`** are null on every row this item
-- writes. A comment against a specific version needs FR-DOC-009's version
-- surface to be something a person can point at, and a comment against a task is
-- the task's own surface asking for one.
--
-- # Two permissions, and no update or delete
--
-- `comment:create` and `comment:read`, checked **with** the document's own read:
-- a comment is as private as the document it is about, which is the rule
-- `0031_attachment.sql` states for attachments and the same rule here. There is
-- no `comment:update` and no `comment:delete` because editing and deleting are
-- FR-CMT-003 and Sprint 13, and a permission row nothing checks is the
-- `delegations` situation **D-13** spent two decisions undoing.
--
-- # N−1 compatibility
--
-- Three new tables and nothing altered. The previous release's binary does not
-- name them in any statement and starts against this schema unchanged.

CREATE TABLE comments (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID        REFERENCES users (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID        REFERENCES users (id),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    document_id     UUID        NOT NULL REFERENCES documents (id) ON DELETE CASCADE,
    document_version_id UUID    REFERENCES document_versions (id),
    attachment_id   UUID        REFERENCES attachments (id),
    task_id         UUID        REFERENCES workflow_tasks (id),
    parent_comment_id UUID      REFERENCES comments (id),
    comment_type    TEXT        NOT NULL DEFAULT 'COMMENT'
                    CHECK (comment_type IN ('COMMENT', 'NOTE', 'QUESTION', 'ISSUE', 'CLARIFICATION',
                                            'APPROVAL_NOTE', 'REJECTION_NOTE', 'INTERNAL_NOTE')),
    body            TEXT        NOT NULL,
    visibility      TEXT        NOT NULL DEFAULT 'INTERNAL'
                    CHECK (visibility IN ('INTERNAL', 'EXTERNAL', 'RESTRICTED')),
    status          VARCHAR(40) NOT NULL DEFAULT 'OPEN'
                    CHECK (status IN ('OPEN', 'RESOLVED', 'CLOSED')),
    resolved_by     UUID        REFERENCES users (id),
    resolved_at     TIMESTAMPTZ
);

CREATE INDEX idx_comments_document_id
    ON comments (document_id, created_at) WHERE deleted_at IS NULL;
CREATE INDEX idx_comments_parent_comment_id ON comments (parent_comment_id);

COMMENT ON TABLE comments IS
    'A conversation about a document (FR-CMT-001). Not the decision comment: that is FR-TASK-006, written into workflow_tasks.comment, approval_decisions.comment and workflow_history.comment in one transaction, and immutable because the decision is.';

COMMENT ON COLUMN comments.parent_comment_id IS
    'Threading (FR-CMT-002), Sprint 13. Written by nothing in this release.';

COMMENT ON COLUMN comments.status IS
    'Resolving a thread (FR-CMT-004), Sprint 13. Every row this release writes is OPEN and nothing moves it.';

CREATE TABLE comment_mentions (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID        REFERENCES users (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID        REFERENCES users (id),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    comment_id      UUID        NOT NULL REFERENCES comments (id) ON DELETE CASCADE,
    mentioned_user_id UUID      NOT NULL REFERENCES users (id),
    notified_at     TIMESTAMPTZ,
    read_at         TIMESTAMPTZ
);

CREATE UNIQUE INDEX uq_comment_mentions_comment_id_mentioned_user_id
    ON comment_mentions (comment_id, mentioned_user_id);
CREATE INDEX idx_comment_mentions_mentioned_user_id
    ON comment_mentions (mentioned_user_id, read_at);

COMMENT ON TABLE comment_mentions IS
    'Created by 0032 and written by nothing. Mentions are FR-CMT-005 and FR-CMT-006, Sprint 13.';

CREATE TABLE comment_attachments (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID        REFERENCES users (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID        REFERENCES users (id),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    comment_id      UUID        NOT NULL REFERENCES comments (id) ON DELETE CASCADE,
    attachment_id   UUID        NOT NULL REFERENCES attachments (id)
);

CREATE UNIQUE INDEX uq_comment_attachments_comment_id_attachment_id
    ON comment_attachments (comment_id, attachment_id);

COMMENT ON TABLE comment_attachments IS
    'Created by 0032 and written by nothing. A file on a comment is FR-CMT-003, Sprint 13.';

INSERT INTO permissions (id, tenant_id, permission_code, module, description) VALUES
    ('00000000-0000-0000-0001-000000000053', '00000000-0000-0000-0000-000000000001',
     'comment:create', 'comment', 'Comment on a document'),
    ('00000000-0000-0000-0001-000000000054', '00000000-0000-0000-0000-000000000001',
     'comment:read',   'comment', 'Read a document''s comments');

-- ROLE-ADMIN holds every permission in the catalogue (0002_identity.sql); grant
-- only the two new rows rather than re-inserting the ones already granted.
INSERT INTO role_permissions (id, tenant_id, role_id, permission_id)
SELECT
    gen_random_uuid(),
    '00000000-0000-0000-0000-000000000001',
    '00000000-0000-0000-0002-000000000001',
    id
FROM permissions
WHERE permission_code IN ('comment:create', 'comment:read');
