import { getItem, getPage, postItem } from './client'
import type { Page } from '@/types/api'
import type {
  Form,
  FormSubmission,
  ListRow,
  LookupOption,
  LookupQuery,
  RadAction,
  RenderableList,
} from '@/types/rad'

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

/**
 * One list, resolved for drawing (FR-RAD-003, FR-RAD-010, #340).
 *
 * **By key, not by id**, because a rendered list is reached from a URL somebody
 * bookmarks and `listKey` is the tenant-unique name a menu and a document type
 * already use.
 *
 * The definition arrives already checked against what can be drawn: a column
 * naming nothing, a filter the documents query has no parameter for, or a
 * default sort on an unsortable column is a 422 naming the key rather than a
 * table that renders blank. This side reads the answer; it does not re-check.
 */
export function getRenderableList(listKey: string): Promise<RenderableList> {
  return getItem<RenderableList>(`/rad/lists/by-key/${encodeURIComponent(listKey)}`)
}

/**
 * One page of the rows a list arranges.
 *
 * **Filters go by the definition's own `key`.** The server maps each to the
 * query parameter it sets, so this side never learns that `stage` means
 * `status` — and a filter the definition does not declare is refused rather
 * than ignored, which is why nothing here invents one.
 *
 * **`pageSize` is dropped rather than forwarded**, and it is dropped here
 * because `useQueryBackedList` adds one of its own: the definition decides the
 * page size, the endpoint refuses the parameter by name, and a composable that
 * paginates every other list in the product should not have to know that. The
 * server's `meta.pageSize` is what the pager then reads back.
 */
export function listRenderedRows(
  listId: string,
  query: Record<string, string | number | undefined> = {},
): Promise<Page<ListRow>> {
  const params: Record<string, string | number> = {}

  for (const [key, value] of Object.entries(query)) {
    if (key === 'pageSize') {
      continue
    }

    if (value !== undefined && value !== null && value !== '') {
      params[key] = value as string | number
    }
  }

  return getPage<ListRow>(`/rad/lists/${listId}/rows`, params)
}

/**
 * The configured actions this caller may invoke in one context (§5.10).
 *
 * **Everything returned is already permitted.** The server drops any action
 * whose `required_permission` the caller does not hold, so there is nothing to
 * filter and nothing to disable — a disabled button would publish the existence
 * of an action the permission was set to hide.
 */
export function listActions(context: RadAction['context']): Promise<Page<RadAction>> {
  return getPage<RadAction>('/rad/actions', { context } as Record<string, string>)
}
