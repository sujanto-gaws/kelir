import { flushPromises, mount } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { h } from 'vue'

import SearchSelect from './SearchSelect.vue'
import type { SearchSelectQuery, SearchSource } from './types'
import { ApiError } from '@/api/error'
import type { Page } from '@/types/api'

interface Thing {
  id: string
  code: string
  name: string
  locked?: boolean
}

/** `count` rows, `T001`…, in the order a server sorting by code returns them. */
function things(count: number, prefix = 'T'): Thing[] {
  return Array.from({ length: count }, (_, index) => {
    const code = `${prefix}${String(index + 1).padStart(3, '0')}`

    return { id: `id-${code}`, code, name: `Thing ${code}` }
  })
}

function page(items: Thing[], total = items.length): Page<Thing> {
  return { items, meta: { page: 1, pageSize: 100, total } }
}

/** A server over `all`, searching code and name and answering the first 100. */
function serverOver(all: Thing[]) {
  return vi.fn(async ({ search, pageSize }: SearchSelectQuery) => {
    const needle = search.toLowerCase()
    const matching = all.filter(
      (thing) =>
        thing.code.toLowerCase().includes(needle) || thing.name.toLowerCase().includes(needle),
    )

    return page(matching.slice(0, pageSize), matching.length)
  })
}

function sourceOf(
  fetch: SearchSource<Thing>['fetch'],
  extra: Partial<SearchSource<Thing>> = {},
): SearchSource<Thing> {
  return {
    fetch,
    value: (thing) => thing.id,
    label: (thing) => `${thing.name} (${thing.code})`,
    ...extra,
  }
}

function mountChooser(props: Record<string, unknown>, slots: Record<string, unknown> = {}) {
  return mount(SearchSelect, {
    props: { id: 'thing', label: 'Thing', testId: 'thing', modelValue: '', ...props },
    slots,
  } as never)
}

function optionLabels(wrapper: ReturnType<typeof mountChooser>): string[] {
  return wrapper.findAll('option').map((option) => option.text())
}

describe('SearchSelect', () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('asks for one page of 100 with no search when it opens', async () => {
    const fetch = serverOver(things(3))
    const wrapper = mountChooser({ source: sourceOf(fetch), placeholder: 'None' })
    await flushPromises()

    expect(fetch).toHaveBeenCalledTimes(1)
    expect(fetch).toHaveBeenCalledWith({ search: '', pageSize: 100 })
    expect(optionLabels(wrapper)).toEqual([
      'None',
      'Thing T001 (T001)',
      'Thing T002 (T002)',
      'Thing T003 (T003)',
    ])
    // The label is the control's, and the search box names what it searches.
    expect(wrapper.get('label').attributes('for')).toBe('thing')
    expect(wrapper.get('[data-testid="thing-search"]').attributes('aria-label')).toBe(
      'Search Thing',
    )
  })

  it('waits for typing to pause, then searches once with the last value, trimmed', async () => {
    const fetch = serverOver(things(3))
    const wrapper = mountChooser({ source: sourceOf(fetch) })
    await flushPromises()

    const box = wrapper.get('[data-testid="thing-search"]')
    await box.setValue('T')
    vi.advanceTimersByTime(100)
    await box.setValue('T0')
    vi.advanceTimersByTime(100)
    await box.setValue(' T002 ')
    vi.advanceTimersByTime(249)
    await flushPromises()

    expect(fetch).toHaveBeenCalledTimes(1)

    vi.advanceTimersByTime(1)
    await flushPromises()

    expect(fetch).toHaveBeenCalledTimes(2)
    expect(fetch).toHaveBeenLastCalledWith({ search: 'T002', pageSize: 100 })
    expect(optionLabels(wrapper)).toEqual(['Thing T002 (T002)'])
  })

  it('searches at once on Enter, and Enter does not submit the form around it', async () => {
    const fetch = serverOver(things(3))
    const submit = vi.fn()
    const wrapper = mount(
      {
        components: { SearchSelect },
        setup: () => ({ source: sourceOf(fetch), submit }),
        template: `<form @submit.prevent="submit">
          <SearchSelect id="thing" label="Thing" test-id="thing" model-value="" :source="source" />
        </form>`,
      },
      { attachTo: document.body },
    )
    await flushPromises()

    const box = wrapper.get('[data-testid="thing-search"]')
    await box.setValue('T003')
    await box.trigger('keydown', { key: 'Enter' })
    await flushPromises()

    expect(fetch).toHaveBeenCalledTimes(2)
    expect(fetch).toHaveBeenLastCalledWith({ search: 'T003', pageSize: 100 })
    expect(submit).not.toHaveBeenCalled()

    // The debounce the typing started was cancelled, not run a second time.
    vi.advanceTimersByTime(1000)
    await flushPromises()
    expect(fetch).toHaveBeenCalledTimes(2)

    wrapper.unmount()
  })

  it('says when the server holds more than the page it showed', async () => {
    const fetch = serverOver(things(250))
    const wrapper = mountChooser({ source: sourceOf(fetch) })
    await flushPromises()

    expect(wrapper.findAll('option')).toHaveLength(100)
    expect(wrapper.get('[data-testid="thing-status"]').text()).toBe(
      'Showing 100 of 250 — refine your search to find the rest.',
    )

    await wrapper.get('[data-testid="thing-search"]').setValue('T250')
    vi.advanceTimersByTime(250)
    await flushPromises()

    expect(optionLabels(wrapper)).toEqual(['Thing T250 (T250)'])
    expect(wrapper.get('[data-testid="thing-status"]').text()).toBe('')
  })

  it('says when a search matches nothing', async () => {
    const wrapper = mountChooser({ source: sourceOf(serverOver(things(3))) })
    await flushPromises()

    await wrapper.get('[data-testid="thing-search"]').setValue('zzz')
    vi.advanceTimersByTime(250)
    await flushPromises()

    expect(wrapper.get('[data-testid="thing-status"]').text()).toBe('Nothing matches “zzz”.')
  })

  it('keeps a stored value outside the results on screen, by the label it was given', async () => {
    const wrapper = mountChooser({
      source: sourceOf(serverOver(things(3))),
      modelValue: 'id-X999',
      known: [{ value: 'id-X999', label: 'Kept by name' }],
    })
    await flushPromises()

    const select = wrapper.get('select').element as HTMLSelectElement

    expect(optionLabels(wrapper)[0]).toBe('Kept by name')
    expect(select.value).toBe('id-X999')
  })

  it('reads the label of a stored value no search returned, when the source can', async () => {
    const stored: Thing = { id: 'id-Z500', code: 'Z500', name: 'Far down' }
    const resolve = vi.fn(async () => stored)
    const wrapper = mountChooser({
      source: sourceOf(serverOver(things(3)), { resolve }),
      modelValue: stored.id,
    })
    await flushPromises()

    expect(resolve).toHaveBeenCalledTimes(1)
    expect(resolve).toHaveBeenCalledWith('id-Z500')
    expect(optionLabels(wrapper)[0]).toBe('Far down (Z500)')
  })

  it('falls back to saying a stored value is outside the results when nothing names it', async () => {
    const wrapper = mountChooser({
      source: sourceOf(serverOver(things(3))),
      modelValue: 'id-unknown',
    })
    await flushPromises()

    expect(optionLabels(wrapper)[0]).toBe('The current choice, outside these results')
  })

  it('keeps a chosen value by its label after the search moves away from it', async () => {
    const wrapper = mountChooser({
      source: sourceOf(serverOver(things(3))),
      'onUpdate:modelValue': (value: string) => wrapper.setProps({ modelValue: value }),
    })
    await flushPromises()

    await wrapper.get('select').setValue('id-T002')
    await wrapper.get('[data-testid="thing-search"]').setValue('T003')
    vi.advanceTimersByTime(250)
    await flushPromises()

    expect(optionLabels(wrapper)).toEqual(['Thing T002 (T002)', 'Thing T003 (T003)'])
    expect((wrapper.get('select').element as HTMLSelectElement).value).toBe('id-T002')
  })

  it('ignores an older answer that arrives after a newer one', async () => {
    let answerSlow: (value: Page<Thing>) => void = () => undefined
    const fetch = vi.fn(async ({ search }: SearchSelectQuery) => {
      if (search === 'slow') {
        return new Promise<Page<Thing>>((resolve) => {
          answerSlow = resolve
        })
      }

      return page(search === 'fast' ? [{ id: 'f', code: 'F', name: 'Fast' }] : things(2))
    })
    const wrapper = mountChooser({ source: sourceOf(fetch) })
    await flushPromises()

    const box = wrapper.get('[data-testid="thing-search"]')
    await box.setValue('slow')
    vi.advanceTimersByTime(250)
    await box.setValue('fast')
    vi.advanceTimersByTime(250)
    await flushPromises()

    expect(optionLabels(wrapper)).toEqual(['Fast (F)'])

    answerSlow(page([{ id: 's', code: 'S', name: 'Slow' }]))
    await flushPromises()

    expect(optionLabels(wrapper)).toEqual(['Fast (F)'])
    expect(wrapper.get('[data-testid="thing-status"]').text()).toBe('')
  })

  it('greys out a disabled row and leaves out an excluded one', async () => {
    const all: Thing[] = [
      { id: 'a', code: 'A', name: 'Alpha' },
      { id: 'b', code: 'B', name: 'Beta', locked: true },
      { id: 'c', code: 'C', name: 'Gamma' },
    ]
    const wrapper = mountChooser({
      source: sourceOf(serverOver(all), {
        disabled: (thing) => Boolean(thing.locked),
        exclude: (thing) => thing.id === 'c',
      }),
    })
    await flushPromises()

    const options = wrapper.findAll('option')

    expect(options.map((option) => option.text())).toEqual(['Alpha (A)', 'Beta (B)'])
    expect((options[1].element as HTMLOptionElement).disabled).toBe(true)
  })

  it('says the server refused, without throwing', async () => {
    const fetch = vi.fn(async () => {
      throw new ApiError('FORBIDDEN', 'Not yours to read', 403)
    })
    const wrapper = mountChooser({ source: sourceOf(fetch) })
    await flushPromises()

    expect(wrapper.get('[data-testid="thing-error"]').text()).toContain('Not yours to read')
  })

  it('reports the row behind a choice as well as its value', async () => {
    const wrapper = mountChooser({ source: sourceOf(serverOver(things(2))) })
    await flushPromises()

    await wrapper.get('select').setValue('id-T002')

    expect(wrapper.emitted('update:modelValue')).toEqual([['id-T002']])
    expect(wrapper.emitted('pick')?.[0]?.[0]).toMatchObject({ code: 'T002' })
  })

  describe('as a list', () => {
    it('renders a radio per row, with the slot content and a test id each', async () => {
      const wrapper = mountChooser(
        {
          variant: 'list',
          source: sourceOf(serverOver(things(2)), { disabled: (thing) => thing.code === 'T002' }),
          optionTestId: (thing: Thing) => `row-${thing.code}`,
        },
        { option: ({ row }: { row: Thing }) => h('em', row.name) },
      )
      await flushPromises()

      expect(wrapper.get('legend').text()).toBe('Thing')
      expect(wrapper.get('[data-testid="row-T001"]').find('em').text()).toBe('Thing T001')
      expect(
        (wrapper.get('[data-testid="row-T002"] input').element as HTMLInputElement).disabled,
      ).toBe(true)

      await wrapper.get('[data-testid="row-T001"] input').setValue(true)

      expect(wrapper.emitted('update:modelValue')).toEqual([['id-T001']])
    })

    it('shows what the caller says when there is nothing to choose', async () => {
      const wrapper = mountChooser(
        { variant: 'list', source: sourceOf(serverOver([])) },
        { empty: () => 'Nothing is active yet.' },
      )
      await flushPromises()

      expect(wrapper.get('[data-testid="thing-status"]').text()).toBe('Nothing is active yet.')
    })
  })

  describe('with multiple', () => {
    it('toggles values and keeps chosen ones outside the results ticked', async () => {
      const wrapper = mountChooser({
        multiple: true,
        modelValue: ['id-X900'],
        known: [{ value: 'id-X900', label: 'Held already' }],
        source: sourceOf(serverOver(things(2))),
      })
      await flushPromises()

      const boxes = wrapper.findAll('input[type="checkbox"]')

      expect(wrapper.findAll('fieldset label').map((label) => label.text())).toEqual([
        'Held already',
        'Thing T001 (T001)',
        'Thing T002 (T002)',
      ])
      expect((boxes[0].element as HTMLInputElement).checked).toBe(true)

      await boxes[2].setValue(true)

      expect(wrapper.emitted('update:modelValue')).toEqual([[['id-X900', 'id-T002']]])
    })
  })
})
