import { createPinia, setActivePinia } from 'pinia'
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import ExternalSystemFormDialog from './ExternalSystemFormDialog.vue'
import IntegrationCredentialFormDialog from './IntegrationCredentialFormDialog.vue'
import IntegrationEndpointFormDialog from './IntegrationEndpointFormDialog.vue'
import { registerSessionBridge } from '@/api/session'
import {
  installFakeBackend,
  validationReply,
  type FakeBackendHandle,
  type FakeReply,
} from '@/lib/testing/fake-backend'
import type {
  ExternalSystem,
  IntegrationCredential,
  IntegrationEndpoint,
} from '@/types/integration'

/**
 * The independent campaign on #576, for the three integration dialogs that
 * were moved onto `FormUnplacedErrors`.
 *
 * As in the admin campaign, **nothing here reads a dialog's `PLACED` list**:
 * each harness names the paths its template shows a message for, read off the
 * markup, and every detail sent has to be on screen exactly once.
 */

const erp: ExternalSystem = {
  id: 'es-1',
  systemCode: 'ERP',
  systemName: 'Enterprise resource planning',
  systemType: null,
  baseUrl: 'https://erp.example',
  authType: null,
  timeoutSeconds: 30,
  retryPolicy: {},
  description: null,
  status: 'ACTIVE',
  createdAt: '2026-09-01T00:00:00Z',
  updatedAt: '2026-09-01T00:00:00Z',
}

const apiKey: IntegrationCredential = {
  id: 'cr-1',
  externalSystemId: erp.id,
  credentialType: 'API_KEY',
  secretReference: 'vault://kelir/erp/api-key',
  validFrom: null,
  validTo: null,
  isActive: true,
  createdAt: '2026-09-01T00:00:00Z',
  updatedAt: '2026-09-01T00:00:00Z',
}

const createOrder: IntegrationEndpoint = {
  id: 'ep-1',
  externalSystemId: erp.id,
  endpointCode: 'CREATE_PO',
  name: 'Create purchase order',
  method: 'POST',
  path: '/purchase-orders',
  description: null,
  status: 'ACTIVE',
  createdAt: '2026-09-01T00:00:00Z',
  updatedAt: '2026-09-01T00:00:00Z',
}

interface Harness {
  /** The dialog and the mode it is drawn in. */
  name: string
  /** The test id of the dialog's form, and of its list. */
  form: string
  testId: string
  open(): VueWrapper
  /** Paths the template shows a message for: each in `[data-testid="<path>-error"]`. */
  slots: string[]
  /** Paths the template shows no message for, in this mode. */
  elsewhere: string[]
}

const SYSTEM_SLOTS = [
  'systemCode',
  'systemName',
  'systemType',
  'baseUrl',
  'authType',
  'timeoutSeconds',
  'retryPolicy.maxRetries',
  'retryPolicy.initialDelaySeconds',
  'retryPolicy.backoffMultiplier',
  'retryPolicy.deadLetterAfterAttempts',
  'description',
]

const SYSTEM_ELSEWHERE = ['retryPolicy', 'retryPolicy.jitter', 'maxRetries', 'BaseUrl', 'colour']

const CREDENTIAL_SLOTS = ['credentialType', 'secretReference', 'validFrom', 'validTo']

const CREDENTIAL_ELSEWHERE = ['externalSystemId', 'SecretReference', 'secretReference.0', 'colour']

const ENDPOINT_SLOTS = ['endpointCode', 'name', 'method', 'path']

const ENDPOINT_ELSEWHERE = ['status', 'externalSystemId', 'Path', 'path.0', 'colour']

function openSystem(editing: ExternalSystem | null): VueWrapper {
  return mount(ExternalSystemFormDialog, { props: { open: true, editing } })
}

function openCredential(editing: IntegrationCredential | null): VueWrapper {
  return mount(IntegrationCredentialFormDialog, {
    props: { open: true, systemId: erp.id, editing },
  })
}

function openEndpoint(editing: IntegrationEndpoint | null): VueWrapper {
  return mount(IntegrationEndpointFormDialog, {
    props: { open: true, systemId: erp.id, editing },
  })
}

const harnesses: Harness[] = [
  {
    name: 'ExternalSystemFormDialog, registering',
    form: 'external-system-form',
    testId: 'external-system-unplaced-errors',
    open: () => openSystem(null),
    slots: SYSTEM_SLOTS,
    elsewhere: SYSTEM_ELSEWHERE,
  },
  {
    name: 'ExternalSystemFormDialog, editing',
    form: 'external-system-form',
    testId: 'external-system-unplaced-errors',
    open: () => openSystem(erp),
    slots: [...SYSTEM_SLOTS, 'status'],
    elsewhere: SYSTEM_ELSEWHERE,
  },
  {
    name: 'IntegrationCredentialFormDialog, adding',
    form: 'credential-form',
    testId: 'credential-unplaced-errors',
    open: () => openCredential(null),
    slots: CREDENTIAL_SLOTS,
    elsewhere: CREDENTIAL_ELSEWHERE,
  },
  {
    name: 'IntegrationCredentialFormDialog, editing',
    form: 'credential-form',
    testId: 'credential-unplaced-errors',
    open: () => openCredential(apiKey),
    slots: CREDENTIAL_SLOTS,
    elsewhere: CREDENTIAL_ELSEWHERE,
  },
  {
    name: 'IntegrationEndpointFormDialog, adding',
    form: 'endpoint-form',
    testId: 'endpoint-unplaced-errors',
    open: () => openEndpoint(null),
    slots: ENDPOINT_SLOTS,
    elsewhere: ENDPOINT_ELSEWHERE,
  },
  {
    name: 'IntegrationEndpointFormDialog, editing',
    form: 'endpoint-form',
    testId: 'endpoint-unplaced-errors',
    open: () => openEndpoint(createOrder),
    slots: ENDPOINT_SLOTS,
    elsewhere: ENDPOINT_ELSEWHERE,
  },
]

describe('a 422 detail in the integration dialogs: the independent campaign (#576)', () => {
  let backend: FakeBackendHandle
  let saveReply: FakeReply

  beforeEach(() => {
    setActivePinia(createPinia())
    window.localStorage.clear()

    saveReply = validationReply()
    backend = installFakeBackend(() => saveReply)
  })

  afterEach(() => {
    backend.restore()
    registerSessionBridge(null)
  })

  async function submit(wrapper: VueWrapper, harness: Harness): Promise<void> {
    await wrapper.get(`[data-testid="${harness.form}"]`).trigger('submit')
    await flushPromises()
  }

  function list(wrapper: VueWrapper, harness: Harness) {
    return wrapper.find(`[data-testid="${harness.testId}"]`)
  }

  function listed(wrapper: VueWrapper, harness: Harness): string[] {
    return list(wrapper, harness)
      .findAll('li')
      .map((item) => item.text())
  }

  /** How many times `message` is on screen anywhere in the dialog. */
  function shown(wrapper: VueWrapper, message: string): number {
    return wrapper.text().split(message).length - 1
  }

  /** Two digits, so no message is the start of another. */
  function refusal(index: number): string {
    return `refusal-${String(index).padStart(2, '0')}.`
  }

  describe.each(harnesses)('$name', (harness) => {
    it('shows every detail exactly once: under its input, or in the list', async () => {
      const paths = [...harness.slots, ...harness.elsewhere]

      saveReply = validationReply(
        ...paths.map((path, index): [string, string] => [path, refusal(index)]),
      )
      const wrapper = harness.open()

      await submit(wrapper, harness)

      expect(backend.requests).toHaveLength(1)

      // Once each, wherever it is: not dropped, and not said twice.
      expect(paths.filter((path, index) => shown(wrapper, refusal(index)) !== 1)).toEqual([])

      for (const path of harness.slots) {
        expect(wrapper.get(`[data-testid="${path}-error"]`).text()).toBe(
          refusal(paths.indexOf(path)),
        )
      }

      // In the order the server sent them, under the test id and the role the
      // dialog's own markup had before it was moved onto the shared component.
      expect(listed(wrapper, harness)).toEqual(
        harness.elsewhere.map((path) => `${path}: ${refusal(paths.indexOf(path))}`),
      )
      expect(list(wrapper, harness).attributes('role')).toBe('alert')
      expect(wrapper.emitted('saved')).toBeUndefined()
    })

    it('draws no list when every detail has an input to sit under', async () => {
      saveReply = validationReply(
        ...harness.slots.map((path, index): [string, string] => [path, refusal(index)]),
      )
      const wrapper = harness.open()

      await submit(wrapper, harness)

      expect(list(wrapper, harness).exists()).toBe(false)
    })

    it('drops the list when the dialog is closed and opened again', async () => {
      saveReply = validationReply(['colour', 'Unknown field'])
      const wrapper = harness.open()

      await submit(wrapper, harness)
      expect(listed(wrapper, harness)).toEqual(['colour: Unknown field'])

      await wrapper.setProps({ open: false })
      await wrapper.setProps({ open: true })
      await flushPromises()

      expect(list(wrapper, harness).exists()).toBe(false)
    })
  })

  /**
   * Three paths a dialog counts as placed and shows no message for. Each is
   * written as the behaviour #576 asks for and marked `fails`: it is red today,
   * and goes red again, as a passing test under `fails`, once the dialog is
   * fixed, which is the cue to drop the marker.
   */
  describe('a path counted as placed that no message is shown for (defects, open)', () => {
    async function refused(harness: Harness, path: string): Promise<VueWrapper> {
      saveReply = validationReply([path, 'The server refused this'])
      const wrapper = harness.open()

      await submit(wrapper, harness)

      return wrapper
    }

    it.fails('credential: an `isActive` detail is shown', async () => {
      const wrapper = await refused(harnesses[2], 'isActive')

      expect(shown(wrapper, 'The server refused this')).toBe(1)
    })

    it.fails('endpoint: a `description` detail is shown', async () => {
      const wrapper = await refused(harnesses[4], 'description')

      expect(shown(wrapper, 'The server refused this')).toBe(1)
    })

    it.fails('external system: a `status` detail is shown when registering', async () => {
      const wrapper = await refused(harnesses[0], 'status')

      expect(shown(wrapper, 'The server refused this')).toBe(1)
    })
  })
})
