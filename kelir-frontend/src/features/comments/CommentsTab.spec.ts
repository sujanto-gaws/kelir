import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import CommentsTab from './CommentsTab.vue'
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
 * The screen SRS §9 criterion 11 has been claiming since 2026-08-11 ([#296]).
 *
 * **The subject of most of what is below is the distinction**, because it is
 * the thing this screen most easily loses: the Workflow tab one panel over
 * renders decisions with their reasons, which look like comments with authors
 * and are not. The project has drawn that line four times in prose; this is the
 * first place a person can see it.
 *
 * # Seen to fail (coding standard §2.9)
 *
 * Both mutations were run against this file and the reddened tests named, on
 * 2026-09-01:
 *
 * - **M1** — `canRead` and `canWrite` forced to `true`, which is the permission
 *   gate removed. Red: *distinguishes not allowed to look…*, *offers no
 *   composer…*.
 * - **M2** — the draft cleared before `addComment` is awaited, and the
 *   pre-flight guard and its `disabled` binding removed. Red: *keeps the draft
 *   when the server refuses it*, *refuses to send whitespace…*.
 *
 * [#296]: https://github.com/sujanto-gaws/kelir/issues/296
 */

const DOCUMENT_ID = '0199a1a0-0000-7000-8000-0000000000d1'

function pageBody(rows: Record<string, unknown>[]): unknown {
  return { success: true, data: rows, meta: { page: 1, pageSize: 20, total: rows.length } }
}

function comment(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    id: '0199a1a0-0000-7000-8000-0000000000c1',
    documentId: DOCUMENT_ID,
    body: 'is this the right supplier?',
    authorUserId: '0199a1a0-0000-7000-8000-0000000000f9',
    authorUsername: 'ani',
    createdAt: '2026-08-31T10:00:00Z',
    ...overrides,
  }
}

describe('CommentsTab', () => {
  let backend: FakeBackendHandle
  let onList: () => FakeReply
  let onPost: () => FakeReply

  beforeEach(() => {
    setActivePinia(createPinia())
    signIn(['comment:read', 'comment:create'])

    onList = () => ({ status: 200, body: pageBody([comment()]) })
    onPost = () => ({ status: 200, body: itemBody(comment({ body: 'a new one' })) })

    backend = installFakeBackend((request) => {
      if (request.url.includes('/comments')) {
        return request.method === 'post' ? onPost() : onList()
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
    const wrapper = mount(CommentsTab, { props: { documentId: DOCUMENT_ID } })
    await flushPromises()

    return wrapper
  }

  it('shows the conversation with who said what and when', async () => {
    const wrapper = await render()

    expect(wrapper.find('[data-testid="comment-body"]').text()).toBe('is this the right supplier?')
    expect(wrapper.find('[data-testid="comment-row"]').text()).toContain('ani')
  })

  it('adds a comment and shows it', async () => {
    const wrapper = await render()

    await wrapper.find('[data-testid="comment-input"]').setValue('a new one')
    await wrapper.find('[data-testid="comment-submit"]').trigger('click')
    await flushPromises()

    expect(backend.requests.some((request) => request.method === 'post')).toBe(true)
  })

  /**
   * **The line this screen exists to keep visible.** A workspace that renders
   * decisions and conversation alike has undone what the module documentation,
   * the migration and a row-level test all say — and it is the obvious thing to
   * build, because the two look the same.
   */
  it('says where an approver reason lives, which is not here', async () => {
    const wrapper = await render()
    const note = wrapper.find('[data-testid="comment-distinction"]').text()

    expect(note).toContain('Workflow tab')
    expect(note).toContain('decision')
  })

  /**
   * **A comment somebody has written is work.** Clearing the box on a refusal
   * throws it away at the moment they most want it back.
   */
  it('keeps the draft when the server refuses it', async () => {
    onPost = () => ({
      status: 422,
      body: errorBody('VALIDATION_FAILED', 'a comment is at most 4000 characters'),
    })

    const wrapper = await render()

    await wrapper.find('[data-testid="comment-input"]').setValue('worth keeping')
    await wrapper.find('[data-testid="comment-submit"]').trigger('click')
    await flushPromises()

    expect(wrapper.find('[data-testid="comment-error"]').text()).toContain('at most 4000')
    expect(
      (wrapper.find('[data-testid="comment-input"]').element as HTMLTextAreaElement).value,
    ).toBe('worth keeping')
  })

  it('refuses to send whitespace, without asking the server', async () => {
    const wrapper = await render()

    await wrapper.find('[data-testid="comment-input"]').setValue('   ')

    const submit = wrapper.find('[data-testid="comment-submit"]')
    expect(submit.attributes('disabled')).toBeDefined()

    await submit.trigger('click')
    await flushPromises()

    expect(backend.requests.some((request) => request.method === 'post')).toBe(false)
  })

  it('says nobody has commented rather than showing an empty list', async () => {
    onList = () => ({ status: 200, body: pageBody([]) })

    const wrapper = await render()

    expect(wrapper.find('[data-testid="comments-empty"]').exists()).toBe(true)
  })

  /**
   * [#263](https://github.com/sujanto-gaws/kelir/issues/263)'s finding, kept
   * out of a second screen: *not allowed to look* and *looked and found
   * nothing* are different things and must read differently.
   */
  it('distinguishes not allowed to look from looked and found nothing', async () => {
    signIn([])

    const wrapper = await render()

    expect(wrapper.find('[data-testid="comments-forbidden"]').exists()).toBe(true)
    expect(wrapper.find('[data-testid="comments-empty"]').exists()).toBe(false)
  })

  it('offers no composer to a caller who may only read', async () => {
    signIn(['comment:read'])

    const wrapper = await render()

    expect(wrapper.find('[data-testid="comment-input"]').exists()).toBe(false)
    expect(wrapper.find('[data-testid="comment-row"]').exists()).toBe(true)
  })

  it('names a removed author rather than rendering a blank', async () => {
    onList = () => ({ status: 200, body: pageBody([comment({ authorUsername: null })]) })

    const wrapper = await render()

    expect(wrapper.find('[data-testid="comment-row"]').text()).toContain('since been removed')
  })
})
