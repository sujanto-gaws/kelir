import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import NotificationCentrePage from './NotificationCentrePage.vue'
import { useAuthStore } from '@/stores/auth'
import type { CurrentUser } from '@/types/auth'
import {
  errorBody,
  installFakeBackend,
  itemBody,
  type FakeBackendHandle,
  type FakeReply,
} from '@/lib/testing/fake-backend'

/**
 * The in-app notification centre (FR-NTF-003; [#251]).
 *
 * # Seen to fail (coding standard §2.9)
 *
 * Every mutation below was run against this file and the reddened test named,
 * on 2026-09-01:
 *
 * - **F1** — `markRead`'s catch arm dropped, so a refused request leaves the
 *   row stamped. Red: *puts a row back when the server refuses it*.
 * - **F2** — `open` stops calling `markRead`. Red: *following a notification
 *   marks it read*.
 * - **F3** — `load` replaces instead of appending. Red: *appends the next page*.
 * - **F4** — the `notifications-not-inbox` note deleted. Red: *says which
 *   screen answers whether a task is still open*.
 * - **F5** — `open` prefers `documentId` over `taskId`. Red: *follows a task
 *   notification to the task*.
 *
 * [#251]: https://github.com/sujanto-gaws/kelir/issues/251
 */

const TASK_ID = '0199a1a0-0000-7000-8000-0000000000t1'.replace(/t/g, '7')
const DOCUMENT_ID = '0199a1a0-0000-7000-8000-0000000000d1'.replace(/d/g, '6')

const push = vi.fn()

vi.mock('vue-router', () => ({
  useRouter: () => ({ push }),
}))

function pageBody(rows: Record<string, unknown>[], total = rows.length): unknown {
  return { success: true, data: rows, meta: { page: 1, pageSize: 20, total } }
}

function notification(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    id: '0199a1a0-0000-7000-8000-000000000011',
    documentId: DOCUMENT_ID,
    workflowInstanceId: null,
    taskId: TASK_ID,
    notificationType: 'TASK_ASSIGNED',
    title: 'Approve the request',
    body: 'Approve the request is waiting for you. Open your inbox to act on it.',
    readAt: null,
    createdAt: '2026-08-31T10:00:00Z',
    ...overrides,
  }
}

describe('NotificationCentrePage', () => {
  let backend: FakeBackendHandle
  let onList: (page: number) => FakeReply
  let onRead: () => FakeReply
  let onReadAll: () => FakeReply

  beforeEach(() => {
    setActivePinia(createPinia())
    push.mockReset()
    signIn(['notification:read'])

    onList = () => ({ status: 200, body: pageBody([notification()]) })
    onRead = () => ({ status: 204 })
    onReadAll = () => ({ status: 200, body: itemBody({ unread: 0 }) })

    backend = installFakeBackend((request) => {
      if (request.url.endsWith('/notifications/read')) {
        return onReadAll()
      }

      if (request.url.endsWith('/read')) {
        return onRead()
      }

      if (request.url.includes('/notifications/unread-count')) {
        return { status: 200, body: itemBody({ unread: 1 }) }
      }

      if (request.url.includes('/notifications')) {
        return onList(Number(request.params.page ?? 1))
      }

      return { status: 404, body: errorBody('NOT_FOUND', 'no') }
    })
  })

  afterEach(() => {
    backend.restore()
  })

  function signIn(permissions: string[]): void {
    const user: CurrentUser = {
      id: '0199a1a0-0000-7000-8000-0000000000f9',
      username: 'ani',
      displayName: 'Ani Wijaya',
      email: 'ani@example.com',
      roles: ['REQUESTER'],
      permissions,
    }

    useAuthStore().user = user
  }

  async function render(): Promise<VueWrapper> {
    const wrapper = mount(NotificationCentrePage)
    await flushPromises()

    return wrapper
  }

  it('shows what came, in the product’s own words', async () => {
    const wrapper = await render()

    expect(wrapper.find('[data-testid="notification-title"]').text()).toBe('Approve the request')
    expect(wrapper.find('[data-testid="notification-type"]').text()).toBe('Task')
    expect(wrapper.find('[data-testid="notification-row"]').attributes('data-unread')).toBe('true')
  })

  /**
   * **Following it is having read it** — a badge that survived the click is a
   * badge nobody trusts.
   *
   * **Seen red (F2)** with `open` no longer calling `markRead`.
   */
  it('following a notification marks it read', async () => {
    const wrapper = await render()

    await wrapper.find('[data-testid="notification-open"]').trigger('click')
    await flushPromises()

    expect(
      backend.requests.some(
        (request) => request.method === 'post' && request.url.endsWith('/read'),
      ),
    ).toBe(true)
    expect(wrapper.find('[data-testid="notification-row"]').attributes('data-unread')).toBe('false')
  })

  /**
   * **A task notification goes to the task**, which is where acting happens.
   *
   * **Seen red (F5)** with `open` preferring `documentId`: the click lands on
   * the document, one screen away from the thing the person was told to do.
   */
  it('follows a task notification to the task', async () => {
    const wrapper = await render()

    await wrapper.find('[data-testid="notification-open"]').trigger('click')
    await flushPromises()

    expect(push).toHaveBeenCalledWith({ name: 'task', params: { id: TASK_ID } })
  })

  /** A decision has no task of its own, so it goes to the document. */
  it('follows a decision notification to the document', async () => {
    onList = () => ({
      status: 200,
      body: pageBody([
        notification({ taskId: null, notificationType: 'DOCUMENT_DECIDED', title: 'Approved' }),
      ]),
    })

    const wrapper = await render()

    await wrapper.find('[data-testid="notification-open"]').trigger('click')
    await flushPromises()

    expect(push).toHaveBeenCalledWith({ name: 'document', params: { id: DOCUMENT_ID } })
  })

  /**
   * **A refused mark-read puts the row back** rather than leaving a lie on the
   * screen. The stamp is optimistic because a list that waits for a round trip
   * to acknowledge a click feels broken; the correction is what makes that
   * safe.
   *
   * **Seen red (F1)** with the catch arm dropped: the row stays read and the
   * person believes something they were not told.
   */
  it('puts a row back when the server refuses it', async () => {
    onRead = () => ({ status: 500, body: errorBody('INTERNAL', 'the database is away') })

    const wrapper = await render()

    await wrapper.find('[data-testid="notification-mark-read"]').trigger('click')
    await flushPromises()

    expect(wrapper.find('[data-testid="notification-row"]').attributes('data-unread')).toBe('true')
    expect(wrapper.find('[data-testid="notifications-error"]').exists()).toBe(true)
  })

  /**
   * **Appending, not replacing.** Newest first, so appending walks backwards
   * through what has happened to you — replacing takes away the recent rows the
   * reader came for.
   *
   * **Seen red (F3)** with `load` replacing.
   */
  it('appends the next page', async () => {
    onList = (page) =>
      page === 2
        ? { status: 200, body: pageBody([notification({ id: 'older', title: 'Older' })], 2) }
        : { status: 200, body: pageBody([notification({ id: 'newer', title: 'Newer' })], 2) }

    const wrapper = await render()
    expect(wrapper.findAll('[data-testid="notification-row"]')).toHaveLength(1)

    await wrapper.find('[data-testid="notifications-more"]').trigger('click')
    await flushPromises()

    const rows = wrapper.findAll('[data-testid="notification-row"]')
    expect(rows).toHaveLength(2)
    expect(rows[0].text()).toContain('Newer')
    expect(rows[1].text()).toContain('Older')
  })

  it('clears everything and offers the control only while something is unread', async () => {
    const wrapper = await render()

    expect(wrapper.find('[data-testid="notifications-mark-all"]').exists()).toBe(true)

    await wrapper.find('[data-testid="notifications-mark-all"]').trigger('click')
    await flushPromises()

    expect(wrapper.find('[data-testid="notification-row"]').attributes('data-unread')).toBe('false')
    expect(wrapper.find('[data-testid="notifications-mark-all"]').exists()).toBe(false)
  })

  /**
   * **The distinction this screen would otherwise blur.** A list of things that
   * reached you looks like a list of things to do, and a role task's
   * notification is stale the moment somebody else claims it (**D-48**).
   *
   * **Seen red (F4)** with the note deleted.
   */
  it('says which screen answers whether a task is still open', async () => {
    const wrapper = await render()
    const note = wrapper.find('[data-testid="notifications-not-inbox"]').text()

    expect(note).toContain('My Tasks')
  })

  it('distinguishes nothing-yet from could-not-be-read', async () => {
    onList = () => ({ status: 200, body: pageBody([]) })
    const empty = await render()
    expect(empty.find('[data-testid="notifications-empty"]').exists()).toBe(true)
    expect(empty.find('[data-testid="notifications-error"]').exists()).toBe(false)

    onList = () => ({ status: 500, body: errorBody('INTERNAL', 'the database is away') })
    const failed = await render()
    expect(failed.find('[data-testid="notifications-error"]').exists()).toBe(true)
    expect(failed.find('[data-testid="notifications-empty"]').exists()).toBe(false)
  })
})
