import * as os from 'node:os'

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
 * for create, edit and publish; deprecate is built, #715, and covered by the
 * Vue specs, not by AC6).
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

// ---------------------------------------------------------------------------
// #721: what one layout of a large definition costs, in Chromium
// ---------------------------------------------------------------------------

/**
 * The read-only graph lays a definition out with dagre, synchronously, on the
 * main thread (#721, Sprint 24 row 17). This test **measures** that at the
 * five sizes #721 names, in a real browser, and asserts nothing about time.
 * The remedy (a worker, a stated ceiling, or the graph declining above a size)
 * is chosen on these numbers afterwards, so a threshold here would be the
 * remedy decided in advance (AC11, AC14).
 *
 * # What the number is, and what it is not
 *
 * **Wall time from the click on *Graph* to the caption**, taken with
 * `performance.now()` in the page: a capture listener stamps the click and a
 * `MutationObserver` stamps the caption showing this draft's counts. In that
 * window run dagre's layout **and** Vue Flow's first render of the nodes and
 * edges, in one synchronous render: `WorkflowGraph.vue` reads the layout while
 * it builds the nodes it hands Vue Flow, and draws the caption in the same
 * pass. Dagre alone cannot be timed without instrumenting the product, which
 * this row does not do (AC10).
 *
 * Beside it, **the long tasks that overlap the window**: their summed
 * duration, their count and the longest. A long task is 50 ms or more of main
 * thread the page could not answer input in, so the longest is how long the
 * tab froze. Then the long tasks **after** the caption until the page is quiet,
 * which is Vue Flow measuring the nodes, drawing the edges and fitting the
 * view. Recorded, not asserted.
 *
 * **The jsdom figures in #721 are not comparable.** jsdom renders nothing, and
 * its shape was not this one; the issue's table is not reproduced here as if
 * it were the measured shape (AC6).
 *
 * # The shape, and why it is not quite `large()`'s
 *
 * `workflowGraph.shapes.spec.ts`'s `large()`: steps approving forward, each
 * step after the first returning to the first step and rejecting to one final
 * `REJECTED` state. **A draft save refuses that shape**: the save runs the
 * structural rules, S9 among them, and `large()` maps no state to `COMPLETED`
 * or `CANCELLED`. Posted on 2026-10-22, it came back `422` `S9`
 * `NO_TERMINAL_STATUS`. So the last step approves into a final `COMPLETED`
 * state, and N states are N−2 steps, `REJECTED` and `COMPLETED`, with 3N−8
 * transitions. The counts are read from the definition built, and logged
 * beside each size (AC5, AC6). Every edge names the first test's role in
 * `allowedBy` (S5) and every step offers its task to it; a draft is never
 * published, so the role check at publish (D-111) does not apply.
 *
 * # One layout per size, and the chunk already fetched
 *
 * One page, so one browser context, throughout. The *Graph* tab is opened
 * once on the smallest draft first, which fetches the graph's lazy chunk
 * (ADR-0046); that open is logged as a warm-up and is not a measured size
 * (AC7). Each size is then a fresh page load with the tab closed, so the click
 * lays the graph out once, smallest to largest (AC9).
 *
 * **A fresh load does not keep the chunk warm by itself.** The release stack
 * serves the bundle with no `Cache-Control`, so Chromium holds the chunk only
 * as long as its heuristic freshness lasts, and then asks for it again on the
 * click. On 2026-10-22 that put the chunk's four files inside the measured
 * window at 150 and at 300 states. So the warm-up records which files the
 * click fetched, and each fresh load is given them before its click: the
 * scripts as `modulepreload`, which puts them in the page's module map where
 * the click's `import()` finds them, and the stylesheet as a stylesheet. **The
 * test then asserts that nothing was fetched inside the window**, so a cold
 * chunk fails the measurement rather than joining it.
 */
const LAYOUT_SIZES = [20, 40, 80, 150, 300] as const

/**
 * The test's own timeout (AC2), above the config's 60 s.
 *
 * jsdom took 43.7 s for one layout at 300 states. Chromium has not been
 * measured, which is the point of the test, and on top of the layouts come six
 * page loads in which the editor lists every state. Ten minutes leaves the
 * 300-state case many times the jsdom figure before the test gives up.
 */
const LAYOUT_TEST_TIMEOUT = 10 * 60_000

/**
 * The wait for the caption, explicit because the config's 15 s `expect`
 * timeout would otherwise govern it (AC2). Inside the test's own, and the only
 * wait in this test that a slow layout can fail.
 */
const CAPTION_TIMEOUT = 5 * 60_000

/** Loading the editor on a large draft lists every state. That is not measured. */
const EDITOR_TIMEOUT = 2 * 60_000

/** How long without a long task counts as quiet: before the click, and after the caption. */
const QUIET_MS = 1_500

/** The fixed tail of the caption `WorkflowGraph.vue` draws. */
const CAPTION_TAIL = '. Read-only: change the workflow in the list.'

/** A seeded draft, with the counts read from the definition that was posted. */
interface LayoutDraft {
  readonly id: string
  readonly workflowKey: string
  readonly size: number
  readonly states: number
  readonly transitions: number
}

/** What the page records. Stamps are `performance.now()`, in milliseconds. */
interface LayoutProbe {
  clickAt: number | null
  captionAt: number | null
  frameAt: number | null
  longTasks: { start: number; duration: number }[]
}

type ProbedWindow = Window & { __layoutProbe?: LayoutProbe }

/** The approval chain at `size` states, as described in the test's header. */
function approvalChain(size: number, workflowKey: string) {
  const steps = Array.from({ length: size - 2 }, (_, at) => `STEP_${at}`)
  const allowedBy = `ROLE:${roleCode}`

  const states: Record<string, unknown>[] = steps.map((code, at) => ({
    code,
    name: `Approval step ${at + 1}`,
    mapsToDocumentStatus: 'PENDING_APPROVAL',
    task: {
      taskDefinitionKey: `step_${at}`,
      taskName: `Approve at step ${at + 1}`,
      assignment: { assigneeType: 'ROLE', roleCode },
    },
  }))
  states.push(
    { code: 'REJECTED', name: 'Rejected', mapsToDocumentStatus: 'REJECTED', isFinal: true },
    // What S9 needs and `large()` lacks: the chain approves into it.
    { code: 'COMPLETED', name: 'Completed', mapsToDocumentStatus: 'COMPLETED', isFinal: true },
  )

  const transitions: Record<string, unknown>[] = []
  steps.forEach((code, at) => {
    transitions.push({ from: code, to: steps[at + 1] ?? 'COMPLETED', action: 'APPROVE', allowedBy })

    if (at > 0) {
      transitions.push(
        { from: code, to: steps[0], action: 'RETURN', allowedBy },
        { from: code, to: 'REJECTED', action: 'REJECT', allowedBy },
      )
    }
  })

  return {
    workflowKey,
    version: '1.0.0',
    name: `Layout at ${size} states ${suffix}`,
    initialState: steps[0],
    states,
    transitions,
  }
}

/** Posts one draft (AC3). Saved, never published. */
async function seedLayoutDraft(size: number): Promise<LayoutDraft> {
  const workflowKey = `e2e_layout_${size}_${suffix}`.toLowerCase()
  const definition = approvalChain(size, workflowKey)

  const created = await session.context.post(`${API_PREFIX}/workflow/definitions`, {
    data: { workflowKey, name: definition.name, definition },
  })

  expect(
    created.ok(),
    `seeding the ${size}-state draft failed: ${created.status()} ${await created.text()}`,
  ).toBeTruthy()

  return {
    id: ((await created.json()) as { data: { id: string } }).data.id,
    workflowKey,
    size,
    states: definition.states.length,
    transitions: definition.transitions.length,
  }
}

function captionOf(draft: LayoutDraft): string {
  return `${draft.states} states, ${draft.transitions} transitions${CAPTION_TAIL}`
}

/** A fresh load of the editor on a draft, with every state listed and the graph closed. */
async function openEditor(page: Page, draft: LayoutDraft): Promise<void> {
  await page.goto(`/admin/workflows/${draft.id}`)

  await expect(page.getByTestId('workflow-key')).toHaveValue(draft.workflowKey, {
    timeout: EDITOR_TIMEOUT,
  })
  await expect(page.getByTestId(`state-code-${draft.states - 1}`)).toHaveValue('COMPLETED', {
    timeout: EDITOR_TIMEOUT,
  })
  await expect(page.getByTestId('workflow-graph')).toHaveCount(0)
}

/**
 * Installs the page's clocks before the click (AC8).
 *
 * **The window is from the click to the caption showing this draft's
 * counts.** The click is stamped by a capture listener, before the tab's own
 * handler runs; the caption by a `MutationObserver`, which runs once the
 * render that inserted it has finished. One frame later, two animation frames
 * after the caption, `frameAt` is stamped, and the observer is read only after
 * that.
 */
async function installProbe(page: Page, caption: string): Promise<void> {
  await page.evaluate((expected) => {
    const probe: LayoutProbe = { clickAt: null, captionAt: null, frameAt: null, longTasks: [] }
    ;(window as ProbedWindow).__layoutProbe = probe

    // `buffered`, so the page load's long tasks are there to wait out.
    new PerformanceObserver((list) => {
      for (const entry of list.getEntries()) {
        probe.longTasks.push({ start: entry.startTime, duration: entry.duration })
      }
    }).observe({ type: 'longtask', buffered: true })

    document.addEventListener(
      'click',
      (event) => {
        if (
          probe.clickAt === null &&
          event.target instanceof Element &&
          event.target.closest('[data-testid="view-graph"]')
        ) {
          probe.clickAt = performance.now()
        }
      },
      { capture: true },
    )

    const watcher = new MutationObserver(() => {
      const shown = document.querySelector('[data-testid="workflow-graph-summary"]')

      // Whitespace collapsed, as Playwright's `toHaveText` reads it.
      const text = shown?.textContent?.replace(/\s+/g, ' ').trim()

      if (probe.clickAt === null || probe.captionAt !== null || text !== expected) {
        return
      }

      probe.captionAt = performance.now()
      watcher.disconnect()
      requestAnimationFrame(() =>
        requestAnimationFrame(() => {
          probe.frameAt = performance.now()
        }),
      )
    })
    watcher.observe(document.body, { childList: true, subtree: true, characterData: true })
  }, caption)
}

/** Waits until no long task has run for `QUIET_MS`, counting from the caption once it shows. */
async function waitForQuiet(page: Page): Promise<void> {
  await page.waitForFunction(
    (quiet) => {
      const probe = (window as ProbedWindow).__layoutProbe

      if (probe === undefined) {
        return false
      }

      const lastEnd = probe.longTasks.reduce(
        (end, task) => Math.max(end, task.start + task.duration),
        probe.captionAt ?? 0,
      )

      return performance.now() - lastEnd >= quiet
    },
    QUIET_MS,
    { timeout: EDITOR_TIMEOUT, polling: 250 },
  )
}

/**
 * Clicks *Graph* and waits for the caption; returns one greppable line.
 *
 * **The caption reading exactly the seeded counts is the one assertion**
 * (AC11): the graph drew every state and every transition of a large
 * definition, and nothing as *not declared*.
 */
async function measureLayout(
  page: Page,
  draft: LayoutDraft,
  chromium: string,
): Promise<{ line: string; fetched: string[] }> {
  const caption = captionOf(draft)

  await installProbe(page, caption)
  await waitForQuiet(page)

  await page.getByTestId('view-graph').click()

  await expect(page.getByTestId('workflow-graph-summary')).toHaveText(caption, {
    timeout: CAPTION_TIMEOUT,
  })
  await page.waitForFunction(
    () => typeof (window as ProbedWindow).__layoutProbe?.frameAt === 'number',
    null,
    { timeout: EDITOR_TIMEOUT },
  )
  await waitForQuiet(page)

  const { probe, fetched } = await page.evaluate(() => {
    const probe = (window as ProbedWindow).__layoutProbe as LayoutProbe
    const fetched = performance
      .getEntriesByType('resource')
      .filter(
        (entry) =>
          entry.startTime >= (probe.clickAt ?? 0) &&
          entry.startTime <= (probe.captionAt ?? 0) &&
          (entry as PerformanceResourceTiming).transferSize > 0,
      )
      .map((entry) => new URL(entry.name).pathname)

    return { probe, fetched }
  })

  const clickAt = probe.clickAt as number
  const captionAt = probe.captionAt as number
  const frameAt = probe.frameAt as number

  // A task overlapping the window counts whole. The click's own task began
  // before the listener stamped it, and the render ends after the observer
  // stamped the caption, so the layout's task straddles both edges.
  const within = probe.longTasks.filter(
    (task) => task.start <= captionAt && task.start + task.duration >= clickAt,
  )
  const after = probe.longTasks.filter((task) => task.start > captionAt)
  const settledAt = after.reduce((end, task) => Math.max(end, task.start + task.duration), frameAt)

  const sum = (tasks: { duration: number }[]) =>
    Math.round(tasks.reduce((total, task) => total + task.duration, 0))
  const longest = Math.round(Math.max(0, ...within.map((task) => task.duration)))

  const line = [
    `#721 layout: size=${draft.size}`,
    `states=${draft.states}`,
    `transitions=${draft.transitions}`,
    `wall_ms=${Math.round(captionAt - clickAt)}`,
    `longtask_ms=${sum(within)}`,
    `longtasks=${within.length}`,
    `longtask_max_ms=${longest}`,
    `frame_ms=${Math.round(frameAt - clickAt)}`,
    `after_longtask_ms=${sum(after)}`,
    `after_longtasks=${after.length}`,
    `settled_ms=${Math.round(settledAt - clickAt)}`,
    // What the page fetched inside the window, by path. The graph's chunk
    // here would mean the warm-up did not warm it.
    `fetched=${fetched.length}${fetched.length > 0 ? `[${fetched.join(',')}]` : ''}`,
    `chromium=${chromium}`,
  ].join(' ')

  return { line, fetched }
}

/** The files under `/assets/` the page fetched since `since`, by path. */
async function assetsFetchedSince(page: Page, since: number): Promise<string[]> {
  return page.evaluate(
    (from) =>
      performance
        .getEntriesByType('resource')
        .filter((entry) => entry.startTime >= from)
        .map((entry) => new URL(entry.name).pathname)
        .filter((path) => path.startsWith('/assets/')),
    since,
  )
}

/**
 * Hands a fresh load the graph's chunk before its click (see the test's
 * header). The `href` is the path, as the bundle's own preload helper writes
 * it, so the helper finds the links and adds none of its own.
 */
async function preloadChunk(page: Page, chunk: readonly string[]): Promise<void> {
  await page.evaluate(
    (paths) =>
      Promise.all(
        paths.map(
          (path) =>
            new Promise<void>((resolve, reject) => {
              const link = document.createElement('link')
              link.rel = path.endsWith('.css') ? 'stylesheet' : 'modulepreload'
              link.href = path
              link.onload = () => resolve()
              link.onerror = () => reject(new Error(`could not preload ${path}`))
              document.head.append(link)
            }),
        ),
      ).then(() => undefined),
    [...chunk],
  )
}

/** Records a line where a reader of the run will find it (AC12). */
function record(type: string, line: string): void {
  console.log(line)
  test.info().annotations.push({ type, description: line })
}

test('the graph of a large draft is laid out and drawn once per size, and the time it takes is recorded', async ({
  browser,
}) => {
  test.setTimeout(LAYOUT_TEST_TIMEOUT)

  const chromium = browser.version()
  const cpus = os.cpus()
  record(
    '#721 machine',
    `#721 layout machine: chromium=${chromium} cpu="${cpus[0]?.model.trim() ?? 'unknown'}" ` +
      `threads=${cpus.length} memory_gb=${Math.round(os.totalmem() / 2 ** 30)} ` +
      `os=${os.platform()}-${os.release()}`,
  )

  const drafts: LayoutDraft[] = []
  for (const size of LAYOUT_SIZES) {
    drafts.push(await seedLayoutDraft(size))
  }

  const page = await browser.newPage()

  try {
    const { username, password } = credentials()
    await signIn(page, username, password)

    // --- Warm-up: the chunk is fetched here, and this open is not a size ----
    const smallest = drafts[0]
    await openEditor(page, smallest)
    const warmFrom = await page.evaluate(() => performance.now())
    const warmStart = Date.now()
    await page.getByTestId('view-graph').click()
    await expect(page.getByTestId('workflow-graph-summary')).toHaveText(captionOf(smallest), {
      timeout: CAPTION_TIMEOUT,
    })
    const warmMs = Date.now() - warmStart
    const chunk = await assetsFetchedSince(page, warmFrom)
    record(
      '#721 warm-up',
      `#721 layout warm-up, not a measurement: size=${smallest.size} ` +
        `click_to_caption_ms=${warmMs} (Playwright clock) chunk=[${chunk.join(',')}]`,
    )
    expect(chunk, 'opening the graph fetched no chunk, so there is nothing to warm').not.toEqual([])

    // --- One layout per size, each on a fresh load, smallest first ----------
    for (const draft of drafts) {
      await openEditor(page, draft)
      await preloadChunk(page, chunk)

      const { line, fetched } = await measureLayout(page, draft, chromium)
      record('#721 layout', line)
      // Not a time: whether the number is the layout's alone (AC7).
      expect(fetched, `fetched inside the measured window at ${draft.size} states`).toEqual([])
    }
  } finally {
    await page.close()
  }
})
