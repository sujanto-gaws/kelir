import { expect, test } from '@playwright/test'

import { signInOverApi, type ApiSession } from '../support/api'
import { credentials } from '../support/env'
import { seedDocument, seedList, type SeededList } from '../support/lists'

/**
 * A stored list definition becomes a list a person can use (#340 AC5).
 *
 * **This is the criterion that decides whether the item is Done**, and it is
 * the shape [retrospective 11](../../projects/retrospectives/11.%20Sprint%2013%20Retrospective.md)'s
 * fourth action asks for: a *users can* requirement is demonstrated by a
 * browser, not by a component test. `ListRendererPage.spec.ts` covers the
 * mapping from a definition to a table — every format, every absent value, the
 * cases this run is too slow to enumerate. Only this covers the definition
 * travelling over HTTP into a real browser and coming back as a table somebody
 * can sort, filter and page.
 *
 * **The list is seeded with columns and a sort nothing else in the product
 * uses**, deliberately. A flow that asserted the columns the built-in document
 * list happens to show would pass against a renderer that ignored the
 * definition entirely — which is exactly the failure #340 was filed about, one
 * layer up.
 */

let session: ApiSession
let list: SeededList

test.beforeAll(async () => {
  session = await signInOverApi()

  list = await seedList(session, {
    title: 'Requisitions (e2e)',
    // Two columns in an order the built-in list does not use, and a label that
    // is not the field's own name — so a table that showed `Title` rather than
    // `Subject` would be showing something other than this definition.
    columns: [
      { columnKey: 'title', label: 'Subject' },
      { columnKey: 'status', label: 'Stage' },
    ],
    filters: [{ filterKey: 'search', label: 'Find', filterType: 'TEXT' }],
    // Ascending by title, which is the reverse of newest-first — so the first
    // row proves the order came from the definition.
    defaultSort: [{ key: 'title', dir: 'asc' }],
    pageSize: 20,
  })

  // Created newest-last by title, so creation order and definition order
  // disagree.
  await seedDocument(session, list, 'Zinc plating rig')
  await seedDocument(session, list, 'Aluminium ladders')
})

test.afterAll(async () => {
  await session?.context.dispose()
})

test('a stored list definition renders as a list, and every part of it comes from the definition', async ({
  page,
}) => {
  const { username, password } = credentials()

  // --- Sign in -------------------------------------------------------------
  await page.goto('/login')
  await page.getByLabel('Username or email').fill(username)
  await page.getByLabel('Password').fill(password)
  await page.getByRole('button', { name: 'Sign in' }).click()

  await expect(page).toHaveURL(/\/$/)

  // --- Open the list -------------------------------------------------------
  //
  // By its key, which is the URL a menu row points at. There is no list-of-lists
  // screen to reach it from: that is the builder surface (FR-RAD-004, #341).
  await page.goto(`/lists/${list.listKey}`)

  // --- AC1: the title, the columns and their labels are the definition's ----
  await expect(page.getByTestId('list-title')).toHaveText('Requisitions (e2e)')
  await expect(page.getByTestId('column-title')).toContainText('Subject')
  await expect(page.getByTestId('column-status')).toContainText('Stage')

  // A column the definition does not declare is not drawn — which is what makes
  // "every part of it comes from the definition" an assertion rather than a
  // hope. The built-in document list shows a reference and a type; this one
  // declared neither.
  await expect(page.getByTestId('column-documentRef')).toHaveCount(0)
  await expect(page.getByTestId('column-documentTypeCode')).toHaveCount(0)

  // --- AC2: the order is the definition's, not newest-first ----------------
  //
  // `Aluminium ladders` was created *second*. A list ignoring `defaultSort`
  // would put `Zinc plating rig` first, which is what the API did before #340.
  const subjects = page.getByTestId('cell-title')

  await expect(subjects.first()).toHaveText('Aluminium ladders')
  await expect(subjects.nth(1)).toHaveText('Zinc plating rig')

  // And the header says which way it is sorted, so the order is legible rather
  // than merely correct.
  await expect(page.getByTestId('column-title')).toHaveAttribute('aria-sort', 'ascending')

  // --- AC2: sorting is driven from the table -------------------------------
  await page.getByTestId('sort-title').click()

  await expect(page).toHaveURL(/dir=desc/)
  await expect(subjects.first()).toHaveText('Zinc plating rig')
  await expect(page.getByTestId('column-title')).toHaveAttribute('aria-sort', 'descending')

  // --- AC3: the declared filter works, and it is the only one offered ------
  await expect(page.getByTestId('filter-search')).toBeVisible()

  // The definition declares one filter. A status control would be the renderer
  // offering something the author did not.
  await expect(page.getByTestId('filter-status')).toHaveCount(0)
  await expect(page.getByTestId('filter-priority')).toHaveCount(0)

  await page.getByTestId('filter-search').fill('Aluminium')

  await expect(page).toHaveURL(/search=Aluminium/)
  await expect(subjects).toHaveCount(1)
  await expect(subjects.first()).toHaveText('Aluminium ladders')

  // --- The row opens the document it is ------------------------------------
  await subjects.first().click()

  await expect(page).toHaveURL(/\/documents\/[0-9a-f-]{36}$/)
})

/**
 * **AC4: a definition that cannot be drawn says so.**
 *
 * The failure this whole item is about is a table with no rows, which reads as
 * *this tenant has no documents*. A list whose column names nothing must not
 * produce one — and the message has to name the column, because the person
 * looking at the screen is not the person who wrote the definition.
 */
test('a list whose definition cannot be rendered says which part is wrong', async ({ page }) => {
  const { username, password } = credentials()

  // A column key that is neither a document field nor a `form_data.` path.
  const broken = await seedList(session, {
    title: 'Broken list (e2e)',
    columns: [{ columnKey: 'supplier_rating', label: 'Rating' }],
  })

  await page.goto('/login')
  await page.getByLabel('Username or email').fill(username)
  await page.getByLabel('Password').fill(password)
  await page.getByRole('button', { name: 'Sign in' }).click()

  await expect(page).toHaveURL(/\/$/)
  await page.goto(`/lists/${broken.listKey}`)

  const refusal = page.getByTestId('list-refusal')

  await expect(refusal).toBeVisible()
  await expect(refusal).toContainText('supplier_rating')

  // **Instead of the table, not beside it.** A refusal next to an empty table
  // lets somebody read the table.
  await expect(page.getByTestId('rendered-list')).toHaveCount(0)
})
