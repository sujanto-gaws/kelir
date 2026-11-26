import type { RouteMeta, RouteParamsGeneric, RouteRecordNameGeneric } from 'vue-router'

import { requiredPermission } from '@/router/guards'

/**
 * Where a shell page sits, as its route declares it (#968, NFR-USE-002).
 *
 * **The route table is the hierarchy.** Each route names its parent and the
 * trail follows that chain, so a page opened from a bookmark reads the same as
 * one reached by clicking — neither the browser's history nor the URL's
 * segments are consulted. A route without `breadcrumb` has no trail: the
 * dashboard, which is the root, `forbidden`, and everything outside the shell.
 */
export interface BreadcrumbMeta {
  /** The route above this one; the dashboard when omitted. */
  parent?: string
  /**
   * A heading above this route that is not a place — "Administration", the
   * label over the Configuration (§10) and Administration (§11) screens of the
   * User Manual. Plain text: there is no page to link to.
   */
  section?: string
  /**
   * The param naming the record this page shows. Its crumb takes the name the
   * page publishes for that record, and the route's `title` until then.
   */
  record?: string
}

declare module 'vue-router' {
  interface RouteMeta {
    title?: string
    breadcrumb?: BreadcrumbMeta
  }
}

/** One crumb. `to` is a route name, or null for plain text. */
export interface Crumb {
  label: string
  to: string | null
  current: boolean
}

/** The readable name a record page has loaded, for the record `key` names. */
export interface PublishedName {
  key: string
  name: string
}

const ROOT = 'dashboard'

/** More levels than this is a cycle in the table, not a hierarchy. */
const MAX_DEPTH = 8

/**
 * The trail for a route.
 *
 * A parent is a link only if the user could open it — the same
 * `meta.permission`, through the same `can`, that the guard checks. That is
 * cosmetic: the guard and the server still decide.
 */
export function buildBreadcrumbs(
  route: { name: RouteRecordNameGeneric; params: RouteParamsGeneric; meta: RouteMeta },
  metaOf: (name: string) => RouteMeta | undefined,
  can: (permission: string) => boolean,
  published: PublishedName | null,
): Crumb[] {
  const own = route.meta.breadcrumb
  if (!own || route.name === ROOT) {
    return []
  }

  const reversed: Crumb[] = [
    { label: currentLabel(route, own, published), to: null, current: true },
  ]
  let node: BreadcrumbMeta | undefined = own

  for (let depth = 0; node && depth < MAX_DEPTH; depth += 1) {
    if (node.section) {
      reversed.push({ label: node.section, to: null, current: false })
    }

    const parent: string = node.parent ?? ROOT
    const meta = metaOf(parent)
    const permission = meta ? requiredPermission(meta) : null

    reversed.push({
      label: meta?.title ?? parent,
      to: meta && (permission === null || can(permission)) ? parent : null,
      current: false,
    })

    node = parent === ROOT ? undefined : meta?.breadcrumb
  }

  return reversed.reverse()
}

function currentLabel(
  route: { params: RouteParamsGeneric; meta: RouteMeta },
  own: BreadcrumbMeta,
  published: PublishedName | null,
): string {
  const label = route.meta.title ?? ''

  if (!own.record || !published || published.name.trim() === '') {
    return label
  }

  const param = route.params[own.record]
  const key = Array.isArray(param) ? param[0] : param

  return key !== undefined && key === published.key ? published.name : label
}
