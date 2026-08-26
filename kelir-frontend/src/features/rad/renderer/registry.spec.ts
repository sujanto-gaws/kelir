import { describe, expect, it } from 'vitest'

import { NOT_YET_RENDERED, SUPPORTED, declaredGap, declaredTypes, resolve } from './registry'
import type { JfssComponent, JfssDefinition, JfssRole } from '@/types/jfss'

/**
 * #162 AC1: every component type is rendered or is explicitly declared as not
 * rendered, **with the list in one place rather than discovered per component**.
 *
 * **The acceptance criterion says "every component type the JFSS meta-schema
 * defines", and the meta-schema defines none.** It validates `type` as a bare
 * `string`, and JFSS §4.4 says why: *"`type` is an open vocabulary defined by
 * each implementation's component registry"*. So there is no upstream list to
 * check against, and a test that walked the meta-schema for types would
 * enumerate nothing and pass forever.
 *
 * What is checkable is the property the criterion is actually after: **no type
 * anywhere in this repository is undeclared.** So the test discovers its
 * subjects rather than listing them — every JFSS fixture, walked for every
 * `type` it uses — which is the [Sprint 6
 * retrospective](../../../../../projects/retrospectives/04.%20Sprint%206%20Retrospective.md)'s
 * eighth action. Adding a fixture that uses a new type fails this test until
 * the registry has an opinion about it, which is the whole mechanism.
 */

/** Every fixture in the repository, found rather than named. */
const fixtures = import.meta.glob<JfssDefinition>('../__fixtures__/*.json', {
  eager: true,
  import: 'default',
})

/**
 * Every `(type, role)` a definition uses.
 *
 * Traverses all three of JFSS §4.3.1's child-container shapes plus a
 * repeater's row template. **A traversal that missed one would make this test
 * pass by not looking** — the exact failure §4.3.1 describes: *"traversing only
 * `components` will silently ignore every child nested inside a `columns` or
 * `tabs` container"*. The fixtures put a `lookup` inside `tabs` and a `number`
 * inside a `datagrid` template precisely so that a narrowed walk goes red here.
 */
function usedTypes(components: JfssComponent[]): Map<string, JfssRole> {
  const found = new Map<string, JfssRole>()

  const walk = (nodes: JfssComponent[]): void => {
    for (const node of nodes) {
      found.set(node.type, node.role)

      const container = node as {
        components?: JfssComponent[]
        columns?: { components?: JfssComponent[] }[]
        tabs?: { components?: JfssComponent[] }[]
      }

      walk(container.components ?? [])

      for (const slot of [...(container.columns ?? []), ...(container.tabs ?? [])]) {
        walk(slot.components ?? [])
      }
    }
  }

  walk(components)

  return found
}

describe('the component registry', () => {
  it('has at least one fixture to discover types from', () => {
    // Without this, every assertion below passes over an empty set — a green
    // suite proving that nothing was looked at, which is what §2.9 calls a test
    // reporting on nothing.
    expect(Object.keys(fixtures).length).toBeGreaterThan(0)
  })

  it.each(Object.entries(fixtures))('declares every component type %s uses', (_path, definition) => {
    const declared = declaredTypes()

    for (const type of usedTypes(definition.components).keys()) {
      expect(declared, `component type "${type}" is in neither list`).toContain(type)
    }
  })

  it.each(Object.entries(fixtures))('agrees with %s about which role each type has', (_path, definition) => {
    for (const [type, role] of usedTypes(definition.components)) {
      const entry = resolve(type)

      // A declared gap has no entry and no role to disagree about.
      if (entry) {
        expect(entry.role, `registry has "${type}" as ${entry.role}, the fixture uses it as ${role}`).toBe(role)
      }
    }
  })

  it('never has a type in both lists', () => {
    // A type in both would resolve as supported and explain itself as a gap,
    // which is two answers to one question and the kind of thing that survives
    // review because each list looks right on its own.
    for (const type of Object.keys(SUPPORTED)) {
      expect(NOT_YET_RENDERED, `"${type}" is both supported and declared missing`).not.toHaveProperty(type)
    }
  })

  it('renders the lookup type the backend expects it to', () => {
    // `domain/jfss.rs` hard-codes `lookup` as the type whose options come from
    // master data and refuses a binding on anything else. Renaming it on this
    // side would leave every stored definition pointing at a type that no
    // longer exists, and nothing else would fail.
    expect(resolve('lookup')?.role).toBe('data')
  })

  it('gives a reason for every type it declares missing', () => {
    for (const [type, reason] of Object.entries(NOT_YET_RENDERED)) {
      expect(reason, `"${type}" is declared missing without saying why`).not.toBe('')
      expect(declaredGap(type)).toBe(reason)
    }
  })

  it('has no opinion about a type nobody declared', () => {
    expect(resolve('nonexistent-widget')).toBeUndefined()
    expect(declaredGap('nonexistent-widget')).toBeUndefined()
  })
})
