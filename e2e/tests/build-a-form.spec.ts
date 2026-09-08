import { expect, test } from '@playwright/test'

import { signInOverApi, runSuffix, type ApiSession } from '../support/api'
import { credentials } from '../support/env'

/**
 * An administrator builds a form through a screen, and a document is then
 * raised against the form they just made (#373 AC5).
 *
 * **The assertion is not that the builder renders.** It is
 * [construction plan 08](../../projects/planning/08.%20Sprint%2013%20MVP%20Construction%20Plan.md)
 * §1.2's rule, the one that discharged **D-64** through
 * `configure-a-document-type.spec.ts`: *a criterion that says X can is not met
 * by an endpoint*. So this flow ends with **a document that could not have
 * existed without the screen** — its form was authored, published and bound in
 * the browser, and `e2e/support/forms.ts` seeded nothing.
 *
 * **That file is why this test exists.** Every other browser flow in this
 * suite seeds its form with `POST /rad/forms`, because until now nothing else
 * could make one — which is exactly what **D-70** named when it shipped the
 * document type builder without this half. The seeding stays for the other
 * specs: they are about documents, and a form built through forty interactions
 * would make each of them a test of this screen.
 */
let session: ApiSession
let formKey: string
let typeCode: string

test.beforeAll(async () => {
  session = await signInOverApi()
  // The deployment keeps its database between runs, so fixed identifiers
  // conflict on the second one — the reason `runSuffix` exists.
  formKey = `e2e_built_${runSuffix()}`.toLowerCase()
  typeCode = `E2E_BUILT_${runSuffix()}`.toUpperCase()
})

test.afterAll(async () => {
  await session?.context.dispose()
})

test('an administrator builds a form through a screen, and a document is raised against it', async ({
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
  // Clicked rather than navigated to by URL: a screen nobody can find from the
  // sidebar is a screen an administrator does not have.
  await page.getByRole('link', { name: 'Forms' }).click()

  await expect(page).toHaveURL(/\/admin\/forms$/)

  // --- Create the form ------------------------------------------------------
  await page.getByTestId('new-form').click()

  await expect(page.getByTestId('form-create-dialog')).toBeVisible()

  await page.getByTestId('form-key').fill(formKey)
  await page.getByTestId('form-title').fill('Built in the browser')
  await page.getByTestId('save-form').click()

  // Straight into the editor, which is where somebody who just made a form
  // wants to be.
  await expect(page).toHaveURL(/\/admin\/forms\/[0-9a-f-]{36}$/)
  await expect(page.getByTestId('component-field_1')).toBeVisible()

  // --- Author the definition ------------------------------------------------
  await page.getByTestId('key-field_1').fill('reference')
  await page.getByTestId('label-field_1').fill('Reference')
  await page.getByTestId('required-field_1').check()

  await page.getByTestId('add-field').click()

  await page.getByTestId('key-field_2').fill('amount')
  await page.getByTestId('label-field_2').fill('Amount')
  await page.getByTestId('type-field_2').selectOption('number')
  await page.getByTestId('vtype-field_2').selectOption('number')

  await page.getByTestId('save-definition').click()

  // **What is on screen is what the server stored.** The banner says so, and it
  // is the editor's own claim about the round trip.
  await expect(page.getByTestId('save-confirmed')).toBeVisible()

  // The preview draws the definition being edited through the same renderer a
  // document uses — so a definition this screen produced that no document could
  // be filled against would fail here.
  await expect(page.getByTestId('form-preview')).toContainText('Reference')
  await expect(page.getByTestId('form-preview')).toContainText('Amount')

  // --- Publish it -----------------------------------------------------------
  await page.getByTestId('publish-form').click()

  // **The screen refuses the edit and says why** (ADR-0027): documents pin the
  // revision they were filled against.
  await expect(page.getByTestId('published-notice')).toBeVisible()
  await expect(page.getByTestId('save-definition')).toHaveCount(0)
  await expect(page.getByTestId('new-revision')).toBeVisible()

  // --- Bind it to a document type, through the other builder ----------------
  await page.getByRole('link', { name: 'Document Types' }).click()
  await page.getByTestId('new-document-type').click()

  await page.getByTestId('type-code').fill(typeCode)
  await page.getByTestId('type-name').fill('Built in the browser')

  // The chooser offers published revisions, which is what a document may pin —
  // and the only published revision this flow has is the one it just made.
  await page.getByTestId('type-form').selectOption({ label: 'Built in the browser (r1)' })
  await page.getByTestId('type-status').selectOption('ACTIVE')
  await page.getByTestId('save-document-type').click()

  await expect(page.getByTestId(`type-${typeCode}`)).toContainText('Bound')

  // --- **The assertion #373 AC5 asks for** ----------------------------------
  //
  // A document whose form was authored in a browser. Nothing in this run
  // touched `POST /rad/forms`, so a builder that wrote a definition the
  // renderer cannot draw fails here rather than passing on its own rendering.
  await page.goto('/documents/new')

  await page.getByTestId(`type-${typeCode}`).getByRole('radio').check()
  await page.getByTestId('new-document-title').fill('A form nobody seeded')
  await page.getByTestId('create-document').click()

  await expect(page).toHaveURL(/\/documents\/[0-9a-f-]{36}$/)
  await expect(page.getByTestId('document-title')).toHaveText('A form nobody seeded')

  // The fields the flow authored are the fields the document asks for.
  await expect(page.getByLabel('Reference')).toBeVisible()
  await expect(page.getByLabel('Amount')).toBeVisible()
})
