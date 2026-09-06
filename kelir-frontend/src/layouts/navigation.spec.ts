import { describe, expect, it } from 'vitest'

import type { MenuEntry } from '@/types/rad'

import { mergeNavigation, type CoreDestination } from './navigation'

/**
 * The sidebar merge (#341 AC2).
 *
 * **This is where "a menu definition takes effect" is asserted.** The browser
 * flow proves the screen writes an entry and the sidebar shows it; these are
 * the cases the browser is too slow to enumerate — a tenant that configures
 * nothing, an entry hidden by permission, a disabled one, a heading, a parent
 * that is not on the page, and a tree deep enough to be a mistake.
 */

function core(overrides: Partial<CoreDestination> = {}): CoreDestination {
  return {
    name: 'documents',
    label: 'Documents',
    icon: 'FileText',
    enabled: true,
    sortOrder: 30,
    ...overrides,
  }
}

function entry(overrides: Partial<MenuEntry> = {}): MenuEntry {
  return {
    id: 'm1',
    menuKey: 'purchasing',
    label: 'Purchasing',
    icon: null,
    parentMenuId: null,
    routePath: '/lists/purchase_requisitions',
    requiredPermission: null,
    source: 'CONFIG',
    sortOrder: 35,
    isEnabled: true,
    createdAt: '2026-09-06T00:00:00Z',
    updatedAt: '2026-09-06T00:00:00Z',
    ...overrides,
  }
}

const everything = () => true
const nothing = () => false

function labels(items: { label: string }[]): string[] {
  return items.map((item) => item.label)
}

describe('mergeNavigation — the built-in navigation is never taken away', () => {
  /**
   * **The property that made this safe to ship without a data migration.** The
   * alternative design seeded the built-ins as rows, and an empty table would
   * then be an application nobody can navigate.
   */
  it('renders exactly the built-in destinations when nothing is configured', () => {
    const merged = mergeNavigation(
      [core(), core({ name: 'tasks', label: 'My Tasks' })],
      [],
      everything,
    )

    expect(labels(merged)).toEqual(['Documents', 'My Tasks'])
  })

  it('keeps a built-in destination that is not yet enabled, greyed rather than gone', () => {
    // A built-in marked `enabled: false` is the product saying *this arrives in
    // a later phase*, which is a different statement from a tenant switching
    // one of their own off.
    const merged = mergeNavigation([core({ enabled: false })], [], everything)

    expect(merged).toHaveLength(1)
    expect(merged[0].enabled).toBe(false)
  })

  it('hides a built-in destination the viewer has no permission for', () => {
    const merged = mergeNavigation([core({ permission: 'document:read' })], [], nothing)

    expect(merged).toHaveLength(0)
  })
})

describe('mergeNavigation — a configured entry joins the built-in ones', () => {
  it('interleaves by sortOrder rather than appending', () => {
    // The built-ins are numbered in tens so a tenant can sit between two of
    // them without renumbering anything.
    const merged = mergeNavigation(
      [core({ name: 'tasks', label: 'My Tasks', sortOrder: 20 }), core({ sortOrder: 40 })],
      [entry({ label: 'Purchasing', sortOrder: 30 })],
      everything,
    )

    expect(labels(merged)).toEqual(['My Tasks', 'Purchasing', 'Documents'])
  })

  it('carries the configured route as a path, not a route name', () => {
    // A built-in navigates by name and a configured entry by the path somebody
    // typed; the two are different `RouterLink` shapes.
    const merged = mergeNavigation([], [entry({ routePath: '/lists/x' })], everything)

    expect(merged[0].routePath).toBe('/lists/x')
    expect(merged[0].routeName).toBeNull()
  })

  it('hides a configured entry the viewer has no permission for', () => {
    const merged = mergeNavigation([], [entry({ requiredPermission: 'document:read' })], nothing)

    expect(merged).toHaveLength(0)
  })

  it('offers a configured entry that names no permission to everybody', () => {
    const merged = mergeNavigation([], [entry({ requiredPermission: null })], nothing)

    expect(merged).toHaveLength(1)
  })

  /**
   * A tenant's switched-off entry is not rendered at all, where a built-in
   * one renders greyed — showing it as unavailable would advertise a page
   * nobody asked to see.
   */
  it('drops a configured entry that is switched off', () => {
    const merged = mergeNavigation([core()], [entry({ isEnabled: false })], everything)

    expect(labels(merged)).toEqual(['Documents'])
  })
})

describe('mergeNavigation — the tree', () => {
  it('renders a child after its parent, one level deeper', () => {
    const merged = mergeNavigation(
      [],
      [
        entry({ id: 'p', menuKey: 'ops', label: 'Operations', routePath: null }),
        entry({ id: 'c', menuKey: 'purchasing', label: 'Purchasing', parentMenuId: 'p' }),
      ],
      everything,
    )

    expect(labels(merged)).toEqual(['Operations', 'Purchasing'])
    expect(merged[0].depth).toBe(0)
    expect(merged[1].depth).toBe(1)
  })

  /** An entry with no route is a heading — a label, not a link that does nothing. */
  it('renders a parent with no route as a heading', () => {
    const merged = mergeNavigation(
      [],
      [entry({ id: 'p', label: 'Operations', routePath: null })],
      everything,
    )

    expect(merged[0].routePath).toBeNull()
    expect(merged[0].routeName).toBeNull()
  })

  /**
   * A subtree travels with its root rather than being sorted apart from it:
   * letting a child's own `sortOrder` move it would draw a tree that is not the
   * tree.
   */
  it('keeps a subtree together when a built-in sorts between the parent and child', () => {
    const merged = mergeNavigation(
      [core({ label: 'Documents', sortOrder: 50 })],
      [
        entry({ id: 'p', label: 'Operations', routePath: null, sortOrder: 40 }),
        entry({ id: 'c', label: 'Purchasing', parentMenuId: 'p', sortOrder: 90 }),
      ],
      everything,
    )

    expect(labels(merged)).toEqual(['Operations', 'Purchasing', 'Documents'])
  })

  /**
   * **An entry whose parent is hidden rises rather than vanishing.** It is the
   * tenant's own entry, and disappearing with a parent they cannot see would
   * lose it — which is what the delete path does in the database, for the same
   * reason.
   */
  it('lifts a child whose parent the viewer cannot see', () => {
    const merged = mergeNavigation(
      [],
      [
        entry({ id: 'p', label: 'Operations', requiredPermission: 'secret', routePath: null }),
        entry({ id: 'c', label: 'Purchasing', parentMenuId: 'p' }),
      ],
      (permission) => permission !== 'secret',
    )

    expect(labels(merged)).toEqual(['Purchasing'])
    expect(merged[0].depth).toBe(0)
  })

  it('lifts a child whose parent is not on the page at all', () => {
    const merged = mergeNavigation([], [entry({ parentMenuId: 'gone' })], everything)

    expect(merged).toHaveLength(1)
    expect(merged[0].depth).toBe(0)
  })

  /**
   * **A cycle renders rather than hanging.** The API refuses to create one
   * (`service::menu`'s ancestor walk, #191) and a renderer that trusted it
   * completely would have no answer when the refusal was wrong.
   */
  it('terminates on a tree that has a loop in it', () => {
    const merged = mergeNavigation(
      [],
      [
        entry({ id: 'a', label: 'A', parentMenuId: 'b' }),
        entry({ id: 'b', label: 'B', parentMenuId: 'a' }),
      ],
      everything,
    )

    // Neither is a root, so the walk from the top finds nothing — and it
    // returns rather than recursing forever, which is the assertion.
    expect(merged).toHaveLength(0)
  })

  it('stops nesting past the depth it will draw', () => {
    const chain: MenuEntry[] = []

    for (let level = 0; level <= 8; level += 1) {
      chain.push(
        entry({
          id: `n${level}`,
          menuKey: `n${level}`,
          label: `Level ${level}`,
          parentMenuId: level === 0 ? null : `n${level - 1}`,
        }),
      )
    }

    const merged = mergeNavigation([], chain, everything)

    expect(merged.length).toBeLessThan(chain.length)
    expect(Math.max(...merged.map((item) => item.depth))).toBeLessThanOrEqual(4)
  })
})
