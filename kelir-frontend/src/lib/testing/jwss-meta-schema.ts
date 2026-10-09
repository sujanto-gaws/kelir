import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

import Ajv from 'ajv'

/**
 * Test support — never imported by application code.
 *
 * Checks a document against `docs/schema/jwss-meta-v1.0.0.json`, the normative
 * JWSS meta-schema (JWSS §1.3), **read from the file itself** so a test cannot
 * drift from it. It is how #426 AC2 — *what the editor writes validates against
 * the meta-schema* — is asserted in this suite.
 *
 * **It is not a validator in the browser**, which AC3 rules out: nothing in
 * `src/` outside tests imports it, and the editor shows the server's verdict.
 *
 * **Ajv 6 is reached as ESLint's own dependency**, not one this package
 * declares: no dependency can be added without a lockfile change, and Ajv 6
 * evaluates every keyword this meta-schema uses (`$ref` by JSON pointer into
 * `$defs`, `if`/`then`/`else`, `const`, `oneOf`, `pattern`, `exclusiveMinimum`).
 * Only the `$schema` line is dropped, because Ajv 6 does not know the 2020-12
 * dialect's URI and nothing below depends on the dialect. If ESLint stops
 * bringing Ajv, this import fails loudly rather than any test passing quietly.
 */
// Resolved from the Vitest root (`kelir-frontend`), as `rad/__fixtures__`
// resolves the JFSS one: the transform does not leave `import.meta.url` a
// `file:` URL.
const META_SCHEMA_PATH = resolve(process.cwd(), '../docs/schema/jwss-meta-v1.0.0.json')

const metaSchema = JSON.parse(readFileSync(META_SCHEMA_PATH, 'utf8')) as Record<string, unknown>

delete metaSchema.$schema

const ajv = new Ajv({ allErrors: true })
const validate = ajv.compile(metaSchema)

/** The meta-schema's complaints about a document, as `instancePath message`; empty when it conforms. */
export function jwssViolations(document: unknown): string[] {
  if (validate(document)) {
    return []
  }

  return (validate.errors ?? []).map((error) => `${error.dataPath} ${error.message}`)
}
