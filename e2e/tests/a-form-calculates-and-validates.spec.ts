import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

import { expect, test } from '@playwright/test'

import { createSupplier, runSuffix, signInOverApi, type ApiSession } from '../support/api'
import { credentials } from '../support/env'
import { publishForm, type SeededForm } from '../support/forms'

/**
 * A rendered form does its own arithmetic and refuses its own bad data
 * (#163 AC5).
 *
 * **This is the criterion that decides whether the item is Done.** Component
 * tests cover the rules over a payload — `useFormEvaluation.spec.ts` has thirty
 * of them — and none of them proves that a number typed into a real browser
 * reaches a WebAssembly module that was fetched over HTTP and comes back as a
 * total on the screen. That is the gap **D-14** and #101 exist because of, and
 * #162 AC5 makes a status row reading `verified by inspection only` a finding
 * rather than a formatting choice.
 *
 * **The same fixture the unit tests use**, read rather than copied, for the
 * reason `render-a-form.spec.ts` gives: the copy that drifts is always the one
 * only the slow suite reads.
 *
 * **A supplier is seeded because the form requires one.** `supplier_id` is
 * `required` in the definition, so a form that cannot offer a supplier cannot
 * be submitted at all — and the accepted path is half of what this file is
 * about. `render-a-form.spec.ts` deliberately asserts nothing about what master
 * data holds; this one has to.
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
  code: `E2E-RAD-${suffix}`,
  name: `Orion Office Supplies ${suffix}`,
  supplierNumber: `SUP-O-${suffix}`,
}

let session: ApiSession
let form: SeededForm

test.beforeAll(async () => {
  session = await signInOverApi()

  await createSupplier(session, supplier)
  form = await publishForm(session, definition, 'Purchase requisition (rules)')
})

test.afterAll(async () => {
  await session?.context.dispose()
})

test('a form calculates as it is typed into, and refuses to be submitted incomplete', async ({
  page,
}) => {
  const { username, password } = credentials()

  await page.goto('/login')
  await page.getByLabel('Username or email').fill(username)
  await page.getByLabel('Password').fill(password)
  await page.getByRole('button', { name: 'Sign in' }).click()

  await expect(page).toHaveURL(/\/$/)

  await page.goto(`/forms/${form.id}`)
  await expect(page.getByTestId('form-title')).toHaveText('Purchase requisition (rules)')

  // --- Nothing is red before anything has been typed (§5.6) ----------------
  //
  // Construction plan §5.6, and the rule D-24 needed beside it: a form that
  // greets its user with fields it filled in itself and then marked wrong has
  // told them nothing.
  await expect(page.getByTestId('field-error')).toHaveCount(0)

  // --- A line total, computed in the browser (AC2) -------------------------
  //
  // `defaultItems: 1`, so row one is already there. `line_total` carries
  // `calculate` and no `readOnly`; S4.2.3 Case B makes it read-only anyway,
  // which is asserted here because a definition that forgets the property must
  // not get an input whose value is overwritten as it is typed into.
  await expect(page.locator('#jfss-row-0-line-total')).toBeDisabled()

  await page.locator('#jfss-row-0-line-quantity').fill('2')
  await page.locator('#jfss-row-0-line-unit-price').fill('10')

  await expect(page.locator('#jfss-row-0-line-total')).toHaveValue('20')

  // --- And the registry §6.1 invoice total over `sum` and `map` (AC2) ------
  //
  // Two lines worth 20 and 22. **42 is the figure the whole Tamper-Proof
  // argument is built on**, and the figure the operator-parity spike watched
  // become a silent 0 on an engine that returned unknown operators instead of
  // rejecting them. `sum` here is the custom operator `lib/jsonlogic.ts`
  // registers — Calculation Rule Registry §4.3 forbids a second one, and
  // `jsonlogic.spec.ts` holds that from the other side.
  await page.getByRole('button', { name: 'Add row' }).click()
  await page.locator('#jfss-row-1-line-quantity').fill('2')
  await page.locator('#jfss-row-1-line-unit-price').fill('11')

  await expect(page.locator('#jfss-grand-total-field')).toHaveValue('42')

  // The `paragraph` beside it takes its text from an expression rather than
  // from `content` (JFSS §4.4), and shows the number with nothing added to it.
  await expect(page.getByText('42', { exact: true })).toBeVisible()

  // --- The two calculate modes, side by side (§5.1) ------------------------
  //
  // `baseline_budget` reads `budget` under `calculateMode: "generated"` and
  // `grand_total` is `derived`. Typing a budget moves neither: the generated
  // field resolved once and keeps its figure, and the derived field is over the
  // lines rather than over the budget. The mode is **declared** and never
  // inferred from the operators, which is S8.1.1.
  await page.locator('#jfss-budget-field').fill('5000')

  await expect(page.locator('#jfss-baseline-field')).toHaveValue('0')
  await expect(page.locator('#jfss-grand-total-field')).toHaveValue('42')

  // --- A conditional opens a branch (JFSS §7) ------------------------------
  //
  // The budget above 1,000 is what `justification`'s `conditional` asks for. It
  // was absent from the page before, and asserting both states is what
  // distinguishes a working conditional from a field that never rendered.
  await page.getByRole('tab', { name: 'Notes' }).click()
  await expect(page.locator('#jfss-justification-field')).toBeVisible()

  // --- The form refuses to be submitted incomplete (AC1) -------------------
  //
  // Nothing but the lines and the budget has been filled in. The messages are
  // the *definition's* — `validation.messages.required` on `title`, and the
  // `regex` rule's own `message` further down — not sentences the renderer
  // invented.
  await page.getByRole('button', { name: 'Submit request' }).click()

  await expect(page.getByTestId('form-action')).toHaveCount(0)
  await expect(page.locator('#jfss-title-field-error')).toHaveText('Every request needs a title.')
  await expect(page.locator('#jfss-justification-field-error')).toHaveText(
    'A request over 1,000 needs a justification.',
  )

  // --- An advanced rule from the registry, decided in the browser ----------
  //
  // `regex`, scoped `both` (Validation Rule Registry §3.1). The pattern pins
  // its digit class to `[0-9]`, which is the registry's interim guidance:
  // ECMA-262 reads `\d` as ASCII and the Rust crate reads it as Unicode `Nd`,
  // so the two sides of a `both` rule would otherwise disagree with nothing
  // raised on either.
  await page.locator('#jfss-cost-centre-field').fill('fn-142')
  await expect(page.locator('#jfss-cost-centre-field-error')).toHaveText(
    'A cost centre looks like FN-0142.',
  )

  await page.locator('#jfss-cost-centre-field').fill('FN-0142')
  await expect(page.locator('#jfss-cost-centre-field-error')).toHaveCount(0)

  // --- Fill it in, and the submit is let through ---------------------------
  //
  // The other half of AC1: a form that refuses everything is not validating, it
  // is broken. #164 is what a passed submit then *does*; that it happens at all
  // is this issue's.
  await page.getByLabel('Title', { exact: false }).fill('Two standing desks')

  // The chooser is searched down to the one seeded supplier before it is
  // selected. Picking by position out of whatever master data happens to hold
  // would pass on a fresh database and pick somebody else's row on a reused one
  // — and the deployment the harness drives keeps its database between runs,
  // which is the reason `runSuffix` exists at all. The search runs on the
  // server (`role_view.rs` matches the code, the name and the role number), so
  // this exercises #161's endpoint as well as the field.
  await page.getByLabel('Search Supplier').fill(supplier.code)
  await expect(page.locator('#jfss-supplier-field option')).toHaveCount(2)
  await page.locator('#jfss-supplier-field').selectOption({ index: 1 })

  await page.locator('#jfss-priority-field').selectOption({ label: 'Normal' })
  await page.locator('#jfss-justification-field').fill('The current desks are failing.')

  await page.getByRole('tab', { name: 'Lines' }).click()
  await page.locator('#jfss-row-0-line-description').fill('Standing desk')
  await page.locator('#jfss-row-1-line-description').fill('Desk mat')

  await page.getByRole('button', { name: 'Submit request' }).click()

  await expect(page.getByTestId('form-action')).toContainText('submit')
})
