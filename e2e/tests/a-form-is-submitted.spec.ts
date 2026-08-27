import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

import { expect, test, type Page } from '@playwright/test'

import { createSupplier, runSuffix, signInOverApi, type ApiSession } from '../support/api'
import { credentials } from '../support/env'
import { publishForm, type SeededForm } from '../support/forms'

/**
 * A form is submitted, and the server stores its own arithmetic (#164).
 *
 * **This is the criterion that decides whether the item is Done.** The unit
 * tests cover the re-evaluation over a payload —
 * `kelir-backend/src/modules/rad/service/evaluation.rs` has twenty-two of them
 * and `tests/rad_form_submissions.rs` fourteen more through the endpoint — and
 * none of them proves that a number typed into a real browser reaches a real
 * `POST` and comes back as the number that was stored. That is the gap **D-14**
 * and #101 exist because of, and #162 AC5 makes a status row reading `verified
 * by inspection only` a finding rather than a formatting choice.
 *
 * **The tamper is done to the request in flight, which is the threat model.**
 * The second test fills the form in through the UI exactly as the first does,
 * and rewrites the payload between the browser and the server — a proxy, an
 * extension, a patched bundle, `curl`. Driving the UI could not express it: the
 * renderer computes `grand_total` itself and makes the field read-only, which is
 * precisely the control the server must not depend on. And because the tamper
 * happens on the way out, the *page* is what receives the server's answer — so
 * the assertion is a banner a person would see rather than a JSON body only a
 * test reads.
 *
 * **The same fixture the unit tests use**, read rather than copied, for the
 * reason `render-a-form.spec.ts` gives: the copy that drifts is always the one
 * only the slow suite reads.
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
  code: `E2E-SUB-${suffix}`,
  name: `Meridian Office Supplies ${suffix}`,
  supplierNumber: `SUP-M-${suffix}`,
}

let session: ApiSession
let form: SeededForm

test.beforeAll(async () => {
  session = await signInOverApi()

  await createSupplier(session, supplier)
  form = await publishForm(session, definition, 'Purchase requisition (submit)')
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
  // `exact: false`, as `a-form-calculates-and-validates.spec.ts` has it: the
  // label element carries the `required` asterisk beside the word, so an exact
  // match finds nothing. "Title" is unambiguous in this fixture — the trap
  // `render-a-form.spec.ts` documents is "Budget" beside "Baseline budget",
  // where substring matching resolves to two controls.
  await page.getByLabel('Title', { exact: false }).fill('Two standing desks')

  // Searched down to the one seeded supplier rather than picked by position:
  // the deployment the harness drives keeps its database between runs, which is
  // why `runSuffix` exists at all.
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

  await expect(page.locator('#jfss-grand-total-field')).toHaveValue('42')
}

test('a filled-in form is submitted and the two sides agree on the number', async ({ page }) => {
  await signIn(page)

  await page.goto(`/forms/${form.id}`)
  await expect(page.getByTestId('form-title')).toHaveText('Purchase requisition (submit)')

  // --- Nothing is submitted while the form's own rules refuse --------------
  //
  // #163 AC1's other half, at the point where it now matters: the button no
  // longer merely records that an action arrived, so a form that let this
  // through would create a row.
  await page.getByRole('button', { name: 'Submit request' }).click()

  await expect(page.getByTestId('submit-success')).toHaveCount(0)
  await expect(page.locator('#jfss-title-field-error')).toHaveText('Every request needs a title.')

  await fillInTwoLines(page)

  await page.getByRole('button', { name: 'Submit request' }).click()

  await expect(page.getByTestId('submit-success')).toContainText('revision')

  // The banner that appears only when the server's answer differs from the
  // screen's. Both sides run one engine compiled for two runtimes (**D-10**)
  // and `parity/forms.json` holds them to the same answers over whole
  // submissions, so this must be empty — and its being on the screen at all is
  // #164 AC5: a form that changes your number without saying so is its own
  // defect.
  await expect(page.getByTestId('submit-corrections')).toHaveCount(0)
})

test('a tampered total is stored as the number the rules produce', async ({ page }) => {
  await signIn(page)

  /** What the server answered the browser, captured off the wire. */
  let stored: Record<string, unknown> | undefined

  /**
   * The man in the middle. Everything computed is claimed as something else,
   * and a value is smuggled in for a field the conditional hides.
   *
   * `route.fetch` rather than `route.continue`, so the *response* is in hand:
   * the page compares what came back against what **it** sent, and it sent 42 —
   * so a request rewritten behind its back is correctly invisible to it. What
   * this test needs is the server's own answer to the tampered payload, and
   * that is the body travelling back over the wire. It is then fulfilled
   * unchanged, so the page carries on as it would have.
   *
   * Installed before the page is opened, so nothing races it.
   */
  await page.route('**/rad/forms/*/submissions', async (route) => {
    const body = route.request().postDataJSON() as { payload: Record<string, unknown> }
    const rows = (body.payload.line_items as Record<string, unknown>[]) ?? []

    body.payload.grand_total = 1
    body.payload.line_items = rows.map((row) => ({ ...row, line_total: 0, line_no: 99 }))
    body.payload.justification = 'smuggled past a conditional that hides this field'

    const response = await route.fetch({ postData: JSON.stringify(body) })
    const answered = (await response.json()) as {
      data?: { payload?: Record<string, unknown> }
    }

    stored = answered.data?.payload

    await route.fulfill({ response })
  })

  await page.goto(`/forms/${form.id}`)
  await expect(page.getByTestId('form-title')).toHaveText('Purchase requisition (submit)')

  await fillInTwoLines(page)
  await page.getByRole('button', { name: 'Submit request' }).click()

  await expect(page.getByTestId('submit-success')).toContainText('revision')

  // **The security control, end to end.** The request that reached the server
  // said the two lines were worth nothing and the grand total was 1. The rules
  // say 42, and 42 is what came back — which is what was stored, because the
  // response is the row read back (`service::submission::submit_form`).
  expect(stored, 'the server answered with a payload').toBeTruthy()
  expect(stored?.grand_total).toBe(42)

  const lines = stored?.line_items as Record<string, unknown>[]

  expect(lines[0].line_total).toBe(20)
  expect(lines[1].line_total).toBe(22)

  // The row numbers are the server's (JFSS §9.2's sequence overwrite).
  expect(lines[0].line_no).toBe(1)
  expect(lines[1].line_no).toBe(2)

  // And the value smuggled in for the hidden field is not in the stored row at
  // all (S10.2): the budget is 500 and `justification` needs 1,000.
  expect(stored?.justification).toBeUndefined()

  // The page, meanwhile, says nothing about a correction — and that is right.
  // It sent 42 and 42 came back; it has no claim to make about a payload it did
  // not produce.
  await expect(page.getByTestId('submit-corrections')).toHaveCount(0)
})
