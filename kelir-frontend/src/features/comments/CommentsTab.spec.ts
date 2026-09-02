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
 * The tail ([#253]) added three more, run 2026-09-01:
 *
 * - **M3** — `threads` flattened to one row per comment, the nesting dropped.
 *   Red: *reads a reply under the comment it answers*, *offers no reply control
 *   on a reply*, *says a deleted comment was deleted…* — three, because every
 *   claim about a thread is a claim about the nesting.
 * - **M4** — `deleted()` forced to `false`, so a tombstone renders as a comment
 *   with no text. Red: *says a deleted comment was deleted rather than showing
 *   an empty one*.
 * - **M5** — `mine()` forced to `true`, which is the authorship gate removed.
 *   Red: *offers no edit or delete on somebody elses comment*.
 *
 * [#253]: https://github.com/sujanto-gaws/kelir/issues/253
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
    parentCommentId: null,
    authorUserId: '0199a1a0-0000-7000-8000-0000000000f9',
    authorUsername: 'ani',
    createdAt: '2026-08-31T10:00:00Z',
    editedAt: null,
    deletedAt: null,
    ...overrides,
  }
}

/** A comment somebody else wrote — the author id is the one thing that differs. */
function theirs(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return comment({
    id: '0199a1a0-0000-7000-8000-0000000000c9',
    authorUserId: '0199a1a0-0000-7000-8000-0000000000e1',
    authorUsername: 'budi',
    ...overrides,
  })
}

/** A reply to the comment above, as the API returns it: after its root. */
function replyRow(): Record<string, unknown> {
  return comment({
    id: '0199a1a0-0000-7000-8000-0000000000c2',
    body: 'yes, they are approved',
    parentCommentId: '0199a1a0-0000-7000-8000-0000000000c1',
    authorUserId: '0199a1a0-0000-7000-8000-0000000000e1',
    authorUsername: 'budi',
  })
}

describe('CommentsTab', () => {
  let backend: FakeBackendHandle
  let onList: () => FakeReply
  let onPost: () => FakeReply
  let onPut: () => FakeReply
  let onDelete: () => FakeReply

  beforeEach(() => {
    setActivePinia(createPinia())
    signIn(['comment:read', 'comment:create'])

    onList = () => ({ status: 200, body: pageBody([comment()]) })
    onPost = () => ({ status: 200, body: itemBody(comment({ body: 'a new one' })) })
    onPut = () => ({ status: 200, body: itemBody(comment({ body: 'reconsidered' })) })
    onDelete = () => ({ status: 204, body: undefined })

    backend = installFakeBackend((request) => {
      if (request.url.includes('/comments')) {
        if (request.method === 'post') {
          return onPost()
        }

        if (request.method === 'put') {
          return onPut()
        }

        return request.method === 'delete' ? onDelete() : onList()
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
  // -------------------------------------------------------------------------
  // #253 — the tail: replies, edits, deletes, and who may do them
  // -------------------------------------------------------------------------

  /**
   * **The order is the server's and the nesting is this screen's.** The API
   * returns a root then what answers it; drawing the reply at the top level
   * would lose the one thing threading is for.
   */
  it('reads a reply under the comment it answers', async () => {
    onList = () => ({ status: 200, body: pageBody([comment(), replyRow()]) })

    const wrapper = await render()

    expect(wrapper.findAll('[data-testid="comment-row"]')).toHaveLength(1)

    const replies = wrapper.findAll('[data-testid="comment-reply-row"]')
    expect(replies).toHaveLength(1)
    expect(replies[0].text()).toContain('yes, they are approved')
  })

  it('sends a reply with the comment it answers, from the same endpoint', async () => {
    const wrapper = await render()

    await wrapper.find('[data-testid="comment-reply"]').trigger('click')
    await wrapper.find('[data-testid="reply-input"]').setValue('yes, they are approved')
    await wrapper.find('[data-testid="reply-submit"]').trigger('click')
    await flushPromises()

    const posted = backend.requests.find((request) => request.method === 'post')

    expect(posted?.body).toMatchObject({
      body: 'yes, they are approved',
      parentCommentId: '0199a1a0-0000-7000-8000-0000000000c1',
    })
  })

  /**
   * **One level** (D-50). The server refuses a reply to a reply, and a control
   * whose only outcome is a 422 is worse than no control.
   */
  it('offers no reply control on a reply', async () => {
    onList = () => ({ status: 200, body: pageBody([comment(), replyRow()]) })

    const wrapper = await render()

    const reply = wrapper.find('[data-testid="comment-reply-row"]')
    expect(reply.find('[data-testid="comment-reply"]').exists()).toBe(false)
  })

  it('edits a comment through the comment it is about', async () => {
    signIn(['comment:read', 'comment:create', 'comment:update'])

    const wrapper = await render()

    await wrapper.find('[data-testid="comment-edit"]').trigger('click')
    await wrapper.find('[data-testid="edit-input"]').setValue('is this still the right supplier?')
    await wrapper.find('[data-testid="edit-submit"]').trigger('click')
    await flushPromises()

    const sent = backend.requests.find((request) => request.method === 'put')

    expect(sent?.url).toContain('/comments/0199a1a0-0000-7000-8000-0000000000c1')
    expect(sent?.body).toEqual({ body: 'is this still the right supplier?' })
  })

  it('marks a comment that has been edited, and says nothing about one that has not', async () => {
    const untouched = await render()
    expect(untouched.find('[data-testid="comment-edited"]').exists()).toBe(false)

    onList = () => ({
      status: 200,
      body: pageBody([comment({ editedAt: '2026-09-01T09:00:00Z' })]),
    })

    const wrapper = await render()
    expect(wrapper.find('[data-testid="comment-edited"]').text()).toContain('edited')
  })

  /**
   * **A tombstone is not an empty comment** (D-51). It says what happened to the
   * words that were there, and the replies under it are still on the screen —
   * which is the whole reason it is still on the screen itself.
   */
  it('says a deleted comment was deleted rather than showing an empty one', async () => {
    onList = () => ({
      status: 200,
      body: pageBody([comment({ body: null, deletedAt: '2026-09-01T09:00:00Z' }), replyRow()]),
    })

    const wrapper = await render()

    const row = wrapper.find('[data-testid="comment-row"]')
    expect(row.find('[data-testid="comment-deleted"]').text()).toContain('deleted')
    expect(row.find('[data-testid="comment-body"]').exists()).toBe(false)
    expect(wrapper.find('[data-testid="comment-reply-row"]').text()).toContain(
      'yes, they are approved',
    )
  })

  /**
   * **The ask is in the page, not in a browser dialog**, and it says what
   * survives: somebody deleting a comment that has been answered needs to know
   * the answers stay.
   */
  it('asks before deleting, and sends nothing until the second click', async () => {
    signIn(['comment:read', 'comment:create', 'comment:delete'])

    const wrapper = await render()

    await wrapper.find('[data-testid="comment-delete"]').trigger('click')
    await flushPromises()

    expect(backend.requests.some((request) => request.method === 'delete')).toBe(false)
    expect(wrapper.find('[data-testid="delete-ask"]').text()).toContain('replies')

    await wrapper.find('[data-testid="comment-delete-confirm"]').trigger('click')
    await flushPromises()

    expect(backend.requests.some((request) => request.method === 'delete')).toBe(true)
  })

  it('keeps the comment when the ask is declined', async () => {
    signIn(['comment:read', 'comment:create', 'comment:delete'])

    const wrapper = await render()

    await wrapper.find('[data-testid="comment-delete"]').trigger('click')
    await wrapper.find('[data-testid="comment-delete-cancel"]').trigger('click')
    await flushPromises()

    expect(backend.requests.some((request) => request.method === 'delete')).toBe(false)
    expect(wrapper.find('[data-testid="comment-delete"]').exists()).toBe(true)
  })

  /**
   * **Authorship is not a permission, and the screen asks both questions.** The
   * server refuses either way; offering controls that always answer 403 would be
   * a screen lying about what this person can do.
   */
  it('offers no edit or delete on somebody elses comment', async () => {
    signIn(['comment:read', 'comment:create', 'comment:update', 'comment:delete'])
    onList = () => ({ status: 200, body: pageBody([theirs()]) })

    const wrapper = await render()

    expect(wrapper.find('[data-testid="comment-edit"]').exists()).toBe(false)
    expect(wrapper.find('[data-testid="comment-delete"]').exists()).toBe(false)
    expect(wrapper.find('[data-testid="comment-reply"]').exists()).toBe(true)
  })

  it('offers no edit to an author who may not update', async () => {
    signIn(['comment:read', 'comment:create', 'comment:delete'])

    const wrapper = await render()

    expect(wrapper.find('[data-testid="comment-edit"]').exists()).toBe(false)
    expect(wrapper.find('[data-testid="comment-delete"]').exists()).toBe(true)
  })

  it('keeps the edit when the server refuses it', async () => {
    signIn(['comment:read', 'comment:create', 'comment:update'])
    onPut = () => ({
      status: 422,
      body: errorBody('VALIDATION_FAILED', 'a comment is at most 4000 characters'),
    })

    const wrapper = await render()

    await wrapper.find('[data-testid="comment-edit"]').trigger('click')
    await wrapper.find('[data-testid="edit-input"]').setValue('worth keeping')
    await wrapper.find('[data-testid="edit-submit"]').trigger('click')
    await flushPromises()

    expect(wrapper.find('[data-testid="edit-error"]').text()).toContain('at most 4000')
    expect((wrapper.find('[data-testid="edit-input"]').element as HTMLTextAreaElement).value).toBe(
      'worth keeping',
    )
  })

  /**
   * The distinction note is where the tail is most easily misread: a
   * conversation whose entries can be edited sits one tab from decisions whose
   * reasons never can, and the note now says both halves.
   */
  it('says a comment can be edited and an approver reason cannot', async () => {
    const wrapper = await render()
    const note = wrapper.find('[data-testid="comment-distinction"]').text()

    expect(note).toContain('edit')
    expect(note).toContain('cannot be changed')
  })
})
