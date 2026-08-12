-- 0004_string_lengths.sql — give bounded string columns explicit lengths.
--
-- Migrations are immutable once merged (coding standard §2.5), so this alters
-- what 0001–0003 created rather than editing them.
--
-- Every length comes from the standard set in Database Schema §1.3.1. In
-- PostgreSQL VARCHAR(n) and TEXT are the same type — same storage, same
-- performance — so this adds a constraint, not a conversion. What it buys:
--
--   * Indexed columns stop failing unpredictably. A btree entry cannot exceed
--     ~8 KB and index entries are compressed but never TOASTed, so an oversized
--     value succeeded or failed depending on how compressible it was. Measured
--     on users.username before this migration: a 10,000-character repetitive
--     value inserted fine; a 9,600-character random one raised "index row
--     requires 9632 bytes, maximum size is 8191" — a 500 rather than a 422.
--   * MariaDB can index these columns. It cannot index TEXT without a prefix
--     length, and a prefix index does not enforce real uniqueness (§1.6).
--
-- Safe to run against existing data here because these tables are new and
-- effectively empty. On a populated table each ALTER takes a full rewrite under
-- an exclusive lock, which is precisely why this lands before the master-data
-- migration rather than after it.

-- Core ----------------------------------------------------------------------

ALTER TABLE tenants
    ALTER COLUMN tenant_code TYPE VARCHAR(64),
    ALTER COLUMN name        TYPE VARCHAR(200),
    ALTER COLUMN status      TYPE VARCHAR(40);

ALTER TABLE system_settings
    ALTER COLUMN setting_key TYPE VARCHAR(64);
-- description stays TEXT: free-form prose.

-- Identity ------------------------------------------------------------------

ALTER TABLE departments
    ALTER COLUMN department_code TYPE VARCHAR(64),
    ALTER COLUMN name            TYPE VARCHAR(200),
    ALTER COLUMN status          TYPE VARCHAR(40);

ALTER TABLE users
    ALTER COLUMN username      TYPE VARCHAR(64),
    ALTER COLUMN email         TYPE VARCHAR(320),   -- RFC 5321 maximum
    ALTER COLUMN password_hash TYPE VARCHAR(255),   -- Argon2id PHC string
    ALTER COLUMN display_name  TYPE VARCHAR(200),
    ALTER COLUMN status        TYPE VARCHAR(40);

ALTER TABLE refresh_tokens
    ALTER COLUMN token_hash     TYPE VARCHAR(255),
    ALTER COLUMN revoked_reason TYPE VARCHAR(200);

ALTER TABLE roles
    ALTER COLUMN role_code TYPE VARCHAR(64),
    ALTER COLUMN name      TYPE VARCHAR(200);
-- description stays TEXT.

ALTER TABLE permissions
    ALTER COLUMN permission_code TYPE VARCHAR(64),
    ALTER COLUMN module          TYPE VARCHAR(64),
    ALTER COLUMN source          TYPE VARCHAR(40);
-- description stays TEXT.

ALTER TABLE delegations
    ALTER COLUMN scope TYPE VARCHAR(40);
-- reason stays TEXT: an explanation, not a code.

-- Audit ---------------------------------------------------------------------

ALTER TABLE audit_events
    ALTER COLUMN entity_type           TYPE VARCHAR(64),
    ALTER COLUMN event_type            TYPE VARCHAR(64),
    ALTER COLUMN action                TYPE VARCHAR(40),
    ALTER COLUMN object_type           TYPE VARCHAR(40),
    ALTER COLUMN ip_address            TYPE VARCHAR(64),   -- IPv6 with a zone index fits
    ALTER COLUMN digital_signature_ref TYPE VARCHAR(255),
    ALTER COLUMN previous_hash         TYPE VARCHAR(255),
    ALTER COLUMN current_hash          TYPE VARCHAR(255);
-- reason stays TEXT.
