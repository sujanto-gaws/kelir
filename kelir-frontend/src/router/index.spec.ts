import { describe, expect, it } from 'vitest'
import type { RouteRecordRaw } from 'vue-router'

import { routes } from './index'

/**
 * Tests the route table the application actually ships, not a fixture.
 *
 * Every other guard spec builds its own routes, which means they verify the
 * guard's logic and nothing about the real table: deleting
 * `permission: 'identity:user:read'` from the live `admin/users` route left the
 * whole suite green while opening the screen to anyone signed in. The route
 * table is configuration, it is security-relevant, and it was covered by
 * nothing.
 *
 * The shape of the assertion is deliberate. It is not a snapshot of the current
 * routes — that would need updating for every new page and would be approved
 * without thought. It is the *rule*: anything reachable inside the authenticated
 * shell is protected unless it is on a short, explicit list of things that are
 * deliberately not.
 */

/** Depth-first walk yielding every leaf route with its full path and merged meta. */
function leaves(
  records: readonly RouteRecordRaw[],
  parentPath = '',
  parentMeta: Record<string, unknown> = {},
): { path: string; name: string; meta: Record<string, unknown> }[] {
  return records.flatMap((record) => {
    const path = record.path.startsWith('/')
      ? record.path
      : `${parentPath.replace(/\/$/, '')}/${record.path}`.replace(/\/$/, '') || '/'

    // Vue Router merges meta down the matched chain, so a child inherits it.
    const meta = { ...parentMeta, ...(record.meta ?? {}) }

    if (record.children && record.children.length > 0) {
      return leaves(record.children, path, meta)
    }

    return [{ path, name: String(record.name ?? path), meta }]
  })
}

/**
 * Routes inside the authenticated shell that are deliberately reachable without
 * a permission.
 *
 * Adding a name here is the moment to ask whether the page should really be
 * open to every signed-in user. Keep it short.
 */
const PERMISSION_EXEMPT = new Set(['dashboard', 'forbidden'])

/**
 * Routes that are deliberately reachable with no session at all.
 *
 * The two reset routes have to be: somebody who cannot sign in is exactly who
 * needs them, so requiring a session would make the flow unreachable to its
 * only users. Neither reads anything — `forgot-password` answers identically
 * whatever it is given, and `reset-password` is guarded by the token in the
 * link rather than by the router.
 */
const PUBLIC_ROUTES = new Set(['login', 'forgot-password', 'reset-password', 'not-found'])

describe('the shipped route table', () => {
  const all = leaves(routes)

  it('has the admin screens behind their identity permissions', () => {
    // Named explicitly, because these are the two this sprint added and the
    // ones an accidental deletion would silently open.
    const byName = new Map(all.map((route) => [route.name, route]))

    expect(byName.get('admin-users')?.meta).toMatchObject({
      requiresAuth: true,
      permission: 'identity:user:read',
    })
    expect(byName.get('admin-roles')?.meta).toMatchObject({
      requiresAuth: true,
      permission: 'identity:role:read',
    })
  })

  it('protects every route that is not deliberately public', () => {
    const unprotected = all
      .filter((route) => !PUBLIC_ROUTES.has(route.name))
      .filter((route) => route.meta.requiresAuth !== true)
      .map((route) => route.name)

    expect(unprotected).toEqual([])
  })

  it('requires a permission on every authenticated route outside the exempt list', () => {
    const missing = all
      .filter((route) => route.meta.requiresAuth === true)
      .filter((route) => !PERMISSION_EXEMPT.has(route.name))
      .filter((route) => typeof route.meta.permission !== 'string')
      .map((route) => route.name)

    expect(missing).toEqual([])
  })

  it('declares requiresAuth alongside every permission', () => {
    // The guard now derives one from the other, so this cannot open a route on
    // its own. It is here so the table stays readable: a route that names a
    // permission but claims to be public is a contradiction worth catching in
    // review rather than relying on the guard to paper over.
    const contradictory = all
      .filter((route) => typeof route.meta.permission === 'string')
      .filter((route) => route.meta.requiresAuth !== true)
      .map((route) => route.name)

    expect(contradictory).toEqual([])
  })

  it('gives every route a name, so the exempt lists cannot silently miss one', () => {
    const unnamed = all.filter((route) => route.name === route.path).map((route) => route.path)

    expect(unnamed).toEqual([])
  })
})
