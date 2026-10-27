import type { Page } from '@playwright/test'

/**
 * Narrowing an administration table to the row a spec looks for, wherever its
 * sort has put it ([#598]).
 *
 * **Why this exists.** The document type, role and tenant screens show twenty
 * rows a page, ordered by code or by age. On a fresh database, which is what CI
 * gives every run, whatever a spec looks for is on page one. On a database that
 * already holds twenty rows, such as a release rehearsal running the harness
 * twice or anybody's staging stack, it is on whatever page its sort puts it,
 * and a spec that looked only at page one timed out on a row that was there
 * ([#503]). Those screens had no search, so specs turned the pages the way a
 * person would ([#521]). Each has one since #598, and a spec searches the way a
 * person would.
 *
 * **It returns when the server has answered the search.** The box applies its
 * value when it is committed, on Enter, and the search goes into the URL and
 * from there to the server. Waiting for that response means the caller's next
 * assertion reads the searched rows, and not the page that was on screen
 * before. A reload keeps the search, because the URL holds it.
 *
 * **A search the URL already holds sends nothing**, because the route does not
 * change, so there is no answer to wait for: the rows on screen are already
 * that search's. `term` is compared as the box trims it.
 *
 * [#503]: https://github.com/sujanto-gaws/kelir/issues/503
 * [#521]: https://github.com/sujanto-gaws/kelir/issues/521
 * [#598]: https://github.com/sujanto-gaws/kelir/issues/598
 */
export async function searchTable(page: Page, searchTestId: string, term: string): Promise<void> {
  const search = page.getByTestId(searchTestId)
  const wanted = term.trim()

  if (new URL(page.url()).searchParams.get('search') === wanted) {
    await search.fill(term)
    await search.press('Enter')
    return
  }

  const answered = page.waitForResponse(
    (response) => new URL(response.url()).searchParams.get('search') === wanted,
  )

  await search.fill(term)
  await search.press('Enter')
  await answered
}
