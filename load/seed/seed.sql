-- The data set behind the load test (#777 AC3, AC4), written by SQL.
--
-- Run by `seed.mjs`, in one transaction, against the database the runner made
-- for this run. The refusal to run against anything else lives in `guard.mjs`
-- and in the check at the top of this file: this script needs an empty tenant
-- table beyond SYSTEM, so a second pass, or a database with anything in it,
-- stops on the first statement.
--
-- **Statuses are set by SQL**, as AC3 allows. Nothing here runs the engine, the
-- hook chain or the outbox: the row measures reads, and every read below goes
-- through the same statements a document the engine moved would.
--
-- `:'password_hash'` is an Argon2id PHC string `seed.mjs` made for this run.

\set ON_ERROR_STOP on

BEGIN;

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM tenants WHERE tenant_code <> 'SYSTEM') THEN
        RAISE EXCEPTION 'refused: this database already holds a tenant other than SYSTEM';
    END IF;
END $$;

-- ---------------------------------------------------------------------------
-- The throwaway tenant, its role and its 51 users (AC4)
-- ---------------------------------------------------------------------------

INSERT INTO tenants (id, tenant_code, name)
VALUES ('10ad0000-0000-4000-8000-000000000001', 'TNT-LOAD', 'Load test tenant');

INSERT INTO roles (id, tenant_id, role_code, name, description)
VALUES ('10ad0000-0000-4000-8000-000000000002',
        '10ad0000-0000-4000-8000-000000000001',
        'ROLE-LOAD-READER', 'Load test reader',
        'Answers 2xx on every call in #777 AC1 and AC2.');

-- The catalogue is global and lives in the system tenant, so a grant names the
-- role's tenant and the catalogue's permission (0017_tenant_administration).
INSERT INTO role_permissions (id, tenant_id, role_id, permission_id)
SELECT gen_random_uuid(), '10ad0000-0000-4000-8000-000000000001',
       '10ad0000-0000-4000-8000-000000000002', p.id
FROM permissions p
WHERE p.deleted_at IS NULL
  AND p.permission_code IN (
      'document:read', 'rad:form:read', 'rad:menu:read', 'notification:read',
      'workflow:instance:read', 'workflow:task:read', 'attachment:read', 'comment:read');

CREATE TEMP TABLE load_users ON COMMIT DROP AS
SELECT n, gen_random_uuid() AS id,
       'load-user-' || lpad(n::text, 3, '0') AS username
FROM generate_series(1, 51) AS n;

INSERT INTO users (id, tenant_id, username, email, password_hash, display_name, status,
                   must_change_password)
SELECT id, '10ad0000-0000-4000-8000-000000000001', username,
       username || '@load.kelir.invalid', :'password_hash',
       'Load user ' || lpad(n::text, 3, '0'), 'ACTIVE', false
FROM load_users;

INSERT INTO user_roles (id, tenant_id, user_id, role_id)
SELECT gen_random_uuid(), '10ad0000-0000-4000-8000-000000000001', id,
       '10ad0000-0000-4000-8000-000000000002'
FROM load_users;

-- ---------------------------------------------------------------------------
-- One form of ten fields and no lookup field (AC3)
-- ---------------------------------------------------------------------------
--
-- `rad_forms` alone: `GET /rad/forms/{id}` reads the definition from this row,
-- and the section and field rows exist for authoring, which no view here does.

INSERT INTO rad_forms (id, tenant_id, form_key, title, revision, jfss_version, definition_json,
                       status, published_at)
SELECT '10ad0000-0000-4000-8000-000000000003', '10ad0000-0000-4000-8000-000000000001',
       'load_form', 'Load test form', 1, '2.0.1',
       jsonb_build_object(
           'formId', 'load_form',
           'version', '2.0.1',
           'title', 'Load test form',
           'components',
           (SELECT jsonb_agg(jsonb_build_object(
                       'id', 'field-' || lpad(n::text, 2, '0'),
                       'role', 'data',
                       'type', CASE WHEN n <= 8 THEN 'textfield' ELSE 'textarea' END,
                       'key', 'f' || lpad(n::text, 2, '0'),
                       'label', 'Field ' || n,
                       'validation', jsonb_build_object('type', 'string')) ORDER BY n)
            FROM generate_series(1, 10) AS n)
           || jsonb_build_array(jsonb_build_object(
                  'id', 'submit-button', 'role', 'action', 'type', 'button',
                  'label', 'Submit', 'action', 'submit'))),
       'PUBLISHED', now();

-- ---------------------------------------------------------------------------
-- A one-step workflow, bound to two of the five types (AC3)
-- ---------------------------------------------------------------------------

INSERT INTO workflow_definitions (id, tenant_id, workflow_key, name, version, jwss_version,
                                  definition_json, initial_state, status, published_at)
VALUES ('10ad0000-0000-4000-8000-000000000004', '10ad0000-0000-4000-8000-000000000001',
        'load_one_step', 'One step', 1, '1.0.0',
        '{"workflowKey": "load_one_step", "version": "1.0.0", "name": "One step",
          "initialState": "REVIEW",
          "states": [
            {"code": "REVIEW", "name": "Review", "mapsToDocumentStatus": "IN_REVIEW",
             "task": {"taskDefinitionKey": "review", "taskName": "Review the request",
                      "assignment": {"assigneeType": "ROLE", "roleCode": "ROLE-LOAD-READER"}}},
            {"code": "COMPLETED", "name": "Completed", "mapsToDocumentStatus": "COMPLETED",
             "isFinal": true},
            {"code": "REJECTED", "name": "Rejected", "mapsToDocumentStatus": "REJECTED",
             "isFinal": true}],
          "transitions": [
            {"from": "REVIEW", "to": "COMPLETED", "action": "APPROVE",
             "allowedBy": {"assigneeType": "ROLE", "roleCode": "ROLE-LOAD-READER"}},
            {"from": "REVIEW", "to": "REJECTED", "action": "REJECT",
             "allowedBy": {"assigneeType": "ROLE", "roleCode": "ROLE-LOAD-READER"}}]}',
        'REVIEW', 'ACTIVE', now());

INSERT INTO workflow_states (id, tenant_id, workflow_definition_id, state_code, name,
                             maps_to_document_status, is_initial, is_final, sort_order)
VALUES
    (gen_random_uuid(), '10ad0000-0000-4000-8000-000000000001',
     '10ad0000-0000-4000-8000-000000000004', 'REVIEW', 'Review', 'IN_REVIEW', true, false, 1),
    (gen_random_uuid(), '10ad0000-0000-4000-8000-000000000001',
     '10ad0000-0000-4000-8000-000000000004', 'COMPLETED', 'Completed', 'COMPLETED', false, true, 2),
    (gen_random_uuid(), '10ad0000-0000-4000-8000-000000000001',
     '10ad0000-0000-4000-8000-000000000004', 'REJECTED', 'Rejected', 'REJECTED', false, true, 3);

INSERT INTO workflow_transitions (id, tenant_id, workflow_definition_id, from_state, to_state,
                                  action, allowed_by_json, sort_order)
VALUES
    (gen_random_uuid(), '10ad0000-0000-4000-8000-000000000001',
     '10ad0000-0000-4000-8000-000000000004', 'REVIEW', 'COMPLETED', 'APPROVE',
     '{"assigneeType": "ROLE", "roleCode": "ROLE-LOAD-READER"}', 1),
    (gen_random_uuid(), '10ad0000-0000-4000-8000-000000000001',
     '10ad0000-0000-4000-8000-000000000004', 'REVIEW', 'REJECTED', 'REJECT',
     '{"assigneeType": "ROLE", "roleCode": "ROLE-LOAD-READER"}', 2);

-- ---------------------------------------------------------------------------
-- Five document types, the first two carrying the workflow (AC3)
-- ---------------------------------------------------------------------------

CREATE TEMP TABLE load_types ON COMMIT DROP AS
SELECT n, gen_random_uuid() AS id FROM generate_series(1, 5) AS n;

INSERT INTO document_types (id, tenant_id, type_code, name, form_id, status)
SELECT id, '10ad0000-0000-4000-8000-000000000001', 'LOAD_TYPE_' || n, 'Load type ' || n,
       '10ad0000-0000-4000-8000-000000000003', 'ACTIVE'
FROM load_types;

INSERT INTO document_type_workflows (id, tenant_id, document_type_id, workflow_definition_id)
SELECT gen_random_uuid(), '10ad0000-0000-4000-8000-000000000001', id,
       '10ad0000-0000-4000-8000-000000000004'
FROM load_types
WHERE n <= 2;

-- ---------------------------------------------------------------------------
-- 10,000 documents, 2,000 per type, in the status table AC3 fixes
-- ---------------------------------------------------------------------------
--
-- `n` runs 1..10000. Its type is `(n - 1) % 5`, so each type holds 2,000. Its
-- slot is `((n - 1) / 5) % 100`, so every type walks the 100 slots twenty times
-- and each type holds the table below exactly. `n` also orders `created_at`, so
-- the types and statuses are interleaved on the list's first page.

CREATE TEMP TABLE load_documents ON COMMIT DROP AS
SELECT n, gen_random_uuid() AS id, ((n - 1) % 5) + 1 AS type_no,
       CASE
           WHEN slot < 30 THEN 'DRAFT'
           WHEN slot < 40 THEN 'SUBMITTED'
           WHEN slot < 60 THEN 'IN_REVIEW'
           WHEN slot < 80 THEN 'APPROVED'
           WHEN slot < 85 THEN 'REJECTED'
           WHEN slot < 90 THEN 'RETURNED'
           WHEN slot < 95 THEN 'CANCELLED'
           ELSE 'COMPLETED'
       END AS status,
       now() - (10000 - n) * interval '25 minutes' AS created_at
FROM (SELECT n, ((n - 1) / 5) % 100 AS slot FROM generate_series(1, 10000) AS n) AS numbered;

-- About 2 KB of form data: ten fields of about 200 characters. Hashes rather
-- than one repeated string, so TOAST compresses it no better than real text.
INSERT INTO documents (id, tenant_id, created_by, created_at, updated_at, document_ref,
                       document_number, document_type_id, form_id, title, status, form_data_json,
                       priority, requested_by, amount, currency_uom, submitted_at, completed_at)
SELECT d.id, '10ad0000-0000-4000-8000-000000000001', u.id, d.created_at, d.created_at,
       'DOC-2026-' || lpad(d.n::text, 6, '0'),
       CASE WHEN d.status <> 'DRAFT'
            THEN 'LOAD' || d.type_no || '-2026-' || lpad(d.n::text, 6, '0') END,
       t.id, '10ad0000-0000-4000-8000-000000000003',
       'Load document ' || lpad(d.n::text, 6, '0'), d.status,
       (SELECT jsonb_object_agg('f' || lpad(f::text, 2, '0'), repeat(md5(d.n || '-' || f), 6))
        FROM generate_series(1, 10) AS f),
       (ARRAY['LOW', 'NORMAL', 'HIGH', 'URGENT'])[(d.n % 4) + 1],
       u.id, (d.n % 900 + 100) * 1000, 'IDR',
       CASE WHEN d.status <> 'DRAFT' THEN d.created_at + interval '1 hour' END,
       CASE WHEN d.status = 'COMPLETED' THEN d.created_at + interval '2 hours' END
FROM load_documents d
JOIN load_types t ON t.n = d.type_no
JOIN load_users u ON u.n = (d.n % 50) + 1;

-- ---------------------------------------------------------------------------
-- IN_REVIEW in a workflow-bound type: a live instance and an open task (AC3)
-- ---------------------------------------------------------------------------

CREATE TEMP TABLE load_live ON COMMIT DROP AS
SELECT row_number() OVER (ORDER BY n) AS seq, n, id AS document_id,
       gen_random_uuid() AS instance_id, gen_random_uuid() AS task_id, created_at
FROM load_documents
WHERE status = 'IN_REVIEW' AND type_no <= 2;

INSERT INTO workflow_instances (id, tenant_id, instance_ref, workflow_definition_id, document_id,
                                business_key, status, current_state, started_at)
SELECT l.instance_id, '10ad0000-0000-4000-8000-000000000001',
       'WFI-2026-' || lpad(l.seq::text, 6, '0'), '10ad0000-0000-4000-8000-000000000004',
       l.document_id, 'DOC-2026-' || lpad(l.n::text, 6, '0'), 'RUNNING', 'REVIEW',
       l.created_at + interval '1 hour'
FROM load_live l;

INSERT INTO workflow_tasks (id, tenant_id, task_ref, workflow_instance_id, document_id,
                            task_definition_key, task_name, status, candidate_role_id)
SELECT l.task_id, '10ad0000-0000-4000-8000-000000000001',
       'TASK-2026-' || lpad(l.seq::text, 6, '0'), l.instance_id, l.document_id, 'review',
       'Review the request', 'CREATED', '10ad0000-0000-4000-8000-000000000002'
FROM load_live l;

INSERT INTO workflow_history (id, tenant_id, workflow_instance_id, document_id, to_state, created_at)
SELECT gen_random_uuid(), '10ad0000-0000-4000-8000-000000000001', l.instance_id, l.document_id,
       'REVIEW', l.created_at + interval '1 hour'
FROM load_live l;

UPDATE documents d
SET process_instance_id = l.instance_id
FROM load_live l
WHERE d.id = l.document_id;

-- ---------------------------------------------------------------------------
-- The detail set: 500 typical documents (AC3)
-- ---------------------------------------------------------------------------
--
-- All 500 are IN_REVIEW in a bound type. AC4 wants every call in AC2 to answer
-- 2xx, and `/documents/{id}/workflow` answers 404 for a document no process
-- holds, so this is the one status where the whole set of AC2 calls is 2xx.
-- Their `seq` is spread over the 800 live documents, so the detail set is not
-- the 500 oldest.

CREATE TEMP TABLE load_detail ON COMMIT DROP AS
SELECT document_id, created_at
FROM (SELECT *, row_number() OVER (ORDER BY (seq * 509) % 800, seq) AS pick FROM load_live) AS s
WHERE pick <= 500;

-- Every document has its creation row; everything but a draft has the move to
-- where it stands. The detail set instead has the five-row path below.
INSERT INTO document_status_history (id, tenant_id, document_id, old_status, new_status, created_at)
SELECT gen_random_uuid(), '10ad0000-0000-4000-8000-000000000001', d.id, NULL, 'DRAFT', d.created_at
FROM load_documents d;

INSERT INTO document_status_history (id, tenant_id, document_id, old_status, new_status, created_at)
SELECT gen_random_uuid(), '10ad0000-0000-4000-8000-000000000001', d.id, 'DRAFT', d.status,
       d.created_at + interval '1 hour'
FROM load_documents d
WHERE d.status <> 'DRAFT'
  AND d.id NOT IN (SELECT document_id FROM load_detail);

INSERT INTO document_status_history (id, tenant_id, document_id, old_status, new_status, created_at)
SELECT gen_random_uuid(), '10ad0000-0000-4000-8000-000000000001', l.document_id, path.old_status,
       path.new_status, l.created_at + path.step * interval '20 minutes'
FROM load_detail l
CROSS JOIN (VALUES (1, 'DRAFT', 'SUBMITTED'), (2, 'SUBMITTED', 'RETURNED'),
                   (3, 'RETURNED', 'SUBMITTED'), (4, 'SUBMITTED', 'IN_REVIEW'))
           AS path (step, old_status, new_status);

INSERT INTO attachment_categories (id, tenant_id, category_code, name)
VALUES ('10ad0000-0000-4000-8000-000000000005', '10ad0000-0000-4000-8000-000000000001',
        'QUOTATION', 'Quotation');

-- Rows and no objects: the views list attachments and never download one, so
-- nothing reads `storage_reference`, and the scanner has nothing to do.
CREATE TEMP TABLE load_attachments ON COMMIT DROP AS
SELECT gen_random_uuid() AS id, l.document_id, l.created_at, f.no
FROM load_detail l
CROSS JOIN generate_series(1, 2) AS f (no);

INSERT INTO attachments (id, tenant_id, created_at, document_id, file_name, original_file_name,
                         mime_type, file_size, checksum, storage_reference, category_id,
                         virus_scan_status)
SELECT a.id, '10ad0000-0000-4000-8000-000000000001', a.created_at, a.document_id,
       a.id || '.pdf', 'quotation-' || a.no || '.pdf', 'application/pdf', 20480,
       md5(a.id::text), 'load/' || a.id, '10ad0000-0000-4000-8000-000000000005', 'CLEAN'
FROM load_attachments a;

INSERT INTO attachment_versions (id, tenant_id, attachment_id, version_number, storage_reference,
                                 file_size, checksum)
SELECT gen_random_uuid(), '10ad0000-0000-4000-8000-000000000001', a.id, 1, 'load/' || a.id, 20480,
       md5(a.id::text)
FROM load_attachments a;

INSERT INTO comments (id, tenant_id, created_at, document_id, body)
SELECT gen_random_uuid(), '10ad0000-0000-4000-8000-000000000001',
       l.created_at + c.no * interval '10 minutes', l.document_id,
       'Comment ' || c.no || ' on this request. ' || repeat(md5(l.document_id::text), 4)
FROM load_detail l
CROSS JOIN generate_series(1, 3) AS c (no);

COMMIT;

-- What a deployment's autovacuum would have done by the time anyone read this.
VACUUM ANALYZE;
