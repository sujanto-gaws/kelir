import { expect, test } from '@playwright/test'

import { runSuffix, signInOverApi, type ApiSession } from '../support/api'
import { createDocumentType, createDraft, submitDocument } from '../support/documents'
import { publishForm } from '../support/forms'
import {
  addApprover,
  bindWorkflow,
  createApproverRole,
  publishSingleStepWorkflow,
  DIRECTOR_DUE_IN_HOURS,
  type SeededApprover,
} from '../support/workflow'

/**
 * The dashboard says what is late, and only what is late (FR-RPT-005, #446 AC6).
 *
 * Sprint 17 shipped three dashboard screens and no browser spec, which is why
 * #446 writes this criterion out rather than assuming it. This flow reaches the
 * fourth widget through the release stack, as the person whose work is late.
 *
 * # Two tasks, because one would prove nothing about *late*
 *
 * A card that listed every open task would pass a flow holding one late task.
 * So the approver holds **two**: one whose deadline has passed, and one with
 * **no deadline at all**, which #446 AC2 says is never overdue. The late one has
 * to be on the card and the undated one has to be absent from it. The undated
 * one also has to be **on the pending card**, which shows the widget tells late
 * work apart from waiting work, rather than being empty because the undated task
 * never arrived.
 *
 * # A role of this run's own
 *
 * The deployment keeps its database between runs (README, first rule). A role
 * named for this run means the approver's late set is exactly this run's one
 * task, so the flow can assert the card's whole contents rather than only that
 * one row is somewhere in it.
 *
 * # What is seeded over the API
 *
 * The form, the types, the workflows, the people, **and the two submits**.
 * Raising and submitting a document through the browser is
 * `a-document-is-approved.spec.ts`'s subject. Driving it again here would fail
 * this flow for reasons that one already reports.
 */
const suffix = runSuffix()

const roleCode = `E2E-LATE-${suffix}`.toUpperCase()
const lateTitle = `Past its date ${suffix}`
const undatedTitle = `No date on it ${suffix}`
const lateTaskName = 'Approve before the deadline'
const undatedTaskName = 'Approve when you can'

/**
 * One optional field, in the shape `a-document-is-approved.spec.ts` already
 * publishes.
 *
 * JFSS requires `validation` on every data component, so it is there, with a
 * type and no rule. The drafts are submitted with no form data, so a
 * `required` or a `minimum` would refuse the submit for a reason this flow is
 * not about.
 */
const definition = {
  formId: `e2e-late-${suffix}`,
  version: '2.0.1',
  title: 'Late request',
  components: [
    {
      id: 'amount-field',
      role: 'data',
      type: 'number',
      key: 'amount',
      label: 'Amount',
      validation: { type: 'number' },
    },
  ],
}

let session: ApiSession
let approver: SeededApprover

test.beforeAll(async () => {
  session = await signInOverApi()

  const form = await publishForm(session, definition, `Late request ${suffix}`)

  // **Two types, because a type binds one workflow**, and the two tasks differ
  // in exactly one thing: whether the definition gives them a deadline.
  const lateType = await createDocumentType(session, form, `Late request ${suffix}`)
  const undatedType = await createDocumentType(session, form, `Undated request ${suffix}`)

  // The role and its holder first: an assignment that resolves to nobody
  // refuses the submit (see `a-document-is-approved.spec.ts`).
  //
  // `reporting:dashboard:read` is the permission the summary requires. The
  // helper's base set already carries `workflow:task:read`, which is what makes
  // a row a link to the task screen. Without it the rows render as plain text
  // and the last step below would have nothing to click.
  const roleId = await createApproverRole(session, roleCode, ['reporting:dashboard:read'])
  approver = await addApprover(session, roleId, 'late')

  // `DIRECTOR_DUE_IN_HOURS` for the reason it gives: a deadline short enough to
  // elapse inside a run, because the widget is what is being demonstrated and
  // not the duration.
  const lateWorkflow = await publishSingleStepWorkflow(session, roleCode, {
    taskName: lateTaskName,
    dueInHours: DIRECTOR_DUE_IN_HOURS,
  })
  const undatedWorkflow = await publishSingleStepWorkflow(session, roleCode, {
    taskName: undatedTaskName,
  })

  await bindWorkflow(session, lateType.id, lateWorkflow)
  await bindWorkflow(session, undatedType.id, undatedWorkflow)

  // Each submit starts its instance and generates its task in the same
  // transaction, and that is when `dueAt` is stamped.
  await submitDocument(session, await createDraft(session, lateType, lateTitle))
  await submitDocument(session, await createDraft(session, undatedType, undatedTitle))
})

test.afterAll(async () => {
  await session?.context.dispose()
})

test('the dashboard lists the late task, not the undated one, and opens it', async ({ page }) => {
  // --- The deadline elapses ---------------------------------------------------
  //
  // **A real wait, for the reason `a-document-is-approved.spec.ts` gives.**
  // `dueAt` was stamped by the server when the task was generated, and lateness
  // is decided by the server against that stamp, so nothing the browser does
  // can bring it forward. The summary is read when the page loads, so the page
  // must be loaded after the deadline passes. 1.44 seconds, plus a margin.
  await page.waitForTimeout(2_000)

  // --- The approver signs in and lands on the dashboard ----------------------
  await page.goto('/login')
  await page.getByLabel('Username or email').fill(approver.username)
  await page.getByLabel('Password').fill(approver.password)
  await page.getByRole('button', { name: 'Sign in' }).click()

  await expect(page).toHaveURL(/\/$/)

  // --- The card lists the late task, and says when it was due ----------------
  const overdue = page.getByTestId('overdue-tasks')

  await expect(overdue).toContainText('What is late')

  const late = overdue.getByTestId('overdue-task').filter({ hasText: lateTitle })

  await expect(late).toHaveCount(1)
  await expect(late).toContainText(lateTaskName)
  await expect(late.getByTestId('overdue-task-due')).toContainText('Was due')

  // --- And nothing else: the undated task is not late -------------------------
  //
  // **Asserted after the positive row, deliberately.** `toHaveCount(0)` passes
  // at once on a card that has not rendered yet. With the late row already on
  // screen, the summary has arrived, so the absence is real.
  //
  // The whole card is this run's one task, because the role is this run's own.
  await expect(overdue.getByTestId('overdue-task')).toHaveCount(1)
  await expect(overdue.getByTestId('overdue-task').filter({ hasText: undatedTitle })).toHaveCount(
    0,
  )
  await expect(overdue.getByTestId('overdue-empty')).toHaveCount(0)
  await expect(overdue.getByTestId('overdue-more')).toHaveCount(0)

  // --- The undated task is waiting: on the pending card, and not marked late --
  //
  // This is what shows the card distinguishes late from waiting. The undated
  // task reached this person's queue. The overdue card left it out because it
  // is not late, not because it never arrived.
  const pending = page.getByTestId('pending-tasks')
  const waiting = pending.getByTestId('pending-task').filter({ hasText: undatedTitle })

  await expect(waiting).toHaveCount(1)
  await expect(waiting).toContainText(undatedTaskName)
  await expect(waiting.getByTestId('pending-task-overdue')).toHaveCount(0)

  // Overdue is a subset of open (`InboxScope`), so the late task is on the
  // pending card too, flagged there. Two cards reading one queue must not
  // disagree about the same row.
  await expect(
    pending.getByTestId('pending-task').filter({ hasText: lateTitle }).getByTestId(
      'pending-task-overdue',
    ),
  ).toHaveCount(1)

  // --- The late row opens the task it names -----------------------------------
  await late.getByRole('link', { name: lateTaskName }).click()

  await expect(page).toHaveURL(/\/tasks\/[0-9a-f-]{36}$/)
  await expect(page.getByTestId('task-name')).toHaveText(lateTaskName)
  await expect(page.getByTestId('task-document')).toContainText(lateTitle)
  await expect(page.getByTestId('task-overdue')).toHaveText('Late')
})
