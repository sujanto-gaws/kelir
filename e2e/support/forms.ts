import { expect } from '@playwright/test'

import { runSuffix, type ApiSession } from './api'
import { API_PREFIX } from './env'

/**
 * Seeding a published form, for the specs that render one.
 *
 * Beside `api.ts` rather than inside it, because that file's own note says what
 * belongs there: fixtures that arrange over HTTP. This is the same idea for a
 * second module, and keeping RAD out of the party helpers means neither grows a
 * dependency on the other.
 */

/** A form the suite created and published. */
export interface SeededForm {
  readonly id: string
  readonly formKey: string
  readonly title: string
}

/**
 * Creates a draft form from a JFSS definition and publishes it.
 *
 * **Two calls, because publication is its own transition.** A draft is editable
 * and a published revision is frozen; the API models that as a state change
 * rather than a flag on create, and a spec that rendered a draft would be
 * exercising a state no document ever pins.
 *
 * The `formId` inside the definition is rewritten to match the generated
 * `formKey`. The deployment keeps its database between runs, so a fixed key
 * conflicts on the second run — the same reason `runSuffix` exists — and a
 * definition whose `formId` disagreed with its row would be a confusing thing
 * to leave behind for whoever reads the seeded data.
 */
export async function publishForm(
  session: ApiSession,
  definition: Record<string, unknown>,
  title: string,
): Promise<SeededForm> {
  const formKey = `e2e_${String(definition.formId ?? 'form').replace(/-/g, '_')}_${runSuffix()}`

  const created = await session.context.post(`${API_PREFIX}/rad/forms`, {
    data: {
      formKey,
      title,
      definition: { ...definition, formId: formKey },
    },
  })

  expect(
    created.ok(),
    `creating form ${formKey} failed: ${created.status()} ${await created.text()}`,
  ).toBe(true)

  const body = (await created.json()) as { data: { id: string } }
  const id = body.data.id

  const published = await session.context.post(`${API_PREFIX}/rad/forms/${id}/publish`)

  expect(
    published.ok(),
    `publishing form ${formKey} failed: ${published.status()} ${await published.text()}`,
  ).toBe(true)

  return { id, formKey, title }
}
