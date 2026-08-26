-- 0019_audit_hash_covers_the_payload.sql — the column comment on
-- audit_events.current_hash was wrong, and this is the only way to correct it
-- (#145).
--
-- `0003_audit.sql` says, on the column itself:
--
--     current_hash    TEXT        NOT NULL,           -- hash over row content + previous_hash
--
-- and that was not true. `chain_hash` covered ten inputs and neither payload
-- column was among them, nor `created_at`. So `old_value_json`, `new_value_json`
-- and `created_at` could each be rewritten without disturbing this row's
-- `current_hash` or any hash after it — the chain still verified. The doc
-- comment on `record` and Database Schema §10.2 carried the same claim; all
-- three described a hash over the row's *content* and the implementation was a
-- hash over its *metadata*.
--
-- The code half of that is fixed in `modules::audit`: the two payload columns
-- and `created_at` are in the hash as of 2026-08-26, while changing the format
-- was still free — nothing verifies a chain yet (FR-AUD-003 is Phase 6), so no
-- stored `previous_hash`/`current_hash` had ever been relied on. Once
-- verification ships the same change needs a re-chaining migration plus a
-- decision about what a row predating the format means to a verifier.
--
-- **Why a migration for a comment.** Migrations are forward-only and are never
-- edited once applied (release process §6): SQLx verifies checksums, so
-- correcting the text in `0003_audit.sql` would fail every existing deployment
-- at startup with a checksum error. `COMMENT ON` is how a comment on an applied
-- schema is changed, and it puts the corrected text where `\d+` and every schema
-- browser will show it — which is where the reader who was misled was looking.
--
-- N−1 compatibility: comments only. No column, constraint, index or row is
-- touched, and the previous release cannot tell the difference.

COMMENT ON COLUMN audit_events.current_hash IS
    'sha256: over every column the writer fills -- previous_hash, id, tenant_id, '
    'created_at, event_type, action, object_type, object_id, actor_user_id, '
    'ip_address, reason, old_value_json and new_value_json -- each length-prefixed. '
    'The columns reserved for later phases (document_id, workflow_instance_id, '
    'entity_type, entity_id, actor_role_id, digital_signature_ref) are NULL on '
    'every row written so far and are outside the hash; they join it when '
    'something starts filling them, which is a format change. The payload '
    'columns are hashed in jsonb''s own text form so a row read back recomputes '
    'to the value stored with it. Format defined by modules::audit::chain_hash; '
    'changed once, 2026-08-26 (#145).';

COMMENT ON COLUMN audit_events.previous_hash IS
    'current_hash of the previous row in this tenant''s chain, or the all-zero '
    'genesis hash for the first. Altering or removing any row breaks every hash '
    'after it, which is what makes tampering detectable (FR-AUD-003).';
