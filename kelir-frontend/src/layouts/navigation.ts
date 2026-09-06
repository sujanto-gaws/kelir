import type { MenuEntry } from '@/types/rad'

/**
 * Merging the configured navigation into the built-in one (FR-RAD-004, #341).
 *
 * **Pure, and separate from `AppLayout` on purpose.** This is the whole of what
 * "a menu definition takes effect" means, and it is the part worth asking
 * questions of: which entries are hidden, in what order they render, what a
 * tenant that has configured nothing sees, and what a tree with a loop in it
 * does. A sidebar component cannot be asked any of those cheaply.
 */

/** A destination compiled into the application. */
export interface CoreDestination {
  /** The route name — a built-in entry navigates by name, not by path. */
  name: string
  label: string
  icon: unknown
  enabled: boolean
  permission?: string
  sortOrder: number
}

/** One rendered row of the sidebar, whatever it came from. */
export interface NavigationItem {
  key: string
  label: string
  /** A route name for a built-in entry; `null` for a configured one. */
  routeName: string | null
  /** A path for a configured entry; `null` for a built-in one. */
  routePath: string | null
  icon: unknown
  /** Lucide's name for a configured entry's icon, which the layout resolves. */
  iconName: string | null
  enabled: boolean
  depth: number
}

/** How deep the sidebar will nest before it stops trusting the tree. */
export const MAX_DEPTH = 4

/**
 * The sidebar's rows: the built-in destinations and the tenant's own, merged.
 *
 * **Additive.** A tenant with no configured entries sees exactly the built-in
 * navigation — which is what makes this safe to ship without a data migration,
 * and the reason the built-ins stay in code as the `CORE` source rather than
 * being seeded as rows.
 *
 * **Every entry is still hidden by its own permission**, built-in or
 * configured, and that hiding is cosmetic in both cases: the route guard and
 * the endpoint behind it are what actually refuse. `AppLayout`'s own comment
 * has said so since before there was anything configurable.
 *
 * **A disabled configured entry is not rendered at all**, where a *built-in*
 * one renders greyed. The two are different statements: a built-in entry marked
 * `enabled: false` is the product saying *this arrives in a later phase*, and a
 * configured one switched off is a tenant saying *not now* — showing the second
 * as unavailable would advertise a page nobody asked to see.
 *
 * **A configured entry with no route is a heading**, rendered as an unclickable
 * label. It is how a tenant groups the entries under it.
 *
 * **A cycle renders flat rather than hanging.** The API refuses to create one
 * (`service::menu`'s ancestor walk, #191), and a renderer that trusted the API
 * completely would have no answer when the refusal was wrong — a row whose
 * parent is not on the page is drawn at the top level, and depth is bounded.
 */
export function mergeNavigation(
  core: readonly CoreDestination[],
  configured: readonly MenuEntry[],
  can: (permission: string) => boolean,
): NavigationItem[] {
  const visibleCore = core
    .filter((item) => !item.permission || can(item.permission))
    .map((item) => ({
      key: `core:${item.name}`,
      label: item.label,
      routeName: item.name,
      routePath: null,
      icon: item.icon,
      iconName: null,
      enabled: item.enabled,
      depth: 0,
      sortOrder: item.sortOrder,
    }))

  const visible = configured.filter(
    (entry) => entry.isEnabled && (!entry.requiredPermission || can(entry.requiredPermission)),
  )

  // Children are grouped by the parent they name — but only where that parent
  // is *also visible*. An entry whose parent was hidden or switched off would
  // otherwise vanish with it, and it is the tenant's own entry: it rises to the
  // top level instead, which is what the delete path does in the database for
  // the same reason.
  const present = new Set(visible.map((entry) => entry.id))
  const byParent = new Map<string | null, MenuEntry[]>()

  for (const entry of visible) {
    const parent = entry.parentMenuId && present.has(entry.parentMenuId) ? entry.parentMenuId : null

    byParent.set(parent, [...(byParent.get(parent) ?? []), entry])
  }

  const configuredItems: (NavigationItem & { sortOrder: number })[] = []

  const walk = (parent: string | null, depth: number): void => {
    if (depth > MAX_DEPTH) {
      return
    }

    for (const entry of byParent.get(parent) ?? []) {
      configuredItems.push({
        key: `config:${entry.id}`,
        label: entry.label,
        routeName: null,
        routePath: entry.routePath,
        icon: null,
        iconName: entry.icon,
        enabled: true,
        depth,
        // A child sorts with its parent rather than on its own number: nesting
        // is the order, and letting a child's `sortOrder` move it out from
        // under its heading would draw a tree that is not the tree.
        sortOrder: parent === null ? entry.sortOrder : Number.MAX_SAFE_INTEGER,
      })

      walk(entry.id, depth + 1)
    }
  }

  walk(null, 0)

  // **Roots interleave by `sortOrder`; a subtree travels with its root.** The
  // built-ins are numbered in tens, so a tenant can put an entry between two of
  // them without renumbering anything.
  const roots = [...visibleCore, ...configuredItems.filter((item) => item.depth === 0)].sort(
    (left, right) => left.sortOrder - right.sortOrder,
  )

  const ordered: NavigationItem[] = []

  for (const root of roots) {
    ordered.push(strip(root))

    if (root.key.startsWith('config:')) {
      // Everything under this root, in the order the walk produced.
      const at = configuredItems.findIndex((item) => item.key === root.key)

      for (let index = at + 1; index < configuredItems.length; index += 1) {
        if (configuredItems[index].depth === 0) {
          break
        }

        ordered.push(strip(configuredItems[index]))
      }
    }
  }

  return ordered
}

function strip(item: NavigationItem & { sortOrder: number }): NavigationItem {
  return {
    key: item.key,
    label: item.label,
    routeName: item.routeName,
    routePath: item.routePath,
    icon: item.icon,
    iconName: item.iconName,
    enabled: item.enabled,
    depth: item.depth,
  }
}
