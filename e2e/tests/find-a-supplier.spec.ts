import { expect, test } from '@playwright/test'

import { createSupplier, runSuffix, signInOverApi, type ApiSession } from '../support/api'
import { credentials } from '../support/env'

/**
 * Sign in, reach a master-data list, filter it (issue #153, acceptance
 * criterion 2).
 *
 * **One flow, driven the way a person drives it.** The point of this file is
 * not coverage — it is that the capability exists and is exercised by something
 * real. #101 shipped the master-data list and was verified by reading the
 * source, because nothing in this repository could open it; that is the gap
 * decision **D-14** closed, and a harness with no flow through it would have
 * left the gap exactly where it was.
 *
 * **Why the supplier list and not the party list.** The party view takes no
 * filters at all (`features/master-data/views.ts`), so "filter it" cannot be
 * demonstrated there. The supplier view is the nearest list that does, and it
 * is also the one that exercises the most of the stack: a row on it is a party
 * summary joined to a role and a profile, and getting there needs the tabs, the
 * permission check behind them, and the query-string state the page keeps.
 */

const suffix = runSuffix()

/**
 * Two suppliers, alike in everything the screen shows except the word the
 * filter will match. One row and no others is the assertion; a single seeded
 * row would pass against a filter that does nothing at all.
 */
const wanted = {
  code: `E2E-KEPT-${suffix}`,
  name: `Kepler Instruments ${suffix}`,
  supplierNumber: `SUP-K-${suffix}`,
}
const other = {
  code: `E2E-HIDDEN-${suffix}`,
  name: `Vega Logistics ${suffix}`,
  supplierNumber: `SUP-V-${suffix}`,
}

let session: ApiSession

test.beforeAll(async () => {
  session = await signInOverApi()

  await createSupplier(session, wanted)
  await createSupplier(session, other)
})

test.afterAll(async () => {
  await session?.context.dispose()
})

test('a signed-in administrator finds one supplier among many', async ({ page }) => {
  const { username, password } = credentials()

  // --- Sign in -------------------------------------------------------------
  await page.goto('/login')

  await page.getByLabel('Username or email').fill(username)
  await page.getByLabel('Password').fill(password)
  await page.getByRole('button', { name: 'Sign in' }).click()

  // The shell, not the form: the router replaces the login route on success,
  // so a still-visible form is a failed sign-in however the page looks.
  await expect(page).toHaveURL(/\/$/)
  await expect(page.getByRole('navigation', { name: 'Main navigation' })).toBeVisible()

  // --- Reach the list ------------------------------------------------------
  //
  // Through the navigation rather than by `goto`, because the link is part of
  // what #101 delivered and a direct URL would not exercise it.
  await page.getByRole('link', { name: 'Master Data' }).click()
  await expect(page).toHaveURL(/\/master-data/)

  await page.getByRole('button', { name: 'Suppliers' }).click()
  await expect(page).toHaveURL(/\/master-data\/suppliers/)

  // Both seeded rows are here before anything is filtered. Asserting this
  // first is what makes the assertion after the filter mean something.
  const table = page.getByRole('table')
  await expect(table.getByRole('row', { name: new RegExp(wanted.code) })).toBeVisible()
  await expect(table.getByRole('row', { name: new RegExp(other.code) })).toBeVisible()

  // --- Filter it -----------------------------------------------------------
  //
  // `fill` then `blur`: the search box commits on `change`, which is the event
  // a person produces by leaving the field, not by typing into it.
  const search = page.getByLabel('Search')
  await search.fill(wanted.name)
  await search.blur()

  // The URL carries the filter (#101 AC3) — a filtered list can be linked to.
  //
  // Read through `searchParams` rather than matched as a pattern: the router
  // encodes a space as `+` and `encodeURIComponent` writes `%20`, so a regex
  // over the raw URL asserts an encoding rather than a value.
  await expect(page).toHaveURL(/[?&]search=/)
  expect(new URL(page.url()).searchParams.get('search')).toBe(wanted.name)

  const row = table.getByRole('row', { name: new RegExp(wanted.code) })
  await expect(row).toBeVisible()
  await expect(row).toContainText(wanted.name)
  await expect(table.getByRole('row', { name: new RegExp(other.code) })).toHaveCount(0)
})
