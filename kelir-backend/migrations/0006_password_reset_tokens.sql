-- 0006_password_reset_tokens.sql — self-service password reset tokens.
--
-- New table relative to the Database Schema document (§3.9, added in the same
-- change as this migration; see §14 deviation #14). Sprint 4 builds the reset
-- *token* flow only; delivery is stubbed to the log until the notification
-- module lands in Phase 6, so this migration has no dependency on it.
--
-- Base columns and naming per §1. Only a digest of the token is stored, never
-- the token itself — the same design as refresh_tokens.token_hash in
-- 0002_identity.sql. created_by / updated_by stay bare UUID with no FK,
-- matching every other identity-module table (only system_settings, created
-- before users existed, got that constraint added by ALTER).

CREATE TABLE password_reset_tokens (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID,
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    user_id         UUID        NOT NULL REFERENCES users (id),
    token_hash      VARCHAR(255) NOT NULL,          -- SHA-256 of the opaque reset token
    expires_at      TIMESTAMPTZ NOT NULL,
    consumed_at     TIMESTAMPTZ                     -- set once redeemed; NULL means still usable
);
CREATE UNIQUE INDEX uq_password_reset_tokens_token_hash ON password_reset_tokens (token_hash);
CREATE INDEX idx_password_reset_tokens_user_id_expires_at ON password_reset_tokens (user_id, expires_at);
