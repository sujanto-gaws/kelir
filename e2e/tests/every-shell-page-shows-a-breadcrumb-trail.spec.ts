import { expect, test, type Page } from '@playwright/test'

import { signInOverApi, runSuffix, type ApiSession } from '../support/api'
import { credentials } from '../support/env'
import { publishForm, type SeededForm } from '../support/forms'

/**
 * A deep page says where it sits, by name, and a crumb goes back up (#968,
 * NFR-USE-002).
 *
 * **The form is arranged over the API and everything is asserted in the
 * browser.** Its title is long on purpose: at tablet width the last crumb has
 * to truncate rather than push the page sideways, and keep the whole title
 * reachable.
 */
let session: ApiSession
let form: SeededForm

/** One text field: the trail is the subject, not the form. */
const definition = {
  formId: 'breadcrumbs',
  version: '2.0.1',
  title: 'Breadcrumbs',
  components: [
    {
      id: 'note-field',
      role: 'data',
      type: 'textfield',
      key: 'note',
      label: 'Note',
      validation: { type: 'string' },
    },
  ],
}

test.beforeAll(async () => {
  session = await signInOverApi()
  form = await publishForm(
    session,
    definition,
    `Capital expenditure request for the Surabaya warehouse refit ${runSuffix()}`,
  )
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

test('a form in the builder shows Administration › Forms › its title, and Forms goes back', async ({
  page,
}) => {
  await signIn(page)

  // --- Opened directly, as a bookmark or a reload would -----------------------
  await page.goto(`/admin/forms/${form.id}`)

  const trail = page.getByRole('navigation', { name: 'Breadcrumb' })
  const items = trail.getByRole('listitem')

  await expect(items).toHaveText(['Dashboard', 'Administration', 'Forms', form.title])
  await expect(trail).not.toContainText(form.id)

  // The current page is marked and not a link; Administration is a heading.
  await expect(trail.locator('[aria-current="page"]')).toHaveText(form.title)
  await expect(trail.getByRole('link')).toHaveText(['Dashboard', 'Forms'])

  // The sidebar's landmark is still there beside it.
  await expect(page.getByRole('navigation', { name: 'Main navigation' })).toBeVisible()

  // --- Back up by a crumb ---------------------------------------------------
  await trail.getByRole('link', { name: 'Forms' }).click()

  await expect(page).toHaveURL(/\/admin\/forms$/)
  await expect(items).toHaveText(['Dashboard', 'Administration', 'Forms'])
  await expect(page.getByTestId('forms')).toBeVisible()
})

test('at tablet width the trail truncates a long title and the page does not scroll sideways', async ({
  page,
}) => {
  await page.setViewportSize({ width: 768, height: 1024 })
  await signIn(page)
  await page.goto(`/admin/forms/${form.id}`)

  const trail = page.getByRole('navigation', { name: 'Breadcrumb' })
  const current = trail.locator('[aria-current="page"]')

  await expect(current).toHaveText(form.title)

  // The full title is still there for whoever cannot see all of it.
  await expect(current).toHaveAttribute('title', form.title)

  for (const sidebar of ['open', 'closed']) {
    if (sidebar === 'closed') {
      await page.getByRole('button', { name: 'Toggle navigation' }).click()
      await expect(page.getByRole('navigation', { name: 'Main navigation' })).toBeHidden()
    }

    await expect(trail, `trail visible with the sidebar ${sidebar}`).toBeVisible()

    const overflow = await trail.evaluate((nav) => ({
      trail: nav.scrollWidth - nav.clientWidth,
      page: document.documentElement.scrollWidth - document.documentElement.clientWidth,
    }))

    expect(overflow, `no overflow with the sidebar ${sidebar}`).toEqual({ trail: 0, page: 0 })
  }
})
