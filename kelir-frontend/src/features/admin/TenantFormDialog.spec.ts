import { createPinia, setActivePinia } from 'pinia'
import { flushPromises, mount, type DOMWrapper, type VueWrapper } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import TenantFormDialog from './TenantFormDialog.vue'
import { registerSessionBridge } from '@/api/session'
import {
  errorBody,
  installFakeBackend,
  itemBody,
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

    it('says how many people a suspension signs out', async () => {
      const wrapper = mountDialog(acme)

      await wrapper.find('#tenant-status').setValue('SUSPENDED')

      expect(wrapper.text()).toContain('Its 12 user(s) will be signed out')
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
})
