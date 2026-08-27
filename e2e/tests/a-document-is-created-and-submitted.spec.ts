import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

import { expect, test, type Page } from '@playwright/test'

import { createSupplier, runSuffix, signInOverApi, type ApiSession } from '../support/api'
import { createDocumentType, createDraft, type SeededDocumentType } from '../support/documents'
import { credentials } from '../support/env'
import { publishForm, type SeededForm } from '../support/forms'

/**
 * The Phase 4 exit demo, driven rather than described (#172 AC5).
 *
 * > An administrator configures a document type against a published form; a user
 * > creates a document from it, fills the form with validation and calculation
 * > live, submits it and receives a number; the document appears in the list,
 * > opens in its workspace, and moves through a status transition.
 *
 * Every clause of that sentence is a step below, in that order. It is the reason
 * the browser harness was built in Sprint 7 ([#153](https://github.com/sujanto-gaws/kelir/issues/153)).
 *
 * # This flow is also what closes Sprint 8's exit qualifier
 *
 * That sprint's exit was recorded as met *in parts*: the renderer opened a form
 * **by form id**, and no screen traversed the type-to-form binding. Here nobody
 * types a form id. A person picks a document *type* on `/documents/new`, and the
 * form they are given is whatever that type binds — which is the traversal, in a
 * browser, for the first time.
 *
 * # The same fixture the unit tests read, rather than a copy
 *
 * `render-a-form.spec.ts` gives the reason: the copy that drifts is always the
 * one only the slow suite reads.
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

const suffix = runSuffix()

const supplier = {
  code: `E2E-DOC-${suffix}`,
  name: `Northgate Interiors ${suffix}`,
  supplierNumber: `SUP-N-${suffix}`,
}

const title = `Two standing desks ${suffix}`

let session: ApiSession
let form: SeededForm
let documentType: SeededDocumentType

test.beforeAll(async () => {
  session = await signInOverApi()

  await createSupplier(session, supplier)

  // The administrator's half, over the API. Configuring a document type has no
  // screen until FR-DTYPE's, and this flow is about the document.
  form = await publishForm(session, definition, `Purchase requisition (document ${suffix})`)
  documentType = await createDocumentType(session, form, `Purchase requisition ${suffix}`)
})

test.afterAll(async () => {
  await session?.context.dispose()
})

async function signIn(page: Page): Promise<void> {
  const { username, password } = credentials()

  await page.goto('/login')
  await page.getByLabel('Username or email').fill(username)
  await page.getByLabel('Password').fill(password)
  await page.getByRole('button', { name: 'Sign in' }).click()

  await expect(page).toHaveURL(/\/$/)
}

/**
 * Fills the requisition in far enough that its own rules let a submit through,
 * with two lines worth 20 and 22.
 *
 * **42 is the figure the whole Tamper-Proof argument is built on**, and the
 * figure the operator-parity spike watched become a silent 0.
 */
async function fillInTwoLines(page: Page): Promise<void> {
  await page.getByLabel('Title', { exact: false }).fill('Two standing desks')

  await page.getByLabel('Search Supplier').fill(supplier.code)
  await expect(page.locator('#jfss-supplier-field option')).toHaveCount(2)
  await page.locator('#jfss-supplier-field').selectOption({ index: 1 })

  await page.locator('#jfss-priority-field').selectOption({ label: 'Normal' })

  await page.getByRole('tab', { name: 'Lines' }).click()
  await page.locator('#jfss-row-0-line-description').fill('Standing desk')
  await page.locator('#jfss-row-0-line-quantity').fill('2')
  await page.locator('#jfss-row-0-line-unit-price').fill('10')

  await page.getByRole('button', { name: 'Add row' }).click()
  await page.locator('#jfss-row-1-line-description').fill('Desk mat')
  await page.locator('#jfss-row-1-line-quantity').fill('2')
  await page.locator('#jfss-row-1-line-unit-price').fill('11')

  // The calculation, live, before anything is submitted — the middle clause of
  // the exit sentence, which would otherwise be true of nothing on the screen.
  await expect(page.locator('#jfss-grand-total-field')).toHaveValue('42')
}

test('a document is created from a type, filled in, submitted, found and moved', async ({
  page,
}) => {
  await signIn(page)

  // --- A user creates a document from a type ------------------------------
  //
  // The traversal Sprint 8 did not have. Nothing here names a form.
  await page.goto('/documents/new')

  await page.getByTestId(`type-${documentType.typeCode}`).getByRole('radio').check()
  await page.getByTestId('new-document-title').fill(title)
  await page.getByTestId('create-document').click()

  await expect(page).toHaveURL(/\/documents\/[0-9a-f-]{36}$/)

  // --- It opens in its workspace, showing what a person came for -----------
  await expect(page.getByTestId('document-title')).toHaveText(title)
  await expect(page.getByTestId('document-status')).toHaveText('Draft')
  // A draft has no number and the workspace says so rather than showing a gap.
  await expect(page.getByTestId('document-number')).toContainText('submitted')
  await expect(page.getByTestId('document-ref')).toContainText('DOC-')

  // --- The form the *type* binds is what it renders ------------------------
  await fillInTwoLines(page)

  // --- Nothing is submitted while the form's own rules refuse --------------
  //
  // Cleared first, so the refusal is caused rather than waited for: this is the
  // half of the exit that says validation is live, and a form that let it
  // through would take a number for a document nobody finished.
  await page.getByLabel('Title', { exact: false }).fill('')
  await page.getByRole('button', { name: 'Submit request' }).click()

  await expect(page.locator('#jfss-title-field-error')).toHaveText('Every request needs a title.')
  await expect(page.getByTestId('document-status')).toHaveText('Draft')

  await page.getByLabel('Title', { exact: false }).fill('Two standing desks')

  // --- Submitted, and a number appears -------------------------------------
  await page.getByRole('button', { name: 'Submit request' }).click()

  await expect(page.getByTestId('document-status')).toHaveText('Submitted')
  await expect(page.getByTestId('document-number')).toContainText('PR-')

  const number = (await page.getByTestId('document-number').textContent())?.trim() ?? ''

  expect(number, 'the submit assigned a number').toMatch(/^PR-/)

  // The form is read-only now, which is the mode the status decides.
  //
  // **Asserted on a control inside the fieldset rather than on the fieldset.**
  // `toBeDisabled` follows the accessibility notion of disabled, and a
  // `<fieldset disabled>` is not itself disabled by it — only its descendants
  // are. The first CI run reported `unexpected value "enabled"` against an
  // element whose own call log printed `<fieldset disabled …>`, which is the
  // assertion being wrong rather than the product. Asserting a field is also
  // the better claim: what matters is that nobody can type into it.
  await expect(page.getByTestId('document-form')).toHaveAttribute('disabled', '')
  await expect(page.locator('#jfss-title-field')).toBeDisabled()

  // --- It appears in the list, findable by what it is called ---------------
  await page.goto('/documents')
  await page.getByTestId('documents-search').fill(title)
  await page.getByTestId('documents-search').press('Enter')

  const row = page.getByRole('row').filter({ hasText: number })
  await expect(row).toHaveCount(1)

  // --- Opened from the list, and moved through a transition ----------------
  await row.click()

  await expect(page.getByTestId('document-title')).toHaveText(title)

  await page.getByTestId('tab-history').click()
  await page.getByTestId('transition-IN_REVIEW').click()

  await expect(page.getByTestId('document-status')).toHaveText('In review')

  // And the history explains how it got there, from creation onwards — which is
  // what makes the status something a reader can account for rather than a
  // value that is simply true now.
  await expect(page.getByTestId('document-history')).toContainText('Draft')
  await expect(page.getByTestId('document-history')).toContainText('Submitted')
})

test('a tab a later phase fills says what will fill it', async ({ page }) => {
  // #172 AC4, in a browser: neither an empty tab nor a silent one. Asserted
  // here rather than only in the component spec because the failure it guards
  // against — a tab that renders blank — is a thing a person sees.
  //
  // **It seeds its own document rather than reusing the flow above's.** The
  // first version opened the document that test created, and when that test
  // failed on CI this one failed too — reporting a row it could not find rather
  // than the tabs it is about. `README.md`'s first rule, learned again.
  const own = await createDraft(session, documentType, `Tabs ${suffix}`)

  await signIn(page)
  await page.goto(`/documents/${own}`)

  await expect(page.getByTestId('document-title')).toHaveText(`Tabs ${suffix}`)

  await page.getByTestId('tab-attachments').click()
  await expect(page.getByTestId('panel-attachments')).toContainText('Phase 6')

  await page.getByTestId('tab-workflow').click()
  await expect(page.getByTestId('panel-workflow')).toContainText('Phase 5')
})
