import { expect, test } from '@playwright/test'

import { credentials } from '../support/env'

/**
 * Signing out, and the roles screen — the two surfaces [record 14][record] §6
 * finding 2 found running on evidence that could not reach them ([#358]).
 *
 * # Why these two, and why in a browser
 *
 * **SRS §9 criterion 1 is *users can log in and log out*.** Its login half is
 * driven by every spec in this directory, because none of them can do anything
 * without signing in first. Its **logout half was driven by nothing**: no flow
 * clicked sign-out, so nothing asserted that the session ends or that a
 * protected route refuses afterwards.
 *
 * **The roles screen** is one of the *administrators can…* surfaces, and it had
 * component tests and no flow, where criteria 2 and 3 both have one.
 *
 * [Construction plan 08][plan] §1.2's rule, applied three times now: *a
 * criterion that says **users can** is not met by an endpoint* — and, as
 * [status report 14][status] added, not by a component test either. **Logout is
 * the case that argument was made for.** What is interesting about it is what
 * happens to a real browser's storage and to the next navigation, which is
 * exactly the part a jsdom test replaces with a double.
 *
 * # What this asserts that a component test cannot
 *
 * **That the refusal survives the browser.** Signing out and then *navigating*
 * to a protected route is a fresh page load: the router re-runs its guard
 * against whatever the browser actually kept. A component test asserting the
 * store was cleared cannot see a token that outlived it in `localStorage`, and
 * that is the failure this is for.
 *
 * [#358]: https://github.com/sujanto-gaws/kelir/issues/358
 * [record]: ../../projects/verifications/14.%20MVP%20Verification.md
 * [plan]: ../../projects/planning/08.%20Sprint%2013%20MVP%20Construction%20Plan.md
 * [status]: ../../projects/status/14.%20Sprint%2013%20Status.md
 */

test('an administrator reads the roles screen, signs out, and cannot get back in', async ({
  page,
}) => {
  const { username, password } = credentials()

  // --- Sign in -------------------------------------------------------------
  await page.goto('/login')

  await page.getByLabel('Username or email').fill(username)
  await page.getByLabel('Password').fill(password)
  await page.getByRole('button', { name: 'Sign in' }).click()

  await expect(page).toHaveURL(/\/$/)
  await expect(page.getByRole('navigation', { name: 'Main navigation' })).toBeVisible()

  // --- The roles screen, reached the way a person reaches it ---------------
  //
  // Through the navigation rather than by `goto`, for `create-a-tenant`'s
  // stated reason: the entry is gated on `identity:role:read` and a direct URL
  // would not exercise that gate.
  await page.getByRole('link', { name: 'Roles' }).click()
  await expect(page).toHaveURL(/\/admin\/roles/)

  // **The seeded administrator role, with the permissions it grants.** Asserting
  // a row rather than the heading is what distinguishes a screen that loaded its
  // data from one that rendered its own furniture — the distinction record 14
  // drew about the attachment and comment panels, whose flows asserted only
  // that they render empty.
  const table = page.getByRole('table')
  await expect(table.getByRole('row', { name: /ROLE-ADMIN/ })).toBeVisible()

  // The `Permissions` column is the one thing this screen exists to show that a
  // list of names would not.
  // DELIBERATE FAILURE -- branch-protection probe for #335 AC3. This column is
  // called "Permissions"; the name below is nonsense and the assertion must
  // fail. Do not merge this branch; it exists to be red.
  await expect(
    table.getByRole('columnheader', { name: 'A column that does not exist' }),
  ).toBeVisible()

  // --- Sign out ------------------------------------------------------------
  await page.getByRole('button', { name: 'Sign out' }).click()

  await expect(page).toHaveURL(/\/login/)
  await expect(page.getByRole('button', { name: 'Sign in' })).toBeVisible()

  // The application chrome is gone, not merely navigated away from.
  await expect(page.getByRole('navigation', { name: 'Main navigation' })).toHaveCount(0)

  // --- And it stays signed out across a navigation -------------------------
  //
  // **This is the assertion the component tests cannot make.** `goto` is a
  // fresh page load, so the router's guard re-runs against whatever the browser
  // actually kept rather than against a store the test emptied. A token that
  // outlived the sign-out would show up here and nowhere else.
  await page.goto('/admin/roles')

  await expect(page).toHaveURL(/\/login/)
  await expect(page.getByRole('table')).toHaveCount(0)
})
