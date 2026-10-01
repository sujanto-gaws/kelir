import { expect, request } from '@playwright/test'

/**
 * The one system the stack can reach, as the harness sees it (#593).
 *
 * `deploy/staging/docker-compose.e2e.yml` starts a stand-in ERP beside the
 * release stack (`e2e/upstream/server.mjs`). The backend calls it across a
 * private compose network; the harness, which runs on the host, reads its
 * journal through a port published on loopback. Those are two addresses for
 * one server, and both are here so that no spec spells either.
 */

/**
 * The base URL an external system is registered with: the upstream **as the
 * backend reaches it**, by its compose service name. Nothing on the host can
 * open this, and nothing has to.
 */
export const UPSTREAM_BASE_URL = 'http://e2e-upstream:8080/api'

/**
 * The credential reference the flow registers.
 *
 * `SYSTEM` because the flows sign in as the bootstrap administrator, who is
 * in the system tenant, and a tenant reads only names under its own prefix
 * (#618). The overlay sets the variable of this name on the backend.
 */
export const UPSTREAM_SECRET_REFERENCE = 'env://KELIR_INTEGRATION_SECRET_SYSTEM__E2E_UPSTREAM_TOKEN'

/**
 * The secret's value, so a flow can check that no screen shows it.
 *
 * **No default**, for the reason `KELIR_E2E_PASSWORD` has none: a default
 * here would be a credential in the repository, and the stack would have to
 * be started with the same one. The stack that was brought up knows its own.
 */
export function upstreamToken(): string {
  const token = process.env.KELIR_E2E_UPSTREAM_TOKEN

  if (token === undefined || token === '') {
    throw new Error(
      'KELIR_E2E_UPSTREAM_TOKEN is not set. It is the token the stack was ' +
        'brought up with for the e2e upstream (docker-compose.e2e.yml); the ' +
        'flow needs it to check that no screen shows it.',
    )
  }

  return token
}

/** Where the harness reads the upstream's journal: the port the overlay publishes on loopback. */
export function upstreamJournalUrl(): string {
  const value = process.env.KELIR_E2E_UPSTREAM_URL

  return (value === undefined || value === '' ? 'http://127.0.0.1:8089' : value).replace(/\/+$/, '')
}

/** One call the upstream received, as it wrote it down. */
export interface UpstreamCall {
  readonly at: string
  readonly method: string
  readonly path: string
  readonly correlationId: string | null
  /** The `Authorization` header as it arrived, whole. */
  readonly authorization: string | null
  /** What the upstream answered. */
  readonly statusCode: number
}

/**
 * The calls the upstream received under one correlation id.
 *
 * **This reads the upstream, not Kelir.** The harness's rule is that
 * assertions are made in the browser, and it holds: what Kelir shows is
 * asserted there. What a screen cannot show is whether the call left the
 * backend at all, and with what — an answer could come from anywhere — so
 * that one fact is read from the system that received it.
 */
export async function upstreamCalls(correlationId: string): Promise<UpstreamCall[]> {
  const context = await request.newContext({ baseURL: upstreamJournalUrl() })

  try {
    const response = await context.get('/__journal', { params: { correlationId } })

    expect(
      response.ok(),
      `reading the upstream's journal at ${upstreamJournalUrl()} failed: ${response.status()}`,
    ).toBe(true)

    return ((await response.json()) as { entries: UpstreamCall[] }).entries
  } finally {
    await context.dispose()
  }
}

/**
 * A value of the upstream's own, which it answers under the key
 * `sessionToken`. Kelir masks a key of that name whatever it holds, so no
 * screen may show this. The same literal is in `e2e/upstream/server.mjs`,
 * which runs in a container and cannot import from here.
 */
export const UPSTREAM_OWN_SESSION_VALUE = 'upstream-session-4f1d9c'
