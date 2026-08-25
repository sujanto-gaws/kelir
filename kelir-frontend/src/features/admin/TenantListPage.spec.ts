import { createPinia, setActivePinia } from 'pinia'
import { flushPromises, mount, type DOMWrapper, type VueWrapper } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import TenantListPage from './TenantListPage.vue'
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
import type { Tenant } from '@/types/organization'

/**
 * The backend's refusal, hard-coded rather than imported from the component.
 *
 * Spelling it out is what makes a silent edit on either side fail a test;
 * importing the constant would make the assertion pass for any wording.
 */
const OWN_TENANT_REFUSAL = 'You cannot delete the tenant you administer from'

const platform: Tenant = {
  id: 't-1',
  tenantCode: 'SYSTEM',
  name: 'Kelir Platform',
  status: 'ACTIVE',
  isDefault: true,
  userCount: 3,
  createdAt: '2026-08-01T00:00:00Z',
}

const acme: Tenant = {
  id: 't-2',
  tenantCode: 'ACME',
  name: 'Acme Limited',
  status: 'ACTIVE',
  isDefault: false,
  userCount: 12,
  createdAt: '2026-08-20T00:00:00Z',
}

function listBody(data: unknown[], meta?: PageMeta): unknown {
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

async function mountPage(permissions: string[]): Promise<VueWrapper> {
  // Seed before mounting: `onMounted` fires on the first render, and the
  // controls it decides on are gated by these codes.
  signIn(permissions)

  const wrapper = mount(TenantListPage)
  await flushPromises()

  return wrapper
}

function rowsOf(wrapper: VueWrapper): DOMWrapper<Element>[] {
  return wrapper.findAll('tbody tr')
}

function buttonLabelled(
  buttons: DOMWrapper<HTMLButtonElement>[],
  label: string,
): DOMWrapper<HTMLButtonElement> | undefined {
  return buttons.find((button) => button.text() === label)
}

describe('TenantListPage', () => {
  let backend: FakeBackendHandle
  let handler: FakeHandler

  beforeEach(() => {
    setActivePinia(createPinia())
    window.localStorage.clear()

    handler = () => ({ status: 200, body: listBody([platform, acme]) })
    backend = installFakeBackend((request) => handler(request))
  })

  afterEach(() => {
    backend.restore()
    registerSessionBridge(null)
  })

  it('lists every tenant with the users each one holds', async () => {
    const wrapper = await mountPage(['organization:tenant:read', 'organization:tenant:manage'])

    expect(rowsOf(wrapper)).toHaveLength(2)
    expect(wrapper.text()).toContain('ACME')
    expect(wrapper.text()).toContain('Acme Limited')
    expect(wrapper.text()).toContain('12')
  })

  it('hides every action from a caller who can only read', async () => {
    const wrapper = await mountPage(['organization:tenant:read'])
    const buttons = wrapper.findAll('button')

    expect(rowsOf(wrapper)).toHaveLength(2)
    expect(buttonLabelled(buttons, 'New tenant')).toBeUndefined()
    expect(buttonLabelled(buttons, 'Edit')).toBeUndefined()
    expect(buttonLabelled(buttons, 'Delete')).toBeUndefined()
    expect(wrapper.text()).not.toContain('Actions')
  })

  it('marks the tenant administration is performed from and will not delete it', async () => {
    // The backend answers 400 for this, and discovering that by being refused
    // is a worse experience than a control that is visibly unavailable.
    const wrapper = await mountPage(['organization:tenant:read', 'organization:tenant:manage'])

    const [platformRow, acmeRow] = rowsOf(wrapper)

    expect(platformRow.text()).toContain('This deployment')
    expect(acmeRow.text()).not.toContain('This deployment')

    expect(buttonLabelled(platformRow.findAll('button'), 'Delete')?.element.disabled).toBe(true)
    expect(buttonLabelled(acmeRow.findAll('button'), 'Delete')?.element.disabled).toBe(false)
  })

  it('says how many people a deletion signs out before it is confirmed', async () => {
    // The one number on the screen that changes what the confirmation means.
    const wrapper = await mountPage(['organization:tenant:read', 'organization:tenant:manage'])

    await buttonLabelled(rowsOf(wrapper)[1].findAll('button'), 'Delete')?.trigger('click')

    expect(wrapper.text()).toContain('Acme Limited (ACME) will be removed')
    expect(wrapper.text()).toContain('Its 12 user(s) are signed out')
  })

  it('deletes through the API and reloads the list', async () => {
    const wrapper = await mountPage(['organization:tenant:read', 'organization:tenant:manage'])

    await buttonLabelled(rowsOf(wrapper)[1].findAll('button'), 'Delete')?.trigger('click')

    handler = (request) =>
      request.method === 'delete' ? { status: 204 } : { status: 200, body: listBody([platform]) }

    await buttonLabelled(wrapper.find('[role="dialog"]').findAll('button'), 'Delete')?.trigger(
      'click',
    )
    await flushPromises()

    expect(backend.requests.some((request) => request.url === '/organization/tenants/t-2')).toBe(
      true,
    )
    expect(rowsOf(wrapper)).toHaveLength(1)
  })

  it("keeps the confirmation open and shows the server's refusal when a delete is rejected", async () => {
    // A refusal behind a dismissed toast is a refusal the user never reads.
    const wrapper = await mountPage(['organization:tenant:read', 'organization:tenant:manage'])

    await buttonLabelled(rowsOf(wrapper)[1].findAll('button'), 'Delete')?.trigger('click')

    handler = (request) =>
      request.method === 'delete'
        ? { status: 400, body: errorBody('BAD_REQUEST', OWN_TENANT_REFUSAL) }
        : { status: 200, body: listBody([platform, acme]) }

    await buttonLabelled(wrapper.find('[role="dialog"]').findAll('button'), 'Delete')?.trigger(
      'click',
    )
    await flushPromises()

    expect(wrapper.find('[role="dialog"] [role="alert"]').text()).toContain(OWN_TENANT_REFUSAL)
    expect(wrapper.find('[role="dialog"]').exists()).toBe(true)
    expect(rowsOf(wrapper)).toHaveLength(2)
  })

  it('offers a retry rather than an empty screen when the list fails', async () => {
    handler = () => ({ status: 500, body: errorBody('INTERNAL_ERROR', 'Something went wrong') })

    const wrapper = await mountPage(['organization:tenant:read'])

    expect(wrapper.text()).toContain('Something went wrong')
    expect(buttonLabelled(wrapper.findAll('button'), 'Try again')).toBeDefined()
  })
})
