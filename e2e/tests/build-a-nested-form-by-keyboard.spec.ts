import { expect, test, type Locator, type Page } from '@playwright/test'

import { runSuffix, signInOverApi, type ApiSession } from '../support/api'
import { API_PREFIX, credentials } from '../support/env'
import { tabTo } from '../support/keyboard'

/**
 * The form builder's nesting canvas, operated by keyboard alone (#688 I3).
 *
 * **Every drag on the canvas has a keyboard equivalent** (#688 D5), and #695's
 * real-browser keyboard criterion is split between this row and part 2: the
 * canvas half is this flow. jsdom proves the controls exist and call the same
 * mutation the drop handler calls; only a browser proves Tab reaches them,
 * Enter presses them, and focus lands where the next key press expects it.
 *
 * So after the form is created and the starting point is focused, **every step
 * is Tab, Shift+Tab, Enter, Ctrl+A or typing**, through `tabTo`, which fails
 * when a control is not in the tab order. The flow:
 *
 * 1. adds a panel from the palette, and titles it;
 * 2. adds a text field into the panel, from the panel's own *Add component*;
 * 3. adds columns to the form, and a text field into its first column;
 * 4. moves that field into the panel with *Move to…*;
 * 5. removes the field the form was created with;
 * 6. saves.
 *
 * **The assertions are the preview's nesting and the tree read back after a
 * reload**, not that the canvas rendered: the preview is the renderer a
 * document uses, and the reload is what the server stored.
 *
 * **Seen red against a broken canvas** (2026-10-10, against a stack whose
 * frontend image was built from the mutation): with *Move to…* sending the
 * node to the form's root instead of the chosen list, the flow fails at step
 * 4, where the live region says *Moved Text field “Column field” to the
 * form.* Green again on the image rebuilt without it.
 *
 * # A pointer drag, attempted
 *
 * The second test drags a card by its handle with the mouse. It is the one
 * pointer drag #688 D1 asks this flow to attempt; jsdom cannot drive Sortable,
 * so the drag's own handler is what `FormBuilderPage.canvas.spec.ts` proves.
 * **Driven reliably only as written**: a single move from the handle straight
 * to the list failed two runs in five, because the browser had not started a
 * drag; the short first move fixes that, and the flow then passed fifteen
 * runs of fifteen in Chromium (2026-10-10).
 */

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

/** Creates a form through the screen, and lands in its builder. */
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

/**
 * A canvas control by its accessible name. Scoped, because the palette names
 * its buttons the same way: *Add Text field to Panel “Details”* is both the
 * palette's button while the panel is selected and the panel's own chooser's.
 */
function button(page: Page, name: string): Locator {
  return page.getByTestId('form-canvas').getByRole('button', { name, exact: true })
}

function paletteButton(page: Page, name: string): Locator {
  return page.getByTestId('form-palette').getByRole('button', { name, exact: true })
}

/** Replaces a focused input's text, by keyboard. */
async function retype(page: Page, text: string): Promise<void> {
  await page.keyboard.press('ControlOrMeta+a')
  await page.keyboard.type(text)
}

async function storedDefinition(id: string): Promise<{ components: Record<string, unknown>[] }> {
  const response = await session.context.get(`${API_PREFIX}/rad/forms/${id}`)

  expect(response.ok(), await response.text()).toBe(true)

  return ((await response.json()) as { data: { definition: { components: Record<string, unknown>[] } } })
    .data.definition
}

test('an administrator nests a form by keyboard alone, and the server stores the tree', async ({
  page,
}) => {
  await signIn(page)

  const formId = await createForm(page, `e2e_nested_${runSuffix()}`.toLowerCase(), `Nested ${runSuffix()}`)
  const announcement = page.getByTestId('canvas-announcement')

  // The keyboard starts at the title, the control before the canvas, and from
  // there it is Tab.
  await page.getByTestId('definition-title').focus()

  // --- 1. A panel, from the palette, titled ----------------------------------
  //
  // The created field is selected and is not a container, so the palette adds
  // to the form.
  await tabTo(page, paletteButton(page, 'Add Panel to the form'))
  await page.keyboard.press('Enter')
  await expect(page.getByTestId('select-panel_1')).toBeFocused()
  await expect(announcement).toHaveText('Added Panel “Panel” to the form.')

  await tabTo(page, page.getByTestId('title-panel_1'), { limit: 80 })
  await retype(page, 'Details')

  // --- 2. A text field into the panel, from its own Add component ------------
  await tabTo(page, button(page, 'Add component to Panel “Details”'), { backward: true, limit: 80 })
  await page.keyboard.press('Enter')
  // The chooser opens with focus on its first type, which is a text field.
  await expect(button(page, 'Add Text field to Panel “Details”')).toBeFocused()
  await page.keyboard.press('Enter')
  await expect(page.getByTestId('select-field_2')).toBeFocused()

  await tabTo(page, page.getByTestId('label-field_2'), { limit: 80 })
  await retype(page, 'Panel field')

  // --- 3. Columns on the form, and a field in its first column --------------
  await tabTo(page, button(page, 'Add component to the form'), { backward: true, limit: 80 })
  await page.keyboard.press('Enter')
  await tabTo(page, button(page, 'Add Columns to the form'))
  await page.keyboard.press('Enter')
  await expect(page.getByTestId('select-columns_1')).toBeFocused()

  await tabTo(page, button(page, 'Add component to column 1 of Columns'))
  await page.keyboard.press('Enter')
  await page.keyboard.press('Enter')
  await expect(page.getByTestId('select-field_3')).toBeFocused()

  await tabTo(page, page.getByTestId('label-field_3'), { limit: 80 })
  await retype(page, 'Column field')

  // --- 4. Move to… the panel ----------------------------------------------------
  await tabTo(page, button(page, 'Move Text field “Column field” to…'), {
    backward: true,
    limit: 80,
  })
  await page.keyboard.press('Enter')
  await tabTo(page, button(page, 'Move Text field “Column field” to Form › Panel “Details”'))
  await page.keyboard.press('Enter')
  await expect(announcement).toHaveText('Moved Text field “Column field” to Panel “Details”.')
  // Focus stays on the moved node.
  await expect(button(page, 'Move Text field “Column field” to…')).toBeFocused()

  // --- 5. Remove the field the form was created with ---------------------------
  await tabTo(page, button(page, 'Remove Text field “Field 1”'), { backward: true, limit: 80 })
  await page.keyboard.press('Enter')
  await expect(page.getByTestId('component-field_1')).toHaveCount(0)
  await expect(announcement).toHaveText('Removed Text field “Field 1”.')
  // The next sibling takes focus.
  await expect(page.getByTestId('select-panel_1')).toBeFocused()

  // --- 6. Save ---------------------------------------------------------------------
  await tabTo(page, page.getByTestId('save-definition'), { backward: true, limit: 80 })
  await page.keyboard.press('Enter')
  await expect(page.getByTestId('save-confirmed')).toBeVisible()

  // --- The preview nests what the canvas nested ---------------------------------
  const preview = page.getByTestId('form-preview')
  const panel = preview.locator('section', {
    has: page.getByRole('heading', { name: 'Details', exact: true }),
  })

  await expect(panel).toContainText('Panel field')
  await expect(panel).toContainText('Column field')
  await expect(preview).not.toContainText('Field 1')

  // --- And the server stored the tree --------------------------------------------
  await page.reload()

  const panelCard = page.getByTestId('component-panel_1')

  await expect(panelCard.getByTestId('component-field_2')).toBeVisible()
  await expect(panelCard.getByTestId('component-field_3')).toBeVisible()
  await expect(page.getByTestId('component-field_1')).toHaveCount(0)

  const stored = await storedDefinition(formId)

  expect(stored.components).toMatchObject([
    {
      id: 'panel_1',
      type: 'panel',
      title: 'Details',
      components: [
        { id: 'field_2', label: 'Panel field' },
        { id: 'field_3', label: 'Column field' },
      ],
    },
    { id: 'columns_1', type: 'columns', columns: [{ components: [] }, { components: [] }] },
  ])
})

test('a card is dragged by its handle with the pointer', async ({ page }) => {
  await signIn(page)

  const formId = await createForm(page, `e2e_drag_${runSuffix()}`.toLowerCase(), `Dragged ${runSuffix()}`)

  // A panel to drop into, added by its palette button.
  await page.getByTestId('palette-panel').click()
  await expect(page.getByTestId('component-panel_1')).toBeVisible()

  // Drag the created field by its handle into the panel's empty list.
  const handle = page.getByTestId('drag-field_1')
  const target = page.getByTestId('list-components.1.components')

  const from = await handle.boundingBox()
  const box = await target.boundingBox()

  expect(from).not.toBeNull()
  expect(box).not.toBeNull()

  // Down on the handle, a short move so the browser starts a drag, then the
  // travel to the empty list in steps, so Sortable sees the pointer arrive.
  await page.mouse.move(from!.x + from!.width / 2, from!.y + from!.height / 2)
  await page.mouse.down()
  await page.mouse.move(from!.x + from!.width / 2 + 8, from!.y + from!.height / 2 + 8, { steps: 4 })
  await page.mouse.move(box!.x + box!.width / 2, box!.y + box!.height / 2, { steps: 16 })
  await page.mouse.move(box!.x + box!.width / 2, box!.y + box!.height / 2 + 2, { steps: 4 })
  await page.mouse.up()

  await expect(page.getByTestId('component-panel_1').getByTestId('component-field_1')).toBeVisible()

  await page.getByTestId('save-definition').click()
  await expect(page.getByTestId('save-confirmed')).toBeVisible()

  const stored = await storedDefinition(formId)

  expect(stored.components).toMatchObject([
    { id: 'panel_1', components: [{ id: 'field_1' }] },
  ])
})
