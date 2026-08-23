import { createPinia, setActivePinia } from 'pinia'
import { flushPromises, mount, type DOMWrapper, type VueWrapper } from '@vue/test-utils'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import MasterDataListPage from './MasterDataListPage.vue'
import { registerSessionBridge } from '@/api/session'
import {
  installFakeBackend,
  type FakeBackendHandle,
  type FakeHandler,
} from '@/lib/testing/fake-backend'
import { useAuthStore } from '@/stores/auth'
import type { PageMeta } from '@/types/api'
import type { CurrentUser } from '@/types/auth'
import type { MasterDataRow } from '@/types/master-data'

const PARTY_READ = 'master-data:party:read'
const ROLE_READ = 'master-data:party-role:read'

const acme: MasterDataRow = {
  id: 'p-1',
  partyId: 'PARTY-ACME',
  partyTypeId: 'PARTY_GROUP',
  statusId: 'PARTY_ENABLED',
  name: 'Acme Supplies',
  externalId: null,
  createdStamp: '2026-01-01T00:00:00Z',
  lastUpdatedStamp: '2026-01-01T00:00:00Z',
}

const acmeAsSupplier: MasterDataRow = {
  ...acme,
  roleTypeId: 'SUPPLIER',
  roleNumber: 'SUP-0001',
  roleStatusId: 'ACTIVE',
  fromDate: '2026-01-01T00:00:00Z',
  thruDate: null,
}

/** A supplier that holds the role without a profile — legal, and numberless. */
const numberless: MasterDataRow = {
  ...acmeAsSupplier,
  id: 'p-2',
  partyId: 'PARTY-NONUM',
  name: 'No Number Ltd',
  roleNumber: null,
}

function listBody(data: MasterDataRow[], meta?: PageMeta): unknown {
  return { success: true, data, meta: meta ?? { page: 1, pageSize: 20, total: data.length } }
}

function signIn(permissions: string[]): void {
  const user: CurrentUser = {
    id: 'u-1',
    username: 'ana',
    displayName: 'Ana Putri',
    email: 'ana@example.com',
    roles: ['ROLE-ADMIN'],
    permissions,
  }

  useAuthStore().user = user
}

/**
 * A router carrying only the route under test.
 *
 * The real table would drag its guards in, and the guards settle a session
 * against `/auth/me` — which is a different thing from what these tests are
 * about. The path shape is copied so that the view segment and the query string
 * behave as they do in the app.
 */
function makeRouter(): Router {
  return createRouter({
    history: createMemoryHistory(),
    routes: [
      {
        path: '/master-data/:view(parties|suppliers|customers|employees)?',
        name: 'master-data',
        component: MasterDataListPage,
      },
      { path: '/', name: 'home', component: { template: '<div />' } },
    ],
  })
}

async function mountAt(
  permissions: string[],
  location = '/master-data/parties',
): Promise<{ wrapper: VueWrapper; router: Router }> {
  signIn(permissions)

  const router = makeRouter()
  await router.push(location)
  await router.isReady()

  const wrapper = mount(MasterDataListPage, { global: { plugins: [router] } })
  await flushPromises()

  return { wrapper, router }
}

function tabs(wrapper: VueWrapper): string[] {
  return wrapper
    .findAll('nav[aria-label="Master data views"] button')
    .map((button) => button.text())
}

function rows(wrapper: VueWrapper): DOMWrapper<Element>[] {
  return wrapper.findAll('tbody tr')
}

describe('MasterDataListPage', () => {
  let backend: FakeBackendHandle
  let handler: FakeHandler

  beforeEach(() => {
    setActivePinia(createPinia())
    window.localStorage.clear()

    handler = (request) =>
      request.url === '/master-data/parties'
        ? { status: 200, body: listBody([acme]) }
        : { status: 200, body: listBody([acmeAsSupplier, numberless]) }
    backend = installFakeBackend((request) => handler(request))
  })

  afterEach(() => {
    backend.restore()
    registerSessionBridge(null)
  })

  it('offers only the views the caller may open', async () => {
    // #101 AC5. The role views need `master-data:party-role:read` as well as
    // the party one, so a caller holding only the first gets a working screen
    // rather than three tabs that answer 403.
    const { wrapper } = await mountAt([PARTY_READ])

    expect(tabs(wrapper)).toEqual(['Parties'])

    const full = await mountAt([PARTY_READ, ROLE_READ])
    expect(tabs(full.wrapper)).toEqual(['Parties', 'Suppliers', 'Customers', 'Employees'])
  })

  it('falls back to a view the caller may open rather than rendering one they may not', async () => {
    // A pasted `/master-data/suppliers` from someone who lost the role
    // permission. The guard let them onto the page; the page must not then load
    // a list that can only refuse.
    const { wrapper } = await mountAt([PARTY_READ], '/master-data/suppliers')

    expect(wrapper.text()).toContain('Parties')
    expect(backend.requests.map((request) => request.url)).toEqual(['/master-data/parties'])
  })

  it('renders a role view from its own endpoint, with the number that makes it one', async () => {
    const { wrapper } = await mountAt([PARTY_READ, ROLE_READ], '/master-data/suppliers')

    expect(backend.requests.map((request) => request.url)).toEqual(['/master-data/suppliers'])
    expect(rows(wrapper)).toHaveLength(2)
    expect(wrapper.text()).toContain('SUP-0001')
    expect(wrapper.text()).toContain('Supplier no.')
  })

  it('shows a role held without a profile as having no number, not as missing', async () => {
    // A party may hold a role without a profile. Hiding the row would make the
    // list disagree with the role it claims to list; a blank number cell says
    // what is true.
    const { wrapper } = await mountAt([PARTY_READ, ROLE_READ], '/master-data/suppliers')
    const cells = rows(wrapper)[1]
      ?.findAll('td')
      .map((cell) => cell.text())

    expect(cells?.[0]).toBe('PARTY-NONUM')
    expect(cells?.[4]).toBe('—')
  })

  it('offers no filters on the list whose endpoint ignores them', async () => {
    // `/master-data/parties` takes paging and nothing else. A search box that
    // silently did nothing would be worse than its absence.
    const { wrapper } = await mountAt([PARTY_READ, ROLE_READ], '/master-data/parties')

    expect(wrapper.find('input').exists()).toBe(false)
    expect(wrapper.find('select').exists()).toBe(false)
  })

  it('sends a filter to the server and puts it in the URL', async () => {
    // #101 AC3. Both halves matter: the URL is what makes a filtered list
    // linkable, and the wire is what makes it a filter rather than a decoration.
    const { wrapper, router } = await mountAt([PARTY_READ, ROLE_READ], '/master-data/suppliers')

    await wrapper.find('select').setValue('PARTY_DISABLED')
    await flushPromises()

    expect(router.currentRoute.value.query.statusId).toBe('PARTY_DISABLED')
    expect(backend.requests[backend.requests.length - 1]?.params).toMatchObject({
      statusId: 'PARTY_DISABLED',
    })
  })

  it('loads the filters a deep link arrived with rather than the whole population', async () => {
    const { wrapper } = await mountAt(
      [PARTY_READ, ROLE_READ],
      '/master-data/suppliers?search=ACME&roleStatusId=ACTIVE&page=2',
    )

    expect(backend.requests).toHaveLength(1)
    expect(backend.requests[0]?.params).toMatchObject({
      search: 'ACME',
      roleStatusId: 'ACTIVE',
      page: 2,
    })
    // And the controls show what the URL asked for, so the screen and the
    // address bar agree.
    expect((wrapper.find('input').element as HTMLInputElement).value).toBe('ACME')
  })

  it('returns to the first page when a filter changes', async () => {
    // Staying on page 7 of a list that has just been narrowed to two rows
    // shows an empty table under a pager that disagrees with it.
    const { wrapper, router } = await mountAt(
      [PARTY_READ, ROLE_READ],
      '/master-data/suppliers?page=4',
    )

    await wrapper.find('select').setValue('PARTY_DISABLED')
    await flushPromises()

    expect(router.currentRoute.value.query.page).toBeUndefined()
  })

  it('carries no filters between views', async () => {
    // `/parties` accepts none of them; carrying a roleStatusId onto it would
    // put a value in the URL that nothing reads.
    const { wrapper, router } = await mountAt(
      [PARTY_READ, ROLE_READ],
      '/master-data/suppliers?search=ACME',
    )

    const parties = wrapper
      .findAll('nav[aria-label="Master data views"] button')
      .find((button) => button.text() === 'Parties')
    await parties?.trigger('click')
    await flushPromises()

    expect(router.currentRoute.value.query).toEqual({})
    expect(backend.requests[backend.requests.length - 1]?.url).toBe('/master-data/parties')
  })

  it('pages against the total the server reported', async () => {
    handler = () => ({
      status: 200,
      body: listBody([acmeAsSupplier], { page: 1, pageSize: 1, total: 3 }),
    })

    const { wrapper, router } = await mountAt([PARTY_READ, ROLE_READ], '/master-data/suppliers')

    expect(wrapper.text()).toContain('Page 1 of 3')

    const next = wrapper.findAll('button').find((button) => button.text() === 'Next')
    await next?.trigger('click')
    await flushPromises()

    expect(router.currentRoute.value.query.page).toBe('2')
  })

  it('tells an empty result from a failed request', async () => {
    // Three states, not two (coding standard §3.4).
    handler = () => ({ status: 200, body: listBody([]) })
    const empty = await mountAt([PARTY_READ], '/master-data/parties')

    expect(empty.wrapper.text()).toContain('Nothing matches this view')
    expect(empty.wrapper.find('table').exists()).toBe(false)

    handler = () => ({ status: 500, body: undefined })
    const failed = await mountAt([PARTY_READ], '/master-data/parties')

    expect(failed.wrapper.text()).not.toContain('Nothing matches this view')
    expect(failed.wrapper.text()).toContain('Try again')
  })

  it('retries the same query after a failure', async () => {
    handler = () => ({ status: 500, body: undefined })
    const { wrapper } = await mountAt([PARTY_READ, ROLE_READ], '/master-data/suppliers?search=ACME')

    handler = () => ({ status: 200, body: listBody([acmeAsSupplier]) })
    const retry = wrapper.findAll('button').find((button) => button.text() === 'Try again')
    await retry?.trigger('click')
    await flushPromises()

    expect(backend.requests[backend.requests.length - 1]?.params).toMatchObject({ search: 'ACME' })
    expect(rows(wrapper)).toHaveLength(1)
  })
})
