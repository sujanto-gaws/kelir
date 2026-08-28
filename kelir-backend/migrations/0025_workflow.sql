-- 0025_workflow.sql — the workflow engine's storage, and the two foreign keys
-- Phase 4 promised this migration would add.
--
-- Creates every table of Database Schema §7 plus one the schema did not have
-- (`workflow_ref_sequences`, below), and closes the two columns
-- `0015_document.sql` created without their constraints because the tables they
-- point at did not exist yet. Its header named both:
--
--   * `document_type_workflows.workflow_definition_id` — #174 AC2, and the
--     reason #187 can check a binding at all.
--   * `documents.process_instance_id` — the seam #178 is about.
--
-- **The Sprint 9 status report's fifth entry criterion is wrong on a detail and
-- this migration does not carry the error forward.** It reads "#157's *nullable*
-- workflow_definition_id still nullable and still unconstrained". The column is
-- `NOT NULL`: §6.4 specified it that way, `0015` implemented it that way, and
-- both say in as many words that this overrules #157's acceptance criterion 3
-- because where a plan and a docs/ document disagree the document wins. The
-- half that was genuinely deferred is the foreign key, and that half is what
-- lands here. Nothing below changes the nullability.
--
-- Takes 0024 because that is the next free number after 0023 (naming
-- convention §4.3), and it is the number Database Schema §4 already reserved
-- for it.
--
-- ---------------------------------------------------------------------------
-- Column widths follow §1.3.1, not the §7 snippets
-- ---------------------------------------------------------------------------
--
-- §7 was written before `0004_string_lengths.sql` and types most of its codes
-- as `TEXT`. §1.3.1 is the general rule — "bounded columns take a length from
-- this set" — and it exists because an indexed `TEXT` column fails
-- unpredictably at around 8 KB and MariaDB cannot index one without a prefix.
-- `0015_document.sql` already resolved the same disagreement the same way
-- (`documents.document_ref` is `VARCHAR(64)` where §6.6 says `TEXT`), so this
-- follows the established answer rather than inventing a second one. §7 is
-- updated to match in the same change.
--
-- One column changes name as well as type: §7.3's `condition_expression TEXT`
-- is `condition_json JSONB` here, because JWSS §1.1 supersedes string
-- expressions in favour of JSON Logic — *"String expressions in older documents
-- (`amount > 10000000`) are superseded"* — and a projection column typed for the
-- superseded form would be a column nothing could ever correctly fill.
--
-- ---------------------------------------------------------------------------
-- Two constraints that make an invariant a fact rather than a habit
-- ---------------------------------------------------------------------------
--
--   * **`workflow_instances (workflow_definition_id, current_state)` references
--     `workflow_states`.** #175 AC4 asks for "an instance cannot be in a state
--     its definition does not contain, enforced by the database rather than by
--     convention", and `uq_workflow_states_definition_state` is already a unique
--     index over exactly that pair, which PostgreSQL accepts as a foreign-key
--     target. An engine bug that writes a state the definition does not declare
--     becomes a constraint violation at the moment it happens rather than a
--     stuck instance found by whoever was waiting for it.
--
--     It also forces an ordering that is correct anyway: `workflow_states` is
--     written on publish, so **only a published definition can start an
--     instance**. That is not a second rule to remember, it is this one.
--
--   * **`uq_workflow_instances_live_document`** is #178 AC1 — a document links
--     to at most one live process instance. The service refuses a second one
--     with a message; this is what makes the refusal true under concurrency.
--
--   * **`uq_workflow_tasks_open_per_instance`** is the sprint's "sequential
--     approval only" (#174) written as a constraint instead of an intention. An
--     instance is in one state, a JWSS state declares at most one `task`, so an
--     instance has at most one open task. When FR-WF-016 (parallel approval) is
--     scheduled, this index is the thing that has to be argued with — which is
--     the point: the current design's assumption is visible and dated rather
--     than discovered later as a property of code that turned out to allow it.
--
-- ---------------------------------------------------------------------------
-- `workflow_ref_sequences` — one counter table, two prefixes
-- ---------------------------------------------------------------------------
--
-- §7.4 and §7.6 give `workflow_instances.instance_ref` (WFI-9001) and
-- `workflow_tasks.task_ref` (TASK-7001) as NOT NULL and unique per tenant, and
-- nothing produces either. The shape is `document_ref_sequences`', which is
-- `0020_numbering_buckets.sql`': one row per bucket, insert-or-advance in a
-- single statement, `RETURNING next_sequence - 1`, no read to race.
--
-- **One table serves both**, keyed by the prefix and the year (`WFI-2026`,
-- `TASK-2026`), rather than two tables differing only in what they count. Two
-- instances of a proven allocator is two places for the next fix to land in one
-- of, which is what §6.3 used to be and what #200 was.
--
-- It allocates inside the transaction that creates the row it names, so a
-- rolled-back transition leaves neither a hole nor a reference on a task that
-- was never created. That is `document_ref_sequences`' trade taken again and
-- for its reason: an internal handle whose counter is contended for one short
-- transaction costs nothing anybody can see.
--
-- ---------------------------------------------------------------------------
-- `workflow_escalations` is created and nothing reads it
-- ---------------------------------------------------------------------------
--
-- FR-WF-010 is a `Could` and unscheduled. The table is created because §7.9
-- specifies it and because `0015_document.sql` set the precedent for §6 — a
-- migration creates its section's tables together, so that the schema and the
-- document agree at every migration rather than at some of them. **A permission
-- row is the opposite case and is treated as one**: the rows below are the eight
-- routes this sprint actually checks, and escalation gets none, because a
-- permission no route checks reads as a control that exists.

-- ---------------------------------------------------------------------------
-- Definitions, and their two projections (§7.1, §7.2, §7.3)
-- ---------------------------------------------------------------------------

CREATE TABLE workflow_definitions (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID        REFERENCES users (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID        REFERENCES users (id),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    workflow_key    VARCHAR(64) NOT NULL,           -- 'purchase_requisition_standard'
    name            VARCHAR(200) NOT NULL,
    description     TEXT,
    -- The definition revision, not the JWSS spec version. The two were
    -- conflated once already on forms, so they are named apart here: this is
    -- what an instance pins, and `jwss_version` is what a validator reads.
    version         INTEGER     NOT NULL DEFAULT 1,
    jwss_version    VARCHAR(40) NOT NULL,           -- '1.0.0'
    definition_json JSONB       NOT NULL,           -- the JWSS document, authoritative
    initial_state   VARCHAR(64) NOT NULL,
    status          VARCHAR(40) NOT NULL DEFAULT 'DRAFT'
                    CHECK (status IN ('DRAFT', 'ACTIVE', 'DEPRECATED')),
    published_at    TIMESTAMPTZ,
    published_by    UUID        REFERENCES users (id)
);
CREATE UNIQUE INDEX uq_workflow_definitions_tenant_id_workflow_key_version
    ON workflow_definitions (tenant_id, workflow_key, version) WHERE deleted_at IS NULL;

-- The projections. Regenerated whole from `definition_json` on publish (JWSS
-- §9): the JSON is the authority, so a projected row that survives a republish
-- is a row the authority did not ask for.
--
-- **The engine does not read these.** It reads `definition_json`, because that
-- is what JWSS §1 calls the single source of truth, and an engine reading the
-- projection would be executing a copy — two answers to "what does this
-- workflow do", which is the failure this sprint is told twice to avoid one
-- layer up.

CREATE TABLE workflow_states (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID        REFERENCES users (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID        REFERENCES users (id),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    workflow_definition_id UUID NOT NULL REFERENCES workflow_definitions (id) ON DELETE CASCADE,
    state_code      VARCHAR(64) NOT NULL,           -- 'MANAGER_APPROVAL'
    name            VARCHAR(200) NOT NULL,
    maps_to_document_status VARCHAR(40) NOT NULL,   -- a platform status (§6.6)
    is_initial      BOOLEAN     NOT NULL DEFAULT false,
    is_final        BOOLEAN     NOT NULL DEFAULT false,
    sort_order      INTEGER     NOT NULL DEFAULT 0
);
-- Total rather than partial on `deleted_at`, and that is load-bearing twice: it
-- is the foreign-key target below, which a partial index cannot be, and nothing
-- soft-deletes a projection row — a republish deletes and rewrites the set.
CREATE UNIQUE INDEX uq_workflow_states_definition_state
    ON workflow_states (workflow_definition_id, state_code);

CREATE TABLE workflow_transitions (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID        REFERENCES users (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID        REFERENCES users (id),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    workflow_definition_id UUID NOT NULL REFERENCES workflow_definitions (id) ON DELETE CASCADE,
    from_state      VARCHAR(64) NOT NULL,
    to_state        VARCHAR(64) NOT NULL,
    action          VARCHAR(40) NOT NULL,           -- SUBMIT | APPROVE | REJECT | RETURN | RESUBMIT
                                                    -- | DELEGATE | ESCALATE | CANCEL | COMPLETE | AUTO
    allowed_by_json JSONB       NOT NULL DEFAULT '{}',  -- the normalized assignment rule (JWSS §5)
    -- JSON Logic, not a string expression: JWSS §1.1 supersedes the string form.
    condition_json  JSONB,
    guards_json     JSONB       NOT NULL DEFAULT '[]',
    actions_json    JSONB       NOT NULL DEFAULT '[]',
    sort_order      INTEGER     NOT NULL DEFAULT 0
);
CREATE UNIQUE INDEX uq_workflow_transitions_definition_from_action_to
    ON workflow_transitions (workflow_definition_id, from_state, action, to_state);

-- ---------------------------------------------------------------------------
-- Instances and their variables (§7.4, §7.5)
-- ---------------------------------------------------------------------------

CREATE TABLE workflow_instances (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID        REFERENCES users (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID        REFERENCES users (id),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    instance_ref    VARCHAR(64) NOT NULL,           -- WFI-2026-000123
    -- **This reference is the version pin** (#175 AC1). It names a revision
    -- row, and a published revision row never changes, so a `definition_version`
    -- column beside it would be a second copy of a fact this reference already
    -- carries — which is the failure AC3 is about, in the other direction.
    workflow_definition_id UUID NOT NULL REFERENCES workflow_definitions (id),
    document_id     UUID        NOT NULL REFERENCES documents (id),
    business_key    VARCHAR(64),                    -- the document number
    status          VARCHAR(40) NOT NULL DEFAULT 'STARTED'
                    CHECK (status IN ('STARTED', 'RUNNING', 'SUSPENDED', 'COMPLETED',
                                      'CANCELLED', 'FAILED')),
    current_state   VARCHAR(64) NOT NULL,
    outcome         VARCHAR(40) CHECK (outcome IN ('APPROVED', 'REJECTED', 'RETURNED', 'CANCELLED')),
    started_by      UUID        REFERENCES users (id),
    started_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at    TIMESTAMPTZ,
    cancelled_at    TIMESTAMPTZ
);
CREATE UNIQUE INDEX uq_workflow_instances_tenant_id_instance_ref
    ON workflow_instances (tenant_id, instance_ref) WHERE deleted_at IS NULL;
CREATE INDEX idx_workflow_instances_document_id ON workflow_instances (document_id);
CREATE INDEX idx_workflow_instances_tenant_id_status ON workflow_instances (tenant_id, status);

-- #175 AC4, enforced by the database rather than by convention.
ALTER TABLE workflow_instances ADD CONSTRAINT fk_workflow_instances_current_state
    FOREIGN KEY (workflow_definition_id, current_state)
    REFERENCES workflow_states (workflow_definition_id, state_code);

-- #178 AC1, under concurrency rather than only in the service.
CREATE UNIQUE INDEX uq_workflow_instances_live_document
    ON workflow_instances (document_id)
    WHERE status IN ('STARTED', 'RUNNING', 'SUSPENDED') AND deleted_at IS NULL;

CREATE TABLE workflow_variables (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID        REFERENCES users (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID        REFERENCES users (id),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    workflow_instance_id UUID   NOT NULL REFERENCES workflow_instances (id) ON DELETE CASCADE,
    variable_key    VARCHAR(64) NOT NULL,           -- 'amount', 'requires_director_approval'
    -- Typed by `data_type` rather than by the column: a variable's type is the
    -- definition's declaration (JWSS §6.3), and storing five columns to hold one
    -- value would put the type in the schema instead of in the workflow.
    variable_value  TEXT        NOT NULL,
    data_type       VARCHAR(40) NOT NULL DEFAULT 'STRING'
                    CHECK (data_type IN ('STRING', 'NUMBER', 'BOOLEAN', 'DATE', 'JSON'))
);
CREATE UNIQUE INDEX uq_workflow_variables_instance_key
    ON workflow_variables (workflow_instance_id, variable_key) WHERE deleted_at IS NULL;

-- ---------------------------------------------------------------------------
-- Tasks, their history, and the decisions recorded against them (§7.6–§7.8)
-- ---------------------------------------------------------------------------

CREATE TABLE workflow_tasks (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID        REFERENCES users (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID        REFERENCES users (id),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    task_ref        VARCHAR(64) NOT NULL,           -- TASK-2026-000123
    workflow_instance_id UUID   NOT NULL REFERENCES workflow_instances (id) ON DELETE CASCADE,
    document_id     UUID        NOT NULL REFERENCES documents (id),
    task_definition_key VARCHAR(64) NOT NULL,       -- 'finance_approval'
    task_name       VARCHAR(200) NOT NULL,
    task_type       VARCHAR(40) NOT NULL DEFAULT 'APPROVAL_TASK'
                    CHECK (task_type IN ('USER_TASK', 'APPROVAL_TASK', 'REVIEW_TASK', 'SERVICE_TASK',
                                         'SIGNATURE_TASK', 'DATA_ENTRY_TASK')),
    status          VARCHAR(40) NOT NULL DEFAULT 'CREATED'
                    CHECK (status IN ('CREATED', 'ASSIGNED', 'IN_PROGRESS', 'COMPLETED',
                                      'DELEGATED', 'ESCALATED', 'CANCELLED')),
    -- **Assigned to a user or offered to a role, never both.** A task offered to
    -- a role has no assignee until somebody claims it, and that is the
    -- difference #179 AC1 has to show a person: an unclaimed queue item and work
    -- that is already mine are different situations. Writing both would erase
    -- the difference at the moment the task is created.
    assignee_user_id     UUID   REFERENCES users (id),
    candidate_role_id    UUID   REFERENCES roles (id),
    candidate_department_id UUID REFERENCES departments (id),
    delegated_from_user_id UUID REFERENCES users (id),
    priority        VARCHAR(40) NOT NULL DEFAULT 'NORMAL'
                    CHECK (priority IN ('LOW', 'NORMAL', 'HIGH', 'URGENT')),
    due_at          TIMESTAMPTZ,
    action          VARCHAR(40) CHECK (action IN ('APPROVE', 'REJECT', 'RETURN', 'SUBMIT',
                                                  'CONFIRM', 'SIGN', 'COMPLETE')),
    -- Written by nothing in Sprint 10. FR-TASK-006 is #182 in Sprint 11 and the
    -- construction plan §7.5 says what the gap costs: a rejection recorded this
    -- sprint has no reason on it. The column is here because §7.6 put it here.
    comment         TEXT,
    completed_by    UUID        REFERENCES users (id),
    completed_at    TIMESTAMPTZ
);
CREATE UNIQUE INDEX uq_workflow_tasks_tenant_id_task_ref
    ON workflow_tasks (tenant_id, task_ref) WHERE deleted_at IS NULL;
CREATE INDEX idx_workflow_tasks_assignee_status ON workflow_tasks (tenant_id, assignee_user_id, status);
CREATE INDEX idx_workflow_tasks_candidate_role_status ON workflow_tasks (tenant_id, candidate_role_id, status);
CREATE INDEX idx_workflow_tasks_due_at ON workflow_tasks (tenant_id, status, due_at);

-- Sequential approval only (#174), as a constraint rather than as a sentence.
CREATE UNIQUE INDEX uq_workflow_tasks_open_per_instance
    ON workflow_tasks (workflow_instance_id)
    WHERE status IN ('CREATED', 'ASSIGNED', 'IN_PROGRESS') AND deleted_at IS NULL;

-- Append-only (§1.2): no `updated_at`, no `deleted_at`. A history row is a fact
-- rather than a record.
--
-- **It is not `approval_decisions` and it is not FR-WF-012**, and all three are
-- distinguished in `modules/workflow/mod.rs` because this project has twice
-- found out what happens when two records of one event have no stated
-- relationship. This one answers *what happened to this task*.
CREATE TABLE workflow_task_history (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    task_id         UUID        NOT NULL REFERENCES workflow_tasks (id) ON DELETE CASCADE,
    workflow_instance_id UUID   NOT NULL REFERENCES workflow_instances (id),
    document_id     UUID        NOT NULL REFERENCES documents (id),
    old_status      VARCHAR(40),
    new_status      VARCHAR(40) NOT NULL,
    action          VARCHAR(40),
    comment         TEXT,
    actor_user_id   UUID        REFERENCES users (id),   -- null for engine actions
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_workflow_task_history_task_id ON workflow_task_history (task_id, created_at);

-- The formal decision record — *what was decided about this document* (§7.8).
CREATE TABLE approval_decisions (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID        REFERENCES users (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID        REFERENCES users (id),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    document_id     UUID        NOT NULL REFERENCES documents (id),
    workflow_instance_id UUID   NOT NULL REFERENCES workflow_instances (id),
    task_id         UUID        NOT NULL REFERENCES workflow_tasks (id),
    approver_user_id UUID       NOT NULL REFERENCES users (id),
    approver_role_id UUID       REFERENCES roles (id),
    decision        VARCHAR(40) NOT NULL
                    CHECK (decision IN ('APPROVE', 'REJECT', 'RETURN', 'ESCALATE')),
    comment         TEXT,                           -- #182, Sprint 11; see workflow_tasks.comment
    decision_level  VARCHAR(64),                    -- 'MANAGER', 'FINANCE', 'DIRECTOR'
    digital_signature_ref VARCHAR(255),
    decided_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_approval_decisions_document_id ON approval_decisions (document_id, decided_at);

-- ---------------------------------------------------------------------------
-- Escalations (§7.9) — created, and read by nothing. See the header.
-- ---------------------------------------------------------------------------

CREATE TABLE workflow_escalations (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_by      UUID        REFERENCES users (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by      UUID        REFERENCES users (id),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    task_id         UUID        NOT NULL REFERENCES workflow_tasks (id) ON DELETE CASCADE,
    workflow_instance_id UUID   NOT NULL REFERENCES workflow_instances (id),
    escalation_rule_json JSONB  NOT NULL DEFAULT '{}',
    escalated_to_user_id UUID   REFERENCES users (id),
    escalated_to_role_id UUID   REFERENCES roles (id),
    triggered_at    TIMESTAMPTZ,
    resolved_at     TIMESTAMPTZ,
    status          VARCHAR(40) NOT NULL DEFAULT 'PENDING'
                    CHECK (status IN ('PENDING', 'TRIGGERED', 'RESOLVED', 'CANCELLED'))
);
CREATE INDEX idx_workflow_escalations_status ON workflow_escalations (tenant_id, status, triggered_at);

-- ---------------------------------------------------------------------------
-- The reference counter (§3.3 of the construction plan)
-- ---------------------------------------------------------------------------

CREATE TABLE workflow_ref_sequences (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- The bucket: the prefix and the year, as `WFI-2026` or `TASK-2026`. The
    -- prefix is part of the key rather than a second table, so an instance and
    -- a task never contend and neither can hand out the other's number.
    reference_key   VARCHAR(64) NOT NULL,
    next_sequence   BIGINT      NOT NULL DEFAULT 1,
    CONSTRAINT ck_workflow_ref_sequences_sequence CHECK (next_sequence >= 1)
);
CREATE UNIQUE INDEX uq_workflow_ref_sequences_tenant_id_reference_key
    ON workflow_ref_sequences (tenant_id, reference_key);

-- ---------------------------------------------------------------------------
-- The two foreign keys 0015 deferred to this migration
-- ---------------------------------------------------------------------------

ALTER TABLE documents ADD CONSTRAINT fk_documents_process_instance_id
    FOREIGN KEY (process_instance_id) REFERENCES workflow_instances (id);

-- #174 AC2. `0015`'s comment stops describing a pending state with this line.
ALTER TABLE document_type_workflows ADD CONSTRAINT fk_document_type_workflows_workflow_definition_id
    FOREIGN KEY (workflow_definition_id) REFERENCES workflow_definitions (id);

-- ---------------------------------------------------------------------------
-- Eight permission catalogue rows
-- ---------------------------------------------------------------------------
--
-- The resource segment is **required** here, because this module manages
-- several resources — which is the case naming convention §6 names, with
-- `workflow:task:execute` as its own worked example.
--
-- **`workflow:task:execute` covers claiming as well as deciding.** A separate
-- `workflow:task:claim` was the alternative, and it is rejected: a permission
-- that lets somebody take a task off the queue and then not act on it is a
-- permission to stall an approval, which is a worse power than the one it was
-- splitting off. Which *particular* task a caller may act on is not a permission
-- question at all — it is answered against the row, in `service::task`.
--
-- **The task inbox seeds nothing.** `GET /api/v1/tasks` requires
-- `workflow:task:read`, the same permission the workflow module's own task read
-- requires, because it reads the same rows. A `task:read` beside it would let a
-- deployment grant the inbox without granting the task, which is the gap §5.13
-- refused to create for `rad:lookup:read` one module over.
--
-- Continues the id-block convention of 0002_identity.sql: permissions take the
-- 0001 block, which stands at ...0042 after 0023.

INSERT INTO permissions (id, tenant_id, permission_code, module, description) VALUES
    ('00000000-0000-0000-0001-000000000043', '00000000-0000-0000-0000-000000000001',
     'workflow:definition:create',  'workflow', 'Create a workflow definition, or the next revision of one'),
    ('00000000-0000-0000-0001-000000000044', '00000000-0000-0000-0000-000000000001',
     'workflow:definition:read',    'workflow', 'View workflow definitions'),
    ('00000000-0000-0000-0001-000000000045', '00000000-0000-0000-0000-000000000001',
     'workflow:definition:update',  'workflow', 'Edit a draft workflow revision'),
    ('00000000-0000-0000-0001-000000000046', '00000000-0000-0000-0000-000000000001',
     'workflow:definition:publish', 'workflow', 'Publish a revision, fixing it for every instance that starts against it'),
    ('00000000-0000-0000-0001-000000000047', '00000000-0000-0000-0000-000000000001',
     'workflow:definition:delete',  'workflow', 'Retire a workflow definition'),
    ('00000000-0000-0000-0001-000000000048', '00000000-0000-0000-0000-000000000001',
     'workflow:instance:read',      'workflow', 'View a running process and its variables'),
    ('00000000-0000-0000-0001-000000000049', '00000000-0000-0000-0000-000000000001',
     'workflow:task:read',          'workflow', 'View tasks — the inbox, and one task''s detail'),
    ('00000000-0000-0000-0001-000000000050', '00000000-0000-0000-0000-000000000001',
     'workflow:task:execute',       'workflow', 'Claim a task, and record a decision on it');

-- ROLE-ADMIN holds every permission in the catalogue (0002_identity.sql); grant
-- only the eight new rows rather than re-inserting the ones already granted.
INSERT INTO role_permissions (id, tenant_id, role_id, permission_id)
SELECT
    gen_random_uuid(),
    '00000000-0000-0000-0000-000000000001',
    '00000000-0000-0000-0002-000000000001',
    id
FROM permissions
WHERE permission_code IN (
    'workflow:definition:create',
    'workflow:definition:read',
    'workflow:definition:update',
    'workflow:definition:publish',
    'workflow:definition:delete',
    'workflow:instance:read',
    'workflow:task:read',
    'workflow:task:execute'
);
