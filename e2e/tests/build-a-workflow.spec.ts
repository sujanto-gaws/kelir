import { expect, test, type Page } from '@playwright/test'

import { runSuffix, signInOverApi, type ApiSession } from '../support/api'
import { chooseDocumentType, chooseOption } from '../support/choosers'
import { giveNumberingRule } from '../support/documents'
import { API_PREFIX, credentials } from '../support/env'
import { publishForm, type SeededForm } from '../support/forms'
import { tabTo } from '../support/keyboard'
import { createApprover, type SeededApprover } from '../support/workflow'

/**
 * An administrator builds a workflow in the editor, publishes it and binds it
 * to a document type, and a document is then approved through it from somebody
 * else's inbox (#426 AC6; FR-RAD-009, FR-WF-004/006/013/015, and FR-WF-018
 * for create, edit and publish; deprecate joins at row 6b, #713).
 *
 * **The assertion is not that the editor renders.** It is the rule
 * `build-a-form.spec.ts` follows: *a criterion that says X can is not met by
 * an endpoint*. So this flow ends on **a document status that only the
 * condition authored here could have produced**. The workflow was authored,
 * published and bound in the browser, and `e2e/support/workflow.ts` seeded no
 * definition. That file keeps its API seeding for the other specs, which are
 * about approvals, not about this screen.
 *
 * # The condition decides the final status
 *
 * The approval state gets two `APPROVE` edges:
 *
 * - The starter's edge has no condition, and goes to `COMPLETED`.
 * - The edge built here has `formData.amount > 1000`, and goes to a final
 *   state mapped to `APPROVED`.
 *
 * S7 evaluates conditioned edges first. The document carries 45000, so it ends
 * **Approved**. Had the editor written no condition, S7 would refuse the save,
 * because only one fallback may leave a state on one action. Had it written a
 * wrong condition, or had the engine read it as false, the document would end
 * *Completed*. So a flow asserting only that the document was decided would
 * pass on either.
 *
 * **A second document carries 900, and ends *Completed*.** That proves the
 * other branch: the condition is false at or below the threshold, so the
 * fallback decides. Without it, a condition that held for every document
 * would pass the first leg.
 *
 * # The logic builder is driven by keyboard alone
 *
 * Two criteria #426 inherits from #695's trace:
 *
 * 1. **Keyboard, in a real browser.** #695 proved the keyboard path only in
 *    jsdom, through a helper that performs default key actions. Here the
 *    condition is built with Tab and Shift+Tab, typeahead on a native select,
 *    arrow keys, typing, Enter and Space, and nothing else. Every chooser in
 *    the builder is a native `<select>`, so in Chromium on Windows and Linux,
 *    the platforms this suite runs on, typeahead and arrows commit a choice
 *    without opening a popup. On macOS an arrow opens the popup instead.
 * 2. **The editor's real `v-model`.** The number is typed after the variable,
 *    so every keystroke completes the expression and is written through the
 *    editor's draft. The test asserts that focus and the text survive the
 *    echo. Then an operand is emptied, the editor writes elsewhere, and the
 *    test asserts the unfilled operand is still unfilled: a host that cloned
 *    on write would reload the builder and put the last complete value back.
 *
 * The other fields are filled with ordinary `fill` and `selectOption` calls.
 * Their keyboard paths are the browser's own, and no criterion asks for them.
 *
 * # What is seeded over the API, and why
 *
 * - **The form**, because building one is `build-a-form.spec.ts`'s subject.
 * - **The approver and their role**, because the role screen is
 *   `sign-out-and-the-roles-screen.spec.ts`'s. The role must exist before the
 *   publish, which refuses a definition naming a dead role (**D-111**).
 * - **The numbering rule**, because driving it is
 *   `configure-a-document-type.spec.ts`'s subject.
 *
 * The binding itself is made on the document type screen.
 */
const suffix = runSuffix()

const roleCode = `E2E-BUILT-WF-${suffix}`.toUpperCase()
const workflowKey = `e2e_built_wf_${suffix}`.toLowerCase()
const workflowName = `Built in the browser ${suffix}`
const typeCode = `E2E_BUILT_WF_${suffix}`.toUpperCase()
const title = `Six monitor arms ${suffix}`

const TASK_NAME = 'Approve the requisition'
const BRANCH_STATE = 'Approved above the threshold'
const THRESHOLD = 1000
const AMOUNT = 45_000
/** At or below the threshold, so the condition is false and the fallback decides. */
const SMALL_AMOUNT = 900
const smallTitle = `Two desk lamps ${suffix}`

/**
 * How long a click waits for the request it should send. Bounded below the
 * test's own timeout so that a button that sends nothing fails at its step,
 * naming it, rather than as the whole test running out.
 */
const RESPONSE_TIMEOUT = 15_000

/** What the keyboard builds, as the server must store it. */
const CONDITION = { '>': [{ var: 'formData.amount' }, THRESHOLD] }

/** One number field: the smallest form a condition on `formData.amount` can read. */
const definition = {
  formId: `e2e-built-wf-${suffix}`,
  version: '2.0.1',
  title: 'Purchase requisition',
  components: [
    {
      id: 'amount-field',
      role: 'data',
      type: 'number',
      key: 'amount',
      label: 'Amount',
      validation: { type: 'number', minimum: 0 },
    },
    {
      id: 'submit-button',
      role: 'action',
      type: 'button',
      label: 'Submit request',
      action: 'submit',
    },
  ],
}

let session: ApiSession
let form: SeededForm
let approver: SeededApprover

test.beforeAll(async () => {
  session = await signInOverApi()
  // **The title carries the suffix** (#521): the type screen's chooser picks by
  // label, and a fixed title would match every earlier run's form.
  form = await publishForm(session, definition, `Built workflow requisition ${suffix}`)
  approver = await createApprover(session, roleCode)
})

test.afterAll(async () => {
  await session?.context.dispose()
})

async function signIn(page: Page, username: string, password: string): Promise<void> {
  await page.goto('/login')
  await page.getByLabel('Username or email').fill(username)
  await page.getByLabel('Password').fill(password)
  await page.getByRole('button', { name: 'Sign in' }).click()

  await expect(page).toHaveURL(/\/$/)
}

test('an administrator builds a workflow in the editor, and a document is approved through it', async ({
  browser,
}) => {
  const admin = await browser.newPage()
  const decider = await browser.newPage()

  try {
    const { username, password } = credentials()
    await signIn(admin, username, password)

    // --- The editor is reachable from the navigation -------------------------
    await admin.getByRole('link', { name: 'Workflows' }).click()
    await expect(admin).toHaveURL(/\/admin\/workflows$/)

    await admin.getByTestId('new-workflow').click()
    await expect(admin).toHaveURL(/\/admin\/workflows\/new$/)
    await expect(admin.getByTestId('status')).toHaveText('DRAFT')

    // --- The starter: one approval, approved or rejected ---------------------
    //
    // States 0–2 are PENDING_APPROVAL, COMPLETED and REJECTED, and transitions
    // 1 and 2 are its APPROVE and REJECT. **The deciding role is left blank by
    // design** (`starterDefinition`), so the role fields are filled here.
    await admin.getByTestId('workflow-key').fill(workflowKey)
    await admin.getByTestId('workflow-name').fill(workflowName)

    const approval = admin.getByTestId('state-0')
    await expect(admin.getByTestId('state-code-0')).toHaveValue('PENDING_APPROVAL')
    await approval.getByLabel('Task name').fill(TASK_NAME)
    await admin
      .getByLabel('State PENDING_APPROVAL task assigned to: role code', { exact: true })
      .fill(roleCode)
    await admin.getByLabel('Transition 1 allowed by: role code', { exact: true }).fill(roleCode)
    await admin.getByLabel('Transition 2 allowed by: role code', { exact: true }).fill(roleCode)

    // --- A final state the condition routes to -------------------------------
    await admin.getByTestId('add-state-final').click()
    await expect(admin.getByTestId('state-code-3')).toHaveValue('DONE')
    await admin.getByTestId('state-name-3').fill(BRANCH_STATE)
    await admin.getByTestId('state-3').getByLabel('Document status').selectOption('APPROVED')

    // --- A second APPROVE out of the approval, to that state -----------------
    await approval.getByTestId('add-transition-0').click()
    await expect(admin.getByTestId('transition-2')).toBeVisible()
    await expect(admin.getByLabel('Transition 3: action', { exact: true })).toHaveValue('APPROVE')
    await admin.getByLabel('Transition 3: to', { exact: true }).selectOption('DONE')
    await admin.getByLabel('Transition 3 allowed by: role code', { exact: true }).fill(roleCode)

    // --- Its condition, built by keyboard alone ------------------------------
    //
    // Every control below is named by the builder from the transition's label,
    // so the names are the accessible names a screen reader announces.
    const condition = 'Transition 3 condition'
    const control = (name: string) => admin.getByLabel(`${condition}${name}`, { exact: true })
    const unfilled = admin.getByTestId('unfilled-expressions')

    // The flow starts the keyboard at the control just before the builder, and
    // from there it is Tab.
    await admin.getByTestId('requires-comment-2').focus()
    await tabTo(admin, control(': kind'), { limit: 6 })
    await expect(control(': kind')).toHaveValue('')

    // Typeahead: "c" is *Comparison*. The chooser is replaced by the expression
    // it started, a comparison with both operands unfilled, so focus moves on.
    await admin.keyboard.press('c')
    await expect(control(': operator')).toHaveValue('===')
    await expect(control(', operand 1: kind')).toHaveValue('')
    await expect(control(', operand 2: kind')).toHaveValue('')
    // The editor hears that the expression is incomplete.
    await expect(unfilled).toContainText('1 expression has an operand not filled in')

    // Arrows: *is* → *is not* → *equals (loose)* → *not equal (loose)* →
    // *greater than*. Each arrow commits a choice, and focus stays on it.
    await tabTo(admin, control(': operator'))
    for (let step = 0; step < 4; step += 1) {
      await admin.keyboard.press('ArrowDown')
    }
    await expect(control(': operator')).toHaveValue('>')
    await expect(control(': operator')).toBeFocused()

    // Operand 1: a variable, typed as a path. The editor cannot list a form's
    // fields, so `formData.<key>` is typed and counts as offered (User Manual
    // §10.6). The tab order inside the operand is fixed, so each stop is
    // asserted.
    await tabTo(admin, control(', operand 1: kind'))
    await admin.keyboard.press('v')
    await expect(control(', operand 1: kind')).toHaveValue('var')
    await expect(control(', operand 1: kind')).toBeFocused()

    await admin.keyboard.press('Tab')
    await expect(control(', operand 1: variable')).toBeFocused()
    await admin.keyboard.press('Tab')
    await expect(control(', operand 1: variable path')).toBeFocused()

    await admin.keyboard.type('formData.amount')
    await expect(control(', operand 1: variable path')).toHaveValue('formData.amount')
    await expect(control(', operand 1: variable path')).toBeFocused()
    await expect(
      admin.getByTestId('transition-2-detail').getByText('Not in the offered variables'),
    ).toHaveCount(0)

    // Operand 2: a number. "nu" is *Number*, not *Not*.
    await admin.keyboard.press('Tab')
    await expect(control(', operand 2: kind')).toBeFocused()
    await admin.keyboard.type('nu')
    await expect(control(', operand 2: kind')).toHaveValue('number')

    await admin.keyboard.press('Tab')
    const number = control(', operand 2: number')
    await expect(number).toBeFocused()

    // **Through the editor's v-model** (#695, criterion 1). The first digit
    // completes the expression, so each of the four keystrokes is emitted,
    // written to the editor's draft and handed back. A builder that took its own
    // echo for a new value would rebuild its controls under the cursor: focus
    // would leave the field and the text would lose digits.
    await admin.keyboard.type(String(THRESHOLD))
    await expect(number).toHaveValue(String(THRESHOLD))
    await expect(number).toBeFocused()
    await expect(control(', operand 1: variable path')).toHaveValue('formData.amount')
    await expect(unfilled).toHaveCount(0)

    // **An unfilled operand survives a write elsewhere.** Emptying the number
    // leaves the editor holding the last complete expression, `> 1`. The
    // description is then typed, which writes a new draft. An editor that
    // cloned on write would hand the builder an equal copy of what it last
    // emitted, and the builder would reload it and put the 1 back.
    for (let digit = 0; digit < String(THRESHOLD).length; digit += 1) {
      await admin.keyboard.press('Backspace')
    }
    await expect(number).toHaveValue('')
    await expect(unfilled).toContainText('1 expression has an operand not filled in')

    await admin.getByLabel('Description', { exact: true }).fill('Built by #426 AC6.')

    await expect(number).toHaveValue('')
    await expect(unfilled).toContainText('1 expression has an operand not filled in')
    await expect(control(', operand 1: variable path')).toHaveValue('formData.amount')
    await expect(control(': operator')).toHaveValue('>')

    // And back into the builder by Tab, to finish it.
    await tabTo(admin, number, { limit: 80 })
    await admin.keyboard.type(String(THRESHOLD))
    await expect(unfilled).toHaveCount(0)

    // Enter collapses the expression to its summary, and Space opens it again.
    const collapse = admin.getByRole('button', { name: `Collapse ${condition}`, exact: true })
    const expand = admin.getByRole('button', { name: `Expand ${condition}`, exact: true })

    await tabTo(admin, collapse, { backward: true })
    await admin.keyboard.press('Enter')
    await expect(expand).toBeFocused()
    await expect(expand).toHaveAttribute('aria-expanded', 'false')
    await expect(admin.getByTestId('transition-2-detail')).toContainText(
      `(formData.amount > ${THRESHOLD})`,
    )

    await admin.keyboard.press('Space')
    await expect(collapse).toBeFocused()
    await expect(number).toBeVisible()

    // --- Save: what the server stored is what was built ----------------------
    //
    // The save's response names what was stored, which is a stronger claim than
    // the notice. It is a check of the editor's write, not of a list: README
    // rule 2's assertion that matters is the final status, in the browser.
    const created = admin.waitForResponse(
      (response) =>
        response.request().method() === 'POST' &&
        new URL(response.url()).pathname === `${API_PREFIX}/workflow/definitions`,
      { timeout: RESPONSE_TIMEOUT },
    )
    await admin.getByTestId('save-workflow').click()

    const createdResponse = await created
    expect(createdResponse.status(), await createdResponse.text()).toBe(201)
    const stored = (
      (await createdResponse.json()) as {
        data: {
          id: string
          version: number
          definition: { transitions: { condition?: unknown }[] }
        }
      }
    ).data
    expect(stored.definition.transitions[2]?.condition).toEqual(CONDITION)

    await expect(admin).toHaveURL(new RegExp(`/admin/workflows/${stored.id}$`))
    await expect(admin.getByTestId('notice')).toContainText('what the server stored')
    // The editor reloads from the stored definition, and the builder draws it.
    await expect(control(': operator')).toHaveValue('>')
    await expect(control(', operand 1: variable path')).toHaveValue('formData.amount')
    await expect(control(', operand 2: number')).toHaveValue(String(THRESHOLD))

    // --- Publish -------------------------------------------------------------
    //
    // Nothing is unsaved, so the button says *Publish*, not *Save and publish*.
    const publishButton = admin.getByTestId('publish-workflow')
    await expect(publishButton).toHaveText('Publish')

    const published = admin.waitForResponse(
      (response) =>
        response.request().method() === 'POST' &&
        new URL(response.url()).pathname ===
          `${API_PREFIX}/workflow/definitions/${stored.id}/publication`,
      { timeout: RESPONSE_TIMEOUT },
    )
    await publishButton.click()

    const publishedResponse = await published
    expect(publishedResponse.status(), await publishedResponse.text()).toBe(200)

    // AC5's side of it: a published revision is read-only, and says so.
    await expect(admin.getByTestId('status')).toHaveText('ACTIVE')
    await expect(admin.getByTestId('published-notice')).toBeVisible()
    await expect(admin.getByTestId('save-workflow')).toHaveCount(0)
    await expect(admin.getByTestId('new-revision')).toBeVisible()

    // --- Bind it to a document type, on the type screen ----------------------
    await admin.getByRole('link', { name: 'Document Types' }).click()
    await expect(admin).toHaveURL(/\/admin\/document-types$/)
    await admin.getByTestId('new-document-type').click()
    await expect(admin.getByTestId('document-type-dialog')).toBeVisible()

    await admin.getByTestId('type-code').fill(typeCode)
    await admin.getByTestId('type-name').fill(`Built workflow requisition ${suffix}`)
    await chooseOption(admin, 'type-form', form.formKey, `${form.title} (r1)`)
    // The chooser offers only published revisions, and is searched by this
    // run's key, so the option is this run's workflow and no other (#521).
    await chooseOption(admin, 'type-workflow', workflowKey, `${workflowName} (r${stored.version})`)
    await admin.getByTestId('type-status').selectOption('ACTIVE')

    // **Confirmed by the response, not by the row** (#503, #521): it names the
    // workflow bound, which is this run's revision and not merely a workflow.
    const typeCreated = admin.waitForResponse(
      (response) =>
        response.request().method() === 'POST' &&
        new URL(response.url()).pathname === `${API_PREFIX}/document-types`,
      { timeout: RESPONSE_TIMEOUT },
    )
    await admin.getByTestId('save-document-type').click()

    const typeResponse = await typeCreated
    expect(typeResponse.status(), await typeResponse.text()).toBe(201)
    const type = (
      (await typeResponse.json()) as {
        data: {
          id: string
          typeCode: string
          formId: string
          workflows: { workflowDefinitionId: string }[]
        }
      }
    ).data
    expect(type.typeCode).toBe(typeCode)
    expect(type.formId).toBe(form.id)
    expect(type.workflows.map((binding) => binding.workflowDefinitionId)).toEqual([stored.id])
    await expect(admin.getByTestId('document-type-dialog')).toBeHidden()

    await giveNumberingRule(session, type.id, `WF-${suffix}-{sequence}`)

    // --- A document of that type is submitted --------------------------------
    await admin.goto('/documents/new')
    await chooseDocumentType(admin, typeCode)
    await admin.getByTestId('new-document-title').fill(title)
    await admin.getByTestId('create-document').click()

    await expect(admin).toHaveURL(/\/documents\/[0-9a-f-]{36}$/)
    await expect(admin.getByTestId('document-status')).toHaveText('Draft')
    const documentUrl = admin.url()

    await admin.locator('#jfss-amount-field').fill(String(AMOUNT))
    await admin.getByRole('button', { name: 'Submit request' }).click()

    // The initial state maps to PENDING_APPROVAL, so that is the status a
    // submit leaves: the built workflow started.
    await expect(admin.getByTestId('document-status')).toHaveText('Pending approval')
    await expect(admin.getByTestId('document-number')).toContainText(`WF-${suffix}-`)
    const documentNumber = (await admin.getByTestId('document-number').textContent())?.trim() ?? ''

    await admin.getByTestId('tab-workflow').click()
    await expect(admin.getByTestId('workflow-instance')).toContainText(workflowName)
    await expect(admin.getByTestId('workflow-state')).toHaveText('Pending approval')
    await expect(admin.getByTestId('workflow-tasks')).toContainText(TASK_NAME)

    // --- Somebody else approves it from their inbox --------------------------
    //
    // The approver holds only this run's role, so their inbox holds only this
    // run's task.
    await signIn(decider, approver.username, approver.password)
    await decider.goto('/tasks')

    const row = decider.getByRole('row').filter({ hasText: documentNumber })
    await expect(row).toHaveCount(1)
    await row.getByRole('button', { name: 'Open' }).click()

    await expect(decider.getByTestId('task-name')).toHaveText(TASK_NAME)
    await expect(decider.getByTestId('task-document')).toContainText(title)

    // Two APPROVE edges leave the state, so the engine chooses the target and
    // the button names the verb alone (#271).
    const approve = decider.getByTestId('decide-APPROVE')
    await expect(approve).toContainText('Approve')
    await expect(approve).not.toContainText('→')
    await approve.click()

    await expect(decider.getByTestId('task-notice')).toContainText('The document is now Approved')
    await expect(decider.getByTestId('task-decided')).toContainText('has been decided')

    // --- **The assertion AC6 rests on** --------------------------------------
    //
    // Approved, by the conditioned edge, and not Completed, by the fallback.
    // The requester did nothing: the page is reloaded, not acted on.
    await admin.goto(documentUrl)

    await expect(admin.getByTestId('document-status')).toHaveText('Approved')

    await admin.getByTestId('tab-workflow').click()
    await expect(admin.getByTestId('workflow-state')).toHaveText(BRANCH_STATE)

    const history = admin.getByTestId('workflow-history')
    await expect(history).toContainText('PENDING_APPROVAL')
    await expect(history).toContainText('APPROVE')
    await expect(history).toContainText(approver.username)

    // --- **And the other branch: at or below the threshold, the fallback** ----
    //
    // The same type and workflow, and 900. The condition is false, so the
    // unconditioned edge is taken and the document ends *Completed*.
    await admin.goto('/documents/new')
    await chooseDocumentType(admin, typeCode)
    await admin.getByTestId('new-document-title').fill(smallTitle)
    await admin.getByTestId('create-document').click()

    await expect(admin).toHaveURL(/\/documents\/[0-9a-f-]{36}$/)
    const smallUrl = admin.url()

    await admin.locator('#jfss-amount-field').fill(String(SMALL_AMOUNT))
    await admin.getByRole('button', { name: 'Submit request' }).click()

    await expect(admin.getByTestId('document-status')).toHaveText('Pending approval')
    await expect(admin.getByTestId('document-number')).toContainText(`WF-${suffix}-`)
    const smallNumber = (await admin.getByTestId('document-number').textContent())?.trim() ?? ''

    await decider.goto('/tasks')

    const smallRow = decider.getByRole('row').filter({ hasText: smallNumber })
    await expect(smallRow).toHaveCount(1)
    await smallRow.getByRole('button', { name: 'Open' }).click()

    await expect(decider.getByTestId('task-document')).toContainText(smallTitle)
    await decider.getByTestId('decide-APPROVE').click()

    await expect(decider.getByTestId('task-notice')).toContainText('The document is now Completed')

    // Reloaded, not acted on, as above.
    await admin.goto(smallUrl)

    await expect(admin.getByTestId('document-status')).toHaveText('Completed')

    await admin.getByTestId('tab-workflow').click()
    await expect(admin.getByTestId('workflow-state')).toHaveText('Completed')
  } finally {
    await admin.close()
    await decider.close()
  }
})
