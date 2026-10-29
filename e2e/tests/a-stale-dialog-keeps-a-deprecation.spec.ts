import { expect, test } from '@playwright/test'

import { signInOverApi, runSuffix, type ApiSession } from '../support/api'
import { API_PREFIX, credentials } from '../support/env'
import { searchTable } from '../support/table-search'

/**
 * A save from a dialog opened before another administrator deprecated the type
 * leaves the deprecation alone (#742 AC6).
 *
 * The dialog sent `status` back as it read it on every save, and the server
 * writes what it is sent, so editing the description of a type deprecated
 * since the dialog opened made it `ACTIVE` again, with nothing said. Since
 * #742 an edit sends the status only when the administrator changed it.
 *
 * The second administrator is the API, arranging the race; the first is the
 * browser, and the assertion is what its table shows after the save.
 */
let session: ApiSession
let typeId: string
let typeCode: string

test.beforeAll(async () => {
  session = await signInOverApi()
  typeCode = `E2E_STALE_${runSuffix()}`.toUpperCase()

  const created = await session.context.post(`${API_PREFIX}/document-types`, {
    data: { typeCode, name: 'Stale dialog', status: 'ACTIVE' },
  })

  expect(created.ok(), `seeding the type failed: ${await created.text()}`).toBe(true)
  typeId = ((await created.json()) as { data: { id: string } }).data.id
})

test.afterAll(async () => {
  await session?.context.dispose()
})

test('a stale dialog edits the description and leaves a deprecation made since alone', async ({
  page,
}) => {
  const { username, password } = credentials()

  await page.goto('/login')
  await page.getByLabel('Username or email').fill(username)
  await page.getByLabel('Password').fill(password)
  await page.getByRole('button', { name: 'Sign in' }).click()
  await expect(page).toHaveURL(/\/$/)

  await page.goto('/admin/document-types')
  await searchTable(page, 'types-search', typeCode)
  await page.getByTestId(`edit-${typeCode}`).click()

  const dialog = page.getByTestId('document-type-dialog')
  await expect(dialog).toBeVisible()
  await expect(page.getByTestId('type-status')).toHaveValue('ACTIVE')

  // --- Another administrator deprecates the type while the dialog is open ---
  const deprecated = await session.context.put(`${API_PREFIX}/document-types/${typeId}`, {
    data: { status: 'DEPRECATED' },
  })
  expect(deprecated.ok(), await deprecated.text()).toBe(true)

  // --- The open dialog edits only the description, and saves ----------------
  await page.getByTestId('type-description').fill('Edited in a dialog opened before')

  const saved = page.waitForResponse(
    (response) =>
      response.request().method() === 'PUT' &&
      new URL(response.url()).pathname === `${API_PREFIX}/document-types/${typeId}`,
  )
  await page.getByTestId('save-document-type').click()

  const response = await saved
  expect(response.status(), await response.text()).toBe(200)
  // Left off the wire, which is what leaves the stored status alone.
  expect(response.request().postDataJSON()).not.toHaveProperty('status')
  await expect(dialog).toBeHidden()

  // The table, read again, still says what the other administrator made it.
  await page.reload()
  await searchTable(page, 'types-search', typeCode)
  await expect(page.getByTestId(`type-${typeCode}`)).toContainText('DEPRECATED')
})
