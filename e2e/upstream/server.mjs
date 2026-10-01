// The one system the browser flows' stack can reach (#593).
//
// A test call that is *answered* needs something to answer it. This is that
// something: a stand-in for an ERP, started by `deploy/staging/docker-compose.e2e.yml`
// on a network only the backend shares, and nothing a deployment runs.
//
// It does three things, and each is there because the flow asserts on it:
//
//   1. **It answers only a call that carries the credential.** Any request
//      without `Authorization: Bearer <UPSTREAM_EXPECTED_TOKEN>` is a 401, so
//      a `200` in the browser means the backend resolved the tenant's secret
//      and sent it.
//   2. **It echoes what it was sent**, as many test endpoints do: the header
//      under a key that is not itself sensitive, and the token again in
//      base64. Kelir must mask both before a person sees them, and the flow
//      checks that it did. `sessionToken` is the other half of the rule, a
//      value of the system's own under a key Kelir masks by name.
//   3. **It writes down every call it receives**, readable at `/__journal`, so
//      the flow can check that the call it watched in the browser is the call
//      that arrived, with the credential, and not an answer from somewhere else.
//
// No dependencies, on purpose: it runs from a bind mount in a stock Node image,
// so the stack gains no image to build and nothing to install.

import { createServer } from 'node:http'

const PORT = Number(process.env.UPSTREAM_PORT ?? 8080)
const EXPECTED_TOKEN = process.env.UPSTREAM_EXPECTED_TOKEN ?? ''

/** How many calls are kept. A stack left up for days does not grow without end. */
const JOURNAL_LIMIT = 500

/** The value under `sessionToken`: the system's own, and nothing of Kelir's. */
const OWN_SESSION_VALUE = 'upstream-session-4f1d9c'

if (EXPECTED_TOKEN === '') {
  // Refusing to start is the point: with no token to compare, every call would
  // be a 401 and the flow would fail one step later, naming the wrong thing.
  console.error('UPSTREAM_EXPECTED_TOKEN is not set; the upstream would answer nothing')
  process.exit(1)
}

/** @type {Array<{at: string, method: string, path: string, correlationId: string | null, authorization: string | null, statusCode: number}>} */
const journal = []

function send(response, statusCode, body) {
  const text = JSON.stringify(body)

  response.writeHead(statusCode, {
    'Content-Type': 'application/json; charset=utf-8',
    'Content-Length': Buffer.byteLength(text),
    'Cache-Control': 'no-store',
  })
  response.end(text)
}

function header(request, name) {
  const value = request.headers[name]

  if (value === undefined) {
    return null
  }

  return Array.isArray(value) ? value.join(', ') : value
}

/** What the stand-in ERP says to one call, given what it carried. */
function answer(method, path, authorization) {
  if (authorization !== `Bearer ${EXPECTED_TOKEN}`) {
    return {
      statusCode: 401,
      body: {
        error: 'UNAUTHORIZED',
        message: 'The bearer token is missing or is not the one expected',
      },
    }
  }

  if (method === 'GET' && path === '/api/purchase-orders') {
    const token = authorization.slice('Bearer '.length)

    return {
      statusCode: 200,
      body: {
        data: [{ number: 'PO-2026-000123', supplier: 'PT Sumber Makmur', total: 1250000 }],
        servedBy: 'kelir-e2e-upstream',
        echo: authorization,
        echoBase64: Buffer.from(token, 'utf8').toString('base64'),
        sessionToken: OWN_SESSION_VALUE,
      },
    }
  }

  return {
    statusCode: 404,
    body: { error: 'NOT_FOUND', message: `Nothing is served at ${method} ${path}` },
  }
}

const server = createServer((request, response) => {
  const url = new URL(request.url ?? '/', 'http://upstream.invalid')
  const method = request.method ?? 'GET'

  // The two paths that are about the stand-in itself. Neither is written down:
  // the journal is of calls a system would have received.
  if (url.pathname === '/__health') {
    send(response, 200, { status: 'ok' })
    return
  }

  if (url.pathname === '/__journal') {
    const wanted = url.searchParams.get('correlationId')
    const entries =
      wanted === null ? journal : journal.filter((entry) => entry.correlationId === wanted)

    send(response, 200, { entries })
    return
  }

  // Drain the body, so a POST with one is answered rather than reset.
  request.resume()

  const authorization = header(request, 'authorization')
  const { statusCode, body } = answer(method, url.pathname, authorization)

  journal.push({
    at: new Date().toISOString(),
    method,
    path: url.pathname,
    correlationId: header(request, 'x-correlation-id'),
    authorization,
    statusCode,
  })
  if (journal.length > JOURNAL_LIMIT) {
    journal.splice(0, journal.length - JOURNAL_LIMIT)
  }

  send(response, statusCode, body)
})

server.listen(PORT, '0.0.0.0', () => {
  console.log(`the e2e upstream is listening on ${PORT}`)
})

// `docker compose down` sends SIGTERM, and PID 1 ignores it unless it listens.
for (const signal of ['SIGTERM', 'SIGINT']) {
  process.on(signal, () => {
    server.close(() => process.exit(0))
  })
}
