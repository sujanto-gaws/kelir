import { expect } from '@playwright/test'

import { runSuffix, type ApiSession } from './api'
import { API_PREFIX } from './env'
import type { SeededForm } from './forms'

/**
 * Seeding a document type, for the spec that raises a document from one.
 *
 * Beside `forms.ts` rather than inside it, for the reason that file gives about
 * `api.ts`: one module per subject, so neither grows a dependency on the other.
 *
 * **What is seeded here is the administrator's half.** A document type with a
 * form binding and a numbering rule is configuration, and configuring it
 * through the UI is FR-DTYPE's screen — which Sprint 9 does not build. Doing it
 * over the API is what lets the browser flow be about the *document*, which is
 * what #172 AC5 asks to see driven.
 */

export interface SeededDocumentType {
  readonly id: string
  readonly typeCode: string
  readonly name: string
}

/**
 * Creates a document type bound to `form`, and gives it a numbering rule.
 *
 * Two calls, because the numbering rule is a sub-resource: a type has one or it
 * does not, so the API models it as `PUT` on `/{id}/numbering-rule` rather than
 * as a field that would conflict the second time.
 *
 * The `typeCode` carries the run suffix for the reason `publishForm` explains:
 * the deployment keeps its database between runs, so a fixed code conflicts on
 * the second one.
 */
export async function createDocumentType(
  session: ApiSession,
  form: SeededForm,
  name: string,
): Promise<SeededDocumentType> {
  const typeCode = `E2E_DOC_${runSuffix()}`.toUpperCase()

  const created = await session.context.post(`${API_PREFIX}/document-types`, {
    data: { typeCode, name, formId: form.id },
  })

  expect(
    created.ok(),
    `seeding the document type failed: ${created.status()} ${await created.text()}`,
  ).toBeTruthy()

  const id = ((await created.json()) as { data: { id: string } }).data.id

  // `GLOBAL` rather than `YEAR`: the assertion downstream is that a *number
  // appeared*, and a scope whose bucket depends on the calendar would make the
  // expected number depend on when the suite runs.
  const numbering = await session.context.put(
    `${API_PREFIX}/document-types/${id}/numbering-rule`,
    { data: { ruleTemplate: `PR-${runSuffix()}-{sequence}`, sequenceScope: 'GLOBAL' } },
  )

  expect(
    numbering.ok(),
    `seeding the numbering rule failed: ${numbering.status()} ${await numbering.text()}`,
  ).toBeTruthy()

  return { id, typeCode, name }
}

/**
 * Creates a draft document of `documentType` over the API.
 *
 * **Seeding, not asserting.** `README.md`'s first rule is *seed what you assert
 * on*: the deployment keeps its database between runs, so a spec that depends on
 * a row another spec created is a spec that passes in the wrong order and fails
 * in the right one. That is not a hypothetical — the tab spec below was written
 * to reuse the document the flow spec creates, and it failed on CI the moment
 * the flow spec did, reporting a missing row rather than the tabs it is about.
 */
export async function createDraft(
  session: ApiSession,
  documentType: SeededDocumentType,
  title: string,
): Promise<string> {
  const created = await session.context.post(`${API_PREFIX}/documents`, {
    data: { documentTypeId: documentType.id, title },
  })

  expect(
    created.ok(),
    `seeding the document failed: ${created.status()} ${await created.text()}`,
  ).toBeTruthy()

  return ((await created.json()) as { data: { id: string } }).data.id
}
