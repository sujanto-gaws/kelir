import { expect, type Locator, type Page } from '@playwright/test'

/**
 * Finding a row on an administration table that pages, wherever it has landed
 * ([#521]).
 *
 * **Why this exists.** The document type, role and tenant screens show twenty
 * rows a page, ordered by code or by age. On a fresh database, which is what CI
 * gives every run, whatever a spec looks for is on page one. On a database that
 * already holds twenty rows, such as a release rehearsal running the harness
 * twice or anybody's staging stack, it is on whatever page its sort puts it,
 * and a spec that looked only at page one timed out on a row that was there
 * ([#503]). None of these screens has a search, so a spec that has to *see* the
 * row, and not only confirm the save, turns the pages the way a person would.
 *
 * Prefer a search or a filter where the screen has one, or confirming a save by
 * its response, as `build-a-list.spec.ts` does. This is for the screens that
 * leave nothing else.
 *
 * [#503]: https://github.com/sujanto-gaws/kelir/issues/503
 * [#521]: https://github.com/sujanto-gaws/kelir/issues/521
 */
export async function pageUntilVisible(page: Page, row: Locator): Promise<void> {
  const rows = page.getByRole('table').locator('tbody tr')
  const next = page.getByRole('button', { name: 'Next', exact: true })

  // The first page has loaded once any row is drawn, this one or another.
  await expect(rows.first()).toBeVisible()

  for (;;) {
    if (await row.isVisible()) {
      return
    }

    if (!(await next.isVisible()) || (await next.isDisabled())) {
      break
    }

    // **The page has turned when its first row has changed.** The page number
    // moves before the rows arrive (`usePaginatedList.goToPage`), so waiting
    // on it would check the old rows under the new number. Pages do not
    // overlap, so a different first row means the new rows are drawn.
    const firstBefore = await rows.first().innerText()

    await next.click()
    await expect(rows.first()).not.toHaveText(firstBefore, { useInnerText: true })
  }

  await expect(row, 'the row is on no page of the table').toBeVisible()
}
