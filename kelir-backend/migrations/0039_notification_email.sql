-- 0039_notification_email.sql — the rows that turn the second channel on
-- (FR-NTF-004; #257).
--
-- `0034_notification.sql` created `notification_channels`, `notification_templates`
-- and `notification_logs` and wrote none of them, saying in each table's own
-- comment that #257 would. This is #257, and it is **three seeds and no DDL**:
-- every column this item needs has been there since Sprint 13 item 2.
--
-- # Which channels a notification uses is data (#257 AC1)
--
-- `notification_channels` says which channels a tenant has turned on, and
-- `notification_templates` says which notification types have something to say
-- on each. **The sender branches on neither.** It reads the enabled channels,
-- looks for a template per type and channel, and delivers what it finds — so a
-- tenant that wants no email deletes a row rather than waiting for a release,
-- and a type nobody has written an email for simply does not send one.
--
-- The alternative — `match notification_type { TaskAssigned => send_email(..) }`
-- — is the branch AC1 exists to forbid, and it is the shape that makes every
-- new notification type a code change in two places.
--
-- # The templates, and what a template may say
--
-- `{{title}}` and `{{body}}`, which is **everything the notification row
-- carries**. A template that references anything else fails to render and the
-- notification goes out plain (#257 AC5) — so the bound is not a style rule, it
-- is the set of values the sender can actually resolve. Widening it means
-- widening what `notify` is given, and that is a change to the caller rather
-- than to a string here.
--
-- **Seeded for the system tenant only**, which is `0002_identity.sql`'s shape
-- for the permission catalogue and `0037`'s for attachment categories: another
-- tenant's templates are that tenant's to write, and a migration that fanned out
-- across `tenants` would be deciding the wording for deployments it cannot see.
--
-- # N−1 compatibility
--
-- Three inserts into tables the previous release reads from nothing and writes
-- from nothing. `v0.6.0`'s binary neither renders a template nor knows a channel
-- row exists; it keeps writing in-app notifications exactly as it did, and the
-- worker that reads these rows is in this release only.

INSERT INTO notification_channels
    (id, tenant_id, channel_code, channel_type, provider, config_json, is_enabled) VALUES
    ('00000000-0000-0000-0004-000000000001', '00000000-0000-0000-0000-000000000001',
     'EMAIL', 'EMAIL', 'smtp', '{}', true);

COMMENT ON TABLE notification_channels IS
    'Which channels a tenant has turned on (FR-NTF-004, #257). The sender reads this rather than branching on notification type: a deployment turns email off by disabling the row. In-app needs no row — the centre reads `notifications` directly, and an in-app notification is not delivered anywhere.';

INSERT INTO notification_templates
    (id, tenant_id, template_code, notification_type, channel, locale,
     subject_template, body_template, is_enabled) VALUES
    ('00000000-0000-0000-0005-000000000001', '00000000-0000-0000-0000-000000000001',
     'TASK_ASSIGNED_EMAIL', 'TASK_ASSIGNED', 'EMAIL', 'en',
     '{{title}}',
     E'{{body}}\n\nOpen Kelir to see what is waiting for you.',
     true),
    ('00000000-0000-0000-0005-000000000002', '00000000-0000-0000-0000-000000000001',
     'DOCUMENT_DECIDED_EMAIL', 'DOCUMENT_DECIDED', 'EMAIL', 'en',
     '{{title}}',
     E'{{body}}\n\nOpen Kelir to see the document.',
     true);

COMMENT ON TABLE notification_templates IS
    'The subject and body an outbound channel sends per notification type (FR-NTF-004, #257 AC4). Placeholders are {{title}} and {{body}}, which is everything the notification row carries; a template naming anything else fails to render and the notification is sent plain rather than not at all (#257 AC5). In-app has no template: #251 composes its two sentences in the service, because one channel with two message shapes does not need a template engine.';

COMMENT ON TABLE notification_logs IS
    'One row per delivery attempt (#257 AC2). Written by the notification worker: SENT with the provider''s message id where there is one, FAILED with the error. The notification itself is never lost by a failure here — the in-app record is the storage and email is an additional delivery.';
