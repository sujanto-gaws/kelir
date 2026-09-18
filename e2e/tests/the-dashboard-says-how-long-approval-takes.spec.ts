import { expect, test } from '@playwright/test'

import { runSuffix, signInOverApi, type ApiSession } from '../support/api'
import { createDocumentType, createDraft, submitDocument } from '../support/documents'
import { publishForm } from '../support/forms'
import {
  addApprover,
  bindWorkflow,
  createApproverRole,
  decideDocumentTask,
  publishSingleStepWorkflow,
  type SeededApprover,
} from '../support/workflow'

/**
 * The dashboard says how long approval takes (FR-RPT-006, #461 AC5).
 *
 * The approval time card is three numbers over the documents **the caller
 * raised** that were approved or rejected (D-83). The unit specs render it from
 * a fixture; this flow reaches it through the release stack with rows the engine
 * wrote, as a person whose documents have been decided.
 *
 * # Two decided, one in flight, one never sent
 *
 * The requester raises four documents. One is approved and one rejected — **a
 * rejection is a decision**, so the card must say `2` rather than `1`. One is
 * submitted and left waiting, and one stays a draft: neither has been decided,
 * so a card counting submissions, or counting documents, says `3` or `4`.
 *
 * # Somebody else's decided document, same tenant and type
 *
 * The administrator raises one too and it is approved. The card is about what
 * the caller raised, so it must not move the count to `3`.
 *
 * # Two times the card cannot confuse (#488)
 *
 * **The rejection waits a planted 75 seconds; the approval does not.** Until
 * #488 every decision landed seconds after its submission, so both labels were
 * *under a minute*. The flow asserted only that each was *a* duration, and
 * record 17 finding 7 showed it passing with the median rendering the slowest,
 * and with every time labelled *under a minute*.
 *
 * The seeding holds each label a wide margin from its edges:
 *
 * - **Slowest: `1 min`.** The rejection is decided at least 75 seconds after it
 *   was submitted. The label turns `2 min` only past 120 seconds, 45 seconds of
 *   slack for a slow runner.
 * - **Median: `under a minute`.** Two decided, so the median is the mean of
 *   the two, rounded down (`approval_time.rs`). The approval takes a few
 *   seconds and the rejection about 75, which averages about 40. The approval
 *   would have to take 45 seconds before the median reached a minute.
 *
 * So a median that renders the slowest reads `1 min`, and a label that always
 * says *under a minute* reads that for the slowest. Both fail here.
 *
 * **The wait is real time, because the release stack gives the flow no other
 * way to date a submission.** The flow seeds over the API, and nothing on the
 * API sets `started_at`. The wait costs this spec 75 seconds.
 *
 * # A requester of this run's own
 *
 * The deployment keeps its database between runs (README, first rule). A user
 * created for this run has raised exactly this run's documents, so the count is
 * exact.
 */
const suffix = runSuffix()

const roleCode = `E2E-APPROVAL-TIME-${suffix}`.toUpperCase()

/** One optional field, for the reason the status-card flow gives: the submit carries no form data. */
const definition = {
  formId: `e2e-approval-time-${suffix}`,
  version: '2.0.1',
  title: 'Approval time request',
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
let requester: SeededApprover

/** How long the rejection waits after its submission: see *Two times* above. */
const SLOW_DECISION_MS = 75_000

test.beforeAll(async () => {
  // The planted wait, plus the seeding around it.
  test.setTimeout(SLOW_DECISION_MS + 90_000)

  session = await signInOverApi()

  const form = await publishForm(session, definition, `Approval time request ${suffix}`)
  const documentType = await createDocumentType(session, form, `Approval time request ${suffix}`)

  const roleId = await createApproverRole(session, roleCode, [
    'reporting:dashboard:read',
    'document:create',
    'document:submit',
  ])
  requester = await addApprover(session, roleId, 'approval-time')
  const approver = await addApprover(session, roleId, 'approval-time-approver')

  await bindWorkflow(
    session,
    documentType.id,
    await publishSingleStepWorkflow(session, roleCode, { taskName: 'Decide the request' }),
  )

  const asRequester = await signInOverApi({
    username: requester.username,
    password: requester.password,
  })

  try {
    const approved = await createDraft(asRequester, documentType, `Approved ${suffix}`)
    const rejected = await createDraft(asRequester, documentType, `Rejected ${suffix}`)
    const waiting = await createDraft(asRequester, documentType, `Still waiting ${suffix}`)
    await createDraft(asRequester, documentType, `Never sent ${suffix}`)

    // The slow one first, so its clock starts before anything else is done.
    await submitDocument(asRequester, rejected)
    const rejectedSubmittedAt = Date.now()

    for (const id of [approved, waiting]) {
      await submitDocument(asRequester, id)
    }

    await decideDocumentTask(approver, approved, 'APPROVE')

    const remaining = rejectedSubmittedAt + SLOW_DECISION_MS - Date.now()
    await new Promise((resolve) => setTimeout(resolve, Math.max(0, remaining)))
    await decideDocumentTask(approver, rejected, 'REJECT')
  } finally {
    await asRequester.context.dispose()
  }

  // Somebody else's decided document: the administrator's, of the same type in
  // the same tenant.
  const theirs = await createDraft(session, documentType, `Not the requester's ${suffix}`)
  await submitDocument(session, theirs)
  await decideDocumentTask(approver, theirs, 'APPROVE')
})

test.afterAll(async () => {
  await session?.context.dispose()
})

test('the dashboard says how long the requester’s documents took to be decided', async ({
  page,
}) => {
  // --- The requester signs in and lands on the dashboard ---------------------
  await page.goto('/login')
  await page.getByLabel('Username or email').fill(requester.username)
  await page.getByLabel('Password').fill(requester.password)
  await page.getByRole('button', { name: 'Sign in' }).click()

  await expect(page).toHaveURL(/\/$/)

  const card = page.getByTestId('approval-time')

  await expect(card.getByRole('heading', { name: 'How long approval takes' })).toBeVisible()
  await expect(card.getByTestId('approval-time-scope')).toContainText('decided in the last 90 days')

  // --- Two decided: the approval and the rejection, not the waiting one ------
  //
  // Asserted before the absence check below, so that check runs against a
  // card that has rendered its numbers rather than passing at once.
  await expect(card.getByTestId('approval-time-documents').locator('dd')).toHaveText('2')
  await expect(card.getByTestId('approval-time-empty')).toHaveCount(0)

  // --- The two times, exactly: a few seconds and about 75 --------------------
  await expect(card.getByTestId('approval-time-median').locator('dd')).toHaveText('under a minute')
  await expect(card.getByTestId('approval-time-slowest').locator('dd')).toHaveText('1 min')
})
