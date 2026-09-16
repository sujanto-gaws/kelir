import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import DashboardPage from './DashboardPage.vue'
import { useAuthStore } from '@/stores/auth'
import { installFakeBackend, itemBody, type FakeBackendHandle } from '@/lib/testing/fake-backend'

/**
 * The status card when its chart never arrives (FR-RPT-004, [#447]).
 *
 * **A file of its own because the failure is a module mock, and a module mock
 * is per file.** `DashboardPage.spec.ts` replaces the chart with a stub that
 * loads; this one makes the import itself reject — the shape a deploy takes when
 * it rotates the chunk away from a session that is still open.
 *
 * # Why this exists (coding standard §2.9)
 *
 * **J5 came back green, 2026-09-14, and this file is what closed it.** The
 * mutation hid the counts once the chart's loader failed, and all 43 dashboard
 * tests passed, because every one of them loads the chart successfully. The
 * page's promise that *the numbers stay where they are* was a comment with
 * nothing behind it. Re-run against this file, J5 is red.
 *
 * The test asserts the chart's error sentence as well as the counts, and that
 * half matters: without it, a loader that quietly succeeded would leave the
 * counts on screen and this would pass about a failure that never happened.
 *
 * [#447]: https://github.com/sujanto-gaws/kelir/issues/447
 */

vi.mock('./DocumentStatusChart.vue', () => {
  throw new Error('the chart chunk is gone')
})

function statusCounts(): { status: string; count: number }[] {
  return [
    'DRAFT',
    'SUBMITTED',
    'IN_REVIEW',
    'PENDING_APPROVAL',
    'APPROVED',
    'REJECTED',
    'RETURNED',
    'COMPLETED',
    'ARCHIVED',
    'CANCELLED',
  ].map((status) => ({ status, count: status === 'DRAFT' ? 2 : status === 'APPROVED' ? 3 : 0 }))
}

describe('DashboardPage when the status chart cannot load', () => {
  let backend: FakeBackendHandle

  beforeEach(() => {
    setActivePinia(createPinia())
    useAuthStore().user = {
      id: '0199a1a0-0000-7000-8000-0000000000f9',
      username: 'ani',
      displayName: 'Ani Wijaya',
      email: 'ani@example.com',
      roles: ['REQUESTER'],
      permissions: ['reporting:dashboard:read'],
    }

    backend = installFakeBackend(() => ({
      status: 200,
      body: itemBody({
        tasksWaiting: 0,
        tasksOverdue: 0,
        draftDocuments: 2,
        pendingTasks: [],
        overdueTasks: [],
        recentDocuments: [],
        documentsByStatus: statusCounts(),
        approvalTime: { windowDays: 90, documents: 0, medianSeconds: null, slowestSeconds: null },
      }),
    }))
  })

  afterEach(() => {
    backend.restore()
  })

  it('keeps the counts on screen and says the chart could not be drawn', async () => {
    const wrapper = mount(DashboardPage, {
      global: { stubs: { RouterLink: { template: '<a><slot /></a>' } } },
    })

    await flushPromises()
    await vi.dynamicImportSettled()
    await flushPromises()

    const card = wrapper.find('[data-testid="documents-by-status"]')
    const approved = card
      .findAll('[data-testid="status-count"]')
      .find((row) => row.attributes('data-status') === 'APPROVED')

    expect(card.find('[data-testid="status-chart-error"]').text()).toContain(
      'The chart could not be drawn',
    )
    expect(card.findAll('[data-testid="status-count"]')).toHaveLength(10)
    expect(approved?.find('[data-testid="status-value"]').text()).toBe('3')
  })
})
