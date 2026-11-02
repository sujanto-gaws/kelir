// What a list view and a detail view send (#777 AC1, AC2), shared by the load
// scenario and the smoke scenario so the two cannot disagree about either.
//
// **The request lists follow the pages at `8478faa`, and one differs from AC2.**
// AC2 says the page mounts its tabs, which fetch at once. It does not:
// `DocumentWorkspace.vue` mounts each tab with a `v-if` on the open tab, so a
// tab's calls go only when a person opens that tab. That was already so when
// AC2 was written. `openDetail` therefore sends what the page sends, and
// `openTabs` sends the calls AC2 lists, for the run that asks for them
// (`KELIR_LOAD_TABS=1`).

import http from 'k6/http'

/** Bodies are read as text; a body is only ever measured or parsed. */
const HEADERS = { Accept: 'application/json', 'Accept-Encoding': 'zstd, gzip' }

// A 3xx is not a pass here, and the default callback would count it as one.
http.setResponseCallback(http.expectedStatuses({ min: 200, max: 299 }))

export function apiUrl(path) {
  const base = (__ENV.KELIR_LOAD_BASE_URL || '').replace(/\/+$/, '')

  if (!base) {
    throw new Error('KELIR_LOAD_BASE_URL is not set')
  }

  return `${base}/api/v1${path}`
}

/** A session: one user's token, and what its requests answered. */
export function signIn(username, password) {
  const response = http.post(apiUrl('/auth/login'), JSON.stringify({ username, password }), {
    headers: { ...HEADERS, 'Content-Type': 'application/json' },
    tags: { name: 'sign-in' },
  })

  return {
    status: response.status,
    ms: response.timings.duration,
    token: response.status === 200 ? response.json('data.accessToken') : null,
    statuses: { [response.status]: 1 },
    refused: [],
  }
}

function request(session, path, name) {
  return [
    'GET',
    apiUrl(path),
    null,
    {
      headers: { ...HEADERS, Authorization: `Bearer ${session.token}` },
      tags: { name },
    },
  ]
}

function note(session, responses, names) {
  responses.forEach((response, index) => {
    const status = response.status

    session.statuses[status] = (session.statuses[status] || 0) + 1

    if (status < 200 || status > 299) {
      session.refused.push({ name: names[index], status, at: Date.now() })
    }
  })
}

/** Sends the named requests at once and returns their responses, in order. */
function send(session, ...requests) {
  const names = requests.map(([, name]) => name)
  const responses = http.batch(requests.map(([path, name]) => request(session, path, name)))

  note(session, responses, names)

  return responses
}

const bytesOf = (responses) =>
  responses.reduce((total, response) => total + (response.body ? response.body.length : 0), 0)

/** What the shell sends once, at sign-in (AC1): never part of a view. */
export function loadShell(session) {
  send(session, ['/auth/me', 'me'], ['/rad/menus', 'menus'])
}

/**
 * A list view (AC1): both calls leave together, because `AppLayout.vue` watches
 * the route and `useListRoute` loads from it on the same change.
 */
export function openList(session, query = '') {
  const started = Date.now()
  const responses = send(
    session,
    [`/documents?page=1&pageSize=20${query}`, query ? 'list-filtered' : 'list'],
    ['/notifications/unread-count', 'unread-count'],
  )
  const rows = responses[0].status === 200 ? responses[0].json('data') : []

  return {
    started,
    ms: Date.now() - started,
    bytes: bytesOf(responses),
    rows,
    ok: responses.every((response) => response.status === 200),
  }
}

/**
 * A detail view (AC2).
 *
 * The content set is the three sequential calls: `document-form` shows when the
 * last of them has answered. `DocumentHeader.vue` then fetches the linked
 * entity, when there is one, which is what `settled` adds. No seeded document
 * has an entity (AC3 asks for none), so here the two times are the same unless
 * the tabs are sent.
 */
export function openDetail(session, id, withTabs) {
  const started = Date.now()
  const responses = send(
    session,
    [`/documents/${id}`, 'document'],
    ['/notifications/unread-count', 'unread-count'],
  )
  const document = responses[0].status === 200 ? responses[0].json('data') : null

  if (document && document.formId) {
    responses.push(...send(session, [`/rad/forms/${document.formId}`, 'form']))
  }

  responses.push(...send(session, [`/documents/${id}/status-history`, 'status-history']))

  const ms = Date.now() - started
  const contentBytes = bytesOf(responses)

  if (document && document.entityId) {
    responses.push(...send(session, [`/documents/${id}/linked-entity`, 'linked-entity']))
  }

  if (withTabs) {
    responses.push(...openTabs(session, id))
  }

  return {
    started,
    ms,
    settledMs: Date.now() - started,
    bytes: contentBytes,
    settledBytes: bytesOf(responses),
    document,
    ok: responses.every((response) => response.status === 200),
  }
}

/** The calls AC2 lists for the tabs: what the page sends when each is opened. */
export function openTabs(session, id) {
  const first = send(
    session,
    [`/documents/${id}/workflow`, 'tab-workflow'],
    [`/documents/${id}/attachments?page=1`, 'tab-attachments'],
    [`/documents/${id}/references?page=1`, 'tab-references'],
    ['/attachment-categories', 'tab-categories'],
    [`/documents/${id}/comments?page=1`, 'tab-comments'],
    [`/documents/${id}/activity?page=1&pageSize=20`, 'tab-activity'],
  )
  const history = send(session, [
    `/documents/${id}/workflow/history?page=1&pageSize=20`,
    'tab-workflow-history',
  ])

  return [...first, ...history]
}
