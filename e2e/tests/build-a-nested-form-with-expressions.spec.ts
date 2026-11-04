import { expect, test, type Locator, type Page } from '@playwright/test'

import { runSuffix, signInOverApi, type ApiSession } from '../support/api'
import { chooseDocumentType } from '../support/choosers'
import { createDocumentType } from '../support/documents'
import { API_PREFIX, credentials } from '../support/env'
import { tabTo } from '../support/keyboard'

/**
 * An administrator builds a nested form with expressions through the screen, and
 * a document raised against it computes, hides, refuses and numbers what the
 * builder wrote (#688 AC13–AC15, FR-RAD-013; I2's second half and I4).
 *
 * **The assertion is not that the properties panel renders.** It is the rule
 * `build-a-form.spec.ts` follows: *a criterion that says X can is not met by an
 * endpoint*. So the first test ends on **a document whose behaviour only the
 * expressions built here could have produced**: a total that is the product of
 * two fields, a note that shows above a threshold and not at it, a length limit
 * that refuses, and a grid that opens with its rows numbered. The form is
 * authored, saved and published in the browser; only the document type and its
 * numbering rule are seeded over the API, because the type screen is
 * `configure-a-document-type.spec.ts`'s subject.
 *
 * # The first test: the build, by AC13
 *
 * 1. The created text field becomes `reference`, with a `maxLength` of 12.
 * 2. A panel holds the number fields `quantity` and `unit_price`, and a number
 *    `total` whose `calculate`, `quantity × unit_price`, is built in the logic
 *    builder **by keyboard alone**. The builder offers the calculate tier only
 *    (no comparison), and the variables in scope without `total` itself. The
 *    preview computes it.
 * 3. A text area `notes` gets a `conditional`, `total > 1000`, built by keyboard
 *    with the threshold **typed as a number operand**, one digit at a time.
 * 4. A data grid `lines` gets a row template of `line_no` and `item`, a
 *    `sequenceKey` of `line_no` and `defaultItems: 2`.
 * 5. A submit button, then Save and Publish.
 * 6. The stored definition is read back over the API, and the published
 *    builder draws the same expressions.
 * 7. A document is raised against it: the grid opens with rows numbered 1 and
 *    2; 4 × 250 shows `total` 1000 and no `notes`; 5 × 250 shows 1250 and
 *    `notes`; a 15-character reference is refused and the submit with it does
 *    nothing; a 10-character one passes, and the document submits.
 *
 * # The logic builder, by keyboard alone (AC14)
 *
 * Every control of both builders is reached with `tabTo`, which fails when a
 * control is not in the tab order, and operated with typeahead, arrows,
 * typing, Enter and Space. **The number is typed one key at a time**, and
 * after each key the test asserts that the same input still has focus and
 * holds every digit typed. Each digit completes the expression, so each one is
 * emitted, written through the page's `replaceNode`, which clones the whole
 * definition, and handed back to the builder.
 *
 * **An unfinished operand survives a write elsewhere in the panel.** The
 * number is emptied, which leaves the definition holding the last complete
 * condition, and the description is then typed into, key by key, which writes
 * a new definition on every key. The empty operand must still be empty, and
 * the field must still say it is unfinished.
 *
 * # The second test: the raw boxes, caret and text after every key
 *
 * jsdom cannot model a caret, so row 2 named this the riskiest path left
 * untested. Two boxes are typed into, one key at a time, and **after every key
 * the box must hold focus, the exact text typed, and the caret where the key
 * left it**:
 *
 * - **An expression's raw JSON box** (`Edit Calculate as JSON`). Typed from
 *   empty, so a box that remounted on its own value coming back would show the
 *   pretty-printed JSON instead of the compact text typed. Then edited in the
 *   middle, through states that do and do not parse.
 * - **A rule's `params` box**, after *regex* seeds it. Emptied, which writes
 *   `{}`, and retyped compact for the same reason. Then edited in the middle,
 *   through a state that does not parse and back.
 *
 * Each is then shown to have reached the definition: the visual builder draws
 * the typed expression, the preview computes it and enforces the typed
 * pattern, and the save reads both back.
 *
 * # The third test: a data grid's row reads its own row (#733)
 *
 * #732 offers a row's fields the row's keys (AC6, `expressionScope.ts`), and
 * the server evaluates each row against its own object (`rad::domain::engine`,
 * JFSS §4.3.1). jsdom proves the variable lists, and only a browser proves that
 * what the builder writes for a row is what the row's scope reads:
 *
 * 1. Two top-level fields, `reference` and `budget`, and a data grid `lines`
 *    of two rows.
 * 2. A row field `line_total` whose `calculate`, `qty × price`, is built from
 *    the variables offered: the row's keys, and no top-level one.
 * 3. A row field `bulk_reason`, shown when its row's `qty` is above 10.
 * 4. A row field `budget_note` whose condition reads the top-level `budget`,
 *    typed as JSON because it is not offered. The builder badges it, which is
 *    #732's only signal that it is always false at runtime.
 * 5. Save and Publish, and 6. the stored definition read back.
 * 7. A document: 12 × 5 and 3 × 7 show 60 and 21, `bulk_reason` shows in the
 *    row of 12 and not the row of 3, and `budget_note` shows in neither, with
 *    `budget` at 5000. **The save is rewritten in flight**, each row's total
 *    claimed as 0 and a value smuggled into each hidden field, so the server's
 *    answer proves its row scope: it stores 60 and 21, keeps `bulk_reason` in
 *    the row of 12, and leaves the smuggled values out. A server evaluating a
 *    row against the form would store neither total, drop that `bulk_reason`,
 *    and keep `budget_note`.
 *
 * # No drag (AC15)
 *
 * Nothing here is dragged. Every node is added from the palette, which adds to
 * the selected container or the container of the selected node.
 *
 * # Seen red against a broken builder
 *
 * 2026-10-15, on a throwaway stack. Each break was run against a frontend
 * image built from the mutated source, then restored with `git checkout --`.
 * Both tests were green again on the image rebuilt without it.
 *
 * - **Both builders keyed by their expression's JSON** (`:key` on each
 *   `JsonLogicBuilder` in `ExpressionProperties.vue`, so every emission
 *   remounts it). The first test fails in step 2: the calculate is complete,
 *   but `calculate-unfinished-field_4` is still shown, because the remounted
 *   builder never reports its validity. The second test fails at the raw box's
 *   last key, `}`, the first key that parses: the box is gone.
 * - **Only the conditional builder keyed by its JSON.** The first test fails
 *   in step 3, at the threshold's first digit: after the key `1` the number
 *   input holds `1` and **no longer has focus**.
 * - **`calculate` mounted in the conditional tier** (`tier="conditional"`).
 *   The first test fails in step 2, at the calculate builder's kind chooser,
 *   which offers *Comparison*, *All of / any of* and *Not* as well.
 * - **`calculate` never written** (`setCalculate` emits nothing). The first
 *   test fails in step 2, where the preview's `total` stays empty for
 *   4 × 250. The second test fails where the preview's `score` stays empty
 *   for the typed `{"+": [1, 2]}`.
 * - **The builder taking its own echo for a new value** (`isOwnEcho` in
 *   `JsonLogicBuilder.vue` always false). The first test fails at the
 *   threshold's `1`, where focus is lost. The second fails at the raw box's
 *   `}`, where the box is gone.
 * - **The raw box remounted on every value it parses** (`generation` bumped
 *   in `onRawParsed`). The second test fails at `}`: the box holds the value
 *   pretty-printed over six lines, not the text typed, and has lost focus.
 * - **The params box rewriting its text whenever the rule's params change**
 *   (the `watch` in `RuleParamsBox.vue` with its echo check removed). The
 *   second test fails where the box is emptied: it holds `{}`, with the caret
 *   at 2.
 *
 * The third test, 2026-11-04 (#733), each break on its own image, and green
 * again on the image built before them. The first two tests passed against the
 * first break, which is the gap #733 named:
 *
 * - **A builder that offers the top level inside a row** (`scopeListOf` in
 *   `expressionScope.ts` never switching to a row template). The third test
 *   fails in step 2: `line_total`'s first operand offers *Reference*, *Budget*
 *   and *Lines*, not *Quantity* and *Price*.
 * - **A renderer that calculates a row against the form** (`applyCalculations`
 *   in `useFormEvaluation.ts` walking a row's template with the form's values).
 *   The third test fails in step 7: the first row's `line_total` stays empty
 *   for 12 × 5.
 */

const suffix = runSuffix()

const THRESHOLD = 1000
const MAX_LENGTH = 12

/** What the keyboard builds, as the server must store it. */
const CALCULATE = { '*': [{ var: 'quantity' }, { var: 'unit_price' }] }
const CONDITION = { '>': [{ var: 'total' }, THRESHOLD] }

let session: ApiSession

test.beforeAll(async () => {
  session = await signInOverApi()
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

/** Creates a form through the screen, and lands in its builder on `field_1`. */
async function createForm(page: Page, formKey: string, title: string): Promise<string> {
  await page.goto('/admin/forms')
  await page.getByTestId('new-form').click()
  await page.getByTestId('form-key').fill(formKey)
  await page.getByTestId('form-title').fill(title)
  await page.getByTestId('save-form').click()
  await expect(page).toHaveURL(/\/admin\/forms\/[0-9a-f-]{36}$/)
  await expect(page.getByTestId('component-field_1')).toBeVisible()

  return new URL(page.url()).pathname.split('/').pop() as string
}

type Node = Record<string, unknown> & { components?: Node[] }

async function storedDefinition(id: string): Promise<{ components: Node[] }> {
  const response = await session.context.get(`${API_PREFIX}/rad/forms/${id}`)

  expect(response.ok(), await response.text()).toBe(true)

  return ((await response.json()) as { data: { definition: { components: Node[] } } }).data
    .definition
}

/** A control of a logic builder, by the accessible name the builder gives it. */
function control(page: Page, name: string): Locator {
  return page.getByLabel(name, { exact: true })
}

/** Adds a node from the palette and waits for the canvas to select it. */
async function addFromPalette(page: Page, type: string, id: string): Promise<void> {
  await page.getByTestId(`palette-${type}`).click()
  await expect(page.getByTestId(`select-${id}`)).toHaveAttribute('aria-pressed', 'true')
}

/** Names a data node: its key and label, in the properties panel. */
async function name(page: Page, id: string, key: string, label: string): Promise<void> {
  await page.getByTestId(`key-${id}`).fill(key)
  await page.getByTestId(`label-${id}`).fill(label)
}

/**
 * What a text box holds, and where its caret is. A box that has left the page
 * reads as such at once, so the assertion after a key names the key rather
 * than the test timing out.
 */
async function caretOf(
  box: Locator,
): Promise<{ focused: boolean; text: string; start: number | null; end: number | null }> {
  return box
    .evaluate(
      (element) => {
        const input = element as HTMLInputElement | HTMLTextAreaElement

        return {
          focused: input === document.activeElement,
          text: input.value,
          start: input.selectionStart,
          end: input.selectionEnd,
        }
      },
      undefined,
      { timeout: 2_000 },
    )
    .catch(() => ({ focused: false, text: '(the box is not on the page)', start: null, end: null }))
}

/**
 * Types `keys` into the focused `box` one key at a time, starting from `text`
 * with the caret at `at`. **After every key** the box must still have focus,
 * hold exactly the text typed so far, and have its caret just after the key.
 * Returns the text and caret it ends with.
 */
async function typeKeyByKey(
  page: Page,
  box: Locator,
  keys: string,
  text: string,
  at: number,
): Promise<{ text: string; at: number }> {
  let expected = text
  let caret = at

  for (const key of keys) {
    await page.keyboard.type(key)
    expected = `${expected.slice(0, caret)}${key}${expected.slice(caret)}`
    caret += 1

    expect(await caretOf(box), `after the key ${JSON.stringify(key)}`).toEqual({
      focused: true,
      text: expected,
      start: caret,
      end: caret,
    })
  }

  return { text: expected, at: caret }
}

/** Backspace, once, with the same check after the key. */
async function backspace(
  page: Page,
  box: Locator,
  text: string,
  at: number,
): Promise<{ text: string; at: number }> {
  await page.keyboard.press('Backspace')

  const expected = `${text.slice(0, at - 1)}${text.slice(at)}`

  expect(await caretOf(box), 'after Backspace').toEqual({
    focused: true,
    text: expected,
    start: at - 1,
    end: at - 1,
  })

  return { text: expected, at: at - 1 }
}

test('an administrator builds a nested form with expressions, and a document obeys them', async ({
  page,
}) => {
  await signIn(page)

  const formKey = `e2e_expr_${suffix}`.toLowerCase()
  const formTitle = `Expressions ${suffix}`
  const formId = await createForm(page, formKey, formTitle)
  const preview = page.getByTestId('form-preview')

  // --- 1. The created text field: `reference`, at most 12 characters -----------
  await name(page, 'field_1', 'reference', 'Reference')
  await page.getByTestId('max-length-field_1').fill(String(MAX_LENGTH))

  // --- 2. A panel of quantity, unit price and a calculated total ---------------
  //
  // The panel is selected once it is added, so the palette adds into it, and a
  // field in it once selected keeps the palette aimed at the panel.
  await addFromPalette(page, 'panel', 'panel_1')
  await page.getByTestId('title-panel_1').fill('Amounts')

  await addFromPalette(page, 'number', 'field_2')
  await name(page, 'field_2', 'quantity', 'Quantity')
  await addFromPalette(page, 'number', 'field_3')
  await name(page, 'field_3', 'unit_price', 'Unit price')
  await addFromPalette(page, 'number', 'field_4')
  await name(page, 'field_4', 'total', 'Total')

  const panelCard = page.getByTestId('component-panel_1')

  for (const id of ['field_2', 'field_3', 'field_4']) {
    await expect(panelCard.getByTestId(`component-${id}`)).toBeVisible()
  }

  // **`total`'s calculate, by keyboard alone.** The keyboard starts at the
  // label just typed into; from there it is Tab.
  const calculateKind = control(page, 'Calculate: kind')

  await tabTo(page, calculateKind)
  // The calculate tier: a variable or arithmetic, and **no comparison**
  // (Calculation Rule Registry §2.1 and §2.3). Pins the tier.
  await expect(calculateKind.locator('option')).toHaveText([
    'No expression',
    'Variable',
    'Arithmetic',
  ])

  // Typeahead: "a" is *Arithmetic*. The chooser is replaced by the expression it
  // started, `+` with two operands unfilled, so focus moves on.
  await page.keyboard.press('a')
  await expect(calculateKind).toHaveValue('arithmetic')
  await expect(page.getByTestId('calculate-unfinished-field_4')).toBeVisible()

  const operator = control(page, 'Calculate: operator')

  await tabTo(page, operator)
  await expect(operator.locator('option')).toHaveText([
    'plus (+)',
    'minus (-)',
    'times (*)',
    'divided by (/)',
    'remainder of (%)',
  ])
  // "t" is *times*. A native select commits a typeahead choice with no popup.
  await page.keyboard.press('t')
  await expect(operator).toHaveValue('*')
  await expect(operator).toBeFocused()

  // Operand 1: a variable. The tab order inside an operand is fixed, so each
  // stop is asserted.
  await tabTo(page, control(page, 'Calculate, operand 1: kind'))
  await page.keyboard.press('v')
  await expect(control(page, 'Calculate, operand 1: kind')).toHaveValue('var')
  await expect(control(page, 'Calculate, operand 1: kind')).toBeFocused()

  await page.keyboard.press('Tab')
  const operand1 = control(page, 'Calculate, operand 1: variable')

  await expect(operand1).toBeFocused()
  // **The scope's data keys, and not `total` itself** (AC6): the server
  // refuses a calculation that reads its own field.
  await expect(operand1.locator('option')).toHaveText([
    'Choose a variable',
    'Reference — reference (string)',
    'Quantity — quantity (number)',
    'Unit price — unit_price (number)',
  ])
  await page.keyboard.press('q')
  await expect(operand1).toHaveValue('quantity')

  // Operand 2: the unit price.
  await page.keyboard.press('Tab')
  await expect(control(page, 'Calculate, operand 2: kind')).toBeFocused()
  await page.keyboard.press('v')
  await page.keyboard.press('Tab')
  await expect(control(page, 'Calculate, operand 2: variable')).toBeFocused()
  await page.keyboard.press('u')
  await expect(control(page, 'Calculate, operand 2: variable')).toHaveValue('unit_price')

  await expect(page.getByTestId('calculate-unfinished-field_4')).toHaveCount(0)

  // **The preview computes it**: the renderer a document uses, reading the
  // definition the panel wrote. A `calculate` never written leaves it empty.
  await preview.locator('#jfss-field_2').fill('4')
  await preview.locator('#jfss-field_3').fill('250')
  await expect(preview.locator('#jfss-field_4')).toHaveValue('1000')

  // --- 3. Notes, shown above a threshold typed as a number -------------------
  //
  // `reference` is at the form's root and is not a container, so with it
  // selected the palette adds to the form.
  await page.getByTestId('select-field_1').click()
  await addFromPalette(page, 'textarea', 'field_5')
  await name(page, 'field_5', 'notes', 'Notes')

  const conditionKind = control(page, 'Conditional logic: kind')

  await tabTo(page, conditionKind)
  // "c" is *Comparison*, which the conditional tier offers (§2.5).
  await page.keyboard.press('c')
  await expect(conditionKind).toHaveValue('comparison')
  await expect(page.getByTestId('conditional-unfinished-field_5')).toBeVisible()

  // Arrows: *is* → *is not* → *equals (loose)* → *not equal (loose)* →
  // *greater than*. Each arrow commits a choice, and focus stays on it.
  const comparison = control(page, 'Conditional logic: operator')

  await tabTo(page, comparison)
  for (let step = 0; step < 4; step += 1) {
    await page.keyboard.press('ArrowDown')
  }
  await expect(comparison).toHaveValue('>')
  await expect(comparison).toBeFocused()

  await tabTo(page, control(page, 'Conditional logic, operand 1: kind'))
  await page.keyboard.press('v')
  await page.keyboard.press('Tab')

  const subject = control(page, 'Conditional logic, operand 1: variable')

  await expect(subject).toBeFocused()
  // A `conditional` may read its own field, so `notes` is offered here.
  await expect(subject.locator('option')).toHaveText([
    'Choose a variable',
    'Reference — reference (string)',
    'Quantity — quantity (number)',
    'Unit price — unit_price (number)',
    'Total — total (number)',
    'Notes — notes (string)',
  ])
  await page.keyboard.press('t')
  await expect(subject).toHaveValue('total')

  // Operand 2: a number. "nu" is *Number*, not *Not*.
  await page.keyboard.press('Tab')
  await expect(control(page, 'Conditional logic, operand 2: kind')).toBeFocused()
  await page.keyboard.type('nu')
  await expect(control(page, 'Conditional logic, operand 2: kind')).toHaveValue('number')

  await page.keyboard.press('Tab')

  const threshold = control(page, 'Conditional logic, operand 2: number')

  await expect(threshold).toBeFocused()

  // **The threshold, one digit at a time** (AC14). The first digit completes
  // the condition, so every key is emitted, written through the page's cloning
  // `replaceNode`, and handed back. A builder that took that for a new value,
  // or was remounted by it, would lose focus here and drop digits.
  await typeKeyByKey(page, threshold, String(THRESHOLD), '', 0)
  await expect(page.getByTestId('conditional-unfinished-field_5')).toHaveCount(0)

  // **An unfinished operand survives a write elsewhere in the panel.** Emptied,
  // the number leaves the definition holding the last complete condition,
  // `total > 1`. The description is then typed, a write per key; a builder
  // reloaded by any of them would put the 1 back.
  let typed = { text: String(THRESHOLD), at: String(THRESHOLD).length }

  while (typed.at > 0) {
    typed = await backspace(page, threshold, typed.text, typed.at)
  }
  await expect(page.getByTestId('conditional-unfinished-field_5')).toBeVisible()

  const description = page.getByTestId('description-field_5')

  await tabTo(page, description, { backward: true, limit: 80 })
  await page.keyboard.type('Shown above 1,000.')
  await expect(description).toHaveValue('Shown above 1,000.')

  await expect(threshold).toHaveValue('')
  await expect(page.getByTestId('conditional-unfinished-field_5')).toBeVisible()
  await expect(subject).toHaveValue('total')
  await expect(comparison).toHaveValue('>')

  // And back into the builder by Tab, to finish it.
  await tabTo(page, threshold, { limit: 80 })
  await typeKeyByKey(page, threshold, String(THRESHOLD), '', 0)
  await expect(page.getByTestId('conditional-unfinished-field_5')).toHaveCount(0)

  // Enter collapses the condition to its summary, and Space opens it again.
  const collapse = page.getByRole('button', { name: 'Collapse Conditional logic', exact: true })
  const expand = page.getByRole('button', { name: 'Expand Conditional logic', exact: true })

  await tabTo(page, collapse, { backward: true })
  await page.keyboard.press('Enter')
  await expect(expand).toBeFocused()
  await expect(expand).toHaveAttribute('aria-expanded', 'false')
  await expect(page.getByTestId('conditional-field_5')).toContainText(`(total > ${THRESHOLD})`)

  await page.keyboard.press('Space')
  await expect(collapse).toBeFocused()
  await expect(threshold).toHaveValue(String(THRESHOLD))

  // --- 4. A data grid, numbered, opening with two rows -------------------------
  //
  // `notes` is at the root, so the grid is added to the form; once selected,
  // the grid's row template is where the palette adds.
  await addFromPalette(page, 'datagrid', 'field_6')
  await name(page, 'field_6', 'lines', 'Lines')

  await addFromPalette(page, 'number', 'field_7')
  await name(page, 'field_7', 'line_no', 'Line')
  await addFromPalette(page, 'textfield', 'field_8')
  await name(page, 'field_8', 'item', 'Item')

  const gridCard = page.getByTestId('component-field_6')

  await expect(gridCard.getByTestId('component-field_7')).toBeVisible()
  await expect(gridCard.getByTestId('component-field_8')).toBeVisible()

  await page.getByTestId('select-field_6').click()
  await page.getByTestId('sequence-key-field_6').selectOption('line_no')
  await page.getByTestId('default-items-field_6').fill('2')

  // --- 5. A submit button, then Save and Publish --------------------------------
  //
  // With the grid selected the palette aims at its rows, which take no button,
  // so a root field is selected first.
  await page.getByTestId('select-field_5').click()
  await addFromPalette(page, 'button', 'button_1')
  await page.getByTestId('button-label-button_1').fill('Submit requisition')

  await page.getByTestId('save-definition').click()
  await expect(page.getByTestId('save-error')).toHaveCount(0)
  await expect(page.getByTestId('save-confirmed')).toBeVisible()

  await page.getByTestId('publish-form').click()
  await expect(page.getByTestId('published-notice')).toBeVisible()

  // --- 6. The stored definition reads back with the same expressions ------------
  const stored = await storedDefinition(formId)
  const [reference, panel, notes, lines, submit] = stored.components

  expect(stored.components).toHaveLength(5)
  expect(reference).toMatchObject({
    id: 'field_1',
    key: 'reference',
    validation: { type: 'string', maxLength: MAX_LENGTH },
  })
  expect(panel).toMatchObject({ id: 'panel_1', type: 'panel', title: 'Amounts' })
  expect(panel.components?.map((node) => node.key)).toEqual(['quantity', 'unit_price', 'total'])
  expect(panel.components?.[2]?.calculate).toEqual(CALCULATE)
  expect(notes).toMatchObject({ id: 'field_5', key: 'notes', type: 'textarea' })
  expect(notes.conditional).toEqual({ action: 'show', logic: CONDITION })
  expect(lines).toMatchObject({
    id: 'field_6',
    key: 'lines',
    type: 'datagrid',
    sequenceKey: 'line_no',
    defaultItems: 2,
  })
  expect(lines.components?.map((node) => node.key)).toEqual(['line_no', 'item'])
  expect(submit).toMatchObject({ id: 'button_1', type: 'button', action: 'submit' })

  // And the published builder, reloaded, draws them from what was stored.
  await page.reload()
  await expect(page.getByTestId('published-notice')).toBeVisible()

  await page.getByTestId('select-field_4').click()
  await expect(control(page, 'Calculate: operator')).toHaveValue('*')
  await expect(control(page, 'Calculate: operator')).toBeDisabled()
  await expect(control(page, 'Calculate, operand 1: variable')).toHaveValue('quantity')
  await expect(control(page, 'Calculate, operand 2: variable')).toHaveValue('unit_price')

  await page.getByTestId('select-field_5').click()
  await expect(control(page, 'Conditional logic: operator')).toHaveValue('>')
  await expect(control(page, 'Conditional logic, operand 1: variable')).toHaveValue('total')
  await expect(control(page, 'Conditional logic, operand 2: number')).toHaveValue(
    String(THRESHOLD),
  )

  // --- 7. A document raised against it --------------------------------------------
  //
  // The type and its numbering rule are seeded: the type screen is another
  // flow's subject. The form is this run's, published above.
  const type = await createDocumentType(
    session,
    { id: formId, formKey, title: formTitle },
    `Expressions requisition ${suffix}`,
  )
  const documentTitle = `Five monitor arms ${suffix}`

  await page.goto('/documents/new')
  await chooseDocumentType(page, type.typeCode)
  await page.getByTestId('new-document-title').fill(documentTitle)
  await page.getByTestId('create-document').click()

  await expect(page).toHaveURL(/\/documents\/[0-9a-f-]{36}$/)
  await expect(page.getByTestId('document-title')).toHaveText(documentTitle)
  await expect(page.getByTestId('document-status')).toHaveText('Draft')

  // The grid opens with two rows, numbered 1 and 2.
  await expect(page.locator('#jfss-row-0-field_7')).toHaveValue('1')
  await expect(page.locator('#jfss-row-1-field_7')).toHaveValue('2')
  await expect(page.locator('#jfss-row-2-field_7')).toHaveCount(0)

  // `notes` is not drawn before there is a total.
  const notesBox = page.locator('#jfss-field_5')

  await expect(notesBox).toHaveCount(0)

  // **At the threshold, not above it**: 4 × 250 is 1000, and `>` is false.
  await page.locator('#jfss-field_2').fill('4')
  await page.locator('#jfss-field_3').fill('250')
  await expect(page.locator('#jfss-field_4')).toHaveValue('1000')
  await expect(notesBox).toHaveCount(0)

  // Above it: 5 × 250 is 1250, and `notes` is drawn.
  await page.locator('#jfss-field_2').fill('5')
  await expect(page.locator('#jfss-field_4')).toHaveValue('1250')
  await expect(notesBox).toBeVisible()

  // **The length limit refuses a long value**: the submit does nothing, and
  // the field says why. A renderer shows no message before the first submit
  // (§5.6), so the message is asserted after it.
  const submitButton = page.getByRole('button', { name: 'Submit requisition' })

  await page.locator('#jfss-field_1').fill('REQ-2026-000123')
  await submitButton.click()
  await expect(page.locator('#jfss-field_1-error')).toHaveText(
    `Use at most ${MAX_LENGTH} characters.`,
  )
  await expect(page.getByTestId('document-status')).toHaveText('Draft')
  await expect(page.getByTestId('document-number')).toContainText('Assigned when')

  await page.locator('#jfss-field_1').fill('REQ-000123')
  await expect(page.locator('#jfss-field_1-error')).toHaveCount(0)

  await notesBox.fill('Five, for the new desks.')
  await page.locator('#jfss-row-0-field_8').fill('Monitor arm')
  await page.locator('#jfss-row-1-field_8').fill('Cable tray')

  // **The document submits**, and its number is the seeded rule's.
  await submitButton.click()
  await expect(page.getByTestId('document-status')).toHaveText('Submitted')
  await expect(page.getByTestId('document-number')).toContainText('PR-')
})

test('the raw JSON boxes keep the caret and the text under every key', async ({ page }) => {
  await signIn(page)

  const formId = await createForm(
    page,
    `e2e_raw_${suffix}`.toLowerCase(),
    `Raw boxes ${suffix}`,
  )
  const preview = page.getByTestId('form-preview')

  await name(page, 'field_1', 'code', 'Code')
  // A button, so the preview can be submitted: a renderer shows no message
  // before the first submit (§5.6).
  await addFromPalette(page, 'button', 'button_1')
  await addFromPalette(page, 'number', 'field_2')
  await name(page, 'field_2', 'score', 'Score')

  // --- An expression's raw JSON box ---------------------------------------------
  //
  // Opened from the empty builder by keyboard: Enter on *Edit Calculate as
  // JSON*, which is replaced by the box.
  const editAsJson = page.getByRole('button', { name: 'Edit Calculate as JSON', exact: true })

  await tabTo(page, editAsJson)
  await page.keyboard.press('Enter')

  // The button is gone, so the browser's place in the tab order is where it
  // was, and the box is the next stop.
  const raw = control(page, 'Calculate (JSON)')

  await tabTo(page, raw)
  expect(await caretOf(raw)).toEqual({ focused: true, text: '', start: 0, end: 0 })

  // **Typed from empty, compact.** Every key but the last leaves text that does
  // not parse; the last parses to an expression and is written. A box rebuilt
  // from the value would hold it pretty-printed, not as typed.
  const whole = '{"+": [1, 2]}'
  let state = await typeKeyByKey(page, raw, whole, '', 0)

  await expect(page.getByTestId('calculate-refused-field_2')).toHaveCount(0)
  await expect(preview.locator('#jfss-field_2')).toHaveValue('3')

  // **Then in the middle**: after the `1`, five keys from the end.
  for (let step = 0; step < 5; step += 1) {
    await page.keyboard.press('ArrowLeft')
  }
  state = { text: state.text, at: state.at - 5 }
  expect(await caretOf(raw)).toEqual({
    focused: true,
    text: state.text,
    start: state.at,
    end: state.at,
  })

  // `0` parses (10 + 2), `,` and ` ` do not, and `5` parses again (10 + 5 + 2).
  // Each key that parses is written through the page, and comes back.
  state = await typeKeyByKey(page, raw, '0, 5', state.text, state.at)
  expect(state.text).toBe('{"+": [10, 5, 2]}')
  await expect(preview.locator('#jfss-field_2')).toHaveValue('17')

  // Space on *Edit visually*: the builder draws what was typed.
  await tabTo(page, page.getByRole('button', { name: 'Edit Calculate visually', exact: true }))
  await page.keyboard.press('Space')
  await expect(control(page, 'Calculate: operator')).toHaveValue('+')
  await expect(control(page, 'Calculate, operand 1: number')).toHaveValue('10')
  await expect(control(page, 'Calculate, operand 2: number')).toHaveValue('5')
  await expect(control(page, 'Calculate, operand 3: number')).toHaveValue('2')

  // --- A rule's params box ----------------------------------------------------------
  await page.getByTestId('select-field_1').click()
  await page.getByTestId('label-field_1').focus()

  await tabTo(page, page.getByTestId('add-rule-field_1'), { limit: 80 })
  await page.keyboard.press('Enter')

  // "r" is *regex*, whose seeded params are `{"pattern": ""}`.
  const rule = page.getByTestId('rule-field_1-0')

  await tabTo(page, rule)
  await page.keyboard.press('r')
  await expect(rule).toHaveValue('regex')

  const params = page.getByTestId('rule-params-field_1-0')
  const seeded = '{\n  "pattern": ""\n}'

  await expect(params).toHaveValue(seeded)
  await tabTo(page, params)

  // **Emptied, and retyped compact.** An empty box is no parameters, `{}`,
  // which is written; the seeded text is pretty-printed, so a box that rewrote
  // itself from what it wrote would no longer be empty.
  await page.keyboard.press('ControlOrMeta+a')
  await page.keyboard.press('Backspace')
  expect(await caretOf(params), 'after emptying the box').toEqual({
    focused: true,
    text: '',
    start: 0,
    end: 0,
  })

  // Every key but the last leaves text that does not parse, which is marked
  // and not written; the last parses and is written.
  let typedParams = await typeKeyByKey(page, params, '{"pattern": "^[A-Z]+"}', '', 0)

  await expect(page.getByTestId('rule-params-field_1-0-refused')).toHaveCount(0)

  // Then in the middle, before `"}`: the `$` keeps it parsing, so it is
  // written and comes back while the caret is inside the text.
  await page.keyboard.press('ArrowLeft')
  await page.keyboard.press('ArrowLeft')
  typedParams = { text: typedParams.text, at: typedParams.at - 2 }
  typedParams = await typeKeyByKey(page, params, '$', typedParams.text, typedParams.at)

  // A quote ends the string early, so the text does not parse: it is marked
  // and not written. Backspace makes it parse again.
  typedParams = await typeKeyByKey(page, params, '"', typedParams.text, typedParams.at)
  await expect(page.getByTestId('rule-params-field_1-0-refused')).toBeVisible()
  typedParams = await backspace(page, params, typedParams.text, typedParams.at)
  await expect(page.getByTestId('rule-params-field_1-0-refused')).toHaveCount(0)
  expect(typedParams.text).toBe('{"pattern": "^[A-Z]+$"}')

  // The preview enforces the pattern the box wrote, once it has been submitted.
  await preview.locator('#jfss-field_1').fill('abc')
  await preview.getByRole('button', { name: 'Submit' }).click()
  await expect(preview.locator('#jfss-field_1-error')).toHaveText('This value is not valid.')
  await preview.locator('#jfss-field_1').fill('ABC')
  await expect(preview.locator('#jfss-field_1-error')).toHaveCount(0)

  // --- What was typed is what is stored -------------------------------------------
  await page.getByTestId('save-definition').click()
  await expect(page.getByTestId('save-error')).toHaveCount(0)
  await expect(page.getByTestId('save-confirmed')).toBeVisible()

  const stored = await storedDefinition(formId)

  expect(stored.components[0]).toMatchObject({
    id: 'field_1',
    rules: [{ rule: 'regex', params: { pattern: '^[A-Z]+$' } }],
  })
  expect(stored.components[2]).toMatchObject({ id: 'field_2', calculate: { '+': [10, 5, 2] } })
})

/** What the row's expressions are built to, as the server must store them. */
const ROW_CALCULATE = { '*': [{ var: 'qty' }, { var: 'price' }] }
const ROW_CONDITION = { '>': [{ var: 'qty' }, 10] }
/** A row field's condition on a top-level key: typed, since it is not offered. */
const TOP_LEVEL_CONDITION = { '>': [{ var: 'budget' }, 0] }

/** The document save, which the third test rewrites in flight. */
const DOCUMENT_URL = /\/api\/v1\/documents\/[0-9a-f-]{36}$/

test("a data grid's row calculates and shows its fields from its own row", async ({ page }) => {
  await signIn(page)

  const formKey = `e2e_rows_${suffix}`.toLowerCase()
  const formTitle = `Grid rows ${suffix}`
  const formId = await createForm(page, formKey, formTitle)

  // --- 1. Two top-level fields, and a grid of two rows -------------------------
  //
  // `reference` is at the root and is not a container, so the palette adds to
  // the form until the grid is selected; then it adds to the grid's row.
  await name(page, 'field_1', 'reference', 'Reference')
  await addFromPalette(page, 'number', 'field_2')
  await name(page, 'field_2', 'budget', 'Budget')

  await addFromPalette(page, 'datagrid', 'field_3')
  await name(page, 'field_3', 'lines', 'Lines')
  await page.getByTestId('default-items-field_3').fill('2')

  await addFromPalette(page, 'number', 'field_4')
  await name(page, 'field_4', 'qty', 'Quantity')
  await addFromPalette(page, 'number', 'field_5')
  await name(page, 'field_5', 'price', 'Price')
  await addFromPalette(page, 'number', 'field_6')
  await name(page, 'field_6', 'line_total', 'Line total')

  const gridCard = page.getByTestId('component-field_3')

  for (const id of ['field_4', 'field_5', 'field_6']) {
    await expect(gridCard.getByTestId(`component-${id}`)).toBeVisible()
  }

  // --- 2. `line_total` calculates `qty × price`, from the row's keys --------------
  //
  // **The variables offered are the row's, and only the row's** (#732's AC6):
  // a row is evaluated against its own object, so `reference`, `budget` and
  // `lines` are not offered here, and nor is `line_total` itself. A builder
  // that offered the top level inside a row fails on this list.
  await control(page, 'Calculate: kind').selectOption('arithmetic')
  await control(page, 'Calculate: operator').selectOption('*')
  await control(page, 'Calculate, operand 1: kind').selectOption('var')

  const factor = control(page, 'Calculate, operand 1: variable')

  await expect(factor.locator('option')).toHaveText([
    'Choose a variable',
    'Quantity — qty (number)',
    'Price — price (number)',
  ])
  await factor.selectOption('qty')
  await control(page, 'Calculate, operand 2: kind').selectOption('var')
  await control(page, 'Calculate, operand 2: variable').selectOption('price')
  await expect(page.getByTestId('calculate-unfinished-field_6')).toHaveCount(0)

  // --- 3. `bulk_reason`, shown in a row whose own `qty` is above 10 ---------------
  //
  // With a row field selected, the palette still adds to the row.
  await addFromPalette(page, 'textfield', 'field_7')
  await name(page, 'field_7', 'bulk_reason', 'Bulk reason')
  await expect(gridCard.getByTestId('component-field_7')).toBeVisible()

  await control(page, 'Conditional logic: kind').selectOption('comparison')
  await control(page, 'Conditional logic: operator').selectOption('>')
  await control(page, 'Conditional logic, operand 1: kind').selectOption('var')

  const subject = control(page, 'Conditional logic, operand 1: variable')

  // A `conditional` may read its own field, and it is still the row's keys only.
  await expect(subject.locator('option')).toHaveText([
    'Choose a variable',
    'Quantity — qty (number)',
    'Price — price (number)',
    'Line total — line_total (number)',
    'Bulk reason — bulk_reason (string)',
  ])
  await subject.selectOption('qty')
  await control(page, 'Conditional logic, operand 2: kind').selectOption('number')
  await control(page, 'Conditional logic, operand 2: number').fill('10')
  await expect(page.getByTestId('conditional-unfinished-field_7')).toHaveCount(0)

  // --- 4. `budget_note`, whose condition reads the top-level `budget` -------------
  //
  // **#732's one signal for a row field reading outside its row**, which is
  // always false at runtime: the key is not offered, so it is typed as JSON,
  // and the visual builder draws it with a badge while the field is selected.
  await addFromPalette(page, 'textfield', 'field_8')
  await name(page, 'field_8', 'budget_note', 'Budget note')
  await expect(gridCard.getByTestId('component-field_8')).toBeVisible()

  await page.getByRole('button', { name: 'Edit Conditional logic as JSON', exact: true }).click()
  await control(page, 'Conditional logic (JSON)').fill(JSON.stringify(TOP_LEVEL_CONDITION))
  await page.getByRole('button', { name: 'Edit Conditional logic visually', exact: true }).click()

  await expect(control(page, 'Conditional logic, operand 1: variable')).toHaveValue('budget')
  await expect(page.getByTestId('conditional-field_8')).toContainText(
    'Not in the offered variables',
  )

  // And `bulk_reason`'s condition, which reads its own row, carries no badge.
  await page.getByTestId('select-field_7').click()
  await expect(control(page, 'Conditional logic, operand 1: variable')).toHaveValue('qty')
  await expect(page.getByTestId('conditional-field_7')).not.toContainText(
    'Not in the offered variables',
  )

  // --- 5. A submit button at the root, then Save and Publish ----------------------
  await page.getByTestId('select-field_2').click()
  await addFromPalette(page, 'button', 'button_1')
  await page.getByTestId('button-label-button_1').fill('Submit requisition')

  await page.getByTestId('save-definition').click()
  await expect(page.getByTestId('save-error')).toHaveCount(0)
  await expect(page.getByTestId('save-confirmed')).toBeVisible()

  await page.getByTestId('publish-form').click()
  await expect(page.getByTestId('published-notice')).toBeVisible()

  // --- 6. Stored as built: the expressions sit in the row template ----------------
  const stored = await storedDefinition(formId)
  const lines = stored.components[2]
  const row = lines?.components ?? []

  expect(stored.components.map((node) => node.id)).toEqual([
    'field_1',
    'field_2',
    'field_3',
    'button_1',
  ])
  expect(lines).toMatchObject({ id: 'field_3', key: 'lines', type: 'datagrid', defaultItems: 2 })
  expect(row.map((node) => node.key)).toEqual([
    'qty',
    'price',
    'line_total',
    'bulk_reason',
    'budget_note',
  ])
  expect(row[2]?.calculate).toEqual(ROW_CALCULATE)
  expect(row[3]?.conditional).toEqual({ action: 'show', logic: ROW_CONDITION })
  expect(row[4]?.conditional).toEqual({ action: 'show', logic: TOP_LEVEL_CONDITION })

  // --- 7. A document: each row computes and shows from its own values --------------
  const type = await createDocumentType(
    session,
    { id: formId, formKey, title: formTitle },
    `Grid rows requisition ${suffix}`,
  )

  await page.goto('/documents/new')
  await chooseDocumentType(page, type.typeCode)
  await page.getByTestId('new-document-title').fill(`Chairs and desks ${suffix}`)
  await page.getByTestId('create-document').click()

  await expect(page).toHaveURL(/\/documents\/[0-9a-f-]{36}$/)
  await expect(page.getByTestId('document-status')).toHaveText('Draft')

  const cell = (index: number, id: string): Locator => page.locator(`#jfss-row-${index}-${id}`)

  await expect(cell(0, 'field_4')).toBeVisible()
  await expect(cell(1, 'field_4')).toBeVisible()
  await expect(cell(2, 'field_4')).toHaveCount(0)

  await page.locator('#jfss-field_1').fill('REQ-000733')
  await page.locator('#jfss-field_2').fill('5000')

  // Twelve chairs at 5, and three desks at 7: a product per row.
  await cell(0, 'field_4').fill('12')
  await cell(0, 'field_5').fill('5')
  await cell(1, 'field_4').fill('3')
  await cell(1, 'field_5').fill('7')
  await expect(cell(0, 'field_6')).toHaveValue('60')
  await expect(cell(1, 'field_6')).toHaveValue('21')

  // **`bulk_reason` follows its own row's `qty`**: shown in the row of 12, and
  // not in the row of 3. A row reading another row's `qty`, or the top level's
  // (which has none), would show it in both rows or in neither.
  await expect(cell(0, 'field_7')).toBeVisible()
  await expect(cell(1, 'field_7')).toHaveCount(0)

  // **`budget_note` is never drawn**, though `budget` is 5000: a row has no
  // `budget`. The badge in step 4 is the only place that is said.
  await expect(cell(0, 'field_8')).toHaveCount(0)
  await expect(cell(1, 'field_8')).toHaveCount(0)

  await cell(0, 'field_7').fill('Twelve, for the new floor.')

  // **The server's row scope reads what the builder wrote.** The save before
  // the submit is rewritten in flight, as `a-form-is-submitted.spec.ts` does
  // with a submission: each row's total is claimed as 0, and a value is
  // smuggled into each field its row's condition hides. The server's answer
  // is the rows as stored.
  let storedRows: Record<string, unknown>[] | undefined

  await page.route(DOCUMENT_URL, async (route) => {
    if (route.request().method() !== 'PUT') {
      await route.continue()

      return
    }

    const body = route.request().postDataJSON() as { formData: Record<string, unknown> }
    const rows = (body.formData.lines as Record<string, unknown>[] | undefined) ?? []

    body.formData.lines = rows.map((line) => ({
      ...line,
      line_total: 0,
      bulk_reason: line.bulk_reason ?? 'smuggled into a row whose qty is 3',
      budget_note: 'smuggled into a row that has no budget',
    }))

    const response = await route.fetch({ postData: JSON.stringify(body) })
    const answered = (await response.json()) as {
      data?: { formData?: { lines?: Record<string, unknown>[] } }
    }

    storedRows = answered.data?.formData?.lines

    await route.fulfill({ response })
  })

  await page.getByRole('button', { name: 'Submit requisition' }).click()
  await expect(page.getByTestId('document-status')).toHaveText('Submitted')

  expect(storedRows, 'the server answered the save with its rows').toHaveLength(2)

  const [first, second] = storedRows as Record<string, unknown>[]

  expect(first).toMatchObject({
    qty: 12,
    price: 5,
    line_total: 60,
    bulk_reason: 'Twelve, for the new floor.',
  })
  expect(second).toMatchObject({ qty: 3, price: 7, line_total: 21 })
  // Each hidden field's value is left out of its row (S10.2): `bulk_reason`
  // in the row of 3, and `budget_note` in both.
  expect(second.bulk_reason).toBeUndefined()
  expect(first.budget_note).toBeUndefined()
  expect(second.budget_note).toBeUndefined()

  // And the document, read again, draws the rows the server stored.
  await page.unroute(DOCUMENT_URL)
  await page.reload()
  await expect(page.getByTestId('document-status')).toHaveText('Submitted')
  await expect(cell(0, 'field_6')).toHaveValue('60')
  await expect(cell(1, 'field_6')).toHaveValue('21')
  await expect(cell(0, 'field_7')).toHaveValue('Twelve, for the new floor.')
  await expect(cell(1, 'field_7')).toHaveCount(0)
})
