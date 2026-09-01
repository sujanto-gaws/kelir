-- 0034_notification.sql — telling somebody a thing is waiting for them
-- (FR-NTF-001, FR-NTF-002, FR-NTF-003; #251).
--
-- # One table is written and three are not, which is deliberate
--
-- Database Schema §4 maps this file to *notifications, templates, channels*, and
-- §11 declares four tables. **This migration creates all four and #251 fills
-- one.** `notification_templates`, `notification_channels` and
-- `notification_logs` belong to the email channel
-- ([#257](https://github.com/sujanto-gaws/kelir/issues/257)); they are created
-- here so §4 stays true and so the one migration this section gets is the one
-- the mapping table names.
--
-- That is `0032_comment.sql`'s shape — `comment_mentions` and
-- `comment_attachments` arrived created and unwritten, with a `COMMENT ON`
-- naming the sprint that fills them — and the `COMMENT ON` statements below do
-- the same. **An unwritten table is not the `delegations` situation D-13
-- undid**: that was a *permission* nothing checked, which reads as a working
-- control and is not one. A table nothing writes is empty, which is what it
-- looks like.
--
-- # Why in-app only, and why `channel` still has five values
--
-- FR-NTF-003 is the in-app centre. The `channel` CHECK carries the whole
-- vocabulary §11.3 declares because a row a later release writes must be
-- storable by this one — the N−1 rule pointing forwards. Nothing in this
-- release writes anything but `IN_APP`.
--
-- # The unread index is partial, and that is the read it exists for
--
-- The centre's question is *what is waiting for me*, so the index covers
-- `read_at IS NULL`. A notification that has been read is history and is paged
-- rather than counted.
--
-- # N−1 compatibility
--
-- Four new tables, nothing altered, nothing dropped, and one new permission row
-- with its ROLE-ADMIN grant. The previous release names none of them in any
-- statement and starts against this schema unchanged. The permission is
-- *added* here, which is the safe direction: the rule that bites is dropping
-- one the previous release still checks (D-47, #301).

CREATE TABLE notifications (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID        REFERENCES users (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID        REFERENCES users (id),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    recipient_user_id UUID      NOT NULL REFERENCES users (id),
    document_id     UUID        REFERENCES documents (id),
    workflow_instance_id UUID   REFERENCES workflow_instances (id),
    task_id         UUID        REFERENCES workflow_tasks (id),
    notification_type TEXT      NOT NULL,
    title           TEXT        NOT NULL,
    body            TEXT        NOT NULL,
    channel         TEXT        NOT NULL DEFAULT 'IN_APP'
                    CHECK (channel IN ('IN_APP', 'EMAIL', 'MOBILE_PUSH', 'SMS', 'MESSAGE_PLATFORM')),
    status          VARCHAR(40) NOT NULL DEFAULT 'PENDING'
                    CHECK (status IN ('PENDING', 'SENT', 'FAILED', 'READ')),
    sent_at         TIMESTAMPTZ,
    read_at         TIMESTAMPTZ
);

CREATE INDEX idx_notifications_recipient_unread
    ON notifications (recipient_user_id, created_at) WHERE read_at IS NULL AND deleted_at IS NULL;

-- The centre's own page: everything addressed to one person, newest first,
-- read or not. The partial index above answers *how many are waiting*; this
-- one answers *show me them*.
CREATE INDEX idx_notifications_recipient_id_created_at
    ON notifications (tenant_id, recipient_user_id, created_at DESC);

COMMENT ON TABLE notifications IS
    'One row per person told about one thing (FR-NTF-001). Written in the transaction of the action it announces (#251 AC3): a notification that outlives a rolled-back approval is a lie, and one lost when the approval commits is the silence this table exists to end. Scoped to its recipient in the statement, never by the handler (#251 AC7).';

COMMENT ON COLUMN notifications.recipient_user_id IS
    'Who is told. For a task this is the task''s own holder rather than the definition''s named assignee, so a delegation window routes the notification with the task (#251 AC4, #184). For a role task it is one row per current holder of the role — see D-48.';

COMMENT ON COLUMN notifications.read_at IS
    'Null while unread. Marking read is idempotent (#251 AC5): the UPDATE carries read_at IS NULL, so a second call changes nothing and reports the same result as the first.';

COMMENT ON COLUMN notifications.status IS
    'PENDING for a row nothing has delivered, which is every in-app row: the centre reads the table directly, so there is no send step for IN_APP and nothing sets SENT. READ is not written either — read_at is the one place readness lives, because two columns saying it invites them to disagree. Both values exist because §11.3 declares them and #257''s email channel needs them.';

CREATE TABLE notification_templates (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID        REFERENCES users (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID        REFERENCES users (id),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    template_code   VARCHAR(64) NOT NULL,
    notification_type TEXT      NOT NULL,
    channel         TEXT        NOT NULL
                    CHECK (channel IN ('IN_APP', 'EMAIL', 'MOBILE_PUSH', 'SMS', 'MESSAGE_PLATFORM')),
    locale          TEXT        NOT NULL DEFAULT 'en',
    subject_template TEXT,
    body_template   TEXT        NOT NULL,
    is_enabled      BOOLEAN     NOT NULL DEFAULT true
);

CREATE UNIQUE INDEX uq_notification_templates_code_channel_locale
    ON notification_templates (tenant_id, template_code, channel, locale) WHERE deleted_at IS NULL;

COMMENT ON TABLE notification_templates IS
    'Created by 0034 and written by nothing. The email channel fills it (#257, FR-NTF-004). #251 composes its in-app title and body in the service, because one channel with two message shapes does not need a template engine and a template nobody can edit is a worse place for a sentence than the code that writes it.';

CREATE TABLE notification_channels (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID        REFERENCES users (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID        REFERENCES users (id),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    channel_code    VARCHAR(64) NOT NULL,
    channel_type    TEXT        NOT NULL
                    CHECK (channel_type IN ('IN_APP', 'EMAIL', 'MOBILE_PUSH', 'SMS', 'MESSAGE_PLATFORM')),
    provider        TEXT,
    config_json     JSONB       NOT NULL DEFAULT '{}',
    is_enabled      BOOLEAN     NOT NULL DEFAULT true
);

CREATE UNIQUE INDEX uq_notification_channels_tenant_id_channel_code
    ON notification_channels (tenant_id, channel_code) WHERE deleted_at IS NULL;

COMMENT ON TABLE notification_channels IS
    'Created by 0034 and written by nothing. Per-tenant channel configuration for #257, and where a plugin registers a channel of its own (architectures/04). In-app needs no configuration: the centre reads the table.';

-- Append-only, per §1.2's exception: a delivery attempt happened or it did not,
-- so there is nothing to update and nowhere to soft-delete to.
CREATE TABLE notification_logs (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID        REFERENCES users (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    notification_id UUID        NOT NULL REFERENCES notifications (id) ON DELETE CASCADE,
    channel         TEXT        NOT NULL,
    attempt         INTEGER     NOT NULL DEFAULT 1,
    status          VARCHAR(40) NOT NULL CHECK (status IN ('SENT', 'FAILED')),
    provider_message_id TEXT,
    error_message   TEXT
);

CREATE INDEX idx_notification_logs_notification_id
    ON notification_logs (notification_id, created_at);

COMMENT ON TABLE notification_logs IS
    'Created by 0034 and written by nothing. One row per delivery attempt, for #257: an in-app notification is not delivered anywhere, so there is no attempt to log.';

INSERT INTO permissions (id, tenant_id, permission_code, module, description) VALUES
    ('00000000-0000-0000-0001-000000000056', '00000000-0000-0000-0000-000000000001',
     'notification:read', 'notification', 'Read and dismiss your own notifications');

-- ROLE-ADMIN holds every permission in the catalogue (0002_identity.sql); grant
-- only the new row rather than re-inserting the ones already granted.
INSERT INTO role_permissions (id, tenant_id, role_id, permission_id)
SELECT
    gen_random_uuid(),
    '00000000-0000-0000-0000-000000000001',
    '00000000-0000-0000-0002-000000000001',
    id
FROM permissions
WHERE permission_code = 'notification:read';
