-- 0037_attachment_tail.sql — categories with rows in them, a table for links
-- that are not files, and the permission a delete checks (FR-ATT-006,
-- FR-ATT-009, FR-ATT-010; #254, decisions D-52 and D-53, recorded as ADR-0031
-- and ADR-0032).
--
-- `0031_attachment.sql` created `attachment_categories` and wrote nothing to
-- it, and gave `attachments.category_id` a foreign key nothing filled. This is
-- the sprint that fills both, and it brings the two things that could not be
-- columns on an existing table.
--
-- # The four category codes, and why these four
--
-- `QUOTATION`, `CONTRACT`, `APPROVAL`, `EVIDENCE` — the vocabulary
-- [concepts/02](../../docs/concepts/02.%20Handling%20Attachments%20Comments%20and%20Activity%20Log.md)
-- §3.1 names for this column, seeded rather than invented here. They are
-- `is_system = true`, which is the flag `0031` created for exactly this: a
-- tenant may add its own and may not delete these.
--
-- **Seeded for the system tenant only.** Every other tenant's rows are that
-- tenant's to create, and a seed that fanned out across `tenants` would be this
-- migration deciding something for deployments it cannot see — the shape
-- `0002_identity.sql` established for the permission catalogue.
--
-- # An external reference is not an attachment row (**D-53**, ADR-0031)
--
-- FR-ATT-010 asks for a reference to a document that lives somewhere else, and
-- the obvious shape is a row in `attachments` with a URL and no file. **It is
-- refused here for two reasons, and the second is the one that settles it.**
--
-- *It would have to lie.* #254 AC4 asks that a reference be visibly not a file —
-- no size, no scan status, no download — and AC5 that it never read `CLEAN`.
-- `attachments.file_size`, `checksum`, `mime_type` and `storage_reference` are
-- `NOT NULL`, and `virus_scan_status` has four values of which every one is a
-- claim about a scan. A reference in that table either carries sentinels that
-- say something false, or those columns become nullable.
--
-- *And nullable columns would break the previous release.* `v0.5.0`'s binary
-- selects `file_size` and `virus_scan_status` into non-null Rust types through
-- `sqlx::query!`. A row with nulls in them does not fail that binary's
-- migration — it fails its **list**, at run time, for every caller looking at a
-- document that has one. The N−1 rule is not a rule about DDL; it is a rule
-- about the previous binary continuing to work, and a new table is the one
-- shape that cannot reach it.
--
-- So `document_external_references` is its own table with its own columns, and
-- there is no scan column on it to be wrong.
--
-- # A soft-deleted attachment keeps its object (**D-52**, ADR-0032)
--
-- #254 AC2 asks that this be decided and stated, because *soft* and *the bytes
-- are gone* cannot both be true. **The object stays.** A delete that removed it
-- would be a delete nothing can undo, on a row that still says what the file
-- was and what it hashed to; removing bytes is a retention question, and
-- `attachments.retention_policy_id` is where it will be answered by whatever
-- writes it. This item does not write it.
--
-- What the delete does is take the row out of every list and out of the
-- download, and both are held by predicates that already exist:
-- `deleted_at IS NULL` is in `find_stored_file` as well as in the list, which is
-- AC3 — the gate is on the path that serves the bytes rather than only on the
-- one that names them.
--
-- # Two permissions, and the same pairing the comment tail used
--
-- `attachment:delete` and `attachment:reference`. Each is checked **with**
-- authorship for the delete — the permission says whether an account deletes
-- attachments at all, `created_by` says whose — which is `0036_comment_thread.sql`'s
-- rule one table over and the same reason: no code in this release lets one
-- account delete another's upload, so no code is granted for it.
--
-- # N−1 compatibility
--
-- One new table the previous release names in no statement, four rows in a
-- table it reads and does not write, two permission rows, and **no `ALTER` to
-- `attachments` at all** — `category_id` has been there since `0031`. The
-- previous binary starts against this schema and behaves as it did.

INSERT INTO attachment_categories (id, tenant_id, category_code, name, description, is_system) VALUES
    ('00000000-0000-0000-0003-000000000001', '00000000-0000-0000-0000-000000000001',
     'QUOTATION', 'Quotation', 'A supplier''s offer or price comparison', true),
    ('00000000-0000-0000-0003-000000000002', '00000000-0000-0000-0000-000000000001',
     'CONTRACT', 'Contract', 'An agreement or its amendments', true),
    ('00000000-0000-0000-0003-000000000003', '00000000-0000-0000-0000-000000000001',
     'APPROVAL', 'Approval', 'Evidence of an approval taken outside this system', true),
    ('00000000-0000-0000-0003-000000000004', '00000000-0000-0000-0000-000000000001',
     'EVIDENCE', 'Evidence', 'Anything supporting the document''s claims', true);

CREATE TABLE document_external_references (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID        REFERENCES users (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID        REFERENCES users (id),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    document_id     UUID        NOT NULL REFERENCES documents (id) ON DELETE CASCADE,
    category_id     UUID        REFERENCES attachment_categories (id),
    label           VARCHAR(200) NOT NULL,
    url             TEXT        NOT NULL,
    description     TEXT
);

CREATE INDEX idx_document_external_references_document_id
    ON document_external_references (document_id, created_at) WHERE deleted_at IS NULL;

COMMENT ON TABLE document_external_references IS
    'A link to something that lives elsewhere (FR-ATT-010). Not an attachment: this table has no size, no checksum, no storage reference and no scan status, because a reference is not a file and a row that borrowed those columns would have to put something false in them (D-53).';

COMMENT ON COLUMN document_external_references.url IS
    'http or https only, refused at the surface (attachment::domain::normalize_url). A stored URL is rendered as a link, and javascript: or data: in a link is somebody else''s script in this product''s page.';

COMMENT ON COLUMN document_external_references.category_id IS
    'The same vocabulary attachments use, deliberately: a person filing a quotation should not have to know whether the quotation happened to arrive as a file or as a link.';

COMMENT ON COLUMN attachments.category_id IS
    'What kind of thing this file is (FR-ATT-006). Optional: an uncategorized attachment is a file somebody has not filed yet, which is a normal state and not an error. Written from 0037 on; rows created before it are null.';

COMMENT ON COLUMN attachments.deleted_at IS
    'When it was deleted (FR-ATT-009). Soft: the row stays and so does the stored object (D-52). The predicate is in list_for_document, count_for_document and find_stored_file, so a deleted attachment leaves the list and refuses the bytes on the path that serves them.';

INSERT INTO permissions (id, tenant_id, permission_code, module, description) VALUES
    ('00000000-0000-0000-0001-000000000060', '00000000-0000-0000-0000-000000000001',
     'attachment:delete', 'attachment',
     'Delete an attachment (FR-ATT-009). Held with authorship, never instead of it: this grants deleting one''s own uploads, and no permission in this release lets an account delete somebody else''s. The delete is soft and the stored object is kept (D-52).'),
    ('00000000-0000-0000-0001-000000000061', '00000000-0000-0000-0000-000000000001',
     'attachment:reference', 'attachment',
     'Record a link to a document that lives elsewhere (FR-ATT-010). Separate from attachment:create because it grants something different: no bytes enter this product, nothing is scanned, and the risk is a link somebody else follows rather than a file this product stores.');

-- ROLE-ADMIN holds every permission in the catalogue (0002_identity.sql); grant
-- only the two new rows rather than re-inserting the ones already granted.
INSERT INTO role_permissions (id, tenant_id, role_id, permission_id)
SELECT
    gen_random_uuid(),
    '00000000-0000-0000-0000-000000000001',
    '00000000-0000-0000-0002-000000000001',
    id
FROM permissions
WHERE permission_code IN ('attachment:delete', 'attachment:reference');
