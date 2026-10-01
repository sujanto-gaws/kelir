import { createPinia, setActivePinia } from 'pinia'
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import DelegationFormDialog from './DelegationFormDialog.vue'
import { registerSessionBridge } from '@/api/session'
import {
  errorBody,
  installFakeBackend,
  validationReply,
  type FakeBackendHandle,
  type FakeHandler,
  type FakeReply,
} from '@/lib/testing/fake-backend'
import { filler, searchedPage } from '@/lib/testing/searched-page'
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
        ? searchedPage(request, people, ['username', 'email', 'displayName'])
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

    expect(labels).toContain('Budi Santoso (budi)')
    expect(labels).not.toContain('Ani Wijaya (ani)')
    expect(labels).not.toContain('Dedi Kurnia (gone)')
    // Who cannot sign in is a column, so the server is asked to leave them out.
    expect(backend.requests.find((request) => request.url === '/identity/users')?.params).toEqual({
      pageSize: 100,
      status: 'ACTIVE',
    })
  })

  it('reaches, by search, somebody who sorts past the first hundred', async () => {
    // #525: the chooser read one page of a hundred, so whoever sorted after it
    // could not be picked. 120 people sort ahead of the one wanted here.
    const crowd = filler(120, (n) => ({
      ...people[1],
      id: `u-${n}`,
      username: `aa${n}`,
      displayName: `Crowd ${n}`,
    }))
    const wanted = {
      ...people[1],
      id: 'u-zul',
      username: 'zulkifli',
      displayName: 'Zulkifli Hasan',
    }
    handler = (request) =>
      request.url === '/identity/users'
        ? searchedPage(request, [...crowd, wanted], ['username', 'email', 'displayName'])
        : { status: 200, body: listBody([]) }

    const wrapper = await mountDialog()
    const choices = () =>
      wrapper
        .find('#delegation-delegate')
        .findAll('option')
        .map((option) => option.text())

    expect(choices()).not.toContain('Zulkifli Hasan (zulkifli)')
    expect(wrapper.text()).toContain('Showing 100 of 121')

    await wrapper.find('#delegation-delegate-search').setValue('zulk')
    await wrapper.find('#delegation-delegate-search').trigger('keydown', { key: 'Enter' })
    await flushPromises()

    expect(choices()).toEqual(['Choose somebody', 'Zulkifli Hasan (zulkifli)'])
    expect(backend.requests.slice(-1)[0]?.params).toEqual({
      search: 'zulk',
      pageSize: 100,
      status: 'ACTIVE',
    })

    await wrapper.find('#delegation-delegate').setValue('u-zul')
    await fill(wrapper, { 'delegation-starts': soon(1), 'delegation-ends': soon(8) })
    handler = () => ({ status: 201, body: { success: true, data: { id: 'd-1' } } })
    await wrapper.find('form').trigger('submit')
    await flushPromises()

    const posted = backend.requests.find((request) => request.method === 'post')
    expect((posted?.body as Record<string, unknown>).delegateUserId).toBe('u-zul')
  })

  it('reaches, by search, a document type that sorts past the first hundred', async () => {
    const types = [
      ...filler(110, (n) => ({
        id: `t-${n}`,
        typeCode: `AA${n}`,
        name: `Filler ${n}`,
        status: 'ACTIVE',
      })),
      { id: 't-zz', typeCode: 'ZZ_LAST', name: 'Last of all', status: 'ACTIVE' },
    ]
    handler = (request) =>
      request.url === '/document-types'
        ? searchedPage(request, types, ['typeCode', 'name'])
        : searchedPage(request, people, ['username', 'email', 'displayName'])

    const wrapper = await mountDialog()
    await fill(wrapper, { 'delegation-scope': 'DOCUMENT_TYPE' })
    await flushPromises()

    expect(wrapper.find('#delegation-type').findAll('option')).toHaveLength(101)

    await wrapper.find('#delegation-type-search').setValue('zz_l')
    await wrapper.find('#delegation-type-search').trigger('keydown', { key: 'Enter' })
    await flushPromises()

    expect(
      wrapper
        .find('#delegation-type')
        .findAll('option')
        .map((option) => option.text()),
    ).toEqual(['Choose a type', 'Last of all (ZZ_LAST)'])

    await wrapper.find('#delegation-type').setValue('t-zz')
    await fill(wrapper, {
      'delegation-delegate': 'u-budi',
      'delegation-starts': soon(1),
      'delegation-ends': soon(8),
    })
    handler = () => ({ status: 201, body: { success: true, data: { id: 'd-2' } } })
    await wrapper.find('form').trigger('submit')
    await flushPromises()

    const posted = backend.requests.find((request) => request.method === 'post')
    expect((posted?.body as Record<string, unknown>).documentTypeId).toBe('t-zz')
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

  describe('a 422 detail no input reads (#576)', () => {
    const UNPLACED = '[data-testid="delegation-unplaced-errors"]'

    function listed(wrapper: VueWrapper): string[] {
      return wrapper
        .find(UNPLACED)
        .findAll('li')
        .map((item) => item.text())
    }

    const travel = { id: 't-travel', typeCode: 'TRAVEL', name: 'Travel request', status: 'ACTIVE' }

    /** Answers the save with `reply`, and every chooser read as before. */
    function saveAnswers(reply: FakeReply): void {
      handler = (request) =>
        request.method === 'post'
          ? reply
          : request.url === '/document-types'
            ? searchedPage(request, [travel], ['typeCode', 'name'])
            : searchedPage(request, people, ['username', 'email', 'displayName'])
    }

    async function submitWindow(wrapper: VueWrapper): Promise<void> {
      await fill(wrapper, {
        'delegation-delegate': 'u-budi',
        'delegation-starts': soon(1),
        'delegation-ends': soon(8),
      })
      await wrapper.find('form').trigger('submit')
      await flushPromises()
    }

    it('lists it on the form, and announces it', async () => {
      // The scope and the reason have controls but nowhere for a message, and
      // `colour` is no field of this form at all.
      saveAnswers(
        validationReply(
          ['endsAt', 'The window is too long'],
          ['scope', 'ROLE windows are not supported'],
          ['reason', 'Reason is too long'],
          ['colour', 'Unknown field'],
        ),
      )
      const wrapper = await mountDialog()

      await submitWindow(wrapper)

      expect(listed(wrapper)).toEqual([
        'scope: ROLE windows are not supported',
        'reason: Reason is too long',
        'colour: Unknown field',
      ])
      expect(wrapper.find(UNPLACED).attributes('role')).toBe('alert')
      // The detail an input reads is under that input, and only there.
      expect(wrapper.find('#delegation-ends-error').text()).toBe('The window is too long')
      expect(wrapper.emitted('saved')).toBeFalsy()
    })

    it('lists a document type detail while the window covers everything', async () => {
      // The type chooser is drawn only for a narrowed window, so while the
      // scope is ALL nothing on screen would read this.
      saveAnswers(validationReply(['documentTypeId', 'No such document type']))
      const wrapper = await mountDialog()

      await submitWindow(wrapper)

      expect(wrapper.find('#delegation-type-error').exists()).toBe(false)
      expect(listed(wrapper)).toEqual(['documentTypeId: No such document type'])
    })

    it('does not list a detail an input shows', async () => {
      saveAnswers(
        validationReply(
          ['delegateUserId', 'No such user'],
          ['documentTypeId', 'No such document type'],
        ),
      )
      const wrapper = await mountDialog()

      await fill(wrapper, { 'delegation-scope': 'DOCUMENT_TYPE' })
      await flushPromises()
      await wrapper.find('#delegation-type').setValue('t-travel')
      await submitWindow(wrapper)

      expect(wrapper.find('#delegation-delegate-error').text()).toBe('No such user')
      expect(wrapper.find('#delegation-type-error').text()).toBe('No such document type')
      expect(wrapper.find(UNPLACED).exists()).toBe(false)
    })

    it('keeps a 422 with no details, and a denial, as the one message on the form', async () => {
      const wrapper = await mountDialog()

      saveAnswers(validationReply())
      await submitWindow(wrapper)

      expect(wrapper.findAll('[role="alert"]').map((alert) => alert.text())).toEqual([
        'Validation failed',
      ])
      expect(wrapper.find(UNPLACED).exists()).toBe(false)

      saveAnswers({ status: 403, body: errorBody('FORBIDDEN', 'Access denied') })
      await submitWindow(wrapper)

      expect(wrapper.findAll('[role="alert"]').map((alert) => alert.text())).toEqual([
        'Access denied',
      ])
      expect(wrapper.find(UNPLACED).exists()).toBe(false)
    })
  })
})
