import { createPinia, setActivePinia } from 'pinia'
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import DelegationFormDialog from './DelegationFormDialog.vue'
import { registerSessionBridge } from '@/api/session'
import {
  errorBody,
  installFakeBackend,
  type FakeBackendHandle,
  type FakeHandler,
} from '@/lib/testing/fake-backend'
import { useAuthStore } from '@/stores/auth'
import type { CurrentUser } from '@/types/auth'

/**
 * Opening a delegation window (FR-IDM-006, #184).
 *
 * The assertions worth having here are about what the form **cannot** do: name
 * somebody else as the delegator, offer a scope the engine refuses, or send a
 * window that is already over.
 */

function listBody(data: unknown[]): unknown {
  return { success: true, data, meta: { page: 1, pageSize: 100, total: data.length } }
}

const people = [
  {
    id: 'u-ani',
    username: 'ani',
    email: 'ani@example.com',
    displayName: 'Ani Wijaya',
    status: 'ACTIVE',
    departmentId: null,
    mustChangePassword: false,
    lastLoginAt: null,
    lockedUntil: null,
    createdAt: '2026-01-01T00:00:00Z',
    roles: [],
  },
  {
    id: 'u-budi',
    username: 'budi',
    email: 'budi@example.com',
    displayName: 'Budi Santoso',
    status: 'ACTIVE',
    departmentId: null,
    mustChangePassword: false,
    lastLoginAt: null,
    lockedUntil: null,
    createdAt: '2026-01-01T00:00:00Z',
    roles: [],
  },
  {
    id: 'u-gone',
    username: 'gone',
    email: 'gone@example.com',
    displayName: 'Dedi Kurnia',
    status: 'INACTIVE',
    departmentId: null,
    mustChangePassword: false,
    lastLoginAt: null,
    lockedUntil: null,
    createdAt: '2026-01-01T00:00:00Z',
    roles: [],
  },
]

/** Far enough ahead that the "already ended" rule never fires by accident. */
function soon(days: number): string {
  const at = new Date(Date.now() + days * 86_400_000)

  // What `datetime-local` produces: no zone, minute precision.
  return at.toISOString().slice(0, 16)
}

function signIn(): void {
  const user: CurrentUser = {
    id: 'u-ani',
    username: 'ani',
    displayName: 'Ani Wijaya',
    email: 'ani@example.com',
    roles: ['APPROVER'],
    permissions: ['identity:delegation:create', 'identity:user:read'],
  }

  useAuthStore().user = user
}

async function mountDialog(): Promise<VueWrapper> {
  signIn()

  const wrapper = mount(DelegationFormDialog, { props: { open: true } })
  await flushPromises()

  return wrapper
}

async function fill(wrapper: VueWrapper, values: Record<string, string>): Promise<void> {
  for (const [id, value] of Object.entries(values)) {
    await wrapper.find(`#${id}`).setValue(value)
  }
}

describe('DelegationFormDialog', () => {
  let backend: FakeBackendHandle
  let handler: FakeHandler

  beforeEach(() => {
    setActivePinia(createPinia())
    window.localStorage.clear()

    handler = (request) =>
      request.url.startsWith('/identity/users')
        ? { status: 200, body: listBody(people) }
        : { status: 200, body: listBody([]) }

    backend = installFakeBackend((request) => handler(request))
  })

  afterEach(() => {
    backend.restore()
    registerSessionBridge(null)
  })

  it('has no field for whose work is being delegated', async () => {
    // The security property, asserted as an absence. A holder of
    // `identity:delegation:create` who could name somebody else would be able to
    // point their approvals at themselves, and the row would look legitimate.
    const wrapper = await mountDialog()

    expect(wrapper.text()).not.toContain('Delegator')
    expect(wrapper.find('#delegation-delegator').exists()).toBe(false)
  })

  it('offers nobody who cannot take the work, including yourself', async () => {
    const wrapper = await mountDialog()
    const options = wrapper.find('#delegation-delegate').findAll('option')
    const labels = options.map((option) => option.text())

    expect(labels).toContain('Budi Santoso')
    expect(labels).not.toContain('Ani Wijaya')
    expect(labels).not.toContain('Dedi Kurnia')
  })

  it('offers only the two scopes the engine can honour', async () => {
    // `ROLE` is in the column's vocabulary and the API refuses it: a window
    // redirects a task that resolves to a person, and a role task has no
    // assignee to redirect. Offering it would be a control the product declines.
    const wrapper = await mountDialog()
    const values = wrapper
      .find('#delegation-scope')
      .findAll('option')
      .map((option) => (option.element as HTMLOptionElement).value)

    expect(values).toEqual(['ALL', 'DOCUMENT_TYPE'])
  })

  it('refuses a window that has already ended, before sending it', async () => {
    // A year typed wrong. Stored, it is cover somebody believes is in place.
    const wrapper = await mountDialog()

    await fill(wrapper, {
      'delegation-delegate': 'u-budi',
      'delegation-starts': soon(-30),
      'delegation-ends': soon(-20),
    })
    await wrapper.find('form').trigger('submit')
    await flushPromises()

    expect(wrapper.text()).toContain('This window has already ended')
    expect(backend.requests.some((request) => request.method === 'post')).toBe(false)
  })

  it('refuses a window that ends before it starts', async () => {
    const wrapper = await mountDialog()

    await fill(wrapper, {
      'delegation-delegate': 'u-budi',
      'delegation-starts': soon(10),
      'delegation-ends': soon(2),
    })
    await wrapper.find('form').trigger('submit')
    await flushPromises()

    expect(wrapper.text()).toContain('It has to end after it starts')
    expect(backend.requests.some((request) => request.method === 'post')).toBe(false)
  })

  it('asks which type a narrowed window covers', async () => {
    const wrapper = await mountDialog()

    await fill(wrapper, {
      'delegation-delegate': 'u-budi',
      'delegation-starts': soon(1),
      'delegation-ends': soon(8),
      'delegation-scope': 'DOCUMENT_TYPE',
    })
    await wrapper.find('form').trigger('submit')
    await flushPromises()

    expect(wrapper.text()).toContain('Choose the type it covers')
  })

  it('sends the window as instants, with no delegator', async () => {
    const wrapper = await mountDialog()

    await fill(wrapper, {
      'delegation-delegate': 'u-budi',
      'delegation-starts': soon(1),
      'delegation-ends': soon(8),
      'delegation-reason': '  Annual leave  ',
    })

    handler = (request) =>
      request.method === 'post'
        ? { status: 201, body: { success: true, data: { id: 'd-9' } } }
        : { status: 200, body: listBody(people) }

    await wrapper.find('form').trigger('submit')
    await flushPromises()

    const posted = backend.requests.find((request) => request.method === 'post')
    const body = posted?.body as Record<string, unknown>

    expect(posted?.url).toBe('/identity/delegations')
    expect(body.delegateUserId).toBe('u-budi')
    expect(body).not.toHaveProperty('delegatorUserId')
    // `datetime-local` carries no zone; the API takes an instant, and the
    // conversion is what makes the window mean what the person entering it meant.
    expect(String(body.startsAt)).toMatch(/Z$/)
    expect(body.reason).toBe('Annual leave')
    expect(wrapper.emitted('saved')).toBeTruthy()
  })

  it("keeps the dialog open and shows the server's refusal", async () => {
    const wrapper = await mountDialog()

    await fill(wrapper, {
      'delegation-delegate': 'u-budi',
      'delegation-starts': soon(1),
      'delegation-ends': soon(8),
    })

    handler = (request) =>
      request.method === 'post'
        ? {
            status: 422,
            body: errorBody('VALIDATION_ERROR', 'Validation failed', [
              {
                path: 'delegateUserId',
                rule: 'exists',
                code: 'NOT_AVAILABLE',
                message: 'no active user with that id in this tenant',
              },
            ]),
          }
        : { status: 200, body: listBody(people) }

    await wrapper.find('form').trigger('submit')
    await flushPromises()

    expect(wrapper.emitted('saved')).toBeFalsy()
    expect(wrapper.text()).toContain('no active user with that id in this tenant')
  })
})
