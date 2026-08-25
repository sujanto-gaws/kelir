-- 0016_numbering_gap_policy.sql — a numbering rule says whether its sequence
-- tolerates gaps.
--
-- No new tables. `document_type_numbering_rules` arrived with
-- `0015_document.sql`, carrying `next_sequence`, `sequence_scope` and
-- `sequence_key` but nothing that says what a *gap* in the sequence means.
--
-- **The two are different products and the schema should not be ambiguous about
-- which one a type asked for** (#158 acceptance criterion 4). A gapless
-- sequence is a legal requirement in several jurisdictions for invoices and
-- fiscal documents: every number issued must be accounted for, so a number
-- allocated to a submission that then fails must not be lost. A gap-tolerant
-- sequence is the ordinary case, and it buys back the thing gaplessness costs.
--
-- What it costs is concurrency, and the cost is not small:
--
--   * **Gapless** allocates inside the caller's transaction and holds the rule
--     row until that transaction commits. Two concurrent submissions of the
--     same type therefore serialise for as long as the slower one takes. In
--     exchange, a rollback rolls the number back with it.
--   * **Gap-tolerant** allocates in a transaction of its own that commits
--     immediately, so the rule row is held for microseconds. In exchange, a
--     number consumed by a submission that then fails is gone.
--
-- The default is `false` — gapless. A default that silently loses numbers is
-- the wrong way round: a deployment that wants throughput can say so, and one
-- that needed an unbroken sequence and never thought about it gets the safe
-- answer rather than a compliance problem discovered at an audit.
--
-- Takes 0016 because that is the next free number after 0015 (naming convention
-- §4.3). The migrations planned after it — the workflow migration and
-- everything below it — shift down by one in the Database Schema mapping table,
-- which is the only place the sequence lives.

ALTER TABLE document_type_numbering_rules
    ADD COLUMN allow_gaps BOOLEAN NOT NULL DEFAULT false;

COMMENT ON COLUMN document_type_numbering_rules.allow_gaps IS
    'false: the number is allocated inside the submitting transaction and rolls '
    'back with it, so the sequence has no gaps and concurrent submissions of '
    'this type serialise. true: the number is allocated and committed '
    'separately, so it survives a failed submission as a gap and the rule row '
    'is held only for the allocation itself.';
