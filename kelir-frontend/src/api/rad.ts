import { getItem, getPage, postItem } from './client'
import type { Page } from '@/types/api'
import type { Form, FormSubmission, LookupOption, LookupQuery } from '@/types/rad'

/**
 * The RAD endpoints (`/api/v1/rad/*`).
 *
 * Thin by design, like `auth.ts` and `master-data.ts`: one call each through
 * the shared client, so envelope unwrapping and error normalisation happen in
 * exactly one place (coding standard §3.3).
 */

/**
 * One form, definition included.
 *
 * The definition arrives as the backend stored it, having been validated
 * against the meta-schema, the Calculation Rule Registry and the lookup source
 * allow-list at save (`domain/jfss.rs`). The renderer therefore reads it rather
 * than re-checking it (#162 AC2).
 */
export function getForm(id: string): Promise<Form> {
  return getItem<Form>(`/rad/forms/${id}`)
}

/**
 * The options a lookup field offers (FR-RAD-007, #161).
 *
 * **Paged and searched on the server.** The endpoint exists so that a form can
 * offer a chooser over master data the caller may already read — it enforces
 * the permission each underlying master-data endpoint requires, so a lookup
 * cannot become a way to read records the caller could not read directly. A
 * client that fetched every supplier and filtered locally would defeat the
 * paging and nothing else.
 */
export function listLookupOptions(
  source: string,
  query: LookupQuery = {},
): Promise<Page<LookupOption>> {
  // Blank values are dropped rather than sent, as `master-data.ts` does it: an
  // empty search box means "no filter", and `?search=` means "match
  // everything" — harmless here, but the two should not be spelled the same.
  const params: Record<string, string | number> = {}

  for (const [key, value] of Object.entries(query)) {
    if (value !== undefined && value !== null && value !== '') {
      params[key] = value as string | number
    }
  }

  return getPage<LookupOption>(`/rad/lookups/${source}/options`, params)
}

/**
 * Submits a filled-in form and returns what the server stored (#164).
 *
 * **Every data key goes**, visible or not, which is JFSS S10.1 and not an
 * oversight: S10.1.1 errata'd v2.0.0's "omit hidden fields" as not
 * implementable, because a conditional that depends on a hidden field would
 * then be decided from different inputs on the two sides. The server discards
 * the values of the components *it* computes as hidden.
 *
 * **What comes back is the server's payload, not the one that went out.** The
 * caller is expected to look at it: a total the server recomputed differently
 * is the one thing a submitting form must not swallow.
 */
export function submitForm(
  formId: string,
  payload: Record<string, unknown>,
): Promise<FormSubmission> {
  return postItem<FormSubmission>(`/rad/forms/${formId}/submissions`, { payload })
}
