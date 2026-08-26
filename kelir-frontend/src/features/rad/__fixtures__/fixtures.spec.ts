import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

import { describe, expect, it } from 'vitest'

import type { JfssComponent, JfssDefinition } from '@/types/jfss'

/**
 * The fixtures are JFSS v2.0.1 documents, checked against the meta-schema's own
 * vocabulary rather than against what looks right.
 *
 * **This test exists because a fixture that was not a JFSS document reached
 * CI.** `purchase-requisition.json` carried `validation.minItems`, which JSON
 * Schema has and *this* meta-schema does not — it closes `validation` with
 * `additionalProperties: false`. The backend refused the definition with a 422
 * and the browser flow failed on the seeding step, six minutes into the slowest
 * job in the pipeline.
 *
 * **Nothing before that point could have caught it, by design.** The renderer
 * must not validate a definition (#162 AC2) — the backend refuses a
 * non-conforming one at save, because *"a definition is written once and
 * rendered thousands of times, and the render path has no good failure"*. So
 * the component tests mounted the invalid document happily and were right to.
 *
 * **The boundary, because an absence of findings means something only with an
 * edge drawn around it.** This is not JSON Schema validation: it checks that
 * every property name a fixture uses is one the meta-schema declares, at the
 * three levels where a wrong name is silently plausible. It would not catch a
 * wrong *value*, a missing required property, or a `oneOf` violation. The full
 * check is the backend's at save time, and the browser flow is what proves it.
 */

/**
 * The canonical meta-schema, read rather than vendored a third time.
 *
 * Resolved from the Vitest root (`kelir-frontend`) rather than from
 * `import.meta.url`, which the transform does not leave as a `file:` URL. The
 * backend vendors its own copy because its release image cannot reach `docs/`,
 * and `rad_jfss_meta_schema.rs` asserts the two are byte-identical — so reading
 * the canonical file here keeps the count at two rather than three.
 */
const metaSchema = JSON.parse(
  readFileSync(resolve(process.cwd(), '../docs/schema/jfss-meta-v2.0.1.json'), 'utf8'),
) as {
  $defs: {
    validation: { properties: Record<string, unknown> }
    component: {
      properties: Record<string, unknown>
      allOf: {
        if: { properties: { role: { const: string } } }
        then: { properties?: Record<string, unknown> }
      }[]
    }
  }
}

const VALIDATION_KEYWORDS = new Set(Object.keys(metaSchema.$defs.validation.properties))

/** Base component properties plus the ones this role's branch adds. */
function allowedComponentKeys(role: string): Set<string> {
  const base = Object.keys(metaSchema.$defs.component.properties)
  const branch = metaSchema.$defs.component.allOf.find(
    (clause) => clause.if.properties.role.const === role,
  )

  return new Set([...base, ...Object.keys(branch?.then.properties ?? {})])
}

const fixtures = import.meta.glob<JfssDefinition>('./*.json', { eager: true, import: 'default' })

/** Every component, through all three container shapes and a row template. */
function allComponents(components: JfssComponent[]): JfssComponent[] {
  return components.flatMap((component) => {
    const container = component as {
      components?: JfssComponent[]
      columns?: { components?: JfssComponent[] }[]
      tabs?: { components?: JfssComponent[] }[]
    }

    const children = [
      ...(container.components ?? []),
      ...(container.columns ?? []).flatMap((slot) => slot.components ?? []),
      ...(container.tabs ?? []).flatMap((slot) => slot.components ?? []),
    ]

    return [component, ...allComponents(children)]
  })
}

describe('the JFSS fixtures', () => {
  it('found the meta-schema and some fixtures to check', () => {
    // Both halves, because either one empty makes every assertion below vacuous.
    expect(VALIDATION_KEYWORDS.size).toBeGreaterThan(0)
    expect(Object.keys(fixtures).length).toBeGreaterThan(0)
  })

  it.each(Object.entries(fixtures))('declares %s at version 2.0.1', (_path, definition) => {
    expect(definition.version).toMatch(/^2\.\d+\.\d+$/)
  })

  it.each(Object.entries(fixtures))(
    'uses only validation keywords %s may carry',
    (_path, definition) => {
      for (const component of allComponents(definition.components)) {
        const validation = (component as { validation?: Record<string, unknown> }).validation

        if (!validation) {
          continue
        }

        for (const keyword of Object.keys(validation)) {
          expect(
            VALIDATION_KEYWORDS.has(keyword),
            `component "${component.id}" uses validation.${keyword}, which the meta-schema closes out`,
          ).toBe(true)
        }
      }
    },
  )

  it.each(Object.entries(fixtures))(
    'uses only component properties %s may carry',
    (_path, definition) => {
      for (const component of allComponents(definition.components)) {
        const allowed = allowedComponentKeys(component.role)

        for (const property of Object.keys(component)) {
          expect(
            allowed.has(property),
            `component "${component.id}" (role ${component.role}) carries "${property}", which its role's branch does not declare`,
          ).toBe(true)
        }
      }
    },
  )

  it.each(Object.entries(fixtures))(
    'gives every layout in %s exactly one child container',
    (_path, definition) => {
      for (const component of allComponents(definition.components)) {
        if (component.role !== 'layout') {
          continue
        }

        const shapes = (['components', 'columns', 'tabs'] as const).filter(
          (shape) => (component as unknown as Record<string, unknown>)[shape] !== undefined,
        )

        // The meta-schema's `oneOf`, which is the rule most easily broken by
        // hand-editing a definition into a shape that looks reasonable.
        expect(
          shapes,
          `layout "${component.id}" holds ${shapes.length} child containers`,
        ).toHaveLength(1)
      }
    },
  )
})
