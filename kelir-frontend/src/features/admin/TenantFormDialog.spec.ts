import { createPinia, setActivePinia } from 'pinia'
import { flushPromises, mount, type DOMWrapper, type VueWrapper } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import TenantFormDialog from './TenantFormDialog.vue'
import { registerSessionBridge } from '@/api/session'
import {
  errorBody,
  installFakeBackend,
  itemBody,
  validationReply,
  type FakeBackendHandle,
  type FakeHandler,
} from '@/lib/testing/fake-backend'
import type { Tenant } from '@/types/organization'

const acme: Tenant = {
  id: 't-2',
  tenantCode: 'ACME',
  name: 'Acme Limited',
  status: 'ACTIVE',
  isDefault: false,
  userCount: 12,
  createdAt: '2026-08-20T00:00:00Z',
}

const platform: Tenant = { ...acme, id: 't-1', tenantCode: 'SYSTEM', isDefault: true }

function mountDialog(tenant: Tenant | null): VueWrapper {
  return mount(TenantFormDialog, { props: { tenant, open: true } })
}

function buttonLabelled(
  buttons: DOMWrapper<HTMLButtonElement>[],
  label: string,
): DOMWrapper<HTMLButtonElement> | undefined {
  return buttons.find((button) => button.text() === label)
}

async function fillAdministrator(wrapper: VueWrapper): Promise<void> {
  await wrapper.find('#tenant-admin-username').setValue('acme.admin')
  await wrapper.find('#tenant-admin-email').setValue('admin@acme.example')
  await wrapper.find('#tenant-admin-display-name').setValue('Acme Administrator')
  await wrapper.find('#tenant-admin-password').setValue('a-sufficiently-long-password')
}

async function submit(wrapper: VueWrapper): Promise<void> {
  await wrapper.find('form').trigger('submit')
  await flushPromises()
}

describe('TenantFormDialog', () => {
  let backend: FakeBackendHandle
  let handler: FakeHandler

  beforeEach(() => {
    setActivePinia(createPinia())

    handler = () => ({ status: 200, body: itemBody(acme) })
    backend = installFakeBackend((request) => handler(request))
  })

  afterEach(() => {
    backend.restore()
    registerSessionBridge(null)
  })

  describe('creating', () => {
    it('asks for the first administrator alongside the tenant', async () => {
      // A tenant with no user is a row nobody can sign in to — the state this
      // whole surface was held back to avoid (D-13, answered by D-18). The
      // backend creates both in one transaction, so the form collects both.
      const wrapper = mountDialog(null)

      expect(wrapper.find('#tenant-admin-username').exists()).toBe(true)
      expect(wrapper.find('#tenant-admin-password').exists()).toBe(true)
    })

    it('sends the code upper-cased, as the backend stores it', async () => {
      // Codes normalise server-side either way; matching that here means the
      // value the user just typed and the value they will be told to sign in
      // with are the same string.
      const wrapper = mountDialog(null)

      await wrapper.find('#tenant-code').setValue('  tnt-001 ')
      await wrapper.find('#tenant-name').setValue('Acme Limited')
      await fillAdministrator(wrapper)
      await submit(wrapper)

      const created = backend.requests.find((request) => request.method === 'post')
      expect(created?.body).toMatchObject({
        tenantCode: 'TNT-001',
        name: 'Acme Limited',
        administrator: { username: 'acme.admin', displayName: 'Acme Administrator' },
      })
    })

    it('refuses a code carrying punctuation a caller cannot see', async () => {
      // The value is read out over the phone and typed into a login form, so
      // two codes differing only by a space are a support call.
      const wrapper = mountDialog(null)

      await wrapper.find('#tenant-code').setValue('TNT 001')
      await wrapper.find('#tenant-name').setValue('Acme Limited')
      await fillAdministrator(wrapper)
      await submit(wrapper)

      expect(wrapper.find('#tenant-code-error').text()).toContain('letters, digits, dashes')
      expect(backend.requests).toHaveLength(0)
    })

    it('reports every missing administrator field at once', async () => {
      // One round trip should be enough to fix a form (JSON Form Schema S10.3).
      const wrapper = mountDialog(null)

      await wrapper.find('#tenant-code').setValue('TNT-001')
      await wrapper.find('#tenant-name').setValue('Acme Limited')
      await submit(wrapper)

      expect(wrapper.find('#tenant-admin-username-error').exists()).toBe(true)
      expect(wrapper.find('#tenant-admin-email-error').exists()).toBe(true)
      expect(wrapper.find('#tenant-admin-display-name-error').exists()).toBe(true)
      expect(wrapper.find('#tenant-admin-password-error').exists()).toBe(true)
      expect(backend.requests).toHaveLength(0)
    })

    it('binds the nested validation paths the backend sends to their inputs', async () => {
      // The request nests the administrator, so the 422 details do too
      // (`administrator.password`). A detail that named `password` would have
      // nothing to bind to and the message would never be shown.
      handler = () => ({
        status: 422,
        body: errorBody('VALIDATION_ERROR', 'Validation failed', [
          {
            path: 'administrator.password',
            rule: 'minLength',
            code: 'TOO_SHORT',
            message: 'Password must be at least 12 characters',
          },
        ]),
      })
      const wrapper = mountDialog(null)

      await wrapper.find('#tenant-code').setValue('TNT-001')
      await wrapper.find('#tenant-name').setValue('Acme Limited')
      await fillAdministrator(wrapper)
      await submit(wrapper)

      expect(wrapper.find('#tenant-admin-password-error').text()).toBe(
        'Password must be at least 12 characters',
      )
    })

    it('puts a duplicate tenant code against the field that collided', async () => {
      // A 409 carries no details, and a duplicate is a property of a field
      // rather than of the request — a form-level message would leave the
      // offending input looking valid.
      handler = () => ({
        status: 409,
        body: errorBody('CONFLICT', 'That tenant code is already in use'),
      })
      const wrapper = mountDialog(null)

      await wrapper.find('#tenant-code').setValue('TNT-001')
      await wrapper.find('#tenant-name').setValue('Acme Limited')
      await fillAdministrator(wrapper)
      await submit(wrapper)

      expect(wrapper.find('#tenant-code-error').text()).toBe('That tenant code is already in use')
    })
  })

  describe('editing', () => {
    it('does not offer the code or an administrator', async () => {
      // The code is what users sign in with and no session carries it, so
      // changing it would strand them with nothing failing loudly. The backend
      // refuses the field; the form does not present it.
      const wrapper = mountDialog(acme)

      expect((wrapper.find('#tenant-code').element as HTMLInputElement).disabled).toBe(true)
      expect(wrapper.find('#tenant-admin-username').exists()).toBe(false)
      expect(wrapper.text()).toContain('Fixed once the tenant exists')
    })

    it('sends only the name and status', async () => {
      const wrapper = mountDialog(acme)

      await wrapper.find('#tenant-name').setValue('Acme Holdings')
      await submit(wrapper)

      const updated = backend.requests.find((request) => request.method === 'put')
      expect(updated?.body).toEqual({ name: 'Acme Holdings', status: 'ACTIVE' })
    })

    it('says what taking a tenant offline does, and how soon (D-104)', async () => {
      // Read before the status is touched, so it must not promise more than
      // happens: somebody already signed in carries on for up to 15 minutes.
      const wrapper = mountDialog(acme)

      expect(wrapper.text()).toContain(
        'While it is offline its users cannot sign in, and anyone already signed in is signed out within 15 minutes.',
      )
      expect(wrapper.text()).not.toContain('16 minutes')
      expect(wrapper.text()).not.toContain('ends its users’ sessions')
    })

    it('says how many people a suspension shuts out, and how soon (D-104)', async () => {
      const wrapper = mountDialog(acme)

      await wrapper.find('#tenant-status').setValue('SUSPENDED')

      expect(wrapper.text()).toContain(
        'Its 12 user(s) cannot sign in until it is active. Anyone already signed in is signed out within 15 minutes.',
      )
      expect(wrapper.text()).not.toContain('16 minutes')
      expect(wrapper.text()).not.toContain('will be signed out')
    })

    it('will not take the administering tenant offline', async () => {
      // The backend answers 400 for this, and being refused is a worse way to
      // learn it than a control that is visibly unavailable.
      const wrapper = mountDialog(platform)

      expect((wrapper.find('#tenant-status').element as HTMLSelectElement).disabled).toBe(true)
      expect(wrapper.text()).toContain('cannot be taken offline')
    })

    it('keeps the dialog open and shows a refusal verbatim', async () => {
      handler = () => ({ status: 403, body: errorBody('FORBIDDEN', 'Access denied') })
      const wrapper = mountDialog(acme)

      await wrapper.find('#tenant-name').setValue('Acme Holdings')
      await submit(wrapper)

      expect(wrapper.find('[role="alert"]').text()).toContain('Access denied')
      expect(buttonLabelled(wrapper.findAll('button'), 'Save changes')).toBeDefined()
    })
  })

  describe('a 422 detail no input reads (#576)', () => {
    const UNPLACED = '[data-testid="tenant-unplaced-errors"]'

    function listed(wrapper: VueWrapper): string[] {
      return wrapper
        .find(UNPLACED)
        .findAll('li')
        .map((item) => item.text())
    }

    async function submitNew(wrapper: VueWrapper): Promise<void> {
      await wrapper.find('#tenant-code').setValue('TNT-001')
      await wrapper.find('#tenant-name').setValue('Acme Limited')
      await fillAdministrator(wrapper)
      await submit(wrapper)
    }

    it('lists it on the form, and announces it', async () => {
      // `administrator.locale` and `colour` are no fields of this form, and
      // before #576 the refused save left every input looking valid.
      handler = () =>
        validationReply(
          ['administrator.email', 'Email is not valid'],
          ['administrator.locale', 'Unknown field'],
          ['colour', 'Unknown field'],
        )
      const wrapper = mountDialog(null)

      await submitNew(wrapper)

      expect(listed(wrapper)).toEqual([
        'administrator.locale: Unknown field',
        'colour: Unknown field',
      ])
      expect(wrapper.find(UNPLACED).attributes('role')).toBe('alert')
      // The detail an input reads is under that input, and only there.
      expect(wrapper.find('#tenant-admin-email-error').text()).toBe('Email is not valid')
      expect(wrapper.emitted('saved')).toBeUndefined()
    })

    it('lists a status or an administrator detail when editing', async () => {
      // The status has a control but nowhere for a message, and no
      // administrator is asked for once the tenant exists.
      handler = () =>
        validationReply(
          ['status', 'A tenant cannot be archived here'],
          ['administrator.password', 'Password is too short'],
        )
      const wrapper = mountDialog(acme)

      await submit(wrapper)

      expect(wrapper.find('#tenant-admin-password-error').exists()).toBe(false)
      expect(listed(wrapper)).toEqual([
        'status: A tenant cannot be archived here',
        'administrator.password: Password is too short',
      ])
    })

    it('does not list a detail an input shows', async () => {
      handler = () =>
        validationReply(
          ['tenantCode', 'Tenant code is reserved'],
          ['name', 'Name is too long'],
          ['administrator.username', 'Username is not valid'],
          ['administrator.email', 'Email is not valid'],
          ['administrator.displayName', 'Display name is too long'],
          ['administrator.password', 'Password is too short'],
        )
      const wrapper = mountDialog(null)

      await submitNew(wrapper)

      expect(wrapper.find('#tenant-code-error').text()).toBe('Tenant code is reserved')
      expect(wrapper.find('#tenant-admin-password-error').text()).toBe('Password is too short')
      expect(wrapper.find(UNPLACED).exists()).toBe(false)
    })

    it('keeps a 422 with no details, and a denial, as the one message on the form', async () => {
      const wrapper = mountDialog(acme)

      handler = () => validationReply()
      await submit(wrapper)

      expect(wrapper.findAll('[role="alert"]').map((alert) => alert.text())).toEqual([
        'Validation failed',
      ])
      expect(wrapper.find(UNPLACED).exists()).toBe(false)

      handler = () => ({ status: 403, body: errorBody('FORBIDDEN', 'Access denied') })
      await submit(wrapper)

      expect(wrapper.findAll('[role="alert"]').map((alert) => alert.text())).toEqual([
        'Access denied',
      ])
      expect(wrapper.find(UNPLACED).exists()).toBe(false)
    })
  })
})
