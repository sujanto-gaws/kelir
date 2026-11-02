// The seeder (#777 AC3, AC4): writes `seed.sql`, then counts what is there.
//
// `seedDataset` hashes one run-made password and hands it to psql, running
// inside the run's PostgreSQL container, so no client is installed on the host
// and the database is never reached from outside the compose project.
//
// `checkDataset` is the refusal: **every figure in `EXPECTED` is read back from
// the tables, and a difference stops the run**. The runner calls it after a
// seed and again before each measurement, so a data set changed between the
// two is refused too.

import { argon2Sync, randomBytes } from 'node:crypto'
import { readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const HERE = dirname(fileURLToPath(import.meta.url))

const TENANT = `(SELECT id FROM tenants WHERE tenant_code = 'TNT-LOAD')`

/** Document counts per status: AC3's percentages of 10,000. */
export const STATUS_COUNTS = {
  DRAFT: 3000,
  SUBMITTED: 1000,
  IN_REVIEW: 2000,
  APPROVED: 2000,
  REJECTED: 500,
  RETURNED: 500,
  CANCELLED: 500,
  COMPLETED: 500,
}

const DETAIL_SET = 500

/** Every count the data set is held to: a name, the figure and its statement. */
export const EXPECTED = [
  ['users in the tenant', 51, `SELECT count(*) FROM users WHERE tenant_id = ${TENANT}`],
  [
    'users holding the role',
    51,
    `SELECT count(DISTINCT user_id) FROM user_roles WHERE tenant_id = ${TENANT}`,
  ],
  [
    'permissions on the role',
    8,
    `SELECT count(*) FROM role_permissions WHERE tenant_id = ${TENANT}`,
  ],
  ['document types', 5, `SELECT count(*) FROM document_types WHERE tenant_id = ${TENANT}`],
  [
    'types bound to the workflow',
    2,
    `SELECT count(*) FROM document_type_workflows WHERE tenant_id = ${TENANT}`,
  ],
  ['documents', 10000, `SELECT count(*) FROM documents WHERE tenant_id = ${TENANT}`],
  [
    'documents in another tenant',
    0,
    `SELECT count(*) FROM documents WHERE tenant_id <> ${TENANT}`,
  ],
  [
    'smallest type',
    2000,
    `SELECT min(c) FROM (SELECT count(*) AS c FROM documents
       WHERE tenant_id = ${TENANT} GROUP BY document_type_id) AS t`,
  ],
  [
    'largest type',
    2000,
    `SELECT max(c) FROM (SELECT count(*) AS c FROM documents
       WHERE tenant_id = ${TENANT} GROUP BY document_type_id) AS t`,
  ],
  ...Object.entries(STATUS_COUNTS).map(([status, count]) => [
    `documents ${status}`,
    count,
    `SELECT count(*) FROM documents WHERE tenant_id = ${TENANT} AND status = '${status}'`,
  ]),
  [
    'IN_REVIEW and bound, with a live instance and an open task',
    800,
    `SELECT count(*) FROM documents d
       JOIN workflow_instances i ON i.id = d.process_instance_id AND i.status = 'RUNNING'
       JOIN workflow_tasks t ON t.workflow_instance_id = i.id AND t.status = 'CREATED'
     WHERE d.tenant_id = ${TENANT} AND d.status = 'IN_REVIEW'`,
  ],
  [
    'smallest form payload above 1,800 bytes',
    10000,
    `SELECT count(*) FROM documents WHERE tenant_id = ${TENANT}
       AND length(form_data_json::text) BETWEEN 1800 AND 2300`,
  ],
  [
    'detail set: 2 clean attachments, 3 comments, at least 4 history rows',
    DETAIL_SET,
    `SELECT count(*) FROM documents d WHERE d.tenant_id = ${TENANT}
       AND (SELECT count(*) FROM attachments a
            WHERE a.document_id = d.id AND a.virus_scan_status = 'CLEAN') = 2
       AND (SELECT count(*) FROM comments c WHERE c.document_id = d.id) = 3
       AND (SELECT count(*) FROM document_status_history h WHERE h.document_id = d.id) >= 4`,
  ],
  ['attachments', 2 * DETAIL_SET, `SELECT count(*) FROM attachments WHERE tenant_id = ${TENANT}`],
  ['comments', 3 * DETAIL_SET, `SELECT count(*) FROM comments WHERE tenant_id = ${TENANT}`],
  [
    'form fields, no lookup among them',
    10,
    `SELECT count(*) FROM rad_forms f,
       jsonb_array_elements(f.definition_json -> 'components') AS c
     WHERE f.tenant_id = ${TENANT} AND c ->> 'role' = 'data' AND c ->> 'type' <> 'lookup'`,
  ],
]

/** The one statement that counts everything, so the check is one round trip. */
function countingStatement() {
  const columns = EXPECTED.map(([, , statement], index) => `(${statement}) AS c${index}`)

  return `SELECT ${columns.join(',\n')};\n`
}

/** Argon2id at the backend's parameters (argon2 crate defaults), as a PHC string. */
export function hashPassword(password) {
  const nonce = randomBytes(16)
  const hash = argon2Sync('argon2id', {
    message: password,
    nonce,
    parallelism: 1,
    tagLength: 32,
    memory: 19456,
    passes: 2,
  })
  const encode = (buffer) => buffer.toString('base64').replace(/=+$/, '')

  return `$argon2id$v=19$m=19456,t=2,p=1$${encode(nonce)}$${encode(hash)}`
}

/** Compares the read figures with `EXPECTED`, and names every difference. */
export function compareCounts(values) {
  if (values.length !== EXPECTED.length) {
    return [`read ${values.length} counts, expected ${EXPECTED.length}`]
  }

  return EXPECTED.flatMap(([name, expected], index) =>
    Number(values[index]) === expected ? [] : [`${name}: ${values[index]}, expected ${expected}`],
  )
}

/** Reads the counts and throws, naming each one, when any differs. */
export function checkDataset(psql) {
  const line = psql(countingStatement()).trim()
  const differences = compareCounts(line.split('|'))

  if (differences.length > 0) {
    throw new Error(`refused: the data set is not #777 AC3:\n  ${differences.join('\n  ')}`)
  }

  return EXPECTED.length
}

/** Writes the data set, checks it and exports the detail set's ids. */
export function seedDataset(psql, password, runDirectory) {
  const sql = readFileSync(join(HERE, 'seed.sql'), 'utf8')

  psql(sql, ['-v', `password_hash=${hashPassword(password)}`])

  const checked = checkDataset(psql)
  const ids = psql(`SELECT d.id FROM documents d
                    WHERE EXISTS (SELECT 1 FROM attachments a WHERE a.document_id = d.id)
                    ORDER BY d.document_ref;\n`)
    .trim()
    .split(/\r?\n/)

  if (ids.length !== DETAIL_SET) {
    throw new Error(`refused: exported ${ids.length} detail ids, expected ${DETAIL_SET}`)
  }

  writeFileSync(join(runDirectory, 'detail-ids.json'), JSON.stringify(ids))

  return { checked, detailIds: ids.length }
}
