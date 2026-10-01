import { expect, test, type APIResponse, type Page } from '@playwright/test'

import { runSuffix, signInOverApi, type ApiSession } from '../support/api'
import { API_PREFIX, credentials } from '../support/env'
import {
  UPSTREAM_BASE_URL,
  UPSTREAM_OWN_SESSION_VALUE,
  UPSTREAM_SECRET_REFERENCE,
  upstreamCalls,
  upstreamToken,
} from '../support/upstream'

/**
 * An administrator runs a test call that the system **answers**: the dialog
 * says `Success` and `HTTP 200`, the integration log holds the row beside a
 * call the same system answered `404`, and the secret Kelir sent is on no
 * screen and in no response the browser was given ([#593], FR-INT-002,
 * FR-INT-006).
 *
 * # What the two earlier flows could not show
 *
 * `a-test-call-is-refused-and-explained.spec.ts` (#547) and
 * `an-integration-log-is-filtered-and-read.spec.ts` (#548) both end in a
 * refusal, because the release stack had no system a call could reach and no
 * secret to send. #548's AC6 was accepted as partial for that reason, and
 * this flow is its follow-up. The stack it runs against is the release stack
 * with `deploy/staging/docker-compose.e2e.yml` layered over it, which adds a
 * stand-in ERP on a private network, that one address in
 * `KELIR_INTEGRATION_ALLOWED_CIDRS`, and the system tenant's secret.
 *
 * # Skipped in one case, and red in every other
 *
 * **With no `KELIR_E2E_UPSTREAM_TOKEN` and outside CI, the flow is skipped**,
 * with the reason in the report: a harness pointed at a stack that was
 * brought up without the overlay has nothing to call, and that is a stack
 * somebody chose. **In CI it is never skipped**: a missing token there is a
 * failure, so the job cannot go green without this flow. And with the token
 * set it always runs: against a stack missing a piece it is red, and the
 * first assertion prints which, `SECRET_NOT_FOUND` (no variable, which is
 * what a release stack answers), `EGRESS_REFUSED` (the address is not listed)
 * or `HOST_NOT_RESOLVED` (no upstream). `e2e/README.md` has the runs.
 *
 * # What only a real, answered call produces
 *
 * - `Success` and `HTTP 200` in the dialog, and on the log's row and detail.
 *   The upstream answers `401` to any call without the bearer token, so a
 *   `200` means the backend read `KELIR_INTEGRATION_SECRET_SYSTEM__…` for the
 *   system tenant and sent it.
 * - The body the upstream wrote, with a purchase order in it.
 * - **The upstream's own journal**: one call under the correlation id the log
 *   row shows, carrying `Bearer <the token>`. This is the one assertion not
 *   made in the browser, and `support/upstream.ts` says why: a screen cannot
 *   show that the call left the backend.
 *
 * # Redaction, which is a product claim
 *
 * The upstream echoes the `Authorization` header it received under a key
 * that is not sensitive (`echo`), the token again in base64 (`echoBase64`),
 * and a value of its own under `sessionToken`. So the body the backend read
 * **does** contain the secret, twice, and the flow checks that:
 *
 * - the dialog and the stored payload show `[REDACTED]` in all three places;
 * - the stored request shows `"Authorization": "[REDACTED]"`, the credential's
 *   type, and never its reference;
 * - **no response under `/api/` that the browser received during the whole
 *   flow** holds the token, its base64 or the upstream's own value. A page can
 *   hide what a response carried; this reads the responses.
 *
 * # The failure it is shown beside
 *
 * A second endpoint of the same system has a path the upstream does not
 * serve. That call is *answered* too, with a `404`, so it is logged `FAILED`
 * with a status code and no error message — an outcome neither refusal flow
 * reaches. Filtering the log by the system lists the two rows together.
 *
 * #521's rule holds: codes carry `runSuffix()`, endpoints and rows are reached
 * by test id, and the log is filtered by a system only this run created.
 *
 * [#593]: https://github.com/sujanto-gaws/kelir/issues/593
 */

const SYSTEMS_PATH = `${API_PREFIX}/integration/external-systems`
const LOGS_PATH = `${API_PREFIX}/integration/logs`

const ANSWERED_URL = `${UPSTREAM_BASE_URL}/purchase-orders`
const NOT_SERVED_URL = `${UPSTREAM_BASE_URL}/retired-orders`

/** What a test call that the system answered comes back as. */
interface TestCallAnswer {
  data: { logId: string; status: string; statusCode: number }
}

let session: ApiSession
let token: string
let systemId: string
let systemCode: string
let systemName: string
let answered: { id: string; code: string }
let notServed: { id: string; code: string }

async function ok(response: APIResponse, what: string): Promise<{ data: { id: string } }> {
  expect(response.ok(), `${what} failed: ${response.status()} ${await response.text()}`).toBe(true)

  return (await response.json()) as { data: { id: string } }
}

async function seedEndpoint(endpointCode: string, name: string, path: string): Promise<string> {
  const endpoint = await ok(
    await session.context.post(`${SYSTEMS_PATH}/${systemId}/endpoints`, {
      data: { endpointCode, name, method: 'GET', path },
    }),
    `seeding endpoint ${endpointCode}`,
  )

  return endpoint.data.id
}

/** Every spelling of the secret this flow can recognise, and the upstream's own masked value. */
function forbiddenTexts(): { what: string; text: string }[] {
  const bytes = Buffer.from(token, 'utf8')

  return [
    { what: 'the secret', text: token },
    { what: 'the secret in base64', text: bytes.toString('base64') },
    { what: 'the secret in unpadded base64', text: bytes.toString('base64').replace(/=+$/, '') },
    { what: 'the secret in URL-safe base64', text: bytes.toString('base64url') },
    { what: "the upstream's own session value", text: UPSTREAM_OWN_SESSION_VALUE },
  ]
}

/** Runs the test call on one endpoint from the system's page, and returns Kelir's response to it. */
async function runTestCall(
  page: Page,
  endpoint: { id: string; code: string },
): Promise<{ status: number; text: string }> {
  await page.goto(`/admin/external-systems/${systemId}`)
  await page.getByTestId(`test-call-endpoint-${endpoint.code}`).click()
  await expect(page.getByTestId('test-call-dialog')).toHaveAttribute('data-phase', 'confirm')

  const calling = page.waitForResponse(
    (response) =>
      response.request().method() === 'POST' &&
      new URL(response.url()).pathname ===
        `${SYSTEMS_PATH}/${systemId}/endpoints/${endpoint.id}/test-call`,
  )
  await page.getByTestId('test-call-run').click()
  const called = await calling

  return { status: called.status(), text: await called.text() }
}

// The one case this flow does not run in; the header says why CI is excluded.
test.skip(
  !process.env.KELIR_E2E_UPSTREAM_TOKEN && !process.env.CI,
  'KELIR_E2E_UPSTREAM_TOKEN is not set: this flow needs the stack brought up with ' +
    'KELIR_COMPOSE_OVERLAY=docker-compose.e2e.yml and the same token given to the harness ' +
    '(e2e/README.md, "The one system the stack can reach")',
)

test.beforeAll(async () => {
  // Throws when the token is unset, which after the skip above is CI only.
  token = upstreamToken()
  session = await signInOverApi()
  const suffix = runSuffix()

  systemCode = `E2E_UP_${suffix}`
  systemName = `Answering ERP ${suffix}`

  const system = await ok(
    await session.context.post(SYSTEMS_PATH, {
      data: {
        systemCode,
        systemName,
        systemType: 'ERP',
        baseUrl: UPSTREAM_BASE_URL,
        authType: 'BEARER_TOKEN',
      },
    }),
    'seeding the system',
  )
  systemId = system.data.id

  answered = {
    code: `GET_PO_${suffix}`,
    id: await seedEndpoint(`GET_PO_${suffix}`, 'Read purchase orders', '/purchase-orders'),
  }
  notServed = {
    code: `GET_OLD_${suffix}`,
    id: await seedEndpoint(`GET_OLD_${suffix}`, 'Read retired orders', '/retired-orders'),
  }

  await ok(
    await session.context.post(`${SYSTEMS_PATH}/${systemId}/credentials`, {
      data: { credentialType: 'BEARER_TOKEN', secretReference: UPSTREAM_SECRET_REFERENCE },
    }),
    'seeding the credential',
  )
})

test.afterAll(async () => {
  await session?.context.dispose()
})

test('a test call the system answers is shown as a success, logged beside a failure, and its secret is masked in what the browser is given', async ({
  page,
}) => {
  const { username, password } = credentials()

  // Every body the browser is given under /api/, read at the end. A response
  // whose body cannot be read (a redirect, a page that navigated away) had no
  // body a script could have read either.
  const received: Promise<{ url: string; text: string }>[] = []
  page.on('response', (response) => {
    const url = response.url()

    if (new URL(url).pathname.startsWith(API_PREFIX)) {
      received.push(
        response.text().then(
          (text) => ({ url, text }),
          () => ({ url, text: '' }),
        ),
      )
    }
  })

  await page.goto('/login')
  await page.getByLabel('Username or email').fill(username)
  await page.getByLabel('Password').fill(password)
  await page.getByRole('button', { name: 'Sign in' }).click()
  const navigation = page.getByRole('navigation', { name: 'Main navigation' })
  await expect(navigation).toBeVisible()

  // --- The call is answered, and the dialog says so --------------------------
  const first = await runTestCall(page, answered)
  expect(
    first.status,
    `the test call was not answered: ${first.status} ${first.text}\n` +
      'This flow needs the stack brought up with KELIR_COMPOSE_OVERLAY=docker-compose.e2e.yml ' +
      '(e2e/README.md, "The one system the stack can reach").',
  ).toBe(200)
  const logOk = (JSON.parse(first.text) as TestCallAnswer).data.logId

  const dialog = page.getByTestId('test-call-dialog')
  await expect(dialog).toHaveAttribute('data-phase', 'answered')
  await expect(page.getByTestId('test-call-status')).toHaveText('Success')
  await expect(page.getByTestId('test-call-status-code')).toHaveText('HTTP 200')
  await expect(page.getByTestId('test-call-request')).toHaveText(`GET ${ANSWERED_URL}`)
  await expect(page.getByTestId('test-call-failure')).toHaveCount(0)

  // What the upstream wrote, with every place it echoed the secret masked.
  const body = page.getByTestId('test-call-body')
  await expect(body).toContainText('"number":"PO-2026-000123"')
  await expect(body).toContainText('"servedBy":"kelir-e2e-upstream"')
  await expect(body).toContainText('"echo":"[REDACTED]"')
  await expect(body).toContainText('"echoBase64":"[REDACTED]"')
  await expect(body).toContainText('"sessionToken":"[REDACTED]"')
  for (const { what, text } of forbiddenTexts()) {
    await expect(dialog, `the dialog shows ${what}`).not.toContainText(text)
  }

  // --- Its log row, read in full ---------------------------------------------
  await expect(page.getByTestId('test-call-log-id')).toHaveText(logOk)
  await page.getByTestId('test-call-log-link').click()
  await expect(page).toHaveURL(new RegExp(`/admin/integration-logs\\?log=${logOk}$`))

  const detail = page.getByTestId('integration-log-detail')
  await expect(detail.getByTestId('integration-log-detail-id')).toHaveText(logOk)
  await expect(detail.getByTestId('integration-log-detail-status')).toHaveText('Success')
  await expect(detail.getByTestId('integration-log-detail-status-code')).toHaveText('HTTP 200')
  await expect(detail.getByTestId('integration-log-detail-error-message')).toHaveCount(0)
  await expect(detail.getByTestId('integration-log-detail-request')).toHaveText(
    `GET ${ANSWERED_URL}`,
  )

  // The request as stored: the header is recorded as sent and never as its
  // value, with the credential's type and not its reference.
  const requestPayload = detail.getByTestId('integration-log-detail-request-payload')
  await expect(requestPayload).toContainText('"Authorization": "[REDACTED]"')
  await expect(requestPayload).toContainText('"credentialType": "BEARER_TOKEN"')
  await expect(requestPayload).not.toContainText('env://')
  await expect(requestPayload).not.toContainText('KELIR_INTEGRATION_SECRET')

  // The response as stored: the same masked body the dialog showed.
  const responsePayload = detail.getByTestId('integration-log-detail-response-payload')
  await expect(responsePayload).toContainText('"statusCode": 200')
  await expect(responsePayload).toContainText('PO-2026-000123')
  await expect(responsePayload).toContainText('\\"echo\\":\\"[REDACTED]\\"')
  await expect(responsePayload).toContainText('\\"echoBase64\\":\\"[REDACTED]\\"')
  await expect(responsePayload).toContainText('\\"sessionToken\\":\\"[REDACTED]\\"')
  await expect(detail.getByTestId('integration-log-detail-no-response')).toHaveCount(0)
  for (const { what, text } of forbiddenTexts()) {
    await expect(detail, `the log detail shows ${what}`).not.toContainText(text)
  }

  // --- The call the page shows is the call the upstream received -------------
  const correlationId = /"X-Correlation-Id": "([0-9a-f-]{36})"/.exec(
    (await requestPayload.textContent()) ?? '',
  )?.[1]
  expect(correlationId, 'the stored request names its correlation id').toBeDefined()

  const arrived = await upstreamCalls(correlationId ?? '')
  // The count, not the list: an entry holds the header as it arrived, and a
  // failed assertion prints what it was given into the report.
  expect(arrived.length, `calls the upstream received under ${correlationId}`).toBe(1)
  expect(arrived[0].method).toBe('GET')
  expect(arrived[0].path).toBe('/api/purchase-orders')
  expect(arrived[0].statusCode).toBe(200)
  // Compared as a boolean, so a mismatch does not print either value into a report.
  expect(
    arrived[0].authorization === `Bearer ${token}`,
    'the call arrived carrying the bearer token the stack was given',
  ).toBe(true)

  await page.getByTestId('integration-log-detail-close').click()
  await expect(detail).toHaveCount(0)

  // --- A call the same system answers with a 404 -----------------------------
  const second = await runTestCall(page, notServed)
  expect(second.status, second.text).toBe(200)
  const logNotServed = (JSON.parse(second.text) as TestCallAnswer).data.logId
  expect(logNotServed).not.toBe(logOk)

  await expect(dialog).toHaveAttribute('data-phase', 'answered')
  await expect(page.getByTestId('test-call-status')).toHaveText('Failed')
  await expect(page.getByTestId('test-call-status-code')).toHaveText('HTTP 404')
  await expect(page.getByTestId('test-call-request')).toHaveText(`GET ${NOT_SERVED_URL}`)
  await expect(page.getByTestId('test-call-body')).toContainText('"error":"NOT_FOUND"')
  await expect(page.getByTestId('test-call-log-id')).toHaveText(logNotServed)
  await page.getByTestId('test-call-close').click()

  // --- The log, filtered by the system: the success beside the failure -------
  await navigation.getByRole('link', { name: 'Integration Logs' }).click()
  await expect(page).toHaveURL(/\/admin\/integration-logs$/)

  const filtering = page.waitForResponse(
    (response) =>
      new URL(response.url()).pathname === LOGS_PATH &&
      new URL(response.url()).searchParams.get('externalSystemId') === systemId,
  )
  await page
    .getByTestId('integration-logs-system')
    .selectOption({ label: `${systemCode} · ${systemName}` })
  expect((await filtering).status()).toBe(200)

  const rowOk = page.getByTestId(`integration-log-row-${logOk}`)
  await expect(rowOk).toBeVisible()
  await expect(rowOk.getByTestId('integration-log-status')).toHaveText('Success')
  await expect(rowOk).toContainText('HTTP 200')
  await expect(rowOk).toContainText(`GET ${ANSWERED_URL}`)
  await expect(rowOk.getByTestId('integration-log-error-message')).toHaveCount(0)

  const rowNotServed = page.getByTestId(`integration-log-row-${logNotServed}`)
  await expect(rowNotServed).toBeVisible()
  await expect(rowNotServed.getByTestId('integration-log-status')).toHaveText('Failed')
  await expect(rowNotServed).toContainText('HTTP 404')
  await expect(rowNotServed).toContainText(`GET ${NOT_SERVED_URL}`)
  // Answered, so there is a status code and nothing Kelir refused.
  await expect(rowNotServed.getByTestId('integration-log-error-message')).toHaveCount(0)

  // This run's system made exactly these two calls.
  await expect(page.getByTestId('integration-logs-table').locator('tbody tr')).toHaveCount(2)

  // --- Nothing the browser was given carries the secret ----------------------
  const bodies = await Promise.all(received)
  // The two test calls, the two log reads and the filtered list are in there;
  // an empty list would make the loop below pass on nothing.
  expect(bodies.filter(({ text }) => text.includes(logOk)).length).toBeGreaterThanOrEqual(3)

  for (const { what, text } of forbiddenTexts()) {
    const carrying = bodies.filter((response) => response.text.includes(text))
    expect(
      carrying.map((response) => response.url),
      `responses carrying ${what}`,
    ).toEqual([])
    await expect(page.locator('body'), `the page shows ${what}`).not.toContainText(text)
  }
})
