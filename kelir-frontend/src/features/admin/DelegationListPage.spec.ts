import { createPinia, setActivePinia } from 'pinia'
import { flushPromises, mount, type DOMWrapper, type VueWrapper } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import DelegationListPage from './DelegationListPage.vue'
import { registerSessionBridge } from '@/api/session'
import {
  errorBody,
  installFakeBackend,
  type FakeBackendHandle,
  type FakeHandler,
} from '@/lib/testing/fake-backend'
import { useAuthStore } from '@/stores/auth'
import type { PageMeta } from '@/types/api'
import type { CurrentUser } from '@/types/auth'
import type { Delegation } from '@/types/identity'

/**
 * The delegation list (FR-IDM-006, #184).
 *
 * The screen's own decisions are what these assert: that "standing" and
 * "routing" are two facts rather than one, that ending a window says what it
 * does and does not do, and that neither control appears without its permission.
 */

const routing: Delegation = {
  id: 'd-1',
  delegatorUserId: 'u-ani',
  delegatorDisplayName: 'Ani Wijaya',
  delegateUserId: 'u-budi',
  delegateDisplayName: 'Budi Santoso',
  scope: 'ALL',
  documentTypeId: null,
  startsAt: '2026-08-01T00:00:00Z',
  endsAt: '2026-09-30T00:00:00Z',
  reason: 'Annual leave',
  isActive: true,
  isRouting: true,
  createdAt: '2026-07-28T00:00:00Z',
}

/** Set up, not started. Active and not routing — the pair that needs two words. */
const scheduled: Delegation = {
  ...routing,
  id: 'd-2',
  delegateUserId: 'u-citra',
  delegateDisplayName: 'Citra Dewi',
  scope: 'DOCUMENT_TYPE',
  documentTypeId: 'dt-1',
  isActive: true,
  isRouting: false,
}

const ended: Delegation = {
  ...routing,
  id: 'd-3',
  isActive: false,
  isRouting: false,
}

function listBody(data: unknown[], meta?: PageMeta): unknown {
  return { success: true, data, meta: meta ?? { page: 1, pageSize: 20, total: data.length } }
}

function signIn(permissions: string[]): void {
  const user: CurrentUser = {
    id: 'u-ani',
    username: 'ani',
    displayName: 'Ani Wijaya',
    email: 'ani@example.com',
    roles: ['APPROVER'],
    permissions,
  }

  useAuthStore().user = user
}

async function mountPage(permissions: string[]): Promise<VueWrapper> {
  signIn(permissions)

  const wrapper = mount(DelegationListPage)
  await flushPromises()

  return wrapper
}

function rowsOf(wrapper: VueWrapper): DOMWrapper<Element>[] {
  return wrapper.findAll('tbody tr')
}

describe('DelegationListPage', () => {
  let backend: FakeBackendHandle
  let handler: FakeHandler

  beforeEach(() => {
    setActivePinia(createPinia())
    window.localStorage.clear()

    handler = () => ({ status: 200, body: listBody([routing, scheduled, ended]) })
    backend = installFakeBackend((request) => handler(request))
  })

  afterEach(() => {
    backend.restore()
    registerSessionBridge(null)
  })

  it('names both parties and what each window covers', async () => {
    const wrapper = await mountPage(['identity:delegation:read'])

    expect(rowsOf(wrapper)).toHaveLength(3)
    expect(wrapper.text()).toContain('Ani Wijaya')
    expect(wrapper.text()).toContain('Budi Santoso')
    expect(rowsOf(wrapper)[0].text()).toContain('Everything')
    expect(rowsOf(wrapper)[1].text()).toContain('One document type')
  })

  it('tells cover that is running from cover that is only set up', async () => {
    // The two facts the backend sends separately. A screen that folded them
    // into one flag would report a window starting next month as live, which is
    // the question this list is opened with.
    const wrapper = await mountPage(['identity:delegation:read'])
    const [live, upcoming, finished] = rowsOf(wrapper)

    expect(live.find('[data-testid="delegation-state"]').text()).toBe('Routing now')
    expect(upcoming.find('[data-testid="delegation-state"]').text()).toBe('Scheduled')
    expect(finished.find('[data-testid="delegation-state"]').text()).toBe('Ended')
  })

  it('offers neither control to a caller who can only read', async () => {
    const wrapper = await mountPage(['identity:delegation:read'])

    expect(wrapper.find('[data-testid="open-delegation"]').exists()).toBe(false)
    expect(wrapper.find('[data-testid="end-delegation-d-1"]').exists()).toBe(false)
  })

  it('will not offer to end a window that has already ended', async () => {
    const wrapper = await mountPage(['identity:delegation:read', 'identity:delegation:delete'])

    expect(
      (wrapper.find('[data-testid="end-delegation-d-1"]').element as HTMLButtonElement).disabled,
    ).toBe(false)
    expect(
      (wrapper.find('[data-testid="end-delegation-d-3"]').element as HTMLButtonElement).disabled,
    ).toBe(true)
  })

  it('says what ending a window does not do before it is confirmed', async () => {
    // The half people get wrong. Ending it stops work *arriving*; a task already
    // in the delegate's hands stays there, and somebody who expects otherwise
    // will not go and hand it back.
    const wrapper = await mountPage(['identity:delegation:read', 'identity:delegation:delete'])

    await wrapper.find('[data-testid="end-delegation-d-1"]').trigger('click')

    expect(wrapper.text()).toContain('stops reaching Budi Santoso from now')
    expect(wrapper.text()).toContain('Tasks already in their hands stay there')
  })

  it('ends a window through the API and reloads the list', async () => {
    const wrapper = await mountPage(['identity:delegation:read', 'identity:delegation:delete'])

    await wrapper.find('[data-testid="end-delegation-d-1"]').trigger('click')

    handler = (request) =>
      request.method === 'delete'
        ? { status: 204 }
        : { status: 200, body: listBody([{ ...routing, isActive: false, isRouting: false }]) }

    const dialogButtons = wrapper.find('[role="dialog"]').findAll('button')
    await dialogButtons.find((button) => button.text() === 'End it')?.trigger('click')
    await flushPromises()

    expect(backend.requests.some((request) => request.url === '/identity/delegations/d-1')).toBe(
      true,
    )
    expect(rowsOf(wrapper)[0].find('[data-testid="delegation-state"]').text()).toBe('Ended')
  })

  it("keeps the confirmation open and shows the server's words when ending is refused", async () => {
    const wrapper = await mountPage(['identity:delegation:read', 'identity:delegation:delete'])

    await wrapper.find('[data-testid="end-delegation-d-1"]').trigger('click')

    handler = (request) =>
      request.method === 'delete'
        ? { status: 404, body: errorBody('NOT_FOUND', 'Delegation not found') }
        : { status: 200, body: listBody([routing]) }

    const dialogButtons = wrapper.find('[role="dialog"]').findAll('button')
    await dialogButtons.find((button) => button.text() === 'End it')?.trigger('click')
    await flushPromises()

    expect(wrapper.find('[role="dialog"]').exists()).toBe(true)
    expect(wrapper.text()).toContain('Delegation not found')
  })

  it('says plainly when nobody has delegated anything', async () => {
    handler = () => ({ status: 200, body: listBody([]) })

    const wrapper = await mountPage(['identity:delegation:read'])

    expect(wrapper.find('[data-testid="no-delegations"]').exists()).toBe(true)
  })
})
