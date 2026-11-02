// The measured scenario (#777 AC4): 50 signed-in users, each in a closed loop.
//
// One k6 iteration is one user's whole life: sign in once during the ramp, then
// loop until the window ends. Nothing here signs in twice, and the 15-minute
// token outlives the run, so the scenario never refreshes.
//
// **Samples leave as console lines**, one per view, because AC6's P95 is
// nearest-rank over the raw samples and k6's own Trend interpolates. `report.mjs`
// reads them. Prefixes: `S` a sample, `L` a sign-in, `E` a non-2xx, `T` a user's
// count of statuses.

import { sleep } from 'k6'
import exec from 'k6/execution'
import { SharedArray } from 'k6/data'

import { loadShell, openDetail, openList, signIn } from './views.js'

const USERS = Number(__ENV.KELIR_LOAD_USERS || 50)
const PASSWORD = __ENV.KELIR_LOAD_PASSWORD
/** When the first user signs in, epoch milliseconds on this generator's clock. */
const START = Number(__ENV.KELIR_LOAD_START_MS)
const RAMP_MS = 30_000
/** When the window opens, after the ramp and a few seconds for the loops to start. */
const WINDOW_START = START + RAMP_MS + 5_000
const WINDOW_END = WINDOW_START + Number(__ENV.KELIR_LOAD_WINDOW_MS || 300_000)
/** Zero is the saturation run. Anything else is AC4's uniform 1 to 3 seconds. */
const THINK = __ENV.KELIR_LOAD_THINK !== '0'
const WITH_TABS = __ENV.KELIR_LOAD_TABS === '1'

const detailIds = new SharedArray('detail set', () =>
  JSON.parse(open(__ENV.KELIR_LOAD_DETAIL_IDS)),
)

// A threshold nobody can fail: it is what makes k6 keep a per-request figure in
// its summary, for telling which call a slow view spent its time on.
const NAMES = ['sign-in', 'list', 'list-filtered', 'unread-count', 'document', 'form']
const MORE_NAMES = ['status-history', 'tab-workflow', 'tab-attachments', 'tab-activity']

export const options = {
  scenarios: {
    users: {
      executor: 'per-vu-iterations',
      vus: USERS,
      iterations: 1,
      maxDuration: `${Math.ceil((WINDOW_END - Date.now()) / 1000) + 60}s`,
      gracefulStop: '30s',
    },
  },
  summaryTrendStats: ['count', 'med', 'p(95)', 'max'],
  thresholds: Object.fromEntries(
    [...NAMES, ...MORE_NAMES].map((name) => [
      `http_req_duration{name:${name}}`,
      ['max>=0'],
    ]),
  ),
}

function waitUntil(moment) {
  const seconds = (moment - Date.now()) / 1000

  if (seconds > 0) {
    sleep(seconds)
  }
}

const inWindow = (started) => started >= WINDOW_START && started < WINDOW_END

function flushRefusals(session) {
  for (const { name, status, at } of session.refused) {
    console.log(`E,${name},${status},${at},${exec.vu.idInTest}`)
  }

  session.refused = []
}

export default function () {
  const vu = exec.vu.idInTest

  waitUntil(START + ((vu - 1) * RAMP_MS) / USERS)

  const signedAt = Date.now()
  const username = `load-user-${String(vu).padStart(3, '0')}`
  const session = signIn(username, PASSWORD)

  console.log(`L,${vu},${signedAt},${Math.round(session.ms)},${session.status}`)

  if (!session.token) {
    throw new Error(`${username} could not sign in: ${session.status}`)
  }

  loadShell(session)
  flushRefusals(session)

  waitUntil(START + RAMP_MS + Math.random() * 3_000)

  // Where this user stands in the one-in-five, staggered so the users' filtered
  // lists do not arrive together.
  let loop = vu

  while (Date.now() < WINDOW_END) {
    loop += 1

    const list = openList(session)

    if (inWindow(list.started)) {
      console.log(`S,list,${list.started},${list.ms},${list.bytes}`)
    }

    const id = detailIds[Math.floor(Math.random() * detailIds.length)]
    const detail = openDetail(session, id, WITH_TABS)

    if (inWindow(detail.started)) {
      console.log(`S,detail,${detail.started},${detail.ms},${detail.bytes}`)
      console.log(`S,detail_settled,${detail.started},${detail.settledMs},${detail.settledBytes}`)
    }

    // Background load, recorded apart (AC1). The status filter is the page's own.
    if (loop % 5 === 0) {
      const filtered = openList(session, '&status=IN_REVIEW')

      if (inWindow(filtered.started)) {
        console.log(`S,list_filtered,${filtered.started},${filtered.ms},${filtered.bytes}`)
      }
    }

    flushRefusals(session)

    if (THINK) {
      sleep(1 + Math.random() * 2)
    }
  }

  const statuses = Object.entries(session.statuses)
    .map(([status, count]) => `${status}:${count}`)
    .join(' ')

  console.log(`T,${vu},${statuses}`)
}
