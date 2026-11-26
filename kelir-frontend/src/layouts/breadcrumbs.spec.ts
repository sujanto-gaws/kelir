import { describe, expect, it } from 'vitest'
import type { RouteMeta, RouteRecordRaw } from 'vue-router'

import { routes } from '@/router/index'

import { buildBreadcrumbs, type Crumb, type PublishedName } from './breadcrumbs'

/**
 * The trail, built from the route table the application ships (#968).
 *
 * The table is read, not copied: a shell route added without `meta.breadcrumb`
 * fails the first test here, which is what stops a page shipping without one.
 */

const shell = routes.find((record) => record.path === '/')?.children ?? []

/** Every named route, by name — what the layout reads from the live router. */
const byName = new Map<string, RouteRecordRaw>()
function collect(records: readonly RouteRecordRaw[]): void {
  for (const record of records) {
    if (typeof record.name === 'string') {
      byName.set(record.name, record)
    }
    collect(record.children ?? [])
  }
}
collect(routes)

function metaOf(name: string): RouteMeta | undefined {
  return byName.get(name)?.meta
}

const everything = () => true

function trail(
  name: string,
  options: {
    params?: Record<string, string>
    can?: (permission: string) => boolean
    published?: PublishedName | null
  } = {},
): Crumb[] {
  return buildBreadcrumbs(
    { name, params: options.params ?? {}, meta: metaOf(name) ?? {} },
    metaOf,
    options.can ?? everything,
    options.published ?? null,
  )
}

function last(crumbs: Crumb[]): string | undefined {
  return crumbs.slice(-1)[0]?.label
}

function labels(name: string): string {
  return trail(name)
    .map((crumb) => crumb.label)
    .join(' › ')
}

/** AC3, route by route. A record route shows its static label with no name published. */
const EXPECTED: Record<string, string> = {
  'master-data': 'Dashboard › Master data',
  'form-render': 'Dashboard › Form',
  documents: 'Dashboard › Documents',
  'new-document': 'Dashboard › Documents › New document',
  document: 'Dashboard › Documents › Document',
  'rendered-list': 'Dashboard › List',
  notifications: 'Dashboard › Notifications',
  tasks: 'Dashboard › My tasks',
  task: 'Dashboard › My tasks › Task',
  'admin-users': 'Dashboard › Administration › Users',
  'admin-roles': 'Dashboard › Administration › Roles',
  'admin-delegations': 'Dashboard › Administration › Delegations',
  'admin-tenants': 'Dashboard › Administration › Tenants',
  'admin-document-types': 'Dashboard › Administration › Document types',
  'admin-forms': 'Dashboard › Administration › Forms',
  'admin-form-builder': 'Dashboard › Administration › Forms › Form',
  'admin-lists': 'Dashboard › Administration › Lists',
  'admin-list-builder': 'Dashboard › Administration › Lists › List',
  'admin-workflows': 'Dashboard › Administration › Workflows',
  'admin-workflow-new': 'Dashboard › Administration › Workflows › New workflow',
  'admin-workflow-editor': 'Dashboard › Administration › Workflows › Workflow',
  'admin-menus': 'Dashboard › Administration › Navigation',
  'admin-external-systems': 'Dashboard › Administration › External systems',
  'admin-external-system': 'Dashboard › Administration › External systems › External system',
  'admin-integration-logs': 'Dashboard › Administration › Integration logs',
}

describe('the route table (AC1)', () => {
  const trailed = shell
    .map((record) => String(record.name))
    .filter((name) => name !== 'dashboard' && name !== 'forbidden')

  it('gives every shell route except the dashboard and forbidden a trail', () => {
    expect(trailed).toHaveLength(25)

    for (const name of trailed) {
      expect(metaOf(name)?.breadcrumb, `${name} has no meta.breadcrumb`).toBeDefined()
      expect(trail(name)[0], name).toMatchObject({ label: 'Dashboard', to: 'dashboard' })
    }
  })

  it('names every one of them in the expected trails, and no other', () => {
    expect(Object.keys(EXPECTED).sort()).toEqual([...trailed].sort())
  })

  it('gives the dashboard, forbidden and every route outside the shell none', () => {
    for (const name of [
      'dashboard',
      'forbidden',
      'login',
      'forgot-password',
      'reset-password',
      'not-found',
    ]) {
      expect(trail(name), name).toEqual([])
    }
  })
})

describe('the hierarchy (AC2, AC3)', () => {
  it.each(Object.entries(EXPECTED))('%s reads %s', (name, expected) => {
    expect(labels(name)).toBe(expected)
  })

  it('names a parent that exists for every route that declares one', () => {
    for (const name of Object.keys(EXPECTED)) {
      const parent = metaOf(name)?.breadcrumb?.parent
      if (parent) {
        expect(byName.has(parent), `${name}'s parent ${parent}`).toBe(true)
      }
    }
  })

  it('ignores the URL: the same route with other params has the same trail', () => {
    const one = trail('admin-form-builder', { params: { id: 'a' } }).map((crumb) => crumb.label)
    const two = trail('admin-form-builder', { params: { id: 'b' } }).map((crumb) => crumb.label)

    expect(one).toEqual(two)
  })
})

describe('which crumbs are links (AC4)', () => {
  it('links every earlier crumb with a route, and not the last', () => {
    const crumbs = trail('admin-form-builder')

    expect(crumbs.map((crumb) => crumb.to)).toEqual(['dashboard', null, 'admin-forms', null])
    expect(crumbs.map((crumb) => crumb.current)).toEqual([false, false, false, true])
  })

  it('shows Administration as plain text, because it has no route', () => {
    for (const name of Object.keys(EXPECTED).filter((key) => key.startsWith('admin-'))) {
      const administration = trail(name).find((crumb) => crumb.label === 'Administration')

      expect(administration, name).toMatchObject({ to: null, current: false })
    }
  })

  it('shows a parent the user cannot open as plain text', () => {
    // Same rule as the guard: the parent's `meta.permission`, through `can`.
    const crumbs = trail('admin-form-builder', {
      can: (permission) => permission !== 'rad:form:read',
    })

    expect(crumbs[2]).toMatchObject({ label: 'Forms', to: null })
  })

  it('still links a parent whose permission the user holds', () => {
    const crumbs = trail('task', { can: (permission) => permission === 'workflow:task:read' })

    expect(crumbs[1]).toMatchObject({ label: 'My tasks', to: 'tasks' })
  })
})

describe('a record crumb (AC5)', () => {
  it('shows the name its page published for this record', () => {
    const crumbs = trail('document', {
      params: { id: 'doc-a' },
      published: { key: 'doc-a', name: 'Purchase of chairs' },
    })

    expect(last(crumbs)).toBe('Purchase of chairs')
  })

  it('keeps the static label while nothing is published (loading, or a failed load)', () => {
    const crumbs = trail('document', { params: { id: 'doc-a' }, published: null })

    expect(last(crumbs)).toBe('Document')
  })

  it('shows no name published for another record', () => {
    const crumbs = trail('document', {
      params: { id: 'doc-b' },
      published: { key: 'doc-a', name: 'Purchase of chairs' },
    })

    expect(last(crumbs)).toBe('Document')
  })

  it('never shows the id', () => {
    const crumbs = trail('task', { params: { id: 'task-1' }, published: null })

    expect(crumbs.map((crumb) => crumb.label)).not.toContain('task-1')
  })

  it('matches a rendered list by its key', () => {
    const crumbs = trail('rendered-list', {
      params: { listKey: 'po_open' },
      published: { key: 'po_open', name: 'Open purchase orders' },
    })

    expect(crumbs.map((crumb) => crumb.label)).toEqual(['Dashboard', 'Open purchase orders'])
  })

  it('keeps the label for a blank name', () => {
    const crumbs = trail('admin-workflow-editor', {
      params: { id: 'wf-1' },
      published: { key: 'wf-1', name: '   ' },
    })

    expect(last(crumbs)).toBe('Workflow')
  })

  it('ignores a published name on a route that is not a record', () => {
    const crumbs = trail('documents', { published: { key: 'doc-a', name: 'Purchase of chairs' } })

    expect(last(crumbs)).toBe('Documents')
  })
})
