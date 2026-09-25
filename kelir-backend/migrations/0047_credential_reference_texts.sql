-- 0047_credential_reference_texts.sql — two texts `0046_integration.sql`
-- seeded say more than the API checks, and this is the only way to correct
-- them (#552; verification record 19, finding 2).
--
-- `0046` says, on the column and on the catalogue row:
--
--     COMMENT ON COLUMN integration_credentials.secret_reference IS
--         'Where the secret lives (vault://..., env://...), never the secret. ...'
--
--     'integration:credential:read' ... 'View credential references — where a
--     system''s secrets live, never the secrets'
--
-- and *never the secret* is not something the API can make true. It checks a
-- reference's **shape** (`env://NAME`, `vault://path[#field]`; SDD §9.3.3).
-- Whether a string is a secret cannot be decided from the string, and a secret
-- typed as a path segment has the right shape: record 19's probe stored
-- `vault://sk_live_51HxQ2eKZ8r` and read it back. D-88's stopping rule strikes
-- such a claim rather than narrowing it, so the texts below say *reference* and
-- say what is checked. The other seven `integration:*` descriptions make no
-- such claim and are left as they are.
--
-- **Why a migration for a comment and a description.** Migrations are
-- forward-only and never edited once applied (release process §6): SQLx
-- verifies checksums, so correcting the text in `0046` would fail every
-- deployment that has applied it. `0046`'s header comment keeps the same
-- overstatement and cannot be corrected at all; Database Schema §12 says so
-- and is the operative text. `0019` (a column comment) and `0013` (a catalogue
-- description) are the precedents.
--
-- Takes 0047 because that is the next free number. The plugin migration, which
-- the Database Schema mapping table had reserved at 0047, shifts to 0048 there.
--
-- Scoped to the row rather than to a tenant: the catalogue is system-defined
-- seed data (D-6), and this description is wrong wherever the row exists.
--
-- N−1 compatibility: one column comment and one catalogue description. No
-- column, constraint, index or grant is touched, and no code reads either text,
-- so the previous release cannot tell the difference.

COMMENT ON COLUMN integration_credentials.secret_reference IS
    'A reference to where the secret is kept (vault://..., env://...). The API checks its shape only; '
    'a secret typed as a path segment has the right shape. Readable under integration:credential:read.';

UPDATE permissions
SET description = 'View credential references — where a system''s secrets are kept, as entered',
    updated_at = now()
WHERE permission_code = 'integration:credential:read';
