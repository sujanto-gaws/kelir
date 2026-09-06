import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

import { expect, test } from '@playwright/test'

import { signInOverApi, runSuffix, type ApiSession } from '../support/api'
import { credentials } from '../support/env'
import { publishForm, type SeededForm } from '../support/forms'

/**
 * An administrator configures a document type through a screen, and a document
 * is then raised from the type they just made (#341 AC4).
 *
 * **This is the criterion SRS §9 criterion 4 rests on.** **D-64** recorded
 * *administrators can configure document types* as met over the API, on the
 * stated basis that this builder was scheduled — and [construction plan
 * 08](../../projects/planning/08.%20Sprint%2013%20MVP%20Construction%20Plan.md)
 * §1.2's argument is that *a criterion that says X can is not met by an
 * endpoint*. So the assertion that discharges the debt is not that the screen
 * renders: it is that **a document exists that could not have existed without
 * it**.
 *
 * The type is created entirely through the browser. Only the *form* is seeded
 * over the API, because a form builder is FR-RAD-004's other half and is not in
 * this sprint — the same division `a-document-is-created-and-submitted.spec.ts`
 * already makes.
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
let typeCode: string

test.beforeAll(async () => {
  session = await signInOverApi()
  form = await publishForm(session, definition, 'Configured requisition (e2e)')
  // The deployment keeps its database between runs, so a fixed code conflicts
  // on the second one — the reason `runSuffix` exists.
  typeCode = `E2E_CONFIGURED_${runSuffix()}`.toUpperCase()
})

test.afterAll(async () => {
  await session?.context.dispose()
})

test('an administrator configures a document type through a screen, and a document is raised from it', async ({
  page,
}) => {
  const { username, password } = credentials()

  // --- Sign in -------------------------------------------------------------
  await page.goto('/login')
  await page.getByLabel('Username or email').fill(username)
  await page.getByLabel('Password').fill(password)
  await page.getByRole('button', { name: 'Sign in' }).click()

  await expect(page).toHaveURL(/\/$/)

  // --- The screen is reachable from the navigation --------------------------
  //
  // Clicked rather than navigated to by URL: #341 AC3 puts these screens beside
  // users, roles, delegations and tenants, and a screen nobody can find from
  // the sidebar is a screen an administrator does not have.
  await page.getByRole('link', { name: 'Document Types' }).click()

  await expect(page).toHaveURL(/\/admin\/document-types$/)

  // --- Create the type ------------------------------------------------------
  await page.getByTestId('new-document-type').click()

  await expect(page.getByTestId('document-type-dialog')).toBeVisible()

  await page.getByTestId('type-code').fill(typeCode)
  await page.getByTestId('type-name').fill('Configured requisition')
  await page.getByTestId('type-category').fill('PROCUREMENT')

  // **The form is chosen, not typed.** The chooser offers published revisions,
  // which is what a document may pin.
  // Named from the seeded form rather than from a literal, so a change to how
  // the chooser labels a revision fails here rather than silently selecting
  // nothing.
  await page.getByTestId('type-form').selectOption({ label: `${form.title} (r1)` })

  // A document may only be created from an ACTIVE type, so the flow sets it —
  // which is the state a person configuring a type actually has to reach.
  await page.getByTestId('type-status').selectOption('ACTIVE')
  await page.getByTestId('save-document-type').click()

  await expect(page.getByTestId('document-type-dialog')).toBeHidden()
  await expect(page.getByTestId(`type-${typeCode}`)).toBeVisible()
  await expect(page.getByTestId(`type-${typeCode}`)).toContainText('Bound')

  // --- Give it a numbering rule --------------------------------------------
  //
  // Its own dialog because it is its own sub-resource, and a type without one
  // cannot have its documents submitted — which the dialog says.
  await page.getByTestId(`numbering-${typeCode}`).click()

  await expect(page.getByTestId('numbering-is-new')).toBeVisible()
  await page.getByTestId('rule-template').fill(`${typeCode}-{year}-{sequence}`)
  await page.getByTestId('rule-scope').selectOption('YEAR')
  await page.getByTestId('save-numbering-rule').click()

  await expect(page.getByTestId('numbering-dialog')).toBeHidden()

  // --- **The assertion that discharges D-64** -------------------------------
  //
  // A document raised from the type this flow just configured. Nothing else in
  // the run created it, so a screen that had written a broken type would fail
  // here rather than pass on its own rendering.
  await page.goto('/documents/new')

  // The type the flow configured is offered, and it is **selectable** — a type
  // with no form bound is rendered disabled with the reason, so checking this
  // radio asserts the binding took as well as the type.
  await page.getByTestId(`type-${typeCode}`).getByRole('radio').check()
  await page.getByTestId('new-document-title').fill('Two standing desks')
  await page.getByTestId('create-document').click()

  await expect(page).toHaveURL(/\/documents\/[0-9a-f-]{36}$/)
  await expect(page.getByTestId('document-title')).toHaveText('Two standing desks')
})

/**
 * A tenant adds an entry to the navigation and it appears in the sidebar
 * (#341 AC2).
 *
 * **`rad_menus` had no surface at all** — no endpoint, no screen, nothing that
 * read it. The assertion is the last step: the entry the flow authored is in
 * the navigation of the page it lands on, which is the whole of what *takes
 * effect* means.
 */
test('a navigation entry is authored and appears in the sidebar', async ({ page }) => {
  const { username, password } = credentials()
  const menuKey = `e2e_nav_${runSuffix()}`.toLowerCase()
  const label = `Requisitions ${menuKey.slice(-4)}`

  await page.goto('/login')
  await page.getByLabel('Username or email').fill(username)
  await page.getByLabel('Password').fill(password)
  await page.getByRole('button', { name: 'Sign in' }).click()

  await expect(page).toHaveURL(/\/$/)

  await page.getByRole('link', { name: 'Navigation' }).click()

  await expect(page).toHaveURL(/\/admin\/menus$/)

  await page.getByTestId('new-menu').click()
  await page.getByTestId('menu-key').fill(menuKey)
  await page.getByTestId('menu-label').fill(label)
  await page.getByTestId('menu-route').fill('/documents')
  await page.getByTestId('menu-order').fill('35')
  await page.getByTestId('save-menu').click()

  await expect(page.getByTestId('menu-dialog')).toBeHidden()
  await expect(page.getByTestId(`menu-${menuKey}`)).toBeVisible()

  // **It takes effect.** The sidebar reads the configured navigation when the
  // shell mounts, so a reload is what a person would do — and the entry is
  // there, beside the built-in destinations rather than instead of them.
  await page.reload()

  await expect(page.getByRole('link', { name: label })).toBeVisible()
  await expect(page.getByRole('link', { name: 'Documents' })).toBeVisible()
})
