// The smoke scenario (#777 AC10), which #780 runs against two backends.
//
// Two users, one list and one detail each. **It exits non-zero on any non-2xx**,
// through the thresholds below: k6 exits 99 when one fails, and a sign-in that
// fails ends that user with nothing further to send, which `checks` then fails.
//
// It needs no detail-id file. The document comes from the list the user just
// opened, so this runs against any stack holding the seeded users.

import { check, fail } from 'k6'
import exec from 'k6/execution'

import { loadShell, openDetail, openList, signIn } from './views.js'

const PREFIX = __ENV.KELIR_LOAD_USER_PREFIX || 'load-user-'

export const options = {
  scenarios: {
    smoke: { executor: 'per-vu-iterations', vus: 2, iterations: 1, maxDuration: '60s' },
  },
  thresholds: {
    checks: ['rate==1'],
    http_req_failed: ['rate==0'],
  },
}

export default function () {
  const vu = exec.vu.idInTest
  const username = `${PREFIX}${String(vu).padStart(3, '0')}`
  const session = signIn(username, __ENV.KELIR_LOAD_PASSWORD)

  if (!check(session, { 'signed in': (candidate) => candidate.token !== null })) {
    fail(`${username}: sign-in answered ${session.status}`)
  }

  loadShell(session)

  const list = openList(session)

  check(list, {
    'list answered 2xx': (view) => view.ok,
    'list has rows': (view) => view.rows.length > 0,
  })

  if (list.rows.length === 0) {
    fail(`${username}: the list has no rows to open`)
  }

  const detail = openDetail(session, list.rows[(vu - 1) % list.rows.length].id, false)

  check(detail, { 'detail answered 2xx': (view) => view.ok })

  console.log(`${username}: list ${list.ms} ms, detail ${detail.ms} ms`)

  for (const { name, status } of session.refused) {
    console.error(`${username}: ${name} answered ${status}`)
  }
}
