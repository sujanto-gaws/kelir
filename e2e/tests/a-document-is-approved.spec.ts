import { expect, test, type Page } from '@playwright/test'

import { runSuffix, signInOverApi, type ApiSession } from '../support/api'
import { createDocumentType, type SeededDocumentType } from '../support/documents'
import { credentials } from '../support/env'
import { publishForm, type SeededForm } from '../support/forms'
import {
  bindWorkflow,
  createApprover,
  publishWorkflow,
  type SeededApprover,
  type SeededWorkflow,
} from '../support/workflow'

/**
 * The Sprint 10 exit demo, driven rather than described (#179 AC6).
 *
 * > A document type carries a workflow; submitting a document of that type
 * > starts an instance; the instance generates a task that appears in an
 * > approver's inbox; approving it moves the instance and the document's status
 * > follows.
 *
 * Every clause of that sentence is a step below, in that order, and **two
 * people** are involved — which is the point of the flow rather than an
 * incidental detail. An approval one person raises and the same person approves
 * would exercise the mechanism and prove nothing about the seam it exists for:
 * the task has to reach somebody else's inbox, and the requester has to see the
 * result without doing anything.
 *
 * # The form is the smallest one that makes the document real
 *
 * `a-document-is-created-and-submitted.spec.ts` drives the *renderer* — live
 * validation, live calculation, the tamper-proof re-evaluation — and that is
 * that flow's subject. This one is about the **approval**, so the form is one
 * field: a richer one would make this spec fail for reasons the other spec
 * already covers, on a report that says nothing about workflows.
 *
 * # What is seeded over the API, and why that is not a gap
 *
 * Authoring a workflow and binding one to a document type have no screens until
 * the designer (FR-RAD-011, Sprints 14–16). Seeding them over the API is what
 * lets the browser half be about the parts that *do* have screens, which is what
 * the exit asks to see driven.
 */
const suffix = runSuffix()

const roleCode = `E2E-APPROVER-${suffix}`.toUpperCase()
const title = `Ten ergonomic chairs ${suffix}`
const reason = `Within the department budget ${suffix}`
const refusal = `The quotation does not match the figure ${suffix}`
const correction = `Use the revised quotation ${suffix}`

/** One number field, which is all the approval needs the document to hold. */
const definition = {
  formId: `e2e-approval-${suffix}`,
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
let documentType: SeededDocumentType
let workflow: SeededWorkflow
let approver: SeededApprover

test.beforeAll(async () => {
  session = await signInOverApi()

  form = await publishForm(session, definition, `Approval requisition ${suffix}`)
  documentType = await createDocumentType(session, form, `Approval requisition ${suffix}`)

  // The administrator's half: a published workflow, an approver who holds the
  // role it assigns to, and the type pointed at it. **The role has to exist
  // before the binding is used**, because an assignment that resolves to nobody
  // refuses the submit rather than leaving an approval that has silently
  // stopped.
  workflow = await publishWorkflow(session, roleCode)
  approver = await createApprover(session, roleCode)
  await bindWorkflow(session, documentType.id, workflow)
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

test('a submitted document is approved by somebody else, and its status follows', async ({
  browser,
}) => {
  const requester = await browser.newPage()
  const decider = await browser.newPage()

  try {
    const { username, password } = credentials()
    await signIn(requester, username, password)

    // --- A user creates a document from a type that carries a workflow ------
    await requester.goto('/documents/new')

    await requester
      .getByTestId(`type-${documentType.typeCode}`)
      .getByRole('radio')
      .check()
    await requester.getByTestId('new-document-title').fill(title)
    await requester.getByTestId('create-document').click()

    await expect(requester).toHaveURL(/\/documents\/[0-9a-f-]{36}$/)
    await expect(requester.getByTestId('document-status')).toHaveText('Draft')

    const documentUrl = requester.url()

    await requester.locator('#jfss-amount-field').fill('45000')

    // --- Submitting starts the approval, in the same transaction -----------
    //
    // **The status is the workflow's, not `Submitted`.** The initial state maps
    // to `PENDING_APPROVAL`, so that is where the document is at the end of the
    // submit — which is the projection being real rather than described.
    await requester.getByRole('button', { name: 'Submit request' }).click()

    await expect(requester.getByTestId('document-status')).toHaveText('Pending approval')
    await expect(requester.getByTestId('document-number')).toContainText('PR-')

    const number = (await requester.getByTestId('document-number').textContent())?.trim() ?? ''

    // --- And the workspace shows the process deciding it --------------------
    await requester.getByTestId('tab-workflow').click()

    await expect(requester.getByTestId('workflow-state')).toHaveText('Manager approval')
    await expect(requester.getByTestId('workflow-instance')).toContainText('Standard approval')
    await expect(requester.getByTestId('workflow-tasks')).toContainText('Approve the request')

    // --- The task appears in the approver's inbox --------------------------
    //
    // A different person, in a different session. The task is offered to a role
    // rather than assigned, so the inbox has to show it as unclaimed — the
    // distinction #179 AC1 exists for.
    await signIn(decider, approver.username, approver.password)

    await decider.goto('/tasks')

    const row = decider.getByRole('row').filter({ hasText: number })
    await expect(row).toHaveCount(1)
    await expect(row).toContainText('Unclaimed')

    await row.getByRole('button', { name: 'Open' }).click()

    await expect(decider.getByTestId('task-name')).toHaveText('Approve the request')
    // AC4: the task says what it is about and what is being decided, rather
    // than only "approve?".
    await expect(decider.getByTestId('task-document')).toContainText(title)
    await expect(decider.getByTestId('task-decisions')).toContainText('Completed')

    // --- Approving moves the instance, with the reason it was approved for --
    //
    // #182: the decision and the reason are entered together and sent in one
    // request. The APPROVE edge of the seeded workflow does not *require* one —
    // the REJECT does, and the test below drives that — so what this asserts is
    // that an optional reason is kept rather than dropped.
    await decider.getByLabel(/Reason/).fill(reason)
    await decider.getByTestId('decide-APPROVE').click()

    await expect(decider.getByTestId('task-notice')).toContainText('Completed')
    await expect(decider.getByTestId('task-decided')).toContainText('has been decided')

    // And it leaves the queue, which is what makes the inbox an inbox.
    await decider.goto('/tasks')
    await expect(decider.getByRole('row').filter({ hasText: number })).toHaveCount(0)

    // --- The document's status followed, and the requester did nothing ------
    //
    // The last clause of the exit sentence, and the whole seam: this page is
    // reloaded rather than acted on, so what changed it was the approval.
    await requester.goto(documentUrl)

    await expect(requester.getByTestId('document-status')).toHaveText('Completed')

    await requester.getByTestId('tab-workflow').click()
    await expect(requester.getByTestId('workflow-state')).toHaveText('Completed')
    await expect(requester.getByTestId('workflow-tasks')).toContainText('Done')

    // --- And the reason is visible where the decision is --------------------
    //
    // #182 AC2, driven rather than described: the requester — who did not take
    // the decision and cannot see the task — reads why it went the way it did,
    // in the history #181 records. A comment persisted and displayed nowhere
    // would not have been worth capturing.
    const history = requester.getByTestId('workflow-history')

    await expect(history).toContainText('MANAGER_APPROVAL')
    await expect(history).toContainText('APPROVE')
    await expect(history).toContainText(approver.username)
    await expect(history.getByTestId('history-comment')).toContainText(reason)
  } finally {
    await requester.close()
    await decider.close()
  }
})

test('a rejection cannot be recorded without a reason, and carries it once given', async ({
  browser,
}) => {
  // #182 AC4 and AC1, from the side a person meets them. The seeded workflow
  // marks its REJECT edge `requiresComment` and leaves the APPROVE alone
  // (JWSS §4.1), so this flow is what shows the two ends agreeing: the screen
  // refuses an empty box, and the same edge goes through once a reason is
  // there.
  const requester = await browser.newPage()
  const decider = await browser.newPage()

  try {
    const { username, password } = credentials()
    await signIn(requester, username, password)

    await requester.goto('/documents/new')
    await requester.getByTestId(`type-${documentType.typeCode}`).getByRole('radio').check()
    await requester.getByTestId('new-document-title').fill(`Refused outright ${suffix}`)
    await requester.getByTestId('create-document').click()

    await expect(requester).toHaveURL(/\/documents\/[0-9a-f-]{36}$/)

    const documentUrl = requester.url()

    await requester.locator('#jfss-amount-field').fill('120000')
    await requester.getByRole('button', { name: 'Submit request' }).click()

    await expect(requester.getByTestId('document-status')).toHaveText('Pending approval')

    const number = (await requester.getByTestId('document-number').textContent())?.trim() ?? ''

    await signIn(decider, approver.username, approver.password)
    await decider.goto('/tasks')

    await decider
      .getByRole('row')
      .filter({ hasText: number })
      .getByRole('button', { name: 'Open' })
      .click()

    await expect(decider.getByTestId('task-name')).toHaveText('Approve the request')

    // Pressing reject with nothing in the box is refused on the screen, and the
    // task is still there to decide — which is what makes this a refusal rather
    // than a decision with a missing field.
    await decider.getByTestId('decide-REJECT').click()

    await expect(decider.getByTestId('comment-required')).toContainText('needs a reason')
    await expect(decider.getByTestId('task-decided')).toHaveCount(0)

    // The same edge, with a reason on it, goes through.
    await decider.getByLabel(/Reason/).fill(refusal)
    await decider.getByTestId('decide-REJECT').click()

    await expect(decider.getByTestId('task-notice')).toContainText('Rejected')
    await expect(decider.getByTestId('task-decided')).toContainText('has been decided')

    // And the requester finds out why, without having to ask anybody.
    await requester.goto(documentUrl)
    await expect(requester.getByTestId('document-status')).toHaveText('Rejected')

    await requester.getByTestId('tab-workflow').click()
    await expect(
      requester.getByTestId('workflow-history').getByTestId('history-comment'),
    ).toContainText(refusal)
  } finally {
    await requester.close()
    await decider.close()
  }
})

test('a returned document is corrected, sent again, and keeps its number', async ({ browser }) => {
  // #183, the loop that reject cannot make: the approver sends it back with a
  // reason, the requester corrects it without losing the number, and the second
  // pass approves. Driven through both people's screens, because the whole point
  // of return is what it saves the *requester* — and that is invisible from the
  // approver's side.
  const requester = await browser.newPage()
  const decider = await browser.newPage()

  try {
    const { username, password } = credentials()
    await signIn(requester, username, password)

    await requester.goto('/documents/new')
    await requester.getByTestId(`type-${documentType.typeCode}`).getByRole('radio').check()
    await requester.getByTestId('new-document-title').fill(`Sent back once ${suffix}`)
    await requester.getByTestId('create-document').click()

    await expect(requester).toHaveURL(/\/documents\/[0-9a-f-]{36}$/)
    const documentUrl = requester.url()

    await requester.locator('#jfss-amount-field').fill('99000')
    await requester.getByRole('button', { name: 'Submit request' }).click()

    await expect(requester.getByTestId('document-status')).toHaveText('Pending approval')

    const number = (await requester.getByTestId('document-number').textContent())?.trim() ?? ''
    expect(number).not.toEqual('')

    // --- The approver sends it back, with the reason -----------------------
    await signIn(decider, approver.username, approver.password)
    await decider.goto('/tasks')

    await decider
      .getByRole('row')
      .filter({ hasText: number })
      .getByRole('button', { name: 'Open' })
      .click()

    // The button says what it does and is not styled like the terminal one.
    await expect(decider.getByTestId('decide-RETURN')).toContainText('Send back')

    await decider.getByLabel(/Reason/).fill(correction)
    await decider.getByTestId('decide-RETURN').click()

    await expect(decider.getByTestId('task-notice')).toContainText('Returned')
    await expect(decider.getByTestId('task-decided')).toContainText('has been decided')

    // --- The requester finds it back, editable, with its number intact -----
    await requester.goto(documentUrl)

    await expect(requester.getByTestId('document-status')).toHaveText('Returned')
    await expect(requester.getByTestId('document-number')).toHaveText(number)

    // The form is live again — this is the assertion that separates a return
    // from a rejection with a softer name.
    await expect(requester.getByTestId('document-form')).not.toHaveAttribute('data-readonly', 'true')

    // And they are told why, without having to ask anybody.
    await requester.getByTestId('tab-workflow').click()
    await expect(
      requester.getByTestId('workflow-history').getByTestId('history-comment'),
    ).toContainText(correction)

    // --- Corrected and sent again ------------------------------------------
    await requester.getByTestId('tab-form').click()
    await requester.locator('#jfss-amount-field').fill('64000')
    await requester.getByRole('button', { name: 'Submit request' }).click()

    await expect(requester.getByTestId('document-status')).toHaveText('Pending approval')
    await expect(requester.getByTestId('document-number')).toHaveText(number)

    // --- The second pass approves, and the loop closes ---------------------
    await decider.goto('/tasks')

    await decider
      .getByRole('row')
      .filter({ hasText: number })
      .getByRole('button', { name: 'Open' })
      .click()

    await decider.getByTestId('decide-APPROVE').click()
    await expect(decider.getByTestId('task-notice')).toContainText('Completed')

    await requester.goto(documentUrl)
    await expect(requester.getByTestId('document-status')).toHaveText('Completed')
    await expect(requester.getByTestId('document-number')).toHaveText(number)
  } finally {
    await requester.close()
    await decider.close()
  }
})

test('a document under a workflow cannot have its status set by hand', async ({ page }) => {
  // #178 AC2, from the side a person meets it. The synchronization is one-way:
  // a workflow transition sets the document's status, and setting the status
  // does not move the workflow — so the transition the History tab offers is
  // refused, with the backend saying why rather than the screen guessing.
  const { username, password } = credentials()
  await signIn(page, username, password)

  await page.goto('/documents/new')
  await page.getByTestId(`type-${documentType.typeCode}`).getByRole('radio').check()
  await page.getByTestId('new-document-title').fill(`Under approval ${suffix}`)
  await page.getByTestId('create-document').click()

  await expect(page).toHaveURL(/\/documents\/[0-9a-f-]{36}$/)

  await page.locator('#jfss-amount-field').fill('900')
  await page.getByRole('button', { name: 'Submit request' }).click()

  await expect(page.getByTestId('document-status')).toHaveText('Pending approval')

  await page.getByTestId('tab-history').click()

  // `PENDING_APPROVAL` offers no transition in the client's own legality table
  // either — it is the state #169 made reachable from nothing and Phase 5
  // filled — so what the History tab shows is the end of what a person may do
  // here, and the approval is the only way on.
  await expect(page.getByTestId('document-history')).toContainText('Submitted')
  await expect(page.getByTestId('transition-APPROVED')).toHaveCount(0)
  await expect(page.getByTestId('transition-IN_REVIEW')).toHaveCount(0)
})
