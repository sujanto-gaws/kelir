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
 * # What is not asserted
 *
 * **The durations' values.** Every decision here lands within seconds of its
 * submission, so the median and the slowest are *under a minute* on a quick run
 * and a few minutes on a slow one; asserting a number would fail on the
 * runner's speed rather than on the card. The flow asserts that both are shown
 * as a duration, and the unit specs own the formatting.
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

test.beforeAll(async () => {
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

    for (const id of [approved, rejected, waiting]) {
      await submitDocument(asRequester, id)
    }

    await decideDocumentTask(approver, approved, 'APPROVE')
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

  // --- Both times are shown as durations ------------------------------------
  const duration = /^(under a minute|\d+ min|\d+ h( \d+ min)?)$/

  await expect(card.getByTestId('approval-time-median').locator('dd')).toHaveText(duration)
  await expect(card.getByTestId('approval-time-slowest').locator('dd')).toHaveText(duration)
})
