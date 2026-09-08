import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

import { expect, test } from '@playwright/test'

import { signInOverApi, runSuffix, type ApiSession } from '../support/api'
import { credentials } from '../support/env'
import { publishForm, type SeededForm } from '../support/forms'

/**
 * An administrator builds a list through a screen, and the list then opens with
 * rows in it (#374 AC5).
 *
 * **The assertion is the last step.** `rad_lists` got a renderer in Sprint 14
 * and nothing that writes one, so until now every list in every test was seeded
 * over the API. This flow authors the definition in the browser and then opens
 * `/lists/{listKey}` — a screen that could not have been drawn without it.
 *
 * The *form* is still seeded, because this test is about lists and a form built
 * through forty interactions would make it a test of the other builder.
 * `build-a-form.spec.ts` is where that one is asserted.
 */
const definition = JSON.parse(
  readFileSync(
    fileURLToPath(
      new URL(
        '../../kelir-frontend/src/features/rad/__fixtures__/purchase-requisition.json',
        import.meta.url,
      ),
    ),
    'utf8',
  ),
) as Record<string, unknown>

let session: ApiSession
let form: SeededForm
let listKey: string
let typeCode: string

test.beforeAll(async () => {
  session = await signInOverApi()
  form = await publishForm(session, definition, 'Listed requisition (e2e)')
  listKey = `e2e_built_list_${runSuffix()}`.toLowerCase()
  typeCode = `E2E_LISTED_${runSuffix()}`.toUpperCase()
})

test.afterAll(async () => {
  await session?.context.dispose()
})

test('an administrator builds a list through a screen, and it opens with rows', async ({
  page,
}) => {
  const { username, password } = credentials()

  await page.goto('/login')
  await page.getByLabel('Username or email').fill(username)
  await page.getByLabel('Password').fill(password)
  await page.getByRole('button', { name: 'Sign in' }).click()

  await expect(page).toHaveURL(/\/$/)

  // --- The screen is reachable from the navigation --------------------------
  await page.getByRole('link', { name: 'Lists' }).click()

  await expect(page).toHaveURL(/\/admin\/lists$/)

  // --- Create the list ------------------------------------------------------
  await page.getByTestId('new-list').click()
  await page.getByTestId('list-key').fill(listKey)
  await page.getByTestId('list-title').fill('Built in the browser')
  await page.getByTestId('save-list').click()

  await expect(page).toHaveURL(/\/admin\/lists\/[0-9a-f-]{36}$/)

  // **A list nothing binds is its own state, not a broken definition.** It is
  // the ordinary condition of one just authored, and the panel says so — which
  // is what stops an author reading the next message as noise.
  await expect(page.getByTestId('render-unbound')).toBeVisible()

  // --- Author the columns ---------------------------------------------------
  await page.getByTestId('column-label-0').fill('Subject')

  await page.getByTestId('add-column').click()
  await page.getByTestId('column-key-1').selectOption('documentNumber')
  await page.getByTestId('column-label-1').fill('Number')

  await page.getByTestId('add-filter').click()
  await page.getByTestId('filter-key-0').selectOption('status')
  await page.getByTestId('filter-label-0').fill('Status')

  await page.getByTestId('sort-key').selectOption('title')
  await page.getByTestId('save-list').click()

  // Still unbound, and still said plainly rather than left to be inferred.
  await expect(page.getByTestId('render-unbound')).toBeVisible()

  // --- Bind it to a document type, through the other builder ----------------
  await page.getByRole('link', { name: 'Document Types' }).click()
  await page.getByTestId('new-document-type').click()

  await page.getByTestId('type-code').fill(typeCode)
  await page.getByTestId('type-name').fill('Listed requisition')
  await page.getByTestId('type-form').selectOption({ label: `${form.title} (r1)` })
  await page.getByTestId('type-list').selectOption({ label: `Built in the browser (${listKey})` })
  await page.getByTestId('type-status').selectOption('ACTIVE')
  await page.getByTestId('save-document-type').click()

  await expect(page.getByTestId(`type-${typeCode}`)).toBeVisible()

  // --- Give it a document to show -------------------------------------------
  await page.goto('/documents/new')
  await page.getByTestId(`type-${typeCode}`).getByRole('radio').check()
  await page.getByTestId('new-document-title').fill('A row in a list nobody seeded')
  await page.getByTestId('create-document').click()

  await expect(page).toHaveURL(/\/documents\/[0-9a-f-]{36}$/)

  // --- **The assertion #374 AC5 asks for** ----------------------------------
  //
  // The list the flow authored, opened by its key, with the column it declared
  // and the row it was given. Nothing in this run touched POST /rad/lists.
  await page.goto(`/lists/${listKey}`)

  // The renderer's own hooks, the ones `render-a-list.spec.ts` uses: a column
  // is `column-<key>` and a cell is `cell-<key>`.
  await expect(page.getByTestId('list-title')).toHaveText('Built in the browser')
  await expect(page.getByTestId('column-title')).toContainText('Subject')
  await expect(page.getByTestId('column-documentNumber')).toContainText('Number')
  await expect(page.getByTestId('cell-title').first()).toHaveText('A row in a list nobody seeded')
})
