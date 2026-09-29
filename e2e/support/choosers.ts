import type { Page } from '@playwright/test'

/**
 * Picking from a chooser that searches (#525).
 *
 * A chooser shows the first hundred rows of what matches its search. On a
 * deployment that keeps its database between runs, every run adds types and
 * forms that sort ahead of the next run's, so a flow that picked from the
 * unsearched first page started failing after about ten runs. Each helper here
 * types the row's own key first, which is what a person with a long list does.
 */

/** Chooses the document type on `/documents/new`, by its code. */
export async function chooseDocumentType(page: Page, typeCode: string): Promise<void> {
  await page.getByTestId('document-type-search').fill(typeCode)
  await page.getByTestId(`type-${typeCode}`).getByRole('radio').check()
}

/**
 * Chooses an option of a searching select by its label, after searching for
 * `search` — a key unique to this run, so the option is on the page the search
 * returns. `selectOption` waits for the option to be there.
 */
export async function chooseOption(
  page: Page,
  testId: string,
  search: string,
  label: string,
): Promise<void> {
  await page.getByTestId(`${testId}-search`).fill(search)
  await page.getByTestId(testId).selectOption({ label })
}
