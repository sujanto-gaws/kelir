import { expect, test } from '@playwright/test'

import { runSuffix, signInOverApi, type ApiSession } from '../support/api'
import { credentials } from '../support/env'
import { publishForm, type SeededForm } from '../support/forms'

/**
 * A pattern the browser gives up on is left to the server (#496).
 *
 * **This spec runs in Firefox only**, and `playwright.config.ts` scopes it so.
 * Firefox is the engine that *throws* on this pattern and value, with *too much
 * recursion*, and a throw is the give-up the renderer can see. Before #496 the
 * throw was a violation, and a value the server accepts could not be submitted
 * from Firefox at all. Chromium is not given this spec because it does not
 * throw here: it keeps matching, and the tab is busy for as long as it does,
 * which this row does not change.
 *
 * **The pattern and value are the issue's first row**, measured with
 * `scripts/pattern-parity/backtrack.mjs`. The first assertion re-checks, in
 * the page, that this Firefox still throws on them. If a later Firefox stops
 * throwing, that assertion fails and says so, rather than this spec passing
 * without having tested anything.
 *
 * **Both paths carry the pattern**: `validation.pattern` on one field and a
 * `regex` rule on the other, because the renderer runs each separately.
 */
const suffix = runSuffix()

const pattern = '(?:[a-z]|[a-z0-9])*$'
const value = `${'a'.repeat(28)}!`

const definition = {
  formId: `e2e-give-up-${suffix}`,
  version: '2.0.1',
  title: 'A pattern the browser gives up on',
  components: [
    {
      id: 'handle-field',
      role: 'data',
      type: 'textfield',
      key: 'handle',
      label: 'Handle',
      validation: { type: 'string', required: true, pattern },
    },
    {
      id: 'alias-field',
      role: 'data',
      type: 'textfield',
      key: 'alias',
      label: 'Alias',
      validation: { type: 'string', required: true },
      rules: [
        {
          rule: 'regex',
          scope: 'both',
          params: { pattern },
          message: 'An alias is lower-case letters and digits.',
        },
      ],
    },
    {
      id: 'submit-button',
      role: 'action',
      type: 'button',
      label: 'Submit',
      action: 'submit',
    },
  ],
}

const title = `A pattern the browser gives up on ${suffix}`

let session: ApiSession
let form: SeededForm

test.beforeAll(async () => {
  session = await signInOverApi()

  form = await publishForm(session, definition, title)
})

test.afterAll(async () => {
  await session?.context.dispose()
})

test('a value Firefox gives up matching is submitted, and the server decides it', async ({
  page,
}) => {
  // **Firefox spends five to seven seconds on each match before it throws**,
  // measured 2026-09-26 in Playwright's Firefox 153, and #496 does not change
  // that cost. The form matches each field again as values change and when
  // the submit reveals its messages, about five matches in all, with the page
  // busy for each. So this flow gets three minutes and its waits a minute each,
  // rather than the suite's sixty and fifteen seconds.
  test.setTimeout(180_000)

  const busy = { timeout: 60_000 }
  const { username, password } = credentials()

  await page.goto('/login')
  await page.getByLabel('Username or email').fill(username)
  await page.getByLabel('Password').fill(password)
  await page.getByRole('button', { name: 'Sign in' }).click()

  await expect(page).toHaveURL(/\/$/)

  // --- The precondition: this browser throws on this match ------------------
  //
  // Compiled first and matched second, so a pattern this browser cannot build
  // is not mistaken for the throw this spec is about.
  const thrown = await page.evaluate(
    ([source, input]) => {
      const compiled = new RegExp(source)

      try {
        compiled.test(input)

        return undefined
      } catch (error) {
        return String(error)
      }
    },
    [pattern, value],
  )

  expect(thrown, 'this Firefox still throws on the issue-496 pattern').toContain(
    'too much recursion',
  )

  // --- The form does not refuse it ------------------------------------------
  await page.goto(`/forms/${form.id}`)
  await expect(page.getByTestId('form-title')).toHaveText(title)

  await page.locator('#jfss-handle-field').fill(value)
  await page.locator('#jfss-alias-field').fill(value)

  await page.getByRole('button', { name: 'Submit' }).click()

  // --- And the server decides it --------------------------------------------
  //
  // Waited for first, because it is what a refusal would prevent: before #496
  // the form refused both fields and nothing was sent. The server's `regex`
  // crate matches this value, so the submission is stored.
  await expect(page.getByTestId('submit-success')).toContainText('revision', busy)

  // Checked after the submit has settled, so an empty list is the form's
  // answer and not a render that has not happened yet. A refusal from either
  // side would show here as a field error.
  await expect(page.getByTestId('field-error')).toHaveCount(0, busy)

  // It says what it left undecided rather than passing it quietly: the keyword
  // and the rule, each named. The names are read from their `code` elements,
  // because the reason beside each one also says "pattern".
  await expect(page.getByTestId('form-undecided').locator('code')).toHaveText(
    ['pattern', 'regex'],
    busy,
  )
})
