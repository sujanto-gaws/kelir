-- 0023_document.sql — the document surface gets its permissions, and its
-- internal reference gets a counter.
--
-- `0015_document.sql` created every table in Database Schema §6 and seeded
-- permissions for **none** of the document ones, saying why: "those have no
-- endpoint until Sprint 9, and a permission row no route checks reads as a
-- control that exists". Sprint 9 is here and the routes exist, so the rows
-- land — which is that comment being honoured rather than overruled.
--
-- Takes 0023 because that is the next free number after 0022 (naming
-- convention §4.3). The migrations planned after it shift down by one in the
-- Database Schema mapping table, which is the only place the sequence lives;
-- merged migrations are never renumbered.
--
-- ---------------------------------------------------------------------------
-- Six permissions, and why two of them are not `document:update`
-- ---------------------------------------------------------------------------
--
--   * `document:create`, `document:read`, `document:update`, `document:delete`
--     are the four a resource has.
--
--   * **`document:submit` is separate**, because submitting takes a number the
--     document keeps forever and starts a life a workflow will later drive. A
--     deployment that lets a clerk correct a requisition's line items has not
--     thereby decided that the clerk may commit it.
--
--   * **`document:transition` is separate**, which is #99's AC1 restated for
--     documents: a transition has a from-state, a legal set, its own audit
--     action and its own consequences, and putting it behind the update
--     permission would put approval behind typing.
--
-- The resource segment is omitted throughout because this module manages one
-- resource (naming convention §6), exactly as `document-type:*` omits it.
-- Metadata, versions, relations and the link are parts of a document rather
-- than resources beside it, so none of them gets a permission of its own.
--
-- ---------------------------------------------------------------------------
-- `document_ref_sequences`, and why the reference needs one at all
-- ---------------------------------------------------------------------------
--
-- `documents.document_ref` is `NOT NULL` and unique per tenant, and §6.6
-- documents its shape as `DOC-2026-000123` — a tenant-wide, year-scoped
-- counter, which nothing in the codebase produced. It is **not**
-- `document_number`: the number is the business identifier a type's rule
-- renders and a submit assigns, and the ref is the internal handle a draft has
-- for the whole of its life *before* it has a number. A draft with no handle is
-- what a user is looking at until they submit.
--
-- **The shape is `0020_numbering_buckets.sql`'s, deliberately.** One row per
-- bucket, insert-or-advance in a single statement, `RETURNING next_sequence - 1`
-- so the caller never re-reads to learn what it got, and no read to race — the
-- unique index serialises two callers in one bucket and callers in different
-- buckets do not contend at all. Copying a proven allocator beats writing a
-- second one that is subtly different, which is what §6.3 of the schema used to
-- have and what #200 was.
--
-- **The counter is scoped to the tenant and the year, and not to the document
-- type.** That is the difference from `document_type_sequence_buckets` and it
-- is what §6.6's example says: a ref is unique across a tenant, so two types
-- creating documents on the same day take successive refs rather than the same
-- one. The uniqueness the ref needs is the tenant's.
--
-- The allocation happens **inside** the creating transaction, so a failed
-- create rolls the counter back with it and the refs have no gaps. The trade is
-- that draft creations within one tenant-year serialise from the allocation to
-- the commit; a creation transaction is short, and if that ever bites, gaps in
-- an internal handle cost nothing and the gap-tolerant shape is one function
-- away. Recorded in the [Sprint 9 construction plan] §3 as the one question
-- that plan leaves owed, so that a measurement can move it.

-- ---------------------------------------------------------------------------
-- The reference counter
-- ---------------------------------------------------------------------------

CREATE TABLE document_ref_sequences (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- The bucket: the year the reference is issued in, as four characters.
    -- Stored rather than derived so that "which year is this counter in" is a
    -- question about a row instead of a question about the clock.
    reference_key   VARCHAR(64) NOT NULL,
    -- The number the *next* reference takes, which is why the allocator returns
    -- this minus one.
    next_sequence   BIGINT      NOT NULL DEFAULT 1,
    CONSTRAINT ck_document_ref_sequences_sequence CHECK (next_sequence >= 1)
);

-- What `ON CONFLICT` targets, and what serialises two creations in one bucket.
CREATE UNIQUE INDEX uq_document_ref_sequences_tenant_id_reference_key
    ON document_ref_sequences (tenant_id, reference_key);

-- ---------------------------------------------------------------------------
-- The index the list needs and 0015 did not anticipate
-- ---------------------------------------------------------------------------
--
-- `0015` indexed `(tenant_id, status)` and `(tenant_id, document_type_id,
-- status)`, which are the filters it could see. The linked-entity filter is
-- #170's and #171's, and it reads both halves of the pair — an `entity_id`
-- without its `entity_type` is the ambiguity #170 AC1 exists to forbid, so the
-- index covers the pair rather than the id.
--
-- **No index is added for the list's `search`.** It is a case-insensitive
-- substring over three columns, and a trigram index chosen before the query
-- that uses it has been measured is an index chosen on a guess. Sprint 9's
-- construction plan §8.2 says so in the same words.

CREATE INDEX idx_documents_tenant_id_entity
    ON documents (tenant_id, entity_type, entity_id)
    WHERE entity_type IS NOT NULL AND deleted_at IS NULL;

-- ---------------------------------------------------------------------------
-- The permission catalogue rows for the document surface
-- ---------------------------------------------------------------------------
--
-- Continues the id-block convention of 0002_identity.sql: permissions take the
-- 0001 block, which stands at ...0036 after 0021.

INSERT INTO permissions (id, tenant_id, permission_code, module, description) VALUES
    ('00000000-0000-0000-0001-000000000037', '00000000-0000-0000-0000-000000000001',
     'document:create',     'document', 'Create a document from a document type'),
    ('00000000-0000-0000-0001-000000000038', '00000000-0000-0000-0000-000000000001',
     'document:read',       'document', 'View documents'),
    ('00000000-0000-0000-0001-000000000039', '00000000-0000-0000-0000-000000000001',
     'document:update',     'document', 'Edit a draft document and its metadata'),
    ('00000000-0000-0000-0001-000000000040', '00000000-0000-0000-0000-000000000001',
     'document:delete',     'document', 'Discard a draft document'),
    ('00000000-0000-0000-0001-000000000041', '00000000-0000-0000-0000-000000000001',
     'document:submit',     'document', 'Submit a draft, which assigns its number'),
    ('00000000-0000-0000-0001-000000000042', '00000000-0000-0000-0000-000000000001',
     'document:transition', 'document', 'Move a document to another status');

-- ROLE-ADMIN holds every permission in the catalogue (0002_identity.sql); grant
-- only the six new rows rather than re-inserting the ones already granted.
INSERT INTO role_permissions (id, tenant_id, role_id, permission_id)
SELECT
    gen_random_uuid(),
    '00000000-0000-0000-0000-000000000001',
    '00000000-0000-0000-0002-000000000001',
    id
FROM permissions
WHERE permission_code IN (
    'document:create',
    'document:read',
    'document:update',
    'document:delete',
    'document:submit',
    'document:transition'
);
