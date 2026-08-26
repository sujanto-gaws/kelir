import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

import { expect, test } from '@playwright/test'

import { signInOverApi, type ApiSession } from '../support/api'
import { credentials } from '../support/env'
import { publishForm, type SeededForm } from '../support/forms'

/**
 * The fixture the unit tests use, read rather than imported.
 *
 * A second copy of a JFSS document is a second thing to keep in step, and the
 * one that drifts is always the one only the slow suite reads — so this is the
 * same file `JfssRenderer.spec.ts` mounts.
 *
 * `readFileSync` rather than `import … from '…json'`: this package is ESM, and
 * Node requires an import attribute for a JSON module. `tsc --noEmit` resolves
 * the bare import happily, so the type check passes and the spec then fails at
 * run time with `needs an import attribute of "type: json"` — which is exactly
 * how it failed in CI. Reading the file has no such gap between the two.
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

/**
 * A published definition becomes a form a person can fill in (#162 AC4).
 *
 * **This is the criterion that decides whether the item is Done.** AC5 says the
 * Sprint 8 status row for #162 must not read `verified by inspection only`, and
 * that a row which does is a finding rather than a formatting choice. Component
 * tests cover the mapping from a definition to a component tree; only this
 * covers the definition travelling over HTTP into a real browser and coming
 * back as controls a person can type into — which is the gap **D-14** and #101
 * exist because of.
 *
 * **The same fixture the unit tests use**, imported rather than copied. A
 * second copy of a JFSS document is a second thing to keep in step, and the one
 * that drifts is always the one only the slow suite reads.
 */

let session: ApiSession
let form: SeededForm

test.beforeAll(async () => {
  session = await signInOverApi()
  form = await publishForm(session, definition, 'Purchase requisition (e2e)')
})

test.afterAll(async () => {
  await session?.context.dispose()
})

test('a published definition renders as a form, and every part of it comes from the definition', async ({
  page,
}) => {
  const { username, password } = credentials()

  // --- Sign in -------------------------------------------------------------
  await page.goto('/login')
  await page.getByLabel('Username or email').fill(username)
  await page.getByLabel('Password').fill(password)
  await page.getByRole('button', { name: 'Sign in' }).click()

  await expect(page).toHaveURL(/\/$/)

  // --- Open the form -------------------------------------------------------
  //
  // By URL, because there is no list screen to reach it from yet: the form list
  // is a builder surface (FR-RAD-004) and stays in Phase 7 under D-2. What is
  // being exercised is the route and the renderer, both of which are #162's.
  await page.goto(`/forms/${form.id}`)

  await expect(page.getByTestId('form-title')).toHaveText('Purchase requisition (e2e)')

  // --- The definition's labels, help text and required markers (AC3) -------
  //
  // Found by their accessible label rather than by CSS: a control the browser
  // cannot associate with its label is a control a screen reader cannot use,
  // so `getByLabel` asserts the wiring and the text at once.
  await expect(page.getByLabel('Title', { exact: false })).toBeVisible()
  await expect(
    page.getByText('A short description other people will search for.'),
  ).toBeVisible()

  // `title` is required in the definition and `needed_by` is not. Both markers
  // are asserted, because only the pair proves the marker is read from
  // `validation.required` rather than drawn on every field.
  await expect(page.locator('label[for="jfss-title-field"]')).toContainText('*')
  await expect(page.locator('label[for="jfss-needed-by-field"]')).not.toContainText('*')

  // --- Layout comes from the definition (AC3) ------------------------------
  //
  // A `columns` container and a `tabs` container, both of which a renderer that
  // walked only `components` would have dropped silently (JFSS §4.3.1).
  //
  // `exact` on Budget: the same column also holds "Baseline budget", and
  // `getByLabel` matches on substring by default — so the non-exact form
  // resolves to two controls and fails on strict mode rather than on the thing
  // it is asserting.
  await expect(page.getByLabel('Budget', { exact: true })).toBeVisible()
  await expect(page.getByLabel('Priority')).toBeVisible()
  await expect(page.getByRole('tab', { name: 'Lines' })).toBeVisible()
  await expect(page.getByRole('tab', { name: 'Notes' })).toBeVisible()

  // --- It is a form, not a picture of one ----------------------------------
  await page.getByLabel('Title', { exact: false }).fill('Two standing desks')
  await expect(page.getByLabel('Title', { exact: false })).toHaveValue('Two standing desks')

  // The select's options are the definition's four, in its order.
  await expect(page.getByLabel('Priority').locator('option')).toContainText([
    'Low',
    'Normal',
    'High',
    'Urgent',
  ])

  // --- The repeater repeats (JFSS §4.3.1) ----------------------------------
  //
  // `defaultItems: 1`, so one row is present before anything is clicked, and
  // `sequenceKey` has filled the line number. A template rendered once as
  // ordinary fields would show the labels and no row heading at all.
  await expect(page.getByText('Row 1')).toBeVisible()
  await expect(page.locator('#jfss-row-0-line-no')).toHaveValue('1')

  await page.getByRole('button', { name: 'Add row' }).click()
  await expect(page.getByText('Row 2')).toBeVisible()

  // A second row does not collide with the first. `id` is unique per component
  // *instance* in the definition (§4.1), and a repeater renders one instance
  // once per row — so without a per-row scope both rows claim `jfss-line-no`,
  // every row's label points at row one's input, and a radio group's shared
  // `name` makes choosing in row two clear row one.
  await expect(page.locator('#jfss-row-1-line-no')).toHaveValue('2')

  // --- The second tab's fields exist before the tab is opened --------------
  //
  // Hidden, not absent: validation reads the whole tree and #164 submits it, so
  // a required field on an unopened tab must still count.
  //
  // By role as well as by name. The fixture has a tab called "Notes" holding a
  // field called "Notes", and the panel takes its accessible name from its tab
  // — correct ARIA, and two elements answering to `getByLabel('Notes')`. The
  // role is what says which one is meant, and a form whose tab shares a name
  // with one of its fields is ordinary rather than contrived.
  await page.getByRole('tab', { name: 'Notes' }).click()
  await expect(page.getByRole('textbox', { name: 'Notes' })).toBeVisible()
  await expect(page.getByRole('checkbox', { name: 'This request is urgent' })).toBeVisible()

  // --- The lookup reached the server (FR-RAD-007, #161) --------------------
  //
  // The chooser is present and enabled, which means the request completed:
  // `LookupField` disables the select while loading and replaces it with a
  // failure message if the fetch throws. What master data happens to hold is
  // not asserted — `rad_lookups.rs` covers the endpoint, and a seeded supplier
  // here would be testing the fixture.
  await expect(page.locator('#jfss-supplier-field')).toBeEnabled()

  // --- An action reaches the form, and the form's own rules answer it ------
  //
  // Submitting is #164; whether a submission may happen at all is the
  // definition's rules, which #163 made this form evaluate. So a click on a
  // form nobody has filled in is *received* and *refused*, and the refusal is
  // the evidence the button is wired — the accepted path is
  // `a-form-calculates-and-validates.spec.ts`, which seeds a supplier and fills
  // the form in properly.
  await page.getByRole('button', { name: 'Submit request' }).click()

  await expect(page.getByText('Every request needs a title.')).toHaveCount(0)
  await expect(page.locator('#jfss-supplier-field-error')).toBeVisible()
  await expect(page.getByTestId('form-action')).toHaveCount(0)
})
