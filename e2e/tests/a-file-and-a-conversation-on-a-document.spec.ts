import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

import { expect, test, type Page } from '@playwright/test'

import { runSuffix, signInOverApi, type ApiSession } from '../support/api'
import { createDocumentType, createDraft, type SeededDocumentType } from '../support/documents'
import { credentials } from '../support/env'
import { publishForm, type SeededForm } from '../support/forms'

/**
 * **SRS §9 criteria 6 and 11, in a browser** — *users can upload attachments*
 * and *comments can be added* ([Phase 6 MVP Close Plan](../../projects/planning/09.%20Phase%206%20MVP%20Close%20Plan.md) §3).
 *
 * # Why this exists, and why it exists before the MVP verification
 *
 * The [Sprint 13 construction plan](../../projects/planning/08.%20Sprint%2013%20MVP%20Construction%20Plan.md)
 * §1.2 established that **a criterion saying *users can* is not met by an
 * endpoint**, and bought two screens with three `Should` rows to fix it. Those
 * screens shipped. What did not ship is anybody driving them: `e2e/tests/`
 * asserted that the two panels render their *empty states* and nothing more, so
 * the evidence for both criteria was 545 jsdom component tests.
 *
 * A component test is not a user either. This is the same argument one level
 * down, and this file is the answer to it.
 *
 * # It is also the only assertion in this suite that fails on an unwired store
 *
 * Sprint 12's exit demo found three defects in ten minutes, two of which made
 * attachments non-functional in any real deployment while eleven hundred tests
 * stayed green — the compose files are the artefact no test reads. `minio-init`
 * now ends with `mc ls kelir/kelir` and exits 1 without the bucket, and
 * **nothing observes that exit code**: `backend` declares no
 * `service_completed_successfully` dependency and `deploy.sh` smoke-tests only
 * `/health/ready` and `/version`. A stack whose bucket step failed would still
 * deploy green.
 *
 * The upload below is what turns that into a red pipeline.
 *
 * # The scan gate is the part that needed designing
 *
 * Upload returns immediately with `virus_scan_status = PENDING`; a worker clears
 * it afterwards. So the flow **polls until the download control appears** rather
 * than waiting a fixed time. A fixed wait would be flaky in the direction that
 * reads as a product defect, and the gate not opening is exactly
 * [#246](https://github.com/sujanto-gaws/kelir/issues/246)'s failure mode — the
 * thing worth seeing, not papering over.
 *
 * # A real PDF, because the type is decided by content
 *
 * `%PDF-1.7` is the magic `infer` reads. Naming a text file `.pdf` is precisely
 * the mismatch `type_is_allowed` refuses, which `attachment_upload.rs` already
 * tests — so the bytes here are the same shape that file uses.
 */
const definition = JSON.parse(
  readFileSync(
    fileURLToPath(
      new URL(
        '../../kelir-frontend/src/features/rad/__fixtures__/purchase-requisition.json',
        import.meta.url,
      ),
    ),
    'utf8',
  ),
) as Record<string, unknown>

const suffix = runSuffix()

/** The smallest thing `infer` calls a PDF. */
const PDF = Buffer.from('%PDF-1.7\n1 0 obj\n<< /Type /Catalog >>\nendobj\ntrailer\n<<>>\n%%EOF\n')

const FILE_NAME = `quotation-${suffix}.pdf`
const COMMENT = `Checked the quotation against the budget line ${suffix}`

/**
 * How long the scan may take before this is a defect rather than a wait.
 *
 * The measurement behind **D-4** is ~169 ms for 25 MiB against a scanner that is
 * already up, and the compose stack's `clamav` reports healthy roughly twenty
 * seconds after start. This file uploads a hundred bytes. Sixty seconds is
 * therefore generous by two orders of magnitude and still bounded — if it
 * expires, something is wrong with the gate and the run should say so.
 */
const SCAN_TIMEOUT = 60_000

let session: ApiSession
let form: SeededForm
let documentType: SeededDocumentType
let document: string

test.beforeAll(async () => {
  session = await signInOverApi()

  // The administrator's half over the API, as every flow in this directory
  // does: this one is about the file and the conversation, not the type.
  form = await publishForm(session, definition, `Purchase requisition (files ${suffix})`)
  documentType = await createDocumentType(session, form, `Purchase requisition ${suffix}`)
  document = await createDraft(session, documentType, `Quotation review ${suffix}`)
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

test('a file is attached, cleared and downloaded, and a comment is added', async ({ page }) => {
  await signIn(page)
  await page.goto(`/documents/${document}`)
  await expect(page.getByTestId('document-title')).toHaveText(`Quotation review ${suffix}`)

  // --- The tab starts empty, which is the state the old test asserted --------
  await page.getByTestId('tab-attachments').click()
  await expect(page.getByTestId('attachments-empty')).toContainText(
    'Nothing is attached to this document yet',
  )

  // --- Upload (SRS §9 criterion 6) ------------------------------------------
  //
  // The input uploads on `change`, so placing the file is the whole action.
  await page.getByTestId('attachment-input').setInputFiles({
    name: FILE_NAME,
    mimeType: 'application/pdf',
    buffer: PDF,
  })

  const row = page.getByTestId('attachment-row').first()
  await expect(row).toContainText(FILE_NAME)

  // --- The gate is shut, and says which of three refusals this is -----------
  //
  // #246 AC3 and #295's second criterion: PENDING, INFECTED and FAILED are
  // three refusals rather than three stages of one, and a screen rendering one
  // spinner for all three turns a security control into a bug report. This
  // asserts the *waiting* one specifically — that the file is not downloadable
  // and the reason given is that it is being checked.
  //
  // It is a race against the scanner by construction: a fast enough clear would
  // make this assertion flaky. `toPass` is therefore not used here — the badge
  // is read once, immediately, and either state is accepted, while the
  // *download control* is what carries the real assertion below.
  await expect(row.getByTestId('attachment-status')).toHaveText(/Checking|Ready/)

  // --- The gate opens, and only then -----------------------------------------
  await expect(row.getByTestId('attachment-status')).toHaveText('Ready', {
    timeout: SCAN_TIMEOUT,
  })
  await expect(row.getByTestId('attachment-download')).toBeVisible()

  // --- The bytes come back ---------------------------------------------------
  //
  // The screen fetches a blob through the same client every other call uses,
  // because the route is behind a bearer token a browser navigation cannot
  // carry, then saves it through an anchor. Asserting the download event is
  // what distinguishes *the button appeared* from *the file arrived*.
  const [download] = await Promise.all([
    page.waitForEvent('download'),
    row.getByTestId('attachment-download').click(),
  ])
  expect(download.suggestedFilename()).toBe(FILE_NAME)

  // --- Comment (SRS §9 criterion 11) ----------------------------------------
  await page.getByTestId('tab-comments').click()
  await expect(page.getByTestId('comments-empty')).toContainText(
    'Nobody has commented on this document yet',
  )

  await page.getByTestId('comment-input').fill(COMMENT)
  await page.getByTestId('comment-submit').click()

  await expect(page.getByTestId('comment-body').first()).toHaveText(COMMENT)

  // **Not the decision comment**, and this is the one place a person sees the
  // difference. The note under the composer is what says so.
  await expect(page.getByTestId('comment-distinction')).toBeVisible()

  // --- Both reach the timeline (the seam between three modules) -------------
  //
  // Sprint 12's findings lived in this seam, and #292 was found here: the
  // timeline used to carry the file's name to a caller holding no
  // `attachment:read`. It carries the link and not the subject now (**D-45**),
  // so what is asserted is the server's own sentence and the event type —
  // never the file name, which would be asserting the defect back into place.
  await page.getByTestId('tab-activity').click()

  const timeline = page.getByTestId('activity-list')
  await expect(timeline).toContainText('Attached a file')
  await expect(timeline).toContainText('Downloaded a file')
  await expect(timeline).toContainText('Commented on the document')

  await expect(timeline).not.toContainText(FILE_NAME)
})
