import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import type { AxiosResponse, InternalAxiosRequestConfig } from 'axios'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'

import ListsPage from './ListsPage.vue'
import { apiClient } from '@/api/client'
import { registerSessionBridge } from '@/api/session'
import {
  errorBody,
  installFakeBackend,
  itemBody,
  pageBody,
  type FakeBackendHandle,
  type FakeReply,
  type RecordedRequest,
} from '@/lib/testing/fake-backend'
import { useAuthStore } from '@/stores/auth'
import type { CurrentUser } from '@/types/auth'

/**
 * The create form on the list of lists: where a refusal is shown (#641).
 *
 * `listKey` and `title` are placed under their inputs. The form sends one
 * fixed column and no filters, so a `columns.*` or `pageSize` detail has no
 * input here and is listed above the form, as is any unknown field. The
 * details are `rad::domain::list`'s (`bounded`, `check_page_size`,
 * `check_columns`) and `extract::detail`'s.
 */

type Detail = [path: string, rule: string, code: string, message: string]

function refusal(...details: Detail[]): FakeReply {
  return {
    status: 422,
    body: errorBody(
      'VALIDATION_ERROR',
      'Validation failed',
      details.map(([path, rule, code, message]) => ({ path, rule, code, message })),
    ),
  }
}

const KEY_TOO_LONG: Detail = [
  'listKey',
  'maxLength',
  'TOO_LONG',
  'listKey must be at most 64 characters',
]
const TITLE_REQUIRED: Detail = ['title', 'required', 'REQUIRED', 'title is required']
const COLUMN_LABEL_TOO_LONG: Detail = [
  'columns.0.label',
  'maxLength',
  'TOO_LONG',
  'columns.0.label must be at most 200 characters',
]
const PAGE_SIZE_OUT_OF_RANGE: Detail = [
  'pageSize',
  'range',
  'OUT_OF_RANGE',
  'pageSize must be between 1 and 100',
]
const UNKNOWN_FIELD: Detail = [
  'unknownField',
  'unknownField',
  'UNKNOWN_FIELD',
  'Unknown field `unknownField`. Check the spelling and casing — the API is camelCase.',
]

const LIST_ID = '0199a1a0-0000-7000-8000-0000000000c1'
const blank = { template: '<div />' }

const created = {
  id: LIST_ID,
  listKey: 'purchase_requisitions',
  title: 'Purchase requisitions',
  entityId: null,
  defaultSort: null,
  pageSize: 20,
  status: 'ACTIVE',
  columns: [{ columnKey: 'title', label: 'Title', isSortable: true }],
  filters: [],
  createdAt: '2026-11-04T00:00:00Z',
  updatedAt: '2026-11-04T00:00:00Z',
}

function principal(permissions: string[]): CurrentUser {
  return {
    id: '0199a1a0-0000-7000-8000-0000000000u1',
    username: 'admin',
    displayName: 'Administrator',
    email: 'admin@example.test',
    roles: [],
    permissions,
  }
}

describe('ListsPage create form: where a refusal is shown (#641)', () => {
  let backend: FakeBackendHandle
  /** What `POST /rad/lists` answers. */
  let createReply: FakeReply
  let router: Router

  beforeEach(() => {
    setActivePinia(createPinia())
    window.localStorage.clear()

    createReply = { status: 201, body: itemBody(created) }
    backend = installFakeBackend((request: RecordedRequest) =>
      request.method === 'post' ? createReply : { status: 200, body: pageBody([]) },
    )

    router = createRouter({
      history: createMemoryHistory(),
      routes: [
        { path: '/admin/lists', name: 'admin-lists', component: blank },
        { path: '/admin/lists/:id', name: 'admin-list-builder', component: blank },
      ],
    })
  })

  afterEach(() => {
    backend.restore()
    registerSessionBridge(null)
  })

  async function render(): Promise<VueWrapper> {
    useAuthStore().$patch({ user: principal(['rad:list:read', 'rad:list:create']) })

    await router.push('/admin/lists')
    await router.isReady()

    const wrapper = mount(ListsPage, { global: { plugins: [router] } })

    await flushPromises()
    await wrapper.get('[data-testid="new-list"]').trigger('click')

    return wrapper
  }

  async function create(wrapper: VueWrapper): Promise<void> {
    await wrapper.get('[data-testid="list-key"]').setValue('purchase_requisitions')
    await wrapper.get('[data-testid="list-title"]').setValue('Purchase requisitions')
    await wrapper.get('[data-testid="list-create-dialog"] form').trigger('submit')
    await flushPromises()
  }

  function list(wrapper: VueWrapper) {
    return wrapper.find('[data-testid="list-create-unplaced-errors"]')
  }

  function listed(wrapper: VueWrapper): string[] {
    return list(wrapper)
      .findAll('li')
      .map((item) => item.text())
  }

  function blockOf(wrapper: VueWrapper, testId: string): string {
    return wrapper.get(`[data-testid="${testId}"]`).element.parentElement?.textContent ?? ''
  }

  function shown(wrapper: VueWrapper, message: string): number {
    return wrapper.text().split(message).length - 1
  }

  /** Still on the list of lists, with the dialog open: nothing was created. */
  function stayed(wrapper: VueWrapper): void {
    expect(router.currentRoute.value.name).toBe('admin-lists')
    expect(wrapper.find('[data-testid="list-create-dialog"]').exists()).toBe(true)
  }

  it('lists a detail at a path it shows no message for, and does not create', async () => {
    createReply = refusal(UNKNOWN_FIELD)
    const wrapper = await render()

    await create(wrapper)

    expect(list(wrapper).attributes('role')).toBe('alert')
    expect(listed(wrapper)).toEqual([`unknownField: ${UNKNOWN_FIELD[3]}`])
    stayed(wrapper)
  })

  // The form has no input for the column it seeds or for the page size.
  it('lists a refusal of the seeded column and of the page size', async () => {
    createReply = refusal(COLUMN_LABEL_TOO_LONG, PAGE_SIZE_OUT_OF_RANGE)
    const wrapper = await render()

    await create(wrapper)

    expect(listed(wrapper)).toEqual([
      `columns.0.label: ${COLUMN_LABEL_TOO_LONG[3]}`,
      `pageSize: ${PAGE_SIZE_OUT_OF_RANGE[3]}`,
    ])
  })

  it.each([
    ['key', 'list-key', KEY_TOO_LONG],
    ['title', 'list-title', TITLE_REQUIRED],
  ] as const)('places a refusal of the %s under its input', async (_, testId, detail) => {
    createReply = refusal(detail)
    const wrapper = await render()

    await create(wrapper)

    expect(blockOf(wrapper, testId)).toContain(detail[3])
    expect(shown(wrapper, detail[3])).toBe(1)
    expect(list(wrapper).exists()).toBe(false)
  })

  it('shows a placed and an unplaced detail once each, each in its own place', async () => {
    createReply = refusal(KEY_TOO_LONG, UNKNOWN_FIELD)
    const wrapper = await render()

    await create(wrapper)

    expect(blockOf(wrapper, 'list-key')).toContain(KEY_TOO_LONG[3])
    expect(shown(wrapper, KEY_TOO_LONG[3])).toBe(1)
    expect(listed(wrapper)).toEqual([`unknownField: ${UNKNOWN_FIELD[3]}`])
  })

  it('does not also say "Validation failed" when the details say why', async () => {
    createReply = refusal(TITLE_REQUIRED, UNKNOWN_FIELD)
    const wrapper = await render()

    await create(wrapper)

    expect(wrapper.find('[data-testid="list-create-error"]').exists()).toBe(false)
    expect(wrapper.text()).not.toContain('Validation failed')
  })

  it.each([
    ['a 422 naming no field', refusal(), 'Validation failed'],
    [
      'a 403',
      {
        status: 403,
        body: errorBody('FORBIDDEN', 'You do not have permission to perform this action'),
      },
      'You do not have permission to perform this action',
    ],
    [
      'a 409 for a key that is taken',
      {
        status: 409,
        body: errorBody('CONFLICT', 'a list definition with this key already exists'),
      },
      'a list definition with this key already exists',
    ],
    [
      'a network failure',
      { status: 0, networkError: true },
      'Could not reach the server. Check your connection and try again.',
    ],
  ] as [string, FakeReply, string][])(
    'shows %s as one message above the form, and no list',
    async (_, failure, message) => {
      createReply = failure
      const wrapper = await render()

      await create(wrapper)

      expect(wrapper.get('[data-testid="list-create-error"]').text()).toBe(message)
      expect(shown(wrapper, message)).toBe(1)
      expect(list(wrapper).exists()).toBe(false)
      stayed(wrapper)
    },
  )

  it('opens again without the last refusal, placed or listed', async () => {
    createReply = refusal(TITLE_REQUIRED, UNKNOWN_FIELD)
    const wrapper = await render()

    await create(wrapper)
    expect(list(wrapper).exists()).toBe(true)

    await wrapper.get('[data-testid="list-create-dialog"] button[type="button"]').trigger('click')
    await wrapper.get('[data-testid="new-list"]').trigger('click')

    expect(list(wrapper).exists()).toBe(false)
    expect(shown(wrapper, TITLE_REQUIRED[3])).toBe(0)
  })

  it('opens again without the last message above the form', async () => {
    createReply = { status: 403, body: errorBody('FORBIDDEN', 'Not yours to create') }
    const wrapper = await render()

    await create(wrapper)
    expect(wrapper.find('[data-testid="list-create-error"]').exists()).toBe(true)

    await wrapper.get('[data-testid="list-create-dialog"] button[type="button"]').trigger('click')
    await wrapper.get('[data-testid="new-list"]').trigger('click')

    expect(wrapper.find('[data-testid="list-create-error"]').exists()).toBe(false)
  })

  it('clears the last refusal when Create is pressed again', async () => {
    createReply = refusal(TITLE_REQUIRED, UNKNOWN_FIELD)
    const wrapper = await render()

    await create(wrapper)
    expect(list(wrapper).exists()).toBe(true)

    // Refused again, at a different path: only the second refusal is shown.
    createReply = refusal(KEY_TOO_LONG)
    await create(wrapper)

    expect(list(wrapper).exists()).toBe(false)
    expect(shown(wrapper, TITLE_REQUIRED[3])).toBe(0)
    expect(blockOf(wrapper, 'list-key')).toContain(KEY_TOO_LONG[3])
  })

  it('creates and opens the builder, sending the same request as before', async () => {
    const wrapper = await render()

    await create(wrapper)

    const post = backend.requests.find((request) => request.method === 'post')

    expect(post?.url).toBe('/rad/lists')
    expect(post?.body).toEqual({
      listKey: 'purchase_requisitions',
      title: 'Purchase requisitions',
      columns: [{ columnKey: 'title', label: 'Title', isSortable: true }],
      filters: [],
    })
    expect(router.currentRoute.value.name).toBe('admin-list-builder')
  })

  // test-engineer, #641 AC10. The case above clears through the second
  // refusal's own report; this holds the answer back, so it is Create itself
  // that clears. Without it the last refusal stays beside "Creating…" until
  // the server answers.
  it('clears the last refusal as soon as Create is pressed again', async () => {
    createReply = refusal(TITLE_REQUIRED, UNKNOWN_FIELD)
    const wrapper = await render()

    await create(wrapper)
    expect(list(wrapper).exists()).toBe(true)

    const fake = apiClient.defaults.adapter as (
      config: InternalAxiosRequestConfig,
    ) => Promise<AxiosResponse>
    let release!: () => void
    const gate = new Promise<void>((resolve) => {
      release = resolve
    })

    apiClient.defaults.adapter = async (config: InternalAxiosRequestConfig) => {
      if (config.method === 'post') {
        await gate
      }

      return fake(config)
    }
    createReply = refusal(KEY_TOO_LONG)

    await wrapper.get('[data-testid="list-create-dialog"] form').trigger('submit')
    await flushPromises()

    expect(wrapper.get('[data-testid="save-list"]').text()).toBe('Creating…')
    expect(list(wrapper).exists()).toBe(false)
    expect(shown(wrapper, TITLE_REQUIRED[3])).toBe(0)

    release()
    await flushPromises()

    expect(blockOf(wrapper, 'list-key')).toContain(KEY_TOO_LONG[3])
  })
})
