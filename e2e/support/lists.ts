import { expect } from '@playwright/test'

import { runSuffix, type ApiSession } from './api'
import { API_PREFIX } from './env'

/**
 * Seeding a list definition and the documents it arranges, for the spec that
 * renders one (#340).
 *
 * Beside `forms.ts` and `documents.ts` rather than inside either, for the
 * reason those files give about `api.ts`: one module per subject, so none grows
 * a dependency on another.
 *
 * **What is seeded here is the administrator's half.** A list definition, a
 * document type bound to it, and some documents are configuration and data;
 * configuring the list through a screen is FR-RAD-004's builder, which is
 * [#341](https://github.com/sujanto-gaws/kelir/issues/341). Doing it over the
 * API is what lets the browser flow be about the *rendering*, which is what
 * #340 AC5 asks to see driven.
 */

export interface SeededList {
  readonly id: string
  readonly listKey: string
  readonly typeId: string
  readonly typeCode: string
}

/** One column, as the storage API takes it. */
export interface ListColumn {
  columnKey: string
  label: string
  isSortable?: boolean
  format?: string
}

/** One filter, as the storage API takes it. */
export interface ListFilter {
  filterKey: string
  label: string
  filterType: 'TEXT' | 'ENUM' | 'LOOKUP' | 'DATE_RANGE' | 'NUMBER_RANGE' | 'BOOLEAN'
}

/**
 * Creates an `ACTIVE` list, and a document type bound to it.
 *
 * **Both, because neither renders alone.** A list with no document type naming
 * it has no rows *by construction*, and the API refuses it rather than serving
 * an empty table — so a fixture that seeded only the definition would be
 * seeding the failure case.
 *
 * The keys carry the run suffix for the reason `publishForm` explains: the
 * deployment keeps its database between runs, and `uq_rad_lists_tenant_id_list_key`
 * would conflict on the second one.
 */
export async function seedList(
  session: ApiSession,
  options: {
    title: string
    columns: ListColumn[]
    filters?: ListFilter[]
    defaultSort?: { key: string; dir: 'asc' | 'desc' }[]
    pageSize?: number
  },
): Promise<SeededList> {
  const suffix = runSuffix()
  const listKey = `e2e_list_${suffix}`.toLowerCase()

  const created = await session.context.post(`${API_PREFIX}/rad/lists`, {
    data: {
      listKey,
      title: options.title,
      status: 'ACTIVE',
      pageSize: options.pageSize ?? 20,
      defaultSort: options.defaultSort ?? null,
      columns: options.columns,
      filters: options.filters ?? [],
    },
  })

  expect(
    created.ok(),
    `seeding the list failed: ${created.status()} ${await created.text()}`,
  ).toBe(true)

  const { data } = (await created.json()) as { data: { id: string } }
  const typeCode = `E2E_LIST_${suffix}`.toUpperCase()

  const type = await session.context.post(`${API_PREFIX}/document-types`, {
    data: { typeCode, name: options.title, listId: data.id },
  })

  expect(
    type.ok(),
    `binding a document type to the list failed: ${type.status()} ${await type.text()}`,
  ).toBe(true)

  const bound = (await type.json()) as { data: { id: string } }

  return { id: data.id, listKey, typeId: bound.data.id, typeCode }
}

/** A document of the seeded type, so the list has a row to show. */
export async function seedDocument(
  session: ApiSession,
  list: SeededList,
  title: string,
): Promise<string> {
  const created = await session.context.post(`${API_PREFIX}/documents`, {
    data: { documentTypeId: list.typeId, title },
  })

  expect(
    created.ok(),
    `seeding the document "${title}" failed: ${created.status()} ${await created.text()}`,
  ).toBe(true)

  const { data } = (await created.json()) as { data: { id: string } }

  return data.id
}
