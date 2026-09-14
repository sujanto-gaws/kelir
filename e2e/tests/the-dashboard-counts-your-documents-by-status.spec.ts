import { expect, test } from '@playwright/test'

import { runSuffix, signInOverApi, type ApiSession } from '../support/api'
import { createDocumentType, createDraft, submitDocument } from '../support/documents'
import { publishForm } from '../support/forms'
import {
  addApprover,
  bindWorkflow,
  createApproverRole,
  publishSingleStepWorkflow,
  type SeededApprover,
} from '../support/workflow'

/**
 * The dashboard counts your documents by status, and draws them (FR-RPT-004,
 * #447 AC6).
 *
 * The status card is the first surface in the tree with a chart, and the chart
 * is the part no unit test can see: jsdom lays out nothing, and whether the
 * chunk holding Unovis arrives through the release proxy is a question about
 * the bundle a release ships. This flow reaches the fifth widget through that
 * stack, as a person who has raised documents.
 *
 * # Two statuses, and a count that is not one
 *
 * A card that rendered the same number beside every status, or the right
 * numbers in the wrong rows, would pass a flow holding one document. So the
 * requester raises **three**: two left as drafts and one submitted, which the
 * single-step workflow maps to `PENDING_APPROVAL`. The card has to say `2`
 * beside *Draft*, `1` beside *Pending approval*, and `0` beside the other eight,
 * in the lifecycle order the server sends.
 *
 * # Somebody else's draft, in the same tenant and of the same type
 *
 * The administrator raises one too. The summary counts what **the caller**
 * raised, so it must not move *Draft* to `3`. Same tenant and same type is the
 * case a dropped `created_by` would expose, and it costs one call.
 *
 * # A requester of this run's own
 *
 * The deployment keeps its database between runs (README, first rule). A user
 * created for this run has raised exactly this run's documents, so the flow can
 * assert all ten counts rather than only that two are somewhere on the card.
 *
 * # What is seeded over the API
 *
 * The form, the type, the workflow, the people, the drafts **and the submit**,
 * for the reason `the-dashboard-says-what-is-late.spec.ts` gives: raising and
 * submitting through the browser is `a-document-is-created-and-submitted.spec.ts`'s
 * subject, and driving it again here would fail this flow for reasons that one
 * already reports.
 */
const suffix = runSuffix()

const roleCode = `E2E-STATUS-${suffix}`.toUpperCase()

/**
 * The ten statuses as the card labels them, in the lifecycle order the server
 * sends. **Written out rather than imported**, so a page that re-sorted the rows
 * or relabelled one fails here instead of moving the expectation with it.
 */
const STATUS_LABELS = [
  'Draft',
  'Submitted',
  'In review',
  'Pending approval',
  'Approved',
  'Rejected',
  'Returned',
  'Completed',
  'Archived',
  'Cancelled',
]

/** What this run's requester raised: two drafts and one submission, nothing else. */
const EXPECTED_COUNTS = ['2', '0', '0', '1', '0', '0', '0', '0', '0', '0']

/**
 * One optional field, in the shape `the-dashboard-says-what-is-late.spec.ts`
 * publishes, and for its reason: the submit carries no form data, so a
 * `required` rule would refuse it for a reason this flow is not about.
 */
const definition = {
  formId: `e2e-status-${suffix}`,
  version: '2.0.1',
  title: 'Status request',
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

  const form = await publishForm(session, definition, `Status request ${suffix}`)
  const documentType = await createDocumentType(session, form, `Status request ${suffix}`)

  // `reporting:dashboard:read` is the permission the summary requires, and
  // `document:create` and `document:submit` are what raising and sending a
  // document require. The helper's base set carries `document:read`, which is
  // what makes the card's *Open documents* link render.
  const roleId = await createApproverRole(session, roleCode, [
    'reporting:dashboard:read',
    'document:create',
    'document:submit',
  ])
  requester = await addApprover(session, roleId, 'status')

  // **A second holder of the role**, so the submitted document's task resolves
  // to somebody other than its author whatever the assignment rule says about
  // the requester deciding their own work. The approval is not this flow's
  // subject; the submit succeeding is only its fixture.
  await addApprover(session, roleId, 'status-approver')

  await bindWorkflow(
    session,
    documentType.id,
    await publishSingleStepWorkflow(session, roleCode, { taskName: 'Approve the status request' }),
  )

  const asRequester = await signInOverApi({
    username: requester.username,
    password: requester.password,
  })

  try {
    await createDraft(asRequester, documentType, `Still a draft ${suffix}`)
    await createDraft(asRequester, documentType, `Also a draft ${suffix}`)
    await submitDocument(
      asRequester,
      await createDraft(asRequester, documentType, `Sent ${suffix}`),
    )
  } finally {
    await asRequester.context.dispose()
  }

  // Somebody else's draft: the administrator's, of the same type in the same
  // tenant. It is on no card this flow reads.
  await createDraft(session, documentType, `Not the requester's ${suffix}`)
})

test.afterAll(async () => {
  await session?.context.dispose()
})

test('the dashboard counts the requester’s documents by status and draws the chart', async ({
  page,
}) => {
  // --- The requester signs in and lands on the dashboard ---------------------
  await page.goto('/login')
  await page.getByLabel('Username or email').fill(requester.username)
  await page.getByLabel('Password').fill(requester.password)
  await page.getByRole('button', { name: 'Sign in' }).click()

  await expect(page).toHaveURL(/\/$/)

  const card = page.getByTestId('documents-by-status')

  await expect(card).toContainText('Your documents by status')

  // --- Every status, its count as text, in the server's order -----------------
  //
  // `toHaveText` with an array asserts the whole ordered list, so a missing
  // zero row, a re-sorted list or a count in the wrong row each fail here. The
  // administrator's draft is what keeps *Draft* at `2` rather than `3`.
  await expect(card.getByTestId('status-label')).toHaveText(STATUS_LABELS)
  await expect(card.getByTestId('status-value')).toHaveText(EXPECTED_COUNTS)
  await expect(card.getByTestId('status-empty')).toHaveCount(0)

  // The drafts tile is the same read's `DRAFT` entry on the server, so the two
  // numbers on one screen agree.
  await expect(page.getByTestId('draft-documents')).toContainText('2')

  // --- The chart's chunk arrived and the component mounted --------------------
  //
  // `status-chart` is the root of `DocumentStatusChart.vue`, which the page
  // reaches only through `defineAsyncComponent`: it exists in the document only
  // once the chunk carrying Unovis has loaded through the release proxy. What d3
  // drew inside it is not asserted — the counts above are the numbers.
  //
  // **Asserted after the counts, deliberately,** so the absence checks below
  // run against a card that has rendered rather than passing at once.
  await expect(card.getByTestId('status-chart')).toBeVisible()
  await expect(card.getByTestId('status-chart-error')).toHaveCount(0)
  await expect(card.getByTestId('status-chart-loading')).toHaveCount(0)
})
