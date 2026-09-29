import { expect, test, type APIResponse } from '@playwright/test'

import { runSuffix, signInOverApi, type ApiSession } from '../support/api'
import { API_PREFIX, credentials } from '../support/env'

/**
 * An administrator reads the integration log: a test call's log id opens its
 * row, the log is filtered by system, and another row's detail is read
 * ([#548], FR-INT-006, AC6).
 *
 * # Two failures, and no success
 *
 * **A success needs a system that answers**, reachable from the backend
 * container through the egress guard, with a secret in the backend's
 * environment under `KELIR_INTEGRATION_SECRET_`. The release stack has none
 * of the three (`deploy/staging` sets no `KELIR_INTEGRATION_*` variable, so a
 * private or loopback upstream is refused and there is no secret to send), and
 * a public upstream would make the flow depend on the internet. The same
 * reasoning as #547's flow, `a-test-call-is-refused-and-explained.spec.ts`.
 *
 * So the two rows are **two different refusals**, each deterministic on any
 * stack and each logged `FAILED`:
 *
 * - system A has **no credential**: `NO_USABLE_CREDENTIAL`;
 * - system B has a **`vault://` credential**: `SECRET_BACKEND_NOT_CONFIGURED`,
 *   refused after the URL was built and the credential chosen, so its row
 *   carries both — and the request payload shows the credential's type while
 *   never the reference.
 *
 * A success row is covered by the backend's integration tests and the page's
 * own component tests.
 *
 * # What it proves that the component tests cannot
 *
 * That migration `0050` grants `integration:log:read` to `ROLE-ADMIN`, so the
 * navigation entry is drawn and the route opens; that the list and detail
 * routes, the proxy and the page agree on paths, filters and shapes; that the
 * test call dialog's log id opens the row it wrote; and that filtering by
 * system is done by the server — B's row is not on A's page.
 *
 * #521's rule holds: codes carry `runSuffix()`, the chooser is driven by the
 * suffixed label, and rows are reached by their log ids after a system filter
 * that only this run's rows match.
 *
 * [#548]: https://github.com/sujanto-gaws/kelir/issues/548
 */

const SYSTEMS_PATH = `${API_PREFIX}/integration/external-systems`
const LOGS_PATH = `${API_PREFIX}/integration/logs`

interface SeededSystem {
  id: string
  code: string
  name: string
  endpointId: string
  endpointCode: string
}

let session: ApiSession
let systemA: SeededSystem
let systemB: SeededSystem
let logB: string

async function ok(response: APIResponse, what: string): Promise<{ data: { id: string } }> {
  expect(response.ok(), `${what} failed: ${response.status()} ${await response.text()}`).toBe(true)

  return (await response.json()) as { data: { id: string } }
}

async function seedSystem(letter: string, suffix: string): Promise<SeededSystem> {
  const code = `E2E_LOG_${letter}_${suffix}`
  const name = `Logged ERP ${letter} ${suffix}`
  const endpointCode = `GET_PO_${letter}_${suffix}`

  const system = await ok(
    await session.context.post(SYSTEMS_PATH, {
      data: {
        systemCode: code,
        systemName: name,
        systemType: 'ERP',
        baseUrl: 'https://erp.example.com/api',
        authType: 'BEARER_TOKEN',
      },
    }),
    `seeding system ${letter}`,
  )

  const endpoint = await ok(
    await session.context.post(`${SYSTEMS_PATH}/${system.data.id}/endpoints`, {
      data: { endpointCode, name: 'Read purchase orders', method: 'GET', path: '/purchase-orders' },
    }),
    `seeding the endpoint of system ${letter}`,
  )

  return { id: system.data.id, code, name, endpointId: endpoint.data.id, endpointCode }
}

/** The log row id a refused test call's message names. */
function logIdFrom(message: string): string {
  const id = /integration log ([0-9a-f-]{36})/.exec(message)?.[1]
  expect(id, message).toBeDefined()

  return id ?? ''
}

test.beforeAll(async () => {
  session = await signInOverApi()
  const suffix = runSuffix()

  systemA = await seedSystem('A', suffix)
  systemB = await seedSystem('B', suffix)

  await ok(
    await session.context.post(`${SYSTEMS_PATH}/${systemB.id}/credentials`, {
      data: {
        credentialType: 'BEARER_TOKEN',
        secretReference: `vault://kelir/e2e-log-${suffix.toLowerCase()}/token`,
      },
    }),
    'seeding the credential of system B',
  )

  // B's call is arranged over the API; A's is run in the browser below.
  const called = await session.context.post(
    `${SYSTEMS_PATH}/${systemB.id}/endpoints/${systemB.endpointId}/test-call`,
  )
  expect(called.status(), await called.text()).toBe(422)
  const body = (await called.json()) as { error: { code: string; message: string } }
  expect(body.error.code).toBe('SECRET_BACKEND_NOT_CONFIGURED')
  logB = logIdFrom(body.error.message)
})

test.afterAll(async () => {
  await session?.context.dispose()
})

test('an administrator opens a test call log row, filters the log by system and reads a row', async ({
  page,
}) => {
  const { username, password } = credentials()

  await page.goto('/login')
  await page.getByLabel('Username or email').fill(username)
  await page.getByLabel('Password').fill(password)
  await page.getByRole('button', { name: 'Sign in' }).click()
  const navigation = page.getByRole('navigation', { name: 'Main navigation' })
  await expect(navigation).toBeVisible()

  // --- The test call's log id opens its row ----------------------------------
  await page.goto(`/admin/external-systems/${systemA.id}`)
  await page.getByTestId(`test-call-endpoint-${systemA.endpointCode}`).click()

  const calling = page.waitForResponse(
    (response) =>
      response.request().method() === 'POST' &&
      new URL(response.url()).pathname ===
        `${SYSTEMS_PATH}/${systemA.id}/endpoints/${systemA.endpointId}/test-call`,
  )
  await page.getByTestId('test-call-run').click()
  const called = await calling
  expect(called.status(), await called.text()).toBe(422)
  const logA = logIdFrom(((await called.json()) as { error: { message: string } }).error.message)

  await expect(page.getByTestId('test-call-log-id')).toHaveText(logA)
  await page.getByTestId('test-call-log-link').click()

  await expect(page).toHaveURL(new RegExp(`/admin/integration-logs\\?log=${logA}$`))
  const detail = page.getByTestId('integration-log-detail')
  await expect(detail.getByTestId('integration-log-detail-id')).toHaveText(logA)
  await expect(detail.getByTestId('integration-log-detail-status')).toHaveText('Failed')
  await expect(detail.getByTestId('integration-log-detail-error-message')).toContainText(
    'NO_USABLE_CREDENTIAL',
  )
  await page.getByTestId('integration-log-detail-close').click()
  await expect(detail).toHaveCount(0)

  // --- The navigation entry is there, and opens the page ---------------------
  await navigation.getByRole('link', { name: 'Integration Logs' }).click()
  await expect(page).toHaveURL(/\/admin\/integration-logs$/)

  // --- Filtered by system A: A's row, and not B's ----------------------------
  const filteringA = page.waitForResponse(
    (response) =>
      new URL(response.url()).pathname === LOGS_PATH &&
      new URL(response.url()).searchParams.get('externalSystemId') === systemA.id,
  )
  await page
    .getByTestId('integration-logs-system')
    .selectOption({ label: `${systemA.code} · ${systemA.name}` })
  expect((await filteringA).status()).toBe(200)

  const rowA = page.getByTestId(`integration-log-row-${logA}`)
  await expect(rowA).toBeVisible()
  await expect(rowA.getByTestId('integration-log-status')).toHaveText('Failed')
  await expect(rowA.getByTestId('integration-log-error-message')).toContainText(
    'NO_USABLE_CREDENTIAL',
  )
  await expect(page.getByTestId(`integration-log-row-${logB}`)).toHaveCount(0)

  // --- Filtered by system B, and its row read in full ------------------------
  await page
    .getByTestId('integration-logs-system')
    .selectOption({ label: `${systemB.code} · ${systemB.name}` })

  const rowB = page.getByTestId(`integration-log-row-${logB}`)
  await expect(rowB).toBeVisible()
  await expect(page.getByTestId(`integration-log-row-${logA}`)).toHaveCount(0)
  await expect(rowB).toContainText('https://erp.example.com/api/purchase-orders')

  const reading = page.waitForResponse(
    (response) => new URL(response.url()).pathname === `${LOGS_PATH}/${logB}`,
  )
  await page.getByTestId(`integration-log-open-${logB}`).click()
  expect((await reading).status()).toBe(200)

  await expect(page).toHaveURL(new RegExp(`externalSystemId=${systemB.id}.*log=${logB}`))
  await expect(detail.getByTestId('integration-log-detail-id')).toHaveText(logB)
  await expect(detail.getByTestId('integration-log-detail-status')).toHaveText('Failed')
  await expect(detail.getByTestId('integration-log-detail-error-message')).toContainText(
    'SECRET_BACKEND_NOT_CONFIGURED',
  )
  await expect(detail.getByTestId('integration-log-detail-request')).toHaveText(
    'GET https://erp.example.com/api/purchase-orders',
  )
  await expect(detail.getByTestId('integration-log-detail-system-link')).toHaveAttribute(
    'href',
    `/admin/external-systems/${systemB.id}`,
  )

  // The request payload as stored: the credential's type, and never its reference.
  const requestPayload = detail.getByTestId('integration-log-detail-request-payload')
  await expect(requestPayload).toContainText('"credentialType": "BEARER_TOKEN"')
  await expect(requestPayload).not.toContainText('vault://')
  // Refused before anything was sent, so nothing came back.
  await expect(detail.getByTestId('integration-log-detail-no-response')).toBeVisible()
})
