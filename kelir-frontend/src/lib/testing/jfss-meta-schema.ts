import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

import Ajv from 'ajv'

/**
 * Test support — never imported by application code.
 *
 * Checks a form definition against `docs/schema/jfss-meta-v2.0.1.json`, the
 * normative JFSS meta-schema (JFSS §1.3), **read from the file itself** so a
 * test cannot drift from it. It is how #688 E3 — *what the form builder writes
 * validates against the meta-schema* — is asserted.
 *
 * **It is not a validator in the browser** (E4, ADR-0035): nothing in `src/`
 * outside tests imports it, and the builder shows the server's verdict.
 *
 * # Ajv 6 does not close a component, so this walk does
 *
 * Ajv 6.15.0 is the only Ajv in the lockfile, reached as ESLint's own
 * dependency as [`jwss-meta-schema.ts`](./jwss-meta-schema.ts) reaches it. It
 * evaluates `$ref` into `$defs`, `if`/`then`, `const`, `oneOf`, `pattern` and
 * `additionalProperties`, and **it ignores `unevaluatedProperties`**, a 2019-09
 * keyword it predates — and that keyword is the only thing in the meta-schema
 * that refuses a property a component's role does not declare. So Ajv alone
 * passes `{ "role": "layout", "key": "x", ... }`, which the server refuses.
 *
 * **The closure walk below restores it**: every node, row templates and slots
 * included, is checked for keys outside the set the meta-schema declares for
 * its role. The two sets are transcribed from the meta-schema's `allOf`
 * branches, and the test that pins this helper is seen red on a stray
 * property with the walk removed. The other option was an Ajv 8 devDependency,
 * which evaluates `unevaluatedProperties` itself; it was not taken because it
 * adds a package to the lockfile for one keyword a twenty-line walk covers,
 * and this row already adds one dependency.
 */
// Resolved from the Vitest root (`kelir-frontend`), as the JWSS helper does.
const META_SCHEMA_PATH = resolve(process.cwd(), '../docs/schema/jfss-meta-v2.0.1.json')

const metaSchema = JSON.parse(readFileSync(META_SCHEMA_PATH, 'utf8')) as Record<string, unknown>

delete metaSchema.$schema

const ajv = new Ajv({ allErrors: true })
const validate = ajv.compile(metaSchema)

/** The keys every component may carry (`$defs.component.properties`). */
const BASE = ['id', 'role', 'type', 'conditional']

/** The keys each role adds, from the meta-schema's `allOf` `then.properties`. */
const BY_ROLE: Record<string, string[]> = {
  data: [
    'key',
    'label',
    'placeholder',
    'description',
    'defaultValue',
    'validation',
    'options',
    'rules',
    'calculate',
    'readOnly',
    'sequenceKey',
    'defaultItems',
    'components',
    'calculateMode',
  ],
  layout: ['components', 'title', 'grid', 'columns', 'tabs'],
  display: ['content', 'variant', 'calculate'],
  action: ['label', 'action', 'theme'],
}

/** `$defs.columnSlot` and `$defs.tabSlot`. */
const SLOT_KEYS = { columns: ['components'], tabs: ['title', 'components'] }

/** Keys the meta-schema does not declare for a node's role, as `path: key`. */
function undeclared(nodes: unknown, path: string, found: string[]): void {
  if (!Array.isArray(nodes)) {
    return
  }

  nodes.forEach((node: unknown, index) => {
    if (typeof node !== 'object' || node === null) {
      return
    }

    const here = `${path}.${index}`
    const record = node as Record<string, unknown>
    const allowed = new Set([...BASE, ...(BY_ROLE[String(record.role)] ?? [])])

    for (const key of Object.keys(record)) {
      if (!allowed.has(key)) {
        found.push(`${here} has undeclared property "${key}"`)
      }
    }

    undeclared(record.components, `${here}.components`, found)

    for (const slotKey of ['columns', 'tabs'] as const) {
      const slots = record[slotKey]

      if (!Array.isArray(slots)) {
        continue
      }

      slots.forEach((slot: unknown, at) => {
        if (typeof slot !== 'object' || slot === null) {
          return
        }

        for (const key of Object.keys(slot)) {
          if (!SLOT_KEYS[slotKey].includes(key)) {
            found.push(`${here}.${slotKey}.${at} has undeclared property "${key}"`)
          }
        }

        undeclared(
          (slot as Record<string, unknown>).components,
          `${here}.${slotKey}.${at}.components`,
          found,
        )
      })
    }
  })
}

/**
 * The meta-schema's complaints about a definition, as `path message`, plus the
 * closure walk's; empty when it conforms.
 */
export function jfssViolations(definition: unknown): string[] {
  const found = validate(definition)
    ? []
    : (validate.errors ?? []).map((error) => `${error.dataPath} ${error.message}`)

  if (typeof definition === 'object' && definition !== null) {
    undeclared((definition as Record<string, unknown>).components, 'components', found)
  }

  return found
}
