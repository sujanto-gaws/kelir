import { deleteItem, getItem, getPage, postItem, putItem } from './client'
import type { Page, PageQuery } from '@/types/api'
import type {
  CreateFormRequest,
  CreateListRequest,
  CreateMenuRequest,
  Form,
  FormSummary,
  MenuEntry,
  FormSubmission,
  ListRow,
  ListDefinition,
  ListSummary,
  LookupOption,
  LookupQuery,
  RadAction,
  RenderableList,
  UpdateFormRequest,
  UpdateListRequest,
  UpdateMenuRequest,
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

/**
 * Form definitions, for a chooser.
 *
 * The summary shape, which is what the endpoint returns: a page of twenty forms
 * with their JFSS documents inlined is twenty definition trees on the wire to
 * render a list of titles, and `FormSummary` exists for exactly that reason.
 */
export function listForms(query: PageQuery = {}): Promise<Page<FormSummary>> {
  return getPage<FormSummary>('/rad/forms', query)
}

/**
 * Creates a form definition, in `DRAFT` (`rad:form:create`).
 *
 * **The server's answer is the one that counts.** `rad::domain::engine`
 * resolves the definition's rule names against the Validation Rule Registry and
 * its operators against the Calculation one, and refuses an unregistered name
 * or a calculation cycle before anything is stored ([ADR-0035]). A 422 carries
 * S10.3 details whose `path` addresses the offending component, which is what
 * the builder shows against the field rather than as a banner.
 */
export function createForm(request: CreateFormRequest): Promise<Form> {
  return postItem<Form>('/rad/forms', request)
}

/**
 * Edits a **draft** revision in place.
 *
 * A published revision refuses this, and that refusal is the product working:
 * documents pin the revision they were filled against
 * ([ADR-0027](../../../docs/architectures/adr/0027.%20A%20Document%20Pins%20the%20Form%20Revision%20It%20Was%20Filled%20Against.md)),
 * so editing one would change what an already-submitted document claims to
 * have been. {@link createFormRevision} is the way forward from a published
 * form.
 */
export function updateForm(id: string, request: UpdateFormRequest): Promise<Form> {
  return putItem<Form>(`/rad/forms/${id}`, request)
}

/**
 * Publishes a draft, fixing it for every document that will pin it.
 *
 * **The definition is re-validated against the stored revision first**, not
 * against what the browser last sent: a draft written by an older build keeps
 * whatever it was stored with, and a published revision is immutable, so a
 * definition that goes live wrong can never be corrected in place.
 */
export function publishForm(id: string): Promise<Form> {
  return postItem<Form>(`/rad/forms/${id}/publish`, {})
}

/** Opens the next revision as a `DRAFT`, seeded from this one. */
export function createFormRevision(id: string, request: UpdateFormRequest = {}): Promise<Form> {
  return postItem<Form>(`/rad/forms/${id}/revisions`, request)
}

/** Retires a form definition (`rad:form:delete`). */
export function deleteForm(id: string): Promise<void> {
  return deleteItem(`/rad/forms/${id}`)
}

/**
 * The whole configured navigation (FR-RAD-004, #341).
 *
 * **Not paged**, which the endpoint says too: a navigation is read whole on
 * every page load, and a second page of it would be navigation nobody found.
 *
 * **Every entry, enabled or not.** This is the *builder's* read — an
 * administrator editing the navigation has to see the entry they switched off.
 * The sidebar does its own filtering, which is where hiding belongs and where
 * it is cosmetic by design.
 */
export function listMenus(): Promise<Page<MenuEntry>> {
  return getPage<MenuEntry>('/rad/menus', {})
}

export function createMenu(request: CreateMenuRequest): Promise<MenuEntry> {
  return postItem<MenuEntry>('/rad/menus', request)
}

export function updateMenu(id: string, request: UpdateMenuRequest): Promise<MenuEntry> {
  return putItem<MenuEntry>(`/rad/menus/${id}`, request)
}

/** Removes an entry; its children move up to its own parent. */
export function deleteMenu(id: string): Promise<void> {
  return deleteItem(`/rad/menus/${id}`)
}

/**
 * One list definition, its columns and filters included (`rad:list:read`).
 *
 * **The builder's read.** Any status, and what is *stored* rather than what can
 * be drawn — the two are different questions and §8.2.4 gives them different
 * endpoints on purpose.
 */
export function getList(id: string): Promise<ListDefinition> {
  return getItem<ListDefinition>(`/rad/lists/${id}`)
}

export function createList(request: CreateListRequest): Promise<ListDefinition> {
  return postItem<ListDefinition>('/rad/lists', request)
}

/**
 * Replaces a list definition.
 *
 * **A collection that is sent replaces the stored set wholesale**, so the
 * builder sends `columns` and `filters` together every time. Sending one would
 * delete the other.
 */
export function updateList(id: string, request: UpdateListRequest): Promise<ListDefinition> {
  return putItem<ListDefinition>(`/rad/lists/${id}`, request)
}

export function deleteList(id: string): Promise<void> {
  return deleteItem(`/rad/lists/${id}`)
}

/**
 * The list definitions this tenant has (#341).
 *
 * **Requires `rad:list:read`, which the document type builder's caller may not
 * hold.** The chooser that reads this treats a refusal as *no lists to offer*
 * rather than as a page failure: binding a list is optional, and a person who
 * may configure a type but not read list definitions can still configure one.
 */
export function listLists(query: PageQuery = {}): Promise<Page<ListSummary>> {
  return getPage<ListSummary>('/rad/lists', query)
}
