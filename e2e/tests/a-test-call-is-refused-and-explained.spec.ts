import { expect, test } from '@playwright/test'

import { runSuffix, signInOverApi, type ApiSession } from '../support/api'
import { API_PREFIX, credentials } from '../support/env'

/**
 * An administrator runs a test call on an endpoint from the external system's
 * page, and the refusal is explained with the integration log row it wrote
 * ([#547], FR-INT-002).
 *
 * # Why a refusal, and not an answer
 *
 * **An answer needs a system to answer**, reachable from the backend container
 * through the egress guard, with a secret in the backend's environment. The
 * release stack has none of the three, and a spec that brought them would test
 * the stack's plumbing more than the screen. A system with **no credential**
 * is refused with `NO_USABLE_CREDENTIAL` after the endpoint is found and
 * before any secret is read or any address resolved, so the refusal is
 * deterministic on any stack and still writes its log row. The answered path
 * is covered by the backend's integration tests and the dialog's own, **and
 * since #593 by a flow of its own**,
 * `a-test-call-is-answered-and-its-secret-is-masked.spec.ts`, on a stack that
 * brings the three with an overlay. This flow stays a refusal: it is the one
 * that runs on a stack with nothing to call.
 *
 * # What it proves that the component tests cannot
 *
 * That migration `0049` grants `integration:endpoint:call` to `ROLE-ADMIN`, so
 * the button appears; that the route, the proxy and the dialog agree on the
 * path and the 422; and that the log id on the screen is the one the server's
 * message named.
 *
 * #521's rule holds: the system and endpoint codes carry `runSuffix()`, and the
 * endpoint row is reached by its test id, not by position.
 *
 * [#547]: https://github.com/sujanto-gaws/kelir/issues/547
 */

const SYSTEMS_PATH = `${API_PREFIX}/integration/external-systems`

let session: ApiSession
let systemId: string
let endpointId: string
let endpointCode: string

test.beforeAll(async () => {
  session = await signInOverApi()
  const suffix = runSuffix()
  endpointCode = `GET_PO_${suffix}`

  const system = await session.context.post(SYSTEMS_PATH, {
    data: {
      systemCode: `E2E_CALL_${suffix}`,
      systemName: `Test-called ERP ${suffix}`,
      systemType: 'ERP',
      baseUrl: 'https://erp.example.com/api',
      authType: 'BEARER_TOKEN',
    },
  })
  expect(system.ok(), `seeding the system failed: ${system.status()} ${await system.text()}`).toBe(
    true,
  )
  systemId = ((await system.json()) as { data: { id: string } }).data.id

  const endpoint = await session.context.post(`${SYSTEMS_PATH}/${systemId}/endpoints`, {
    data: { endpointCode, name: 'Read a purchase order', method: 'GET', path: '/purchase-orders' },
  })
  expect(
    endpoint.ok(),
    `seeding the endpoint failed: ${endpoint.status()} ${await endpoint.text()}`,
  ).toBe(true)
  endpointId = ((await endpoint.json()) as { data: { id: string } }).data.id
})

test.afterAll(async () => {
  await session?.context.dispose()
})

test('a test call on a system with no credential is refused, explained and logged', async ({
  page,
}) => {
  const { username, password } = credentials()

  await page.goto('/login')
  await page.getByLabel('Username or email').fill(username)
  await page.getByLabel('Password').fill(password)
  await page.getByRole('button', { name: 'Sign in' }).click()
  await expect(page.getByRole('navigation', { name: 'Main navigation' })).toBeVisible()

  await page.goto(`/admin/external-systems/${systemId}`)
  await page.getByTestId(`test-call-endpoint-${endpointCode}`).click()

  // The confirmation comes first, and nothing is sent until it is accepted.
  const dialog = page.getByTestId('test-call-dialog')
  await expect(dialog).toHaveAttribute('data-phase', 'confirm')

  const calling = page.waitForResponse(
    (response) =>
      response.request().method() === 'POST' &&
      new URL(response.url()).pathname ===
        `${SYSTEMS_PATH}/${systemId}/endpoints/${endpointId}/test-call`,
  )
  await page.getByTestId('test-call-run').click()

  const called = await calling
  expect(called.status(), await called.text()).toBe(422)
  const body = (await called.json()) as { error: { code: string; message: string } }
  expect(body.error.code).toBe('NO_USABLE_CREDENTIAL')
  const logId = /integration log ([0-9a-f-]{36})/.exec(body.error.message)?.[1]
  expect(logId, body.error.message).toBeDefined()

  await expect(dialog).toHaveAttribute('data-phase', 'failed')
  await expect(page.getByTestId('test-call-failure-title')).toHaveText(
    'Refused before anything was sent',
  )
  await expect(page.getByTestId('test-call-failure-code')).toHaveText('NO_USABLE_CREDENTIAL')
  await expect(page.getByTestId('test-call-explanation')).toContainText('active and valid today')
  await expect(page.getByTestId('test-call-log-id')).toHaveText(logId ?? '')
})
