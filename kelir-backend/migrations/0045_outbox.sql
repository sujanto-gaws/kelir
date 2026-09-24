-- 0045_outbox.sql — the transactional outbox, and the breaker's index on the
-- execution log (architectures/01 §12.5, §20; #519; ADR-0041).
--
-- # One table of §12's nine, ahead of the integration migration
--
-- Database Schema §12 declares nine integration tables and this creates one.
-- **Its first reader is the hook chain's after half, not an integration**, so
-- it lands with that reader: `webhook_subscriptions`, `webhook_events`,
-- `external_systems` and `inbox_events` wait for the integration migration,
-- which is where something reads them (construction plan 16 §8). A table
-- created here with nothing to fill it would be the shape `0034` justified for
-- one release and this one does not need.
--
-- # A row is the event and the state of its dispatch
--
-- `payload_json` is the Event Envelope Schema envelope, whole, and it is what a
-- webhook will one day receive byte for byte. `status` is how far the worker got
-- with delivering it in process. **`DEAD_LETTER` marks the dispatch exhausted
-- and keeps the envelope** — EES §5.1's *dead-letter the delivery, never the
-- event*, read the way ADR-0041 §2 records.
--
-- `next_attempt_at` means two things by status, and both are *not before*:
--
-- | Status | `next_attempt_at` |
-- |---|---|
-- | `PENDING` | null — due now |
-- | `PROCESSING` | the lease: a worker claimed it and has five minutes, after which the row is claimed again |
-- | `FAILED` | when the retry is due (30 s × 2^(attempt − 1)) |
--
-- # `FAILED` joins the pending index
--
-- §12.8 wrote the predicate as `PENDING` and `PROCESSING`. A `FAILED` row is
-- waiting for its retry exactly as a `PENDING` row is waiting for its first
-- attempt, and an index the worker's claim cannot use for one of the three
-- statuses it claims is a scan of every processed row the table has ever held.
-- The index is keyed on `created_at`, so the claim still filters `FAILED` and
-- lease-expired `PROCESSING` rows on `next_attempt_at <= now()` itself.
--
-- # The breaker reads the execution log, and needs to read it by handler
--
-- ADR-0041's circuit breaker is derived from `document_hook_executions` rather
-- than kept in a table of its own: a handler is open when its last five
-- executions for (tenant, hook, handler) are all `ERROR`. `0015` indexed that
-- log by document, which is the question *what ran on this document* and not
-- *how has this handler been doing*. Without this index every after-hook
-- delivery scans the tenant's whole hook history once per handler it runs.
--
-- # N−1 compatibility
--
-- One new table and one new index on an existing table. Nothing altered or
-- dropped, and no permission: the table has no route. The `v0.8.0` image names
-- neither in any statement and starts against this schema unchanged (release
-- process §6).

CREATE TABLE outbox_events (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    aggregate_type  TEXT        NOT NULL,           -- EES aggregateType enum: DOCUMENT | TASK | WORKFLOW_INSTANCE
                                                    -- | ATTACHMENT | COMMENT | PARTY | FACILITY | PRODUCT
                                                    -- | SERVICE | USER | PLUGIN | SYSTEM
    aggregate_id    UUID        NOT NULL,
    event_type      VARCHAR(64) NOT NULL,           -- 'Workflow.Transitioned'
    payload_json    JSONB       NOT NULL,
    correlation_id  TEXT,
    status          VARCHAR(40) NOT NULL DEFAULT 'PENDING'
                    CHECK (status IN ('PENDING', 'PROCESSING', 'PROCESSED', 'FAILED', 'DEAD_LETTER')),
    attempt_count   INTEGER     NOT NULL DEFAULT 0,
    next_attempt_at TIMESTAMPTZ,
    processed_at    TIMESTAMPTZ,
    last_error      TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_outbox_events_pending
    ON outbox_events (created_at) WHERE status IN ('PENDING', 'PROCESSING', 'FAILED');
CREATE INDEX idx_outbox_events_aggregate ON outbox_events (aggregate_type, aggregate_id);

COMMENT ON TABLE outbox_events IS
    'One row per event, written in the transaction of the business write it records (ADR-0041). payload_json is the EES envelope; status is the state of its in-process dispatch. DEAD_LETTER keeps the envelope: the dispatch is exhausted, the event is not discarded.';
COMMENT ON COLUMN outbox_events.next_attempt_at IS
    'Not before. Null while PENDING; the five-minute lease while PROCESSING; the retry time while FAILED.';

-- The circuit breaker's lookup: one handler's most recent executions.
CREATE INDEX idx_document_hook_executions_breaker
    ON document_hook_executions (tenant_id, hook_name, handler_reference, executed_at DESC);

COMMENT ON INDEX idx_document_hook_executions_breaker IS
    'The after-hook circuit breaker reads a handler''s last five executions for (tenant, hook, handler) before running it (ADR-0041). The document index answers what ran on one document, which is not this question.';
