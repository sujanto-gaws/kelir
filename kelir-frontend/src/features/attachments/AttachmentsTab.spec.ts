import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import AttachmentsTab from './AttachmentsTab.vue'
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
 * The screen SRS §9 criterion 6 has been claiming since 2026-08-11 ([#295]).
 *
 * Driven through the real API client, like every other screen spec: the fake
 * backend replaces the axios adapter and nothing else, so envelope unwrapping
 * and `ApiError` classification run for real.
 *
 * **Most of what is below is about the scan states**, because that is the part
 * a screen most easily flattens — three refusals rendered as one spinner is a
 * security control turned into a bug report, which is this item's second
 * acceptance criterion and #246 AC3's reason for distinguishing them at all.
 *
 * The tail ([#254]) added two more, run 2026-09-02:
 *
 * - **M3** — `safeHref` forced to return whatever it is given. Red: *refuses to
 *   put a javascript link in an href*.
 * - **M4** — `mine()` forced to `true`, the authorship gate removed. Red:
 *   *offers no delete on somebody elses file*.
 *
 * [#254]: https://github.com/sujanto-gaws/kelir/issues/254
 * [#295]: https://github.com/sujanto-gaws/kelir/issues/295
 */

const DOCUMENT_ID = '0199a1a0-0000-7000-8000-0000000000d1'

function pageBody(rows: Record<string, unknown>[]): unknown {
  return { success: true, data: rows, meta: { page: 1, pageSize: 20, total: rows.length } }
}

function attachment(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    id: '0199a1a0-0000-7000-8000-0000000000a1',
    documentId: DOCUMENT_ID,
    originalFileName: 'quotation 2026.pdf',
    mimeType: 'application/pdf',
    fileSize: 2048,
    checksum: 'sha256:abc',
    description: null,
    virusScanStatus: 'CLEAN',
    category: null,
    createdAt: '2026-08-31T10:00:00Z',
    createdBy: '0199a1a0-0000-7000-8000-0000000000f9',
    ...overrides,
  }
}

const QUOTATION = {
  id: '00000000-0000-0000-0003-000000000001',
  code: 'QUOTATION',
  name: 'Quotation',
  isSystem: true,
}

/** A link, as the API reports one — with none of a file's fields (#254 AC4). */
function reference(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    id: '0199a1a0-0000-7000-8000-0000000000b1',
    documentId: DOCUMENT_ID,
    label: 'Vendor portal',
    url: 'https://vendor.example.test/quotes/2026-11',
    description: null,
    category: null,
    createdAt: '2026-08-31T11:00:00Z',
    createdBy: '0199a1a0-0000-7000-8000-0000000000f9',
    ...overrides,
  }
}

describe('AttachmentsTab', () => {
  let backend: FakeBackendHandle
  let onList: () => FakeReply
  let onReferences: () => FakeReply
  let onCategories: () => FakeReply
  let onWrite: () => FakeReply

  beforeEach(() => {
    setActivePinia(createPinia())
    signIn(['attachment:read', 'attachment:create'])

    onList = () => ({ status: 200, body: pageBody([attachment()]) })
    onReferences = () => ({ status: 200, body: pageBody([]) })
    onCategories = () => ({ status: 200, body: pageBody([QUOTATION]) })
    onWrite = () => ({ status: 204, body: undefined })

    backend = installFakeBackend((request) => {
      if (request.url.includes('/attachment-categories')) {
        return onCategories()
      }

      if (request.url.includes('/references')) {
        return request.method === 'get' ? onReferences() : onWrite()
      }

      if (request.url.includes('/attachments')) {
        return request.method === 'get' ? onList() : onWrite()
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
    const wrapper = mount(AttachmentsTab, { props: { documentId: DOCUMENT_ID } })
    await flushPromises()

    return wrapper
  }

  /**
   * **The gate opening is a thing the screen has to notice** ([#326],
   * **D-63**).
   *
   * `AttachmentsTab` used to read the list once, in a `watch` on
   * `documentId`, and never again. So a file uploaded through this screen read
   * `Checking` until the person reloaded the page — while the worker had
   * cleared it, the row said `CLEAN` and the API said `CLEAN`. Found by
   * `a-file-and-a-conversation-on-a-document.spec.ts` on its first run.
   *
   * **It is worse than a stale badge**, which is why this test is here rather
   * than in the e2e suite alone: `SCAN_STATUS_EXPLANATIONS.PENDING` promises
   * the file *will be available to download shortly*, and the whole basis for
   * rendering three refusals rather than one spinner (#246 AC3) is that
   * `PENDING` resolves and the other two do not. A `PENDING` that never visibly
   * resolves is indistinguishable from a permanent refusal.
   *
   * [#326]: https://github.com/sujanto-gaws/kelir/issues/326
   */
  it('shows a scan that cleared without anybody reloading', async () => {
    vi.useFakeTimers()

    try {
      let reads = 0

      onList = () => {
        reads += 1

        return {
          status: 200,
          body: pageBody([attachment({ virusScanStatus: reads === 1 ? 'PENDING' : 'CLEAN' })]),
        }
      }

      const wrapper = mount(AttachmentsTab, { props: { documentId: DOCUMENT_ID } })
      await flushPromises()

      expect(wrapper.find('[data-testid="attachment-status"]').text()).toBe('Checking')
      expect(wrapper.find('[data-testid="attachment-download"]').exists()).toBe(false)

      await vi.advanceTimersByTimeAsync(3_000)
      await flushPromises()

      expect(wrapper.find('[data-testid="attachment-status"]').text()).toBe('Ready')
      expect(wrapper.find('[data-testid="attachment-download"]').exists()).toBe(true)
    } finally {
      vi.useRealTimers()
    }
  })

  /** The ordinary case pays nothing: nothing is pending, so nothing is asked. */
  it('asks again only while a scan is outstanding', async () => {
    vi.useFakeTimers()

    try {
      let reads = 0

      onList = () => {
        reads += 1

        return { status: 200, body: pageBody([attachment({ virusScanStatus: 'CLEAN' })]) }
      }

      mount(AttachmentsTab, { props: { documentId: DOCUMENT_ID } })
      await flushPromises()

      expect(reads).toBe(1)

      await vi.advanceTimersByTimeAsync(30_000)
      await flushPromises()

      expect(reads).toBe(1)
    } finally {
      vi.useRealTimers()
    }
  })

  /**
   * **And it stops once the answer arrives**, which is the half the ceiling
   * does not cover: a cleared scan should end the asking immediately, not leave
   * a timer running until the cap retires it.
   *
   * This test exists because the mutation that removed the stop came back
   * **green** — the three tests written before it covered the poll starting and
   * the ceiling, and nothing covered the ordinary end. A green mutation is a
   * finding (coding standard §2.9), and this is the finding.
   */
  it('stops asking as soon as the scan has cleared', async () => {
    vi.useFakeTimers()

    try {
      let reads = 0

      onList = () => {
        reads += 1

        return {
          status: 200,
          body: pageBody([attachment({ virusScanStatus: reads === 1 ? 'PENDING' : 'CLEAN' })]),
        }
      }

      mount(AttachmentsTab, { props: { documentId: DOCUMENT_ID } })
      await flushPromises()

      await vi.advanceTimersByTimeAsync(3_000)
      await flushPromises()

      // The clearing read, and no more.
      expect(reads).toBe(2)

      await vi.advanceTimersByTimeAsync(3_000 * 10)
      await flushPromises()

      expect(reads).toBe(2)
    } finally {
      vi.useRealTimers()
    }
  })

  /**
   * **A row can be `PENDING` and not on its way anywhere.** A scanner that
   * cannot be reached leaves it there — `attachment_scan.rs` has a test named
   * for exactly that — so the asking has a ceiling rather than running for as
   * long as the tab is open.
   */
  it('stops asking rather than polling for ever', async () => {
    vi.useFakeTimers()

    try {
      let reads = 0

      onList = () => {
        reads += 1

        return { status: 200, body: pageBody([attachment({ virusScanStatus: 'PENDING' })]) }
      }

      mount(AttachmentsTab, { props: { documentId: DOCUMENT_ID } })
      await flushPromises()

      await vi.advanceTimersByTimeAsync(3_000 * 45)
      await flushPromises()

      const atTheCeiling = reads

      await vi.advanceTimersByTimeAsync(3_000 * 20)
      await flushPromises()

      expect(reads).toBe(atTheCeiling)
      expect(reads).toBeLessThanOrEqual(41)
    } finally {
      vi.useRealTimers()
    }
  })

  it('lists what is attached, with its name and size', async () => {
    const wrapper = await render()

    expect(wrapper.find('[data-testid="attachment-row"]').text()).toContain('quotation 2026.pdf')
    expect(wrapper.find('[data-testid="attachment-row"]').text()).toContain('2.0 KB')
  })

  it('offers the bytes only once something has cleared them', async () => {
    onList = () => ({ status: 200, body: pageBody([attachment({ virusScanStatus: 'CLEAN' })]) })

    const wrapper = await render()

    expect(wrapper.find('[data-testid="attachment-download"]').exists()).toBe(true)
  })

  /**
   * **The item's second acceptance criterion, and the reason this screen is not
   * a list of file names.**
   *
   * `PENDING`, `INFECTED` and `FAILED` are three refusals rather than three
   * stages of one. The person waiting has nothing to do; the other two have to
   * act, and differently.
   */
  it('tells the three refusals apart, and offers none of them', async () => {
    const seen = new Set<string>()

    for (const status of ['PENDING', 'INFECTED', 'FAILED'] as const) {
      onList = () => ({ status: 200, body: pageBody([attachment({ virusScanStatus: status })]) })

      const wrapper = await render()

      expect(
        wrapper.find('[data-testid="attachment-download"]').exists(),
        `${status} offered a download`,
      ).toBe(false)

      seen.add(wrapper.find('[data-testid="attachment-explanation"]').text())
    }

    expect(seen.size, 'the three refusals read the same').toBe(3)
  })

  /**
   * **`FAILED` is the file's problem, not the product's.** A scan that could not
   * run has cleared nothing, so it is refused exactly as an infected file is —
   * and a screen that renders it as an internal error invites somebody to
   * retry the download rather than replace the file.
   */
  it('does not present an unscannable file as an error in the product', async () => {
    onList = () => ({ status: 200, body: pageBody([attachment({ virusScanStatus: 'FAILED' })]) })

    const wrapper = await render()
    const explanation = wrapper.find('[data-testid="attachment-explanation"]').text()

    expect(explanation).toContain('Upload it again')
    expect(explanation).not.toContain('error')
  })

  /**
   * **The server's refusal is shown, not one this screen invented.** The size
   * limit and the allowed types are configuration; a browser that re-derived
   * either would be a second policy, drifting from the one that decides.
   *
   * The first version of this test triggered `change` on an input with no file,
   * so `upload` returned before it called anything and the assertion was that
   * no error appeared — which it never would have. It is written down because a
   * test that passes for the wrong reason is worse than one that is missing:
   * the missing one is visible.
   */
  it.each([
    ['a file over the size limit', 'this deployment accepts files up to 26214400 bytes (25 MB)'],
    [
      'a file whose content is not stored here',
      "this file's content is text/html; this deployment stores application/pdf",
    ],
  ])('shows the server refusal for %s in the server words', async (_case, message) => {
    backend.restore()
    backend = installFakeBackend((request) => {
      if (request.method === 'post') {
        return { status: 422, body: errorBody('VALIDATION_FAILED', message) }
      }

      return { status: 200, body: pageBody([]) }
    })

    const wrapper = await render()
    const input = wrapper.find('[data-testid="attachment-input"]')

    // jsdom gives no file picker, so the chosen file is placed on the element
    // the handler reads — which is the same property a picker would set.
    Object.defineProperty(input.element, 'files', {
      value: [new File(['<html></html>'], 'invoice.pdf', { type: 'application/pdf' })],
      configurable: true,
    })

    await input.trigger('change')
    await flushPromises()

    const shown = wrapper.find('[data-testid="attachment-upload-error"]')

    expect(shown.exists(), 'the refusal was swallowed').toBe(true)
    expect(shown.text()).toBe(message)
  })

  it('says nothing is attached rather than showing an empty list', async () => {
    onList = () => ({ status: 200, body: pageBody([]) })

    const wrapper = await render()

    expect(wrapper.find('[data-testid="attachments-empty"]').exists()).toBe(true)
  })

  /**
   * **A caller who may not read attachments is told so**, rather than shown an
   * empty tab — [#263](https://github.com/sujanto-gaws/kelir/issues/263)'s
   * finding, which was a panel that said an approval had no steps when the
   * reader simply could not see them.
   */
  it('distinguishes not allowed to look from looked and found nothing', async () => {
    signIn([])

    const wrapper = await render()

    expect(wrapper.find('[data-testid="attachments-forbidden"]').exists()).toBe(true)
    expect(wrapper.find('[data-testid="attachments-empty"]').exists()).toBe(false)
  })

  it('offers no upload control to a caller who may only read', async () => {
    signIn(['attachment:read'])

    const wrapper = await render()

    expect(wrapper.find('[data-testid="attachment-input"]').exists()).toBe(false)
    expect(wrapper.find('[data-testid="attachment-row"]').exists()).toBe(true)
  })
  // -------------------------------------------------------------------------
  // #254 — categories, the delete, and a link that is visibly not a file
  // -------------------------------------------------------------------------

  it('shows what a file is filed under, and nothing when it is filed under nothing', async () => {
    onList = () => ({
      status: 200,
      body: pageBody([attachment(), attachment({ id: 'x', category: QUOTATION })]),
    })

    const wrapper = await render()
    const badges = wrapper.findAll('[data-testid="attachment-category"]')

    expect(badges).toHaveLength(1)
    expect(badges[0].text()).toBe('Quotation')
  })

  it('sends the chosen category with the upload', async () => {
    const wrapper = await render()

    await wrapper.find('[data-testid="attachment-category-picker"]').setValue(QUOTATION.id)

    const input = wrapper.find('[data-testid="attachment-input"]')
    const file = new File(['%PDF-1.7'], 'quotation.pdf', { type: 'application/pdf' })

    Object.defineProperty(input.element, 'files', { value: [file] })
    await input.trigger('change')
    await flushPromises()

    const posted = backend.requests.find(
      (request) => request.method === 'post' && request.url.includes('/attachments'),
    )

    expect(posted).toBeDefined()
    // The body is the `FormData` the client built, recorded as it was sent —
    // so the assertion reads the part rather than a rendering of the whole.
    expect((posted?.body as FormData).get('categoryId')).toBe(QUOTATION.id)
    expect((posted?.body as FormData).get('file')).toBeInstanceOf(File)
  })

  /**
   * **A link carries no size, no scan badge and no download** (#254 AC4), and
   * the screen cannot render one by accident because the type it renders from
   * has no such field.
   */
  it('shows a link as a link rather than as a file', async () => {
    onList = () => ({ status: 200, body: pageBody([]) })
    onReferences = () => ({ status: 200, body: pageBody([reference()]) })

    const wrapper = await render()
    const row = wrapper.find('[data-testid="reference-row"]')

    expect(row.exists()).toBe(true)
    expect(row.text()).toContain('Vendor portal')
    expect(wrapper.find('[data-testid="reference-badge"]').text()).toBe('Link')
    expect(row.find('[data-testid="attachment-status"]').exists()).toBe(false)
    expect(row.find('[data-testid="attachment-download"]').exists()).toBe(false)

    const open = wrapper.find('[data-testid="reference-open-link"]')
    expect(open.attributes('href')).toBe('https://vendor.example.test/quotes/2026-11')
    expect(open.attributes('rel')).toBe('noopener noreferrer')
    expect(open.attributes('target')).toBe('_blank')
  })

  /**
   * **Defence in depth, and it is the `href` that matters.** The server refuses
   * these; a row that predates the check must still not become script in this
   * page.
   */
  it('refuses to put a javascript link in an href', async () => {
    onList = () => ({ status: 200, body: pageBody([]) })
    onReferences = () => ({
      status: 200,
      body: pageBody([reference({ url: 'javascript:alert(1)' })]),
    })

    const wrapper = await render()

    expect(wrapper.find('[data-testid="reference-open-link"]').exists()).toBe(false)
    expect(wrapper.find('[data-testid="reference-unopenable"]').exists()).toBe(true)
  })

  it('records a link and says what a link is not', async () => {
    onWrite = () => ({ status: 200, body: itemBody(reference()) })
    signIn(['attachment:read', 'attachment:create', 'attachment:reference'])

    const wrapper = await render()

    await wrapper.find('[data-testid="reference-open"]').trigger('click')
    expect(wrapper.find('[data-testid="reference-note"]').text()).toContain('nothing is checked')

    await wrapper.find('[data-testid="reference-label"]').setValue('Vendor portal')
    await wrapper.find('[data-testid="reference-url"]').setValue('https://vendor.example.test/q')
    await wrapper.find('[data-testid="reference-submit"]').trigger('click')
    await flushPromises()

    const posted = backend.requests.find(
      (request) => request.method === 'post' && request.url.includes('/references'),
    )

    expect(posted?.body).toMatchObject({
      label: 'Vendor portal',
      url: 'https://vendor.example.test/q',
    })
  })

  it('offers no link form to a caller who may not record one', async () => {
    const wrapper = await render()

    expect(wrapper.find('[data-testid="reference-open"]').exists()).toBe(false)
  })

  /**
   * **The ask says what *deleted* means here** (D-52): the row goes and the
   * stored copy is kept, which is not what the word implies on its own.
   */
  it('asks before deleting, and says the stored copy is kept', async () => {
    signIn(['attachment:read', 'attachment:create', 'attachment:delete'])

    const wrapper = await render()

    await wrapper.find('[data-testid="attachment-delete"]').trigger('click')
    await flushPromises()

    expect(backend.requests.some((request) => request.method === 'delete')).toBe(false)
    expect(wrapper.find('[data-testid="delete-ask"]').text()).toContain('stored copy is kept')

    await wrapper.find('[data-testid="attachment-delete-confirm"]').trigger('click')
    await flushPromises()

    expect(
      backend.requests.some(
        (request) => request.method === 'delete' && request.url.includes('/attachments/'),
      ),
    ).toBe(true)
  })

  it('offers no delete on somebody elses file', async () => {
    signIn(['attachment:read', 'attachment:create', 'attachment:delete'])
    onList = () => ({ status: 200, body: pageBody([attachment({ createdBy: 'somebody-else' })]) })

    const wrapper = await render()

    expect(wrapper.find('[data-testid="attachment-delete"]').exists()).toBe(false)
    expect(wrapper.find('[data-testid="attachment-download"]').exists()).toBe(true)
  })

  /**
   * **A links call that fails must not blank the files.** Two collections, two
   * results — including the case where an older backend has no `/references` at
   * all during a rolling deploy.
   */
  it('still lists the files when the links cannot be loaded', async () => {
    onReferences = () => ({ status: 404, body: errorBody('NOT_FOUND', 'no such route') })

    const wrapper = await render()

    expect(wrapper.find('[data-testid="attachment-row"]').exists()).toBe(true)
    expect(wrapper.find('[data-testid="attachments-error"]').exists()).toBe(true)
  })
})
