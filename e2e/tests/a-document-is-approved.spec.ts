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

    // --- Approving moves the instance --------------------------------------
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
