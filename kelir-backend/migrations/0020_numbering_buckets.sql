-- 0020_numbering_buckets.sql — a numbering sequence keeps a counter per scope
-- value, not one per rule.
--
-- **The defect this closes** ([#200](https://github.com/sujanto-gaws/kelir/issues/200),
-- found by the Sprint 7 surface verification). `document_type_numbering_rules`
-- held a single `sequence_key`/`next_sequence` pair, and Database Schema §6.3
-- stated that as the design: "One bucket per rule." A `DEPARTMENT_YEAR` rule
-- needs one bucket *per department*, live at the same time, and one row cannot
-- hold two. The allocator restarted the counter whenever the key it computed
-- differed from the key stored and was not judged earlier — and two departments
-- are never earlier than one another — so every allocation that changed
-- department reset the only bucket there was. Measured: allocating for
-- department A, then B, then A, then B issued `000001` four times.
--
-- **Why the counters move rather than the scope going away.** Dropping
-- `DEPARTMENT_YEAR` would have been one line here, and it would have deleted a
-- capability from a published standard: the Document Type Definition Schema
-- v1.0.0 names it in the `sequenceScope` enum, in the `{department}` token
-- table, and in rule **S5**, which requires `{department}` in the template *iff*
-- the scope is `DEPARTMENT_YEAR`. Breaking a v1.0.0 standard to work around an
-- implementation defect is the wrong way round. Decision **D-21**.
--
-- **The bucket belongs to the document type, not to the rule row.** A rule row
-- is replaced on every edit (`replace_rule` deactivates and inserts), and a
-- sequence that reset when somebody corrected a template would re-issue numbers
-- documents already hold. Keying the bucket on `document_type_id` makes
-- "replacing a rule does not un-issue the numbers the old one produced" true by
-- construction rather than by the `highest_issued` check that used to carry it.
--
-- **`next_sequence` and `sequence_key` leave the rule row.** They are not
-- deprecated in place, and the N−1 rule (release process §6) is why that is
-- allowed rather than why it is forbidden: the previous release is `v0.3.0`,
-- whose migrations stop at `0013`, so no released binary has ever seen this
-- table. There is no release N holding the columns to deprecate them in. A
-- column left behind would instead be the thing `0011_record_status_permissions`
-- and §6.3 both warn about — storage nothing moves, reading as a claim the
-- product honours.
--
-- **`document_types` gains `UNIQUE (id, tenant_id)`** for the reason
-- `0017_tenant_administration.sql` gave `roles` the same constraint: it is the
-- target a composite foreign key needs, so a bucket filed under the wrong
-- tenant becomes unwritable rather than merely unwritten. The Sprint 7
-- verification found this module's tenant predicates exercised by nothing
-- (#206); a constraint holds when a test does not.
--
-- Takes 0020 because that is the next free number after 0019 (naming convention
-- §4.3). The workflow migration Phase 5 will write moves to 0021, and every
-- unwritten migration below it moves with it. The Database Schema mapping table
-- is the sequence and is updated with this change.

-- ---------------------------------------------------------------------------
-- The composite target
-- ---------------------------------------------------------------------------

ALTER TABLE document_types
    ADD CONSTRAINT uq_document_types_id_tenant_id UNIQUE (id, tenant_id);

-- ---------------------------------------------------------------------------
-- The buckets
-- ---------------------------------------------------------------------------

CREATE TABLE document_type_sequence_buckets (
    id               UUID        PRIMARY KEY,
    tenant_id        UUID        NOT NULL REFERENCES tenants (id),
    created_by       UUID        REFERENCES users (id),
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by       UUID        REFERENCES users (id),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Present because every table in §6 carries the base columns, and never
    -- set: soft-deleting a counter would hand out numbers documents already
    -- hold, which is the whole defect this migration exists to close. The
    -- unique index below is therefore total rather than partial on this column,
    -- and it has to be — it is an `ON CONFLICT` target.
    deleted_at       TIMESTAMPTZ,
    document_type_id UUID        NOT NULL REFERENCES document_types (id) ON DELETE CASCADE,
    -- The scope value this counter belongs to: '' for GLOBAL, '2026' for YEAR,
    -- '2026-08' for MONTH, '<department-uuid>:2026' for DEPARTMENT_YEAR. The
    -- keys a scope produces sort chronologically, which is what lets a
    -- back-dated allocation be recognised as reaching into a bucket the
    -- sequence has already passed.
    sequence_key     VARCHAR(64) NOT NULL,
    -- The number the next document in this bucket takes.
    next_sequence    BIGINT      NOT NULL DEFAULT 1,
    -- The tenant of the bucket is the tenant of its type. Composite, so a
    -- cross-tenant bucket cannot be written at all (0017's reasoning, #65).
    CONSTRAINT fk_document_type_sequence_buckets_type
        FOREIGN KEY (document_type_id, tenant_id)
        REFERENCES document_types (id, tenant_id) ON DELETE CASCADE,
    CONSTRAINT ck_document_type_sequence_buckets_sequence CHECK (next_sequence >= 1)
);

-- Total, not partial: this is the `ON CONFLICT` target that makes an allocation
-- one statement — insert the bucket or advance it, never read-then-write.
CREATE UNIQUE INDEX uq_document_type_sequence_buckets_type_key
    ON document_type_sequence_buckets (document_type_id, sequence_key);

COMMENT ON TABLE document_type_sequence_buckets IS
    'One counter per (document type, scope value). Advanced by an upsert inside '
    'the allocating transaction, so the row is the contended resource and two '
    'scope values never contend at all.';

-- ---------------------------------------------------------------------------
-- Carry the existing counters across
-- ---------------------------------------------------------------------------
--
-- One bucket per rule that has one, taking the rule's own key and counter. No
-- deployment has documents yet — the document surface is Sprint 9 — so this
-- moves configuration rather than issued numbers. It is written anyway, because
-- a rule configured with `nextSequence: 5000` to continue an existing external
-- numbering is exactly the state a migration must not silently reset.
--
-- `is_active` is not in the predicate: a deactivated rule's counter is still a
-- record of numbers issued under it, and `highest_issued` reads deactivated
-- rules for that reason. `DISTINCT ON` keeps one row per (type, key) where two
-- rules for one type share a bucket, taking the furthest-advanced counter,
-- because the unique index admits one and re-issuing is the failure being
-- closed.

INSERT INTO document_type_sequence_buckets
    (id, tenant_id, document_type_id, sequence_key, next_sequence, created_by, created_at)
SELECT DISTINCT ON (rule.document_type_id, rule.sequence_key)
    gen_random_uuid(),
    rule.tenant_id,
    rule.document_type_id,
    rule.sequence_key,
    rule.next_sequence,
    rule.created_by,
    rule.created_at
FROM document_type_numbering_rules rule
WHERE rule.deleted_at IS NULL
ORDER BY rule.document_type_id, rule.sequence_key, rule.next_sequence DESC;

-- ---------------------------------------------------------------------------
-- The rule row stops holding a counter
-- ---------------------------------------------------------------------------
--
-- What is left describes the *format* of a number and the policy for issuing
-- one: template, scope, padding, gap tolerance, active. The counter is above.
-- See the header for why these are dropped rather than deprecated.

ALTER TABLE document_type_numbering_rules
    DROP CONSTRAINT ck_document_type_numbering_rules_sequence;

ALTER TABLE document_type_numbering_rules
    DROP COLUMN next_sequence,
    DROP COLUMN sequence_key;

COMMENT ON TABLE document_type_numbering_rules IS
    'How a document type numbers its documents: the template, what resets the '
    'sequence, and whether the sequence tolerates gaps. The counters themselves '
    'are in document_type_sequence_buckets, one per scope value.';
