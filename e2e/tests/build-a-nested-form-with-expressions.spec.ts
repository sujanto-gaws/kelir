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
 * - **A rule's `params` box**, between the quotes the seeded `regex` params
 *   leave, through a state that does not parse and back.
 *
 * Each is then shown to have reached the definition: the visual builder draws
 * the typed expression, the preview computes it and enforces the typed
 * pattern, and the save reads both back.
 *
 * # No drag (AC15)
 *
 * Nothing here is dragged. Every node is added from the palette, which adds to
 * the selected container or the container of the selected node.
 *
 * **Seen red against a broken builder**: not yet run. The mutations and the
 * step each one fails at are recorded here once they have been.
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

/** What a text box holds, and where its caret is. */
async function caretOf(
  box: Locator,
): Promise<{ focused: boolean; text: string; start: number | null; end: number | null }> {
  return box.evaluate((element) => {
    const input = element as HTMLInputElement | HTMLTextAreaElement

    return {
      focused: input === document.activeElement,
      text: input.value,
      start: input.selectionStart,
      end: input.selectionEnd,
    }
  })
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
  await expect(calculateKind.locator('option')).toHaveText(['No expression', 'Variable', 'Arithmetic'])

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

  // Between the two quotes: Ctrl+End, then three keys back.
  await page.keyboard.press('ControlOrMeta+End')
  for (let step = 0; step < 3; step += 1) {
    await page.keyboard.press('ArrowLeft')
  }

  let typedParams = { text: seeded, at: seeded.length - 3 }

  expect(await caretOf(params)).toEqual({
    focused: true,
    text: seeded,
    start: typedParams.at,
    end: typedParams.at,
  })

  // Every key leaves a JSON string, so every key is written and comes back.
  typedParams = await typeKeyByKey(page, params, '^[A-Z]+$', typedParams.text, typedParams.at)

  // A quote ends the string early, so the text does not parse: it is marked
  // and not written. Backspace makes it parse again.
  typedParams = await typeKeyByKey(page, params, '"', typedParams.text, typedParams.at)
  await expect(page.getByTestId('rule-params-field_1-0-refused')).toBeVisible()
  typedParams = await backspace(page, params, typedParams.text, typedParams.at)
  await expect(page.getByTestId('rule-params-field_1-0-refused')).toHaveCount(0)
  expect(typedParams.text).toBe('{\n  "pattern": "^[A-Z]+$"\n}')

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
