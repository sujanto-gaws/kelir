-- 0053_notification_delivery_lease.sql — a sender claims a notification
-- before it emails it (FR-NTF-004, NFR-SCA-001, NFR-SCA-003; #853).
--
-- **Two backends sent nearly every email twice.** `notification::worker` read
-- `PENDING` with no claim, so two senders polling one database read one batch
-- and both mailed it; `mark_delivered`'s `status = 'PENDING'` kept the row from
-- being written twice and did nothing for the mailbox. The sender now claims
-- its batch as `outbox::repository::claim` does (ADR-0041 §2): `FOR UPDATE SKIP
-- LOCKED` over the due rows, and a lease written in the same statement.
--
-- **The lease is a column, and `status` keeps its four values.** The outbox
-- holds its lease in `next_attempt_at` beside a `PROCESSING` status; this table
-- has no such status, and adding one to the `CHECK` is the change a binary of
-- the previous release cannot live with: it reads only `PENDING`, so a row a
-- newer sender left `PROCESSING` before a rollback would never be delivered. A
-- leased row stays `PENDING`, so a rolled-back binary reads it exactly as it
-- reads every other row (release process §6's N−1 rule). Nothing else changes
-- for it: the column is nullable with no default, and it names no column the
-- previous release writes.
--
-- **Null is "never claimed"**, which is every row written before this file and
-- every row a previous binary writes. A row is due when it is `PENDING` and its
-- lease is null or has passed; a passed lease is what a sender that crashed
-- mid-batch leaves behind, and the next claim takes it again.
--
-- **The index is the claim's**: its predicate is the claim's status and
-- liveness test, and its order is the claim's order, so a claim walks the
-- `PENDING` rows in order rather than every notification ever sent. It does not
-- leave out a leased row: a row under a live lease is still `PENDING` and stays
-- in the index, and the claim's own lease test skips it, as
-- `idx_outbox_events_pending` leaves `next_attempt_at` to its claim.
--
-- **The index is a plain `CREATE INDEX`, and that was weighed.** It holds a
-- SHARE lock on `notifications` for one scan, so writes to the table wait while
-- it builds; that is accepted at this table's size. `CONCURRENTLY` would avoid
-- the wait, but it cannot run inside a transaction and so needs a
-- `-- no-transaction` migration, which no migration in this tree is.

ALTER TABLE notifications ADD COLUMN lease_expires_at TIMESTAMPTZ;

CREATE INDEX idx_notifications_created_at_id
    ON notifications (created_at, id) WHERE status = 'PENDING' AND deleted_at IS NULL;

COMMENT ON COLUMN notifications.lease_expires_at IS
    'When the latest sender''s claim on this row ends (#853). Null for a row no sender has claimed. A PENDING row is claimed when this is null or has passed, under FOR UPDATE SKIP LOCKED, so two senders never take one row together, and a sender that crashed holding it gives it up when this passes. The settle matches on it, so a sender that outlived its lease cannot settle a row a newer claim holds. Left as it was once the row is settled.';
