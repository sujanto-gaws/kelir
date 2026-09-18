import { expect, test } from '@playwright/test'

import { runSuffix } from '../support/api'
import { credentials } from '../support/env'
import { pageUntilVisible } from '../support/paging'

/**
 * Sign in, reach the tenant list, create a tenant with its first administrator
 * (FR-ORG-001, [#27], decision **D-18**).
 *
 * **This flow is the one that answers D-13.** That decision refused to schedule
 * tenant administration because the surface "would create rows nobody can sign
 * in to" — and the reply, in code, is that creating a tenant creates its
 * administrator in the same transaction. `organization_tenants.rs` proves the
 * transaction; this proves a person can drive it, which is the half a reading
 * of the source cannot establish (**D-14**, verification rule 7).
 *
 * **Nothing is seeded over the API here, unlike the supplier flow.** The thing
 * under test *is* the creation, so arranging it beforehand would leave only the
 * list to assert on. The suffix still applies: the deployment keeps its
 * database between runs, and a fixed tenant code conflicts on the second one.
 *
 * **What this cannot show on a single-tenant deployment**, which is what
 * `deploy-local.sh` brings up: the created administrator signing in. Sign-in
 * resolves the default tenant and ignores the code, deliberately, so the
 * account exists and is unreachable until `KELIR_MULTI_TENANT` is on. That
 * property has its own test in `organization_tenants.rs`
 * (`creating_a_tenant_creates_an_administrator_who_can_sign_in`), which runs a
 * multi-tenant instance; asserting it here would need a second deployment.
 */

const suffix = runSuffix()

const tenant = {
  code: `E2E-TNT-${suffix}`,
  name: `Kepler Holdings ${suffix}`,
  administrator: {
    username: `e2e.admin.${suffix.toLowerCase()}`,
    email: `e2e.admin.${suffix.toLowerCase()}@example.test`,
    displayName: `Kepler Administrator ${suffix}`,
    // Above MIN_PASSWORD_LENGTH. Not a credential worth protecting: the account
    // it unlocks lives in a tenant nobody can sign in to on this deployment.
    password: 'a-sufficiently-long-password',
  },
}

test('an administrator creates a tenant and the person who will run it', async ({ page }) => {
  const { username, password } = credentials()

  // --- Sign in -------------------------------------------------------------
  await page.goto('/login')

  await page.getByLabel('Username or email').fill(username)
  await page.getByLabel('Password').fill(password)
  await page.getByRole('button', { name: 'Sign in' }).click()

  await expect(page).toHaveURL(/\/$/)
  await expect(page.getByRole('navigation', { name: 'Main navigation' })).toBeVisible()

  // This deployment serves one tenant, so the form asked `GET /deployment` and
  // was told not to show a tenant field. Asserting its absence is what would
  // catch the endpoint answering wrongly — a field nobody can fill is #67.
  await expect(page.getByLabel('Tenant code')).toHaveCount(0)

  // --- Reach the list ------------------------------------------------------
  //
  // Through the navigation rather than by `goto`: the entry is gated on
  // `organization:tenant:read`, and a direct URL would not exercise that gate.
  await page.getByRole('link', { name: 'Tenants' }).click()
  await expect(page).toHaveURL(/\/admin\/tenants/)

  // The deployment's own tenant is here, marked as the one administration is
  // performed from. Asserting it before creating anything is what makes the
  // assertion afterwards mean the row is new.
  //
  // **Found on whichever page it is on** (#521). The list shows 20 tenants,
  // newest first, so the deployment's own tenant is the last row of the last
  // page: on page one only while the deployment holds fewer than twenty-one.
  // The screen has no search, so the pages are turned.
  const table = page.getByRole('table')
  const deployment = table.getByRole('row', { name: /SYSTEM/ })
  await pageUntilVisible(page, deployment)
  await expect(deployment).toContainText('This deployment')
  await expect(table.getByRole('row', { name: new RegExp(tenant.code) })).toHaveCount(0)

  // --- Create it -----------------------------------------------------------
  await page.getByRole('button', { name: 'New tenant' }).click()

  const dialog = page.getByRole('dialog')
  await expect(dialog).toBeVisible()

  await dialog.getByLabel('Tenant code').fill(tenant.code)
  await dialog.getByLabel('Name', { exact: true }).fill(tenant.name)

  // The administrator is part of the same form because it is part of the same
  // transaction. A dialog that asked only for a tenant would be the surface
  // D-13 refused.
  await dialog.getByLabel('Username').fill(tenant.administrator.username)
  await dialog.getByLabel('Email').fill(tenant.administrator.email)
  await dialog.getByLabel('Display name').fill(tenant.administrator.displayName)
  await dialog.getByLabel('Password').fill(tenant.administrator.password)

  await dialog.getByRole('button', { name: 'Create tenant' }).click()

  // The dialog closes only on success — it stays open and shows the server's
  // words on a refusal, so a still-visible dialog is a failed creation however
  // the list looks.
  await expect(dialog).toHaveCount(0)

  // --- It is there, with its administrator counted -------------------------
  //
  // The list refreshed the page it was on, which may be the last one. The walk
  // starts again from page one, and is not told where the new row should be.
  await page.reload()
  const row = table.getByRole('row', { name: new RegExp(tenant.code) })
  await pageUntilVisible(page, row)
  await expect(row).toContainText(tenant.name)
  await expect(row).toContainText('Active')
  // One user: the administrator created alongside it. A zero here would be the
  // exact state this whole surface was held back to avoid.
  await expect(row.getByRole('cell', { name: '1', exact: true })).toBeVisible()
  await expect(row).not.toContainText('This deployment')
})
