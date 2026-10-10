import { flushPromises, mount } from '@vue/test-utils'
import { afterEach, describe, expect, it } from 'vitest'
import { defineComponent, ref } from 'vue'

import Dialog from './Dialog.vue'

function renderOpen(props: Record<string, unknown> = {}) {
  return mount(Dialog, {
    props: { open: true, title: 'Reject document', ...props },
    slots: { default: 'Give a reason for the rejection.' },
  })
}

describe('Dialog', () => {
  it('renders nothing while closed', () => {
    const wrapper = mount(Dialog, { props: { open: false, title: 'Reject document' } })

    expect(wrapper.find('[role="dialog"]').exists()).toBe(false)
  })

  it('renders the title and the body once open', () => {
    const wrapper = renderOpen()

    expect(wrapper.find('[role="dialog"]').exists()).toBe(true)
    expect(wrapper.text()).toContain('Reject document')
    expect(wrapper.text()).toContain('Give a reason for the rejection.')
  })

  it('closes on Escape', async () => {
    const wrapper = renderOpen()

    await wrapper.find('[role="dialog"]').trigger('keydown', { key: 'Escape' })

    expect(wrapper.emitted('update:open')).toStrictEqual([[false]])
    expect(wrapper.find('[role="dialog"]').exists()).toBe(false)
  })

  it('closes on an overlay click', async () => {
    const wrapper = renderOpen()

    // Not wrapper.trigger: with a v-if root, wrapper.element is the mount
    // container rather than the overlay, so the click would land nowhere.
    await wrapper.get('div.fixed').trigger('click')

    expect(wrapper.emitted('update:open')).toStrictEqual([[false]])
  })

  it('stays open when the click lands inside the panel', async () => {
    const wrapper = renderOpen()

    await wrapper.find('[role="dialog"]').trigger('click')

    expect(wrapper.emitted('update:open')).toBeUndefined()
    expect(wrapper.find('[role="dialog"]').exists()).toBe(true)
  })

  it('points its aria labels at elements that exist', () => {
    const wrapper = renderOpen({ description: 'This cannot be undone.' })
    const panel = wrapper.get('[role="dialog"]')

    const labelledBy = panel.attributes('aria-labelledby')
    const describedBy = panel.attributes('aria-describedby')

    expect(wrapper.get(`[id="${labelledBy}"]`).text()).toBe('Reject document')
    expect(wrapper.get(`[id="${describedBy}"]`).text()).toBe('This cannot be undone.')
  })

  it('omits aria-describedby when there is no description', () => {
    const wrapper = renderOpen()

    expect(wrapper.get('[role="dialog"]').attributes('aria-describedby')).toBeUndefined()
  })

  it('closes from the header close button', async () => {
    const wrapper = renderOpen()

    await wrapper.find('button[aria-label="Close"]').trigger('click')

    expect(wrapper.emitted('update:open')).toStrictEqual([[false]])
  })

  describe('focus (WAI-ARIA dialog pattern; PR #715, 2026-10-10)', () => {
    /** A page with the button that opens the dialog, and another to move focus to. */
    const Host = defineComponent({
      components: { TestDialog: Dialog },
      setup() {
        const open = ref(false)
        const showOpener = ref(true)

        return { open, showOpener }
      },
      template: `
        <div>
          <button v-if="showOpener" data-testid="opener" @click="open = true">Delete</button>
          <button data-testid="elsewhere">Elsewhere</button>
          <TestDialog v-model:open="open" title="Delete role">Sure?</TestDialog>
        </div>
      `,
    })

    let mounted: ReturnType<typeof mount> | null = null

    afterEach(() => {
      mounted?.unmount()
      mounted = null
    })

    async function openFromButton() {
      mounted = mount(Host, { attachTo: document.body })

      const opener = mounted.get('[data-testid="opener"]').element as HTMLButtonElement

      opener.focus()
      opener.click()
      await flushPromises()

      expect(document.activeElement).toBe(mounted.get('[role="dialog"]').element)

      return { wrapper: mounted, opener }
    }

    it('hands focus back to what opened it when it closes', async () => {
      const { wrapper, opener } = await openFromButton()

      await wrapper.get('[role="dialog"]').trigger('keydown', { key: 'Escape' })
      await flushPromises()

      expect(wrapper.find('[role="dialog"]').exists()).toBe(false)
      expect(document.activeElement).toBe(opener)
    })

    it('leaves focus where it is when something else took it before the close', async () => {
      const { wrapper } = await openFromButton()
      const elsewhere = wrapper.get('[data-testid="elsewhere"]').element as HTMLButtonElement

      elsewhere.focus()
      await wrapper.get('button[aria-label="Close"]').trigger('click')
      await flushPromises()

      expect(document.activeElement).toBe(elsewhere)
    })

    it('hands focus to nothing when what opened it is gone', async () => {
      const { wrapper } = await openFromButton()

      ;(wrapper.vm as unknown as { showOpener: boolean }).showOpener = false
      await flushPromises()
      await wrapper.get('button[aria-label="Close"]').trigger('click')
      await flushPromises()

      expect(document.activeElement).toBe(document.body)
    })
  })
})
