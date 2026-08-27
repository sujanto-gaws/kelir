-- 0022_audit_hash_tells_absent_from_empty.sql — the column comment on
-- audit_events.current_hash describes an encoding that has changed (#203).
--
-- `0019_audit_hash_covers_the_payload.sql` set that comment to say every field
-- is "length-prefixed", and rested a property on the prefix that the prefix did
-- not carry: an absent field was hashed as zero bytes, and so was a
-- present-but-empty one. `ip_address` NULL and `ip_address` '' produced the
-- same digest, and so did `reason` NULL and `reason` ''. Either column could
-- therefore be rewritten either way and the chain would still verify — in a
-- chain whose whole purpose (FR-AUD-003) is to make a rewrite detectable.
--
-- As of 2026-08-27 `modules::audit::chain_hash` hashes an absent field as a
-- length prefix of `u64::MAX` and no bytes. No present field can produce that
-- prefix — a field of that length is sixteen exabytes — so absent and
-- present-but-empty cannot collide, and the guarantee belongs to the sentinel
-- rather than to the length prefix.
--
-- **The second format change, and the second one that was free.** Nothing has
-- ever verified a chain (FR-AUD-003 is Phase 6), so no stored `current_hash`
-- has ever been relied on and no re-chaining is owed. After verification ships,
-- the same one-line change costs a re-chaining migration plus a decision about
-- what a row predating the format means to a verifier — which is the reason
-- both changes were taken now rather than when they would be noticed.
--
-- **Why a migration for a comment.** The same reason `0019` was one: migrations
-- are forward-only and never edited once applied (release process §6), so
-- correcting the text where it was written would fail every existing deployment
-- on a checksum. `COMMENT ON` puts the corrected text where `\d+` shows it.
--
-- N−1 compatibility: comments only. No column, constraint, index or row is
-- touched, and the previous release cannot tell the difference.

COMMENT ON COLUMN audit_events.current_hash IS
    'sha256: over every column the writer fills -- previous_hash, id, tenant_id, '
    'created_at, event_type, action, object_type, object_id, actor_user_id, '
    'ip_address, reason, old_value_json and new_value_json. A present field is '
    'hashed as eight bytes of big-endian length followed by its bytes; an absent '
    'one as a length of 2^64-1 and no bytes, which no present field can produce, '
    'so NULL and '''' are different hashes in every optional column. The columns '
    'reserved for later phases (document_id, workflow_instance_id, entity_type, '
    'entity_id, actor_role_id, digital_signature_ref) are NULL on every row '
    'written so far and are outside the hash; they join it when something starts '
    'filling them, which is a format change. The payload columns are hashed in '
    'jsonb''s own text form so a row read back recomputes to the value stored '
    'with it. Format defined by modules::audit::chain_hash; changed twice, '
    '2026-08-26 (#145) and 2026-08-27 (#203).';
