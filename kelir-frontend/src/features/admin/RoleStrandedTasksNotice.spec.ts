import { mount, type VueWrapper } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'

import RoleStrandedTasksNotice from './RoleStrandedTasksNotice.vue'
import type { Role } from '@/types/identity'

const clerkRole: Role = {
  id: 'r-2',
  roleCode: 'ROLE-CLERK',
  name: 'Clerk',
  description: null,
  isSystem: false,
  permissions: [],
}

function mountNotice(counts: Partial<Role>, canList = true): VueWrapper {
  return mount(RoleStrandedTasksNotice, {
    props: { role: { ...clerkRole, ...counts }, canList },
  })
}

function noticeOf(wrapper: VueWrapper) {
  return wrapper.find('[data-testid="role-stranded-tasks"]')
}

describe('RoleStrandedTasksNotice', () => {
  it('says how many open tasks a role nobody holds is needed by', () => {
    const wrapper = mountNotice({ liveHolders: 0, openTasks: 3 })

    expect(noticeOf(wrapper).exists()).toBe(true)
    expect(noticeOf(wrapper).text()).toBe('3 open tasks, 0 active holders')
  })

  it('says task, not tasks, for one', () => {
    const wrapper = mountNotice({ liveHolders: 0, openTasks: 1 })

    expect(noticeOf(wrapper).text()).toBe('1 open task, 0 active holders')
  })

  it('draws nothing while somebody holds the role', () => {
    const wrapper = mountNotice({ liveHolders: 1, openTasks: 4 })

    expect(noticeOf(wrapper).exists()).toBe(false)
  })

  it('draws nothing when no open task needs the role', () => {
    const wrapper = mountNotice({ liveHolders: 0, openTasks: 0 })

    expect(noticeOf(wrapper).exists()).toBe(false)
  })

  it('draws nothing when the server sent no counts, rather than reading them as 0', () => {
    expect(noticeOf(mountNotice({})).exists()).toBe(false)
    // One count without the other is not enough either: a missing holder count
    // is not a role nobody holds.
    expect(noticeOf(mountNotice({ openTasks: 2 })).exists()).toBe(false)
    expect(noticeOf(mountNotice({ liveHolders: 0 })).exists()).toBe(false)
  })

  it('opens the list of those tasks when it may be read', async () => {
    const wrapper = mountNotice({ liveHolders: 0, openTasks: 2 })
    const link = wrapper.find('[data-testid="role-stranded-tasks-open"]')

    expect(link.element.tagName).toBe('BUTTON')

    await link.trigger('click')

    expect(wrapper.emitted('open')).toEqual([[{ ...clerkRole, liveHolders: 0, openTasks: 2 }]])
  })

  it('without identity:role:delete, says so instead of offering a list that would be refused', async () => {
    const wrapper = mountNotice({ liveHolders: 0, openTasks: 2 }, false)

    expect(wrapper.find('[data-testid="role-stranded-tasks-open"]').exists()).toBe(false)
    expect(wrapper.find('button').exists()).toBe(false)
    expect(noticeOf(wrapper).text()).toContain('2 open tasks, 0 active holders')
    expect(wrapper.find('[data-testid="role-stranded-tasks-unlisted"]').text()).toBe(
      'Listing them needs permission to delete roles.',
    )
    expect(wrapper.emitted('open')).toBeUndefined()
  })
})
