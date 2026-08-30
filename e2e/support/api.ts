import { expect, request, type APIRequestContext } from '@playwright/test'

import { API_PREFIX, baseUrl, credentials } from './env'

/**
 * The API, used for arranging — never for asserting.
 *
 * A browser test that sets its own scene through the UI spends most of its
 * runtime re-proving screens other specs already cover, and fails in the setup
 * when the thing under test is fine. So the fixtures below create their rows
 * over HTTP, against the same deployment the browser then drives: still the
 * real application, still the real permission checks, just not through a form.
 *
 * What is deliberately *not* here is any helper that reads a list and returns
 * rows. The assertion belongs in the browser — checking the API answered
 * correctly would pass on a screen that renders nothing.
 */

/** A signed-in API caller and the token behind it. */
export interface ApiSession {
  readonly context: APIRequestContext
  readonly accessToken: string
}

/**
 * Signs in over the API and returns a context that carries the bearer token.
 *
 * Rate limiting on `/auth/login` counts failures rather than successes
 * (`middleware/rate_limit.rs`), so a suite signing in once per file is in no
 * danger of tripping it — but a suite that signs in wrongly is, which is worth
 * knowing when a 429 appears here.
 */
export async function signInOverApi(as?: {
  username: string
  password: string
}): Promise<ApiSession> {
  // **The administrator unless somebody else is named.** #241's delegation flow
  // needs a session as the delegator, because a window is opened in the
  // caller's own name and carries no `delegatorUserId` — an administrator
  // cannot open one on somebody's behalf, deliberately (#184).
  const { username, password } = as ?? credentials()
  const anonymous = await request.newContext({ baseURL: baseUrl() })

  const response = await anonymous.post(`${API_PREFIX}/auth/login`, {
    data: { username, password },
  })

  expect(
    response.ok(),
    `sign-in failed for "${username}": ${response.status()} ${await response.text()}`,
  ).toBe(true)

  const body = (await response.json()) as { data: { accessToken: string } }
  await anonymous.dispose()

  const context = await request.newContext({
    baseURL: baseUrl(),
    extraHTTPHeaders: { Authorization: `Bearer ${body.data.accessToken}` },
  })

  return { context, accessToken: body.data.accessToken }
}

/** A party the suite created, as the list screen will show it. */
export interface SeededSupplier {
  /** `partyId` — the business code, shown in the Code column. */
  readonly code: string
  /** The group name, shown in the Name column and matched by search. */
  readonly name: string
  /** The supplier number, shown in the Supplier no. column. */
  readonly supplierNumber: string
}

/**
 * Creates a party group and gives it the SUPPLIER role.
 *
 * Two calls rather than one because that is the API: a party exists first and
 * holds roles afterwards, which is the party model the master-data
 * architecture adopted.
 *
 * The supplier number is supplied rather than generated: the profile requires
 * one, and it is unique per tenant — a deleted party keeps its number reserved
 * (#96), so a fixed value works once and conflicts on every later run.
 */
export async function createSupplier(
  session: ApiSession,
  { code, name, supplierNumber }: SeededSupplier,
): Promise<void> {
  const created = await session.context.post(`${API_PREFIX}/master-data/parties`, {
    data: {
      partyId: code,
      partyTypeId: 'PARTY_GROUP',
      partyGroup: { groupName: name },
    },
  })

  expect(
    created.ok(),
    `creating party ${code} failed: ${created.status()} ${await created.text()}`,
  ).toBe(true)

  const body = (await created.json()) as { data: { id: string } }

  const assigned = await session.context.put(
    `${API_PREFIX}/master-data/parties/${body.data.id}/roles/SUPPLIER`,
    {
      data: {
        fromDate: new Date().toISOString(),
        profile: { supplier: { supplierNumber } },
      },
    },
  )

  expect(
    assigned.ok(),
    `assigning SUPPLIER to ${code} failed: ${assigned.status()} ${await assigned.text()}`,
  ).toBe(true)
}

/**
 * A suffix unique to this run.
 *
 * The deployment the harness drives keeps its database between runs, so a fixed
 * code collides with itself the second time and the suite fails on a
 * conflict it did not mean to test. Timestamp plus randomness rather than
 * either alone: two runs can start in the same millisecond in CI.
 */
export function runSuffix(): string {
  const stamp = Date.now().toString(36).toUpperCase()
  const noise = Math.random().toString(36).slice(2, 6).toUpperCase()

  return `${stamp}${noise}`
}
