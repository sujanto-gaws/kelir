import { expect, test, type Page, type Response } from '@playwright/test'

import { runSuffix, signInOverApi, type ApiSession } from '../support/api'
import { API_PREFIX, credentials } from '../support/env'

/**
 * An administrator registers an external system, edits it, adds an endpoint and
 * a credential reference, deactivates it and activates it again — from the
 * browser, against the release stack ([#520] AC-8, FR-INT-001).
 *
 * # What it proves that the component tests cannot
 *
 * That the registry's screens, the Caddy proxy, the permission grants migration
 * `0046` gives `ROLE-ADMIN`, and the routes behind them agree with each other.
 * Every step is driven through a screen; nothing the browser asserts on was
 * written over the API.
 *
 * # #521's rule, which this flow is written against
 *
 * **The database is not assumed empty, and this spec does not assume it is the
 * only writer.** The codes carry `runSuffix()`, so a second run on one stack
 * registers a second system rather than colliding with the first. Saves are
 * confirmed by the response they came back with; rows are reached through the
 * list's search, never by position; and a **decoy** is seeded over the API
 * whose code *contains* this run's code, so the search deliberately returns more
 * than one row and the flow has to pick its own by test id. The decoy also
 * shows that deactivating one system leaves its neighbour alone.
 *
 * # AC-5, as the browser sees it
 *
 * A credential reference is shown as the reference string it is, and nothing
 * else is: the row's cells are the type, the reference, the validity and the
 * status, and the save's response carries only the fields AC-5 names plus the
 * row's own identity and timestamps.
 *
 * [#520]: https://github.com/sujanto-gaws/kelir/issues/520
 */

const SYSTEMS_PATH = `${API_PREFIX}/integration/external-systems`

/**
 * What a credential reference may carry over the wire: AC-5's five fields, and
 * the row's identity and timestamps. A sixth key is a finding.
 */
const CREDENTIAL_KEYS = [
  'createdAt',
  'credentialType',
  'externalSystemId',
  'id',
  'isActive',
  'secretReference',
  'updatedAt',
  'validFrom',
  'validTo',
].sort()

let session: ApiSession
let suffix: string
let systemCode: string
let decoyCode: string

test.beforeAll(async () => {
  session = await signInOverApi()
  suffix = runSuffix()
  systemCode = `E2E_ERP_${suffix}`
  // **Contains `systemCode`**, so searching for this run's system also finds
  // the decoy: the flow cannot pass by assuming its row is the only one.
  decoyCode = `${systemCode}_DECOY`

  const decoy = await session.context.post(SYSTEMS_PATH, {
    data: { systemCode: decoyCode, systemName: `Decoy ERP ${suffix}`, systemType: 'ERP' },
  })

  expect(
    decoy.ok(),
    `seeding the decoy system failed: ${decoy.status()} ${await decoy.text()}`,
  ).toBe(true)
})

test.afterAll(async () => {
  await session?.context.dispose()
})

/** The next response to `method` on a path, awaited after the click that sends it. */
function nextResponse(page: Page, method: string, path: RegExp): Promise<Response> {
  return page.waitForResponse(
    (response) =>
      response.request().method() === method && path.test(new URL(response.url()).pathname),
  )
}

/** Searches the registry for `term`, the way a person narrows it. */
async function search(page: Page, term: string): Promise<void> {
  const box = page.getByTestId('external-systems-search')

  await box.fill(term)
  // The list searches on `change`, which a fill alone does not raise.
  await box.press('Enter')
  await expect(page).toHaveURL(new RegExp(`[?&]search=${term}(&|$)`))
}

test('an administrator registers, edits, extends, deactivates and reactivates an external system', async ({
  page,
}) => {
  const { username, password } = credentials()
  const systemName = `Corporate ERP ${suffix}`
  const renamed = `Corporate ERP (renamed) ${suffix}`
  const endpointCode = `CREATE_PO_${suffix}`
  const secretReference = `vault://kelir/e2e-${suffix.toLowerCase()}/api-key`

  // --- Sign in, and reach the registry the way a person does -----------------
  await page.goto('/login')
  await page.getByLabel('Username or email').fill(username)
  await page.getByLabel('Password').fill(password)
  await page.getByRole('button', { name: 'Sign in' }).click()
  await expect(page.getByRole('navigation', { name: 'Main navigation' })).toBeVisible()

  // Through the navigation, so the entry's `integration:external-system:read`
  // gate is exercised rather than stepped round by a direct URL.
  await page.getByRole('link', { name: 'External Systems', exact: true }).click()
  await expect(page).toHaveURL(/\/admin\/external-systems/)

  // --- Register ---------------------------------------------------------------
  await page.getByTestId('register-external-system').click()
  await page.getByTestId('system-code').fill(systemCode)
  await page.getByTestId('system-name').fill(systemName)
  await page.getByTestId('system-type').selectOption('ERP')
  await page.getByTestId('system-auth-type').selectOption('API_KEY')
  await page.getByTestId('system-base-url').fill('https://erp.example.com/api')

  const registering = nextResponse(page, 'POST', new RegExp(`^${SYSTEMS_PATH}$`))
  await page.getByTestId('save-external-system').click()

  const registered = await registering
  expect(registered.status(), await registered.text()).toBe(201)
  const system = ((await registered.json()) as { data: { id: string; systemCode: string } }).data
  expect(system.systemCode).toBe(systemCode)

  // A registered system opens on its own page.
  await expect(page).toHaveURL(new RegExp(`/admin/external-systems/${system.id}$`))
  await expect(page.getByTestId('external-system-code')).toHaveText(systemCode)
  await expect(page.getByTestId('external-system-name')).toHaveText(systemName)
  await expect(page.getByTestId('external-system-status')).toHaveText('Active')
  await expect(page.getByTestId('external-system-base-url')).toHaveText(
    'https://erp.example.com/api',
  )

  // --- Edit -------------------------------------------------------------------
  await page.getByTestId('edit-external-system').click()
  // The code is read-only once registered, and the dialog opens on the system.
  await expect(page.getByTestId('system-code')).toHaveValue(systemCode)
  await expect(page.getByTestId('system-code')).toBeDisabled()
  await page.getByTestId('system-name').fill(renamed)
  await page.getByTestId('system-timeout').fill('45')
  await page.getByTestId('system-description').fill('Purchase orders go here.')

  const updating = nextResponse(page, 'PUT', new RegExp(`^${SYSTEMS_PATH}/${system.id}$`))
  await page.getByTestId('save-external-system').click()

  const updated = await updating
  expect(updated.status(), await updated.text()).toBe(200)
  await expect(page.getByTestId('save-external-system')).toBeHidden()
  await expect(page.getByTestId('external-system-name')).toHaveText(renamed)
  await expect(page.getByTestId('external-system-timeout')).toHaveText('45 seconds')
  await expect(page.getByTestId('external-system-description')).toHaveText(
    'Purchase orders go here.',
  )

  // --- Add an endpoint --------------------------------------------------------
  await expect(page.getByTestId('endpoints-empty')).toBeVisible()
  await page.getByTestId('add-endpoint').click()
  await page.getByTestId('endpoint-code').fill(endpointCode)
  await page.getByTestId('endpoint-name').fill('Create purchase order')
  await page.getByTestId('endpoint-method').selectOption('POST')
  await page.getByTestId('endpoint-path').fill('/purchase-orders')

  const addingEndpoint = nextResponse(
    page,
    'POST',
    new RegExp(`^${SYSTEMS_PATH}/${system.id}/endpoints$`),
  )
  await page.getByTestId('save-endpoint').click()

  const endpoint = await addingEndpoint
  expect(endpoint.status(), await endpoint.text()).toBe(201)

  const endpointRow = page.getByTestId(`endpoint-row-${endpointCode}`)
  await expect(endpointRow).toBeVisible()
  await expect(endpointRow).toContainText('Create purchase order')
  await expect(endpointRow).toContainText('POST')
  await expect(endpointRow).toContainText('/purchase-orders')

  // --- Add a credential reference --------------------------------------------
  await expect(page.getByTestId('credentials-empty')).toBeVisible()
  await page.getByTestId('add-credential').click()
  await page.getByTestId('credential-type').selectOption('API_KEY')
  await page.getByTestId('credential-secret-reference').fill(secretReference)

  const addingCredential = nextResponse(
    page,
    'POST',
    new RegExp(`^${SYSTEMS_PATH}/${system.id}/credentials$`),
  )
  await page.getByTestId('save-credential').click()

  const credential = await addingCredential
  expect(credential.status(), await credential.text()).toBe(201)

  // **AC-5 on the wire**: the reference and its metadata, and nothing that
  // could be a resolved secret.
  const stored = ((await credential.json()) as { data: Record<string, unknown> }).data
  expect(Object.keys(stored).sort()).toEqual(CREDENTIAL_KEYS)
  expect(stored.secretReference).toBe(secretReference)

  // **AC-5 on the screen**: the reference as the string it is, in its own cell,
  // and the row holds the type, the reference, the validity and the status —
  // the actions cell aside, nothing else.
  const credentialRow = page.getByTestId(`credential-row-${String(stored.id)}`)
  await expect(credentialRow).toBeVisible()
  await expect(credentialRow.getByTestId('credential-secret-reference-value')).toHaveText(
    secretReference,
  )
  const cells = credentialRow.getByRole('cell')
  await expect(cells).toHaveCount(5)
  await expect(cells.nth(0)).toHaveText('API key')
  await expect(cells.nth(1)).toHaveText(secretReference)
  await expect(cells.nth(2)).toHaveText('Open-ended')
  await expect(cells.nth(3)).toHaveText('Active')
  await expect(cells.nth(4)).toHaveText(/^\s*Edit\s*Delete\s*$/)
  await expect(page.getByTestId('credentials-table').locator('tbody tr')).toHaveCount(1)

  // --- Deactivate -------------------------------------------------------------
  await page.getByTestId('deactivate-external-system').click()

  const deactivating = nextResponse(
    page,
    'POST',
    new RegExp(`^${SYSTEMS_PATH}/${system.id}/deactivate$`),
  )
  await page.getByTestId('confirm-action').click()

  const deactivated = await deactivating
  expect(deactivated.status(), await deactivated.text()).toBe(200)
  await expect(page.getByTestId('external-system-status')).toHaveText('Inactive')
  await expect(page.getByTestId('activate-external-system')).toBeVisible()
  await expect(page.getByTestId('deactivate-external-system')).toHaveCount(0)

  // **It stays listed, and its neighbour is untouched.** Found by searching,
  // which returns this run's system *and* the decoy whose code contains it.
  await page.getByRole('link', { name: '← External systems' }).click()
  await expect(page).toHaveURL(/\/admin\/external-systems(\?|$)/)
  await search(page, systemCode)

  const row = page.getByTestId(`external-system-row-${systemCode}`)
  const decoyRow = page.getByTestId(`external-system-row-${decoyCode}`)
  await expect(row).toBeVisible()
  await expect(decoyRow).toBeVisible()
  await expect(row).toContainText(renamed)
  await expect(row).toContainText('Inactive')
  await expect(decoyRow).toContainText('Active')
  await expect(decoyRow).not.toContainText('Inactive')

  // --- Activate again --------------------------------------------------------
  await row.getByRole('link', { name: systemCode, exact: true }).click()
  await expect(page).toHaveURL(new RegExp(`/admin/external-systems/${system.id}$`))
  await expect(page.getByTestId('external-system-status')).toHaveText('Inactive')

  await page.getByTestId('activate-external-system').click()

  const activating = nextResponse(
    page,
    'POST',
    new RegExp(`^${SYSTEMS_PATH}/${system.id}/activate$`),
  )
  await page.getByTestId('confirm-action').click()

  const activated = await activating
  expect(activated.status(), await activated.text()).toBe(200)
  await expect(page.getByTestId('external-system-status')).toHaveText('Active')
  await expect(page.getByTestId('deactivate-external-system')).toBeVisible()

  // What was added survives the round trip: a fresh load of the page shows the
  // endpoint and the reference as they were saved.
  await page.reload()
  await expect(page.getByTestId('external-system-status')).toHaveText('Active')
  await expect(page.getByTestId('external-system-name')).toHaveText(renamed)
  await expect(page.getByTestId(`endpoint-row-${endpointCode}`)).toBeVisible()
  await expect(
    page
      .getByTestId(`credential-row-${String(stored.id)}`)
      .getByTestId('credential-secret-reference-value'),
  ).toHaveText(secretReference)
})
