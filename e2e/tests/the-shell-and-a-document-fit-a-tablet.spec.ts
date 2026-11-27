import { expect, test, type Locator, type Page } from '@playwright/test'

import { runSuffix, signInOverApi, type ApiSession } from '../support/api'
import { createDocumentType, createDraft, submitDocument } from '../support/documents'
import { credentials } from '../support/env'
import { publishForm } from '../support/forms'
import { seedDocument, seedList, type SeededList } from '../support/lists'
import {
  addApprover,
  bindWorkflow,
  createApproverRole,
  publishSingleStepWorkflow,
  type SeededApprover,
} from '../support/workflow'

/**
 * The shell, a list, a document and the task that decides it fit a tablet
 * (#961, NFR-USE-001, NFR-USE-003, NFR-USE-004; plan 22 row 16).
 *
 * **768 by 1024, in Chromium.** The viewport is set here and nowhere else: the
 * `chromium` project is the one every flow runs in, and `devices['iPad']` would
 * switch the engine to WebKit (#961 AC1).
 *
 * # What "fits" means here
 *
 * The page never scrolls sideways: `scrollWidth - clientWidth` on the document
 * element is 0 (AC2). A table may scroll inside its own container, so nothing
 * narrower than the page is measured for that. On the document and task pages
 * the controls a person came to press end inside the viewport's width, with
 * the sidebar open and with it closed, and the breadcrumb trail is on screen
 * (AC6) everywhere but the dashboard, which has none by design (#968).
 *
 * # Two documents, so one defect is reported once
 *
 * The AC2 and AC5 tests open a document with an ordinary title. The AC3 test
 * opens one whose title is a single unbroken token of more than 160
 * characters, and repeats the AC2 assertions for it. So #1026, the document
 * header's `<h1>` that did not wrap, turned the AC3 test red and left the
 * layout tests saying what they are about; the fix landed with this spec.
 *
 * # What is seeded over the API
 *
 * The form, the list, the document types, the workflow, the approver, and the
 * two submits. Raising and submitting through the browser is
 * `a-document-is-approved.spec.ts`'s subject.
 */
test.use({ viewport: { width: 768, height: 1024 } })

const WIDTH = 768
const suffix = runSuffix()

const roleCode = `E2E-TABLET-${suffix}`.toUpperCase()
const taskName = 'Approve on a tablet'
const shortTitle = `Ten ergonomic chairs ${suffix}`
const longTitle = `PR-${'X'.repeat(160)}-${suffix}`

/** One optional field: the drafts are submitted with no form data. */
const definition = {
  formId: `e2e-tablet-${suffix}`,
  version: '2.0.1',
  title: 'Tablet request',
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

const TABS = ['form', 'history', 'workflow', 'attachments', 'comments', 'activity'] as const

/**
 * The panel each tab opens. History's panel predates the `panel-*` naming and
 * two other flows read it as `document-history`, so it keeps that id.
 */
const PANELS: Record<(typeof TABS)[number], string> = {
  form: 'panel-form',
  history: 'document-history',
  workflow: 'panel-workflow',
  attachments: 'panel-attachments',
  comments: 'panel-comments',
  activity: 'panel-activity',
}
const SIDEBAR = ['open', 'closed'] as const

let session: ApiSession
let approver: SeededApprover
let list: SeededList
let listTitle: string
let shortDocumentId: string
let longDocumentId: string

test.beforeAll(async () => {
  session = await signInOverApi()

  // A rendered list with a row in it, and a second row whose title is the
  // long one: a table may scroll inside its own container, the page may not.
  listTitle = `Tablet requisitions ${suffix}`
  list = await seedList(session, {
    title: listTitle,
    columns: [{ columnKey: 'title', label: 'Title' }],
  })
  await seedDocument(session, list, shortTitle)
  await seedDocument(session, list, longTitle)

  // The documents that are decided: a type that carries a one-step workflow
  // assigned to a role this run owns, and somebody who holds it.
  const form = await publishForm(session, definition, `Tablet request ${suffix}`)
  const documentType = await createDocumentType(session, form, `Tablet request ${suffix}`)

  const roleId = await createApproverRole(session, roleCode)
  approver = await addApprover(session, roleId, 'tablet')

  const workflow = await publishSingleStepWorkflow(session, roleCode, { taskName })
  await bindWorkflow(session, documentType.id, workflow)

  shortDocumentId = await createDraft(session, documentType, shortTitle)
  longDocumentId = await createDraft(session, documentType, longTitle)
  await submitDocument(session, shortDocumentId)
  await submitDocument(session, longDocumentId)
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

  // `browser.newPage()` takes the spec's `test.use` options too; checked, so a
  // page that silently opened at the desktop default cannot pass.
  expect(page.viewportSize()).toEqual({ width: WIDTH, height: 1024 })
}

async function signInAsAdministrator(page: Page): Promise<void> {
  const { username, password } = credentials()
  await signIn(page, username, password)
}

/** How far the page itself scrolls sideways. 0 is the only passing value. */
function pageOverflow(page: Page): Promise<number> {
  return page.evaluate(
    () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
  )
}

/** Where a control's box ends, horizontally, in CSS pixels. */
async function rightEdge(locator: Locator): Promise<number> {
  const box = await locator.boundingBox()
  expect(box, 'the control has a box').not.toBeNull()
  return box!.x + box!.width
}

/** Opens or closes the sidebar, whichever it is not, and checks it took. */
async function setSidebar(page: Page, state: (typeof SIDEBAR)[number]): Promise<void> {
  const sidebar = page.getByRole('navigation', { name: 'Main navigation' })

  if ((await sidebar.isVisible()) !== (state === 'open')) {
    await page.getByRole('button', { name: 'Toggle navigation' }).click()
  }

  if (state === 'open') {
    await expect(sidebar).toBeVisible()
  } else {
    await expect(sidebar).toBeHidden()
  }
}

/**
 * The AC2 and AC6 assertions every screen shares.
 *
 * **The dashboard is the root and has no trail** (#968 AC4, `breadcrumbs.ts`),
 * so there the trail's absence is asserted instead: AC6's "the AC2 screens"
 * included `/` in error, as the product owner ruled on 2026-11-26.
 */
async function expectFits(
  page: Page,
  where: string,
  trail: 'visible' | 'absent' = 'visible',
): Promise<void> {
  const breadcrumb = page.getByRole('navigation', { name: 'Breadcrumb' })

  if (trail === 'visible') {
    await expect(breadcrumb, `the trail is visible on ${where}`).toBeVisible()
  } else {
    await expect(breadcrumb, `there is no trail on ${where}`).toHaveCount(0)
  }

  expect(await pageOverflow(page), `${where} does not scroll sideways`).toBe(0)
}

/** The document page's primary controls: its six section tabs. */
async function expectDocumentControlsInside(page: Page, where: string): Promise<void> {
  for (const key of TABS) {
    const tab = page.getByTestId(`tab-${key}`)

    await expect(tab, `tab-${key} is visible on ${where}`).toBeVisible()
    expect(
      await rightEdge(tab),
      `tab-${key} ends inside ${WIDTH}px on ${where}`,
    ).toBeLessThanOrEqual(WIDTH)
  }
}

/** The task page's primary controls: the decisions, and the reason above them. */
async function expectDecisionsInside(page: Page, where: string): Promise<void> {
  for (const testId of ['decide-APPROVE', 'decide-REJECT']) {
    const button = page.getByTestId(testId)

    await expect(button, `${testId} is visible on ${where}`).toBeVisible()
    expect(
      await rightEdge(button),
      `${testId} ends inside ${WIDTH}px on ${where}`,
    ).toBeLessThanOrEqual(WIDTH)
  }

  const reason = page.getByLabel(/Reason/)
  await expect(reason, `the reason box is visible on ${where}`).toBeVisible()
  expect(
    await rightEdge(reason),
    `the reason box ends inside ${WIDTH}px on ${where}`,
  ).toBeLessThanOrEqual(WIDTH)
}

/** Opens the task on the document titled `title` from the approver's inbox. */
async function openTask(page: Page, title: string): Promise<void> {
  await page.goto('/tasks')

  const row = page.getByRole('row').filter({ hasText: title })
  await expect(row).toHaveCount(1)
  await row.getByRole('button', { name: 'Open' }).click()

  await expect(page).toHaveURL(/\/tasks\/[0-9a-f-]{36}$/)
  await expect(page.getByTestId('task-name')).toHaveText(taskName)
  await expect(page.getByTestId('task-document')).toContainText(title)
}

test('the dashboard and a rendered list do not scroll sideways at 768 px, sidebar open or closed', async ({
  page,
}) => {
  await signInAsAdministrator(page)

  // --- The dashboard ---------------------------------------------------------
  await page.goto('/')
  // The long title must be on the dashboard, or its overflow goes unguarded.
  await expect(
    page.getByTestId('recent-document').filter({ hasText: longTitle }).first(),
    'the long-titled document is in What you touched last',
  ).toBeVisible()
  for (const sidebar of SIDEBAR) {
    await setSidebar(page, sidebar)
    await expectFits(page, `the dashboard with the sidebar ${sidebar}`, 'absent')
  }

  // --- A rendered list with a row in it ---------------------------------------
  await page.goto(`/lists/${list.listKey}`)
  await expect(page.getByRole('row').filter({ hasText: shortTitle })).toHaveCount(1)

  for (const sidebar of SIDEBAR) {
    await setSidebar(page, sidebar)
    await expectFits(page, `the list with the sidebar ${sidebar}`)
  }
})

test('the document page and its task fit 768 px with the sidebar open and closed', async ({
  browser,
}) => {
  const requester = await browser.newPage()
  const decider = await browser.newPage()

  try {
    // --- The document page ---------------------------------------------------
    await signInAsAdministrator(requester)
    await requester.goto(`/documents/${shortDocumentId}`)
    await expect(requester.getByTestId('document-title')).toHaveText(shortTitle)
    await expect(requester.getByTestId('document-status')).toHaveText('Pending approval')

    for (const sidebar of SIDEBAR) {
      await setSidebar(requester, sidebar)
      const where = `the document page with the sidebar ${sidebar}`

      await expectFits(requester, where)
      await expectDocumentControlsInside(requester, where)
    }

    // --- The task page where the approval is decided (AC7) -------------------
    await signIn(decider, approver.username, approver.password)
    await openTask(decider, shortTitle)

    for (const sidebar of SIDEBAR) {
      await setSidebar(decider, sidebar)
      const where = `the task page with the sidebar ${sidebar}`

      await expectFits(decider, where)
      await expectDecisionsInside(decider, where)
    }
  } finally {
    await requester.close()
    await decider.close()
  }
})

test('every document tab is visible at 768 px and opens its own panel', async ({ page }) => {
  // AC5, NFR-USE-003. The content of each tab is the Vue specs' subject
  // (`DocumentWorkspace.spec.ts`, `AttachmentsTab.spec.ts`, `CommentsTab.spec.ts`,
  // `ActivityTab.spec.ts`); what this adds is that all six are reachable on a
  // tablet and that pressing one shows the panel it names.
  await signInAsAdministrator(page)
  await page.goto(`/documents/${shortDocumentId}`)
  await expect(page.getByTestId('document-title')).toHaveText(shortTitle)

  for (const key of TABS) {
    await expect(page.getByTestId(`tab-${key}`), `tab-${key} is visible`).toBeVisible()
  }

  for (const key of TABS) {
    await page.getByTestId(`tab-${key}`).click()

    await expect(page.getByTestId(`tab-${key}`)).toHaveAttribute('aria-current', 'page')
    await expect(page.getByTestId(PANELS[key]), `tab-${key} opens ${PANELS[key]}`).toBeVisible()

    for (const other of TABS.filter((candidate) => candidate !== key)) {
      await expect(
        page.getByTestId(PANELS[other]),
        `${PANELS[other]} is hidden behind ${key}`,
      ).toBeHidden()
    }
  }
})

test('a 160-character unbroken title fits 768 px on the document page and its task', async ({
  browser,
}) => {
  // AC3, and #1026: the document header's `<h1>` now wraps. The AC2
  // assertions again, for the long title, and the title's own box.
  const requester = await browser.newPage()
  const decider = await browser.newPage()

  try {
    expect(longTitle.length).toBeGreaterThanOrEqual(160)

    await signInAsAdministrator(requester)
    await requester.goto(`/documents/${longDocumentId}`)

    const heading = requester.getByTestId('document-title')
    await expect(heading).toHaveText(longTitle)

    for (const sidebar of SIDEBAR) {
      await setSidebar(requester, sidebar)
      const where = `the long-titled document with the sidebar ${sidebar}`

      expect(
        await rightEdge(heading),
        `the title ends inside ${WIDTH}px on ${where}`,
      ).toBeLessThanOrEqual(WIDTH)
      await expectFits(requester, where)
      await expectDocumentControlsInside(requester, where)
    }

    await signIn(decider, approver.username, approver.password)
    await openTask(decider, longTitle)

    for (const sidebar of SIDEBAR) {
      await setSidebar(decider, sidebar)
      const where = `the long-titled task with the sidebar ${sidebar}`

      await expectFits(decider, where)
      await expectDecisionsInside(decider, where)
    }
  } finally {
    await requester.close()
    await decider.close()
  }
})
