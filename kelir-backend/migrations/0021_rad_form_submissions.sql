-- 0021_rad_form_submissions.sql — where a submitted form lands, and the
-- permission that opens it (FR-RAD-010, FR-RAD-006, #164).
--
-- **This table holds the server's answer and never the client's.** JFSS S8.1
-- makes the backend re-evaluate every `calculate` expression and overwrite the
-- submitted value before persistence, and S10.2 does the same for every
-- `conditional`. `payload_json` is what came out of that re-evaluation
-- (`rad::service::evaluation`), so a row here is by construction not a record
-- of what the browser computed. The operator-parity spike measured what the
-- other arrangement looks like: the Calculation Rule Registry §6.1 invoice
-- persisting a grand total of 0 in place of 42, with nothing logged and nothing
-- refused.
--
-- **Why a RAD-owned table rather than a document** (construction plan §6.1).
-- The Sprint 8 exit needs the server's own answer *stored*, and every document
-- item — FR-DOC-001..007 — is Sprint 9's under decision **D-16**. Writing into
-- `documents.form_data_json` would pull a slice of [#167](https://github.com/sujanto-gaws/kelir/issues/167)
-- forward against the sprint plan's explicit exclusion, and a stateless
-- evaluate endpoint would satisfy neither the exit nor #164 AC3. A form that
-- submits is not yet a document that exists, and this table is the difference
-- said out loud.
--
-- **It is deliberately not the document store, and nothing should grow it into
-- one.** No status, no number, no workflow instance, no relations: those are
-- §6's tables and Sprint 9 builds on them. What is here is the smallest row
-- that can prove a re-evaluation happened and be read back.
--
-- **`form_revision` is denormalized on purpose.** `form_id` already pins the
-- exact revision — a published revision is immutable and a new revision is a
-- new row (§5.3) — so the number is derivable by a join. It is stored anyway
-- because a submission is evidence: reading "revision 3 of `purchase-requisition`"
-- off the row is what makes a stored payload interpretable years later without
-- trusting that nothing ever hard-deleted a form.
--
-- **`submitted_at` is not `created_at` wearing another name.** `created_at` is
-- when the row was written; `submitted_at` is when the person pressed the
-- button, which is the business fact Sprint 9's numbering and status transitions
-- will be ordered by. They are equal today because the two happen in one
-- request, and they stop being equal the first time a submission is queued,
-- retried or imported.
--
-- Takes 0021 because that is the next free number after 0020 (naming convention
-- §4.3). The workflow migration Phase 5 will write moves to 0022, and every
-- unwritten migration below it moves with it. The Database Schema mapping table
-- is the sequence and is updated with this change.

-- ---------------------------------------------------------------------------
-- The composite key a submission's tenant is anchored to
-- ---------------------------------------------------------------------------
--
-- `rad_forms.id` is already the primary key, so this constraint buys no lookup;
-- it exists to make `(id, tenant_id)` referenceable, which is the same trade
-- `0017_tenant_administration.sql` made on `roles` and `0020_numbering_buckets.sql`
-- made on `document_types`. It has to be created before the table that
-- references it — a foreign key resolves at `CREATE TABLE` time, not at commit.

ALTER TABLE rad_forms
    ADD CONSTRAINT uq_rad_forms_id_tenant_id UNIQUE (id, tenant_id);

-- ---------------------------------------------------------------------------
-- The row (§5.14)
-- ---------------------------------------------------------------------------

CREATE TABLE rad_form_submissions (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID        REFERENCES users (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID        REFERENCES users (id),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Present because §1.2 gives it to every table, and never set by any route
    -- here: a submission is a record of something that happened, and retiring
    -- one would be editing history rather than the record it describes. Reads
    -- still filter it, so the day a retention policy needs one the reads are
    -- already right.
    deleted_at      TIMESTAMPTZ,
    form_id         UUID        NOT NULL REFERENCES rad_forms (id),
    -- The revision `form_id` pins, denormalized — see the header.
    form_revision   INTEGER     NOT NULL,
    -- The server's re-evaluated payload. Never the submitted one.
    payload_json    JSONB       NOT NULL,
    submitted_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- The tenant of a submission is the tenant of its form. Composite, so a
    -- cross-tenant submission cannot be written at all — `0017_tenant_administration.sql`
    -- and `0020_numbering_buckets.sql` give the reasoning: a constraint holds
    -- where a forgotten predicate does not.
    CONSTRAINT fk_rad_form_submissions_form
        FOREIGN KEY (form_id, tenant_id)
        REFERENCES rad_forms (id, tenant_id),
    CONSTRAINT ck_rad_form_submissions_revision_positive CHECK (form_revision >= 1)
);

-- The read a form's submission history makes.
CREATE INDEX idx_rad_form_submissions_tenant_id_form_id
    ON rad_form_submissions (tenant_id, form_id, submitted_at DESC)
    WHERE deleted_at IS NULL;

COMMENT ON TABLE rad_form_submissions IS
    'A filled-in form as the server re-evaluated it (JFSS S8.1, S10.2). '
    'payload_json is the backend''s own answer: every calculate expression '
    'recomputed and every hidden component''s value discarded before the row '
    'was written. It is not a document — FR-DOC-001..007 are Sprint 9.';

COMMENT ON COLUMN rad_form_submissions.payload_json IS
    'The secure payload. Never what the client submitted.';

-- ---------------------------------------------------------------------------
-- The permission (§5.13)
-- ---------------------------------------------------------------------------
--
-- **A submit permission distinct from `rad:form:read`**, because filling in a
-- form and being allowed to record one are different questions: a reviewer who
-- may open a requisition to read it is not thereby somebody who may raise one.
-- The pairing is the same shape `rad:form:publish` has beside `rad:form:update`
-- — reading is what makes the surface usable, and the write is its own grant.
--
-- It is not `rad:submission:create`. The `resource` segment names what the
-- permission is about (naming convention §6), and this is about a form: the
-- submission row is the record it leaves, not the thing being authorized.

INSERT INTO permissions (id, tenant_id, permission_code, module, description) VALUES
    ('00000000-0000-0000-0001-000000000036', '00000000-0000-0000-0000-000000000001',
     'rad:form:submit', 'rad',
     'Fill in a published form and submit it; the server re-evaluates every calculation before it is stored');

-- ROLE-ADMIN holds every permission in the catalogue (0002_identity.sql); grant
-- the one new row rather than re-inserting the ones already granted.
INSERT INTO role_permissions (id, tenant_id, role_id, permission_id)
SELECT
    gen_random_uuid(),
    '00000000-0000-0000-0000-000000000001',
    '00000000-0000-0000-0002-000000000001',
    id
FROM permissions
WHERE permission_code = 'rad:form:submit';
