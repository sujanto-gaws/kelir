import { expect, test, type Page } from '@playwright/test'

import { signInOverApi, runSuffix, type ApiSession } from '../support/api'
import { credentials } from '../support/env'
import { seedDocument, seedList, type SeededList } from '../support/lists'

/**
 * Two more record pages publish their name to the trail (#968 AC5, AC8):
 * a rendered list, matched by its key, and a document, matched by its id.
 *
 * Added by `test-engineer` in the row 27 campaign: the builder's browser test
 * covers only the form builder's wiring, so a page publishing under the wrong
 * key, or not publishing at all, went unnoticed on every other record page.
 * The document's title is one unbreakable token, so the current crumb has to
 * truncate rather than wrap.
 */
let session: ApiSession
let list: SeededList
let listTitle: string
let documentId: string
let documentTitle: string

test.beforeAll(async () => {
  session = await signInOverApi()
  listTitle = `Open purchase requisitions ${runSuffix()}`
  list = await seedList(session, {
    title: listTitle,
    columns: [{ columnKey: 'title', label: 'Title' }],
  })
  documentTitle = `PR-${'X'.repeat(160)}-${runSuffix()}`
  documentId = await seedDocument(session, list, documentTitle)
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

test('a rendered list and a document each show their own name, never the key or id', async ({
  page,
}) => {
  await signIn(page)
  const trail = page.getByRole('navigation', { name: 'Breadcrumb' })

  await page.goto(`/lists/${list.listKey}`)
  await expect(trail.getByRole('listitem')).toHaveText(['Dashboard', listTitle])
  await expect(trail).not.toContainText(list.listKey)

  await page.goto(`/documents/${documentId}`)
  await expect(trail.getByRole('listitem')).toHaveText(['Dashboard', 'Documents', documentTitle])
  await expect(trail).not.toContainText(documentId)
  await expect(trail.getByRole('link')).toHaveText(['Dashboard', 'Documents'])

  // The URL has a segment the hierarchy does not (#968 AC2): `:view` is a tab,
  // not a level, so a trail built by splitting the path would repeat the page.
  await page.goto('/master-data/suppliers')
  await expect(trail.getByRole('listitem')).toHaveText(['Dashboard', 'Master data'])
})

test('at tablet width an unbreakable name is truncated, not wrapped or overflowing', async ({
  page,
}) => {
  await page.setViewportSize({ width: 768, height: 1024 })
  await signIn(page)
  await page.goto(`/documents/${documentId}`)

  const current = page
    .getByRole('navigation', { name: 'Breadcrumb' })
    .locator('[aria-current="page"]')

  await expect(current).toHaveText(documentTitle)
  await expect(current).toHaveAttribute('title', documentTitle)

  // The trail's own box, not the page's: the document header's title is a
  // separate element and wraps or overflows on its own terms (out of #968).
  const box = await current.evaluate((span) => {
    const nav = span.closest('nav') as HTMLElement
    return {
      clipped: span.scrollWidth > span.clientWidth,
      ellipsis: getComputedStyle(span).textOverflow,
      trail: nav.scrollWidth - nav.clientWidth,
      inside: span.getBoundingClientRect().right <= nav.getBoundingClientRect().right + 0.5,
    }
  })

  expect(box).toEqual({ clipped: true, ellipsis: 'ellipsis', trail: 0, inside: true })
})
