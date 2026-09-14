import { mount, type VueWrapper } from '@vue/test-utils'
import { describe, expect, it, vi } from 'vitest'

import DocumentStatusChart from './DocumentStatusChart.vue'
import type { DocumentStatusCount } from '@/types/reporting'

/**
 * The status chart (FR-RPT-004, [#447]).
 *
 * **What this file can see is the chart's configuration, not the chart.** Unovis
 * draws with d3 into an SVG measured by a `ResizeObserver`, and jsdom lays out
 * nothing — a bar rendered here would have no width to assert on. So Unovis is
 * replaced by components that only record what they were given, and these tests
 * are about what *this* component decides: the order, the labels, the colour,
 * and that the picture is hidden from assistive technology because the card
 * states the same numbers as text.
 *
 * Whether the bars actually appear in a browser is not a unit test's question,
 * and neither is whether d3 stays off first load — that is
 * `scripts/check-bundle-split.mjs`, against the build.
 *
 * [#447]: https://github.com/sujanto-gaws/kelir/issues/447
 */

vi.mock('@unovis/ts', () => ({
  Direction: { South: 'south' },
  Orientation: { Horizontal: 'horizontal' },
}))

vi.mock('@unovis/vue', async () => {
  const { defineComponent, h } = await import('vue')

  const recorder = (name: string, props: string[]) =>
    defineComponent({
      name,
      props,
      setup(_props, { slots }) {
        return () => h('div', { 'data-stub': name }, slots.default?.())
      },
    })

  return {
    VisXYContainer: recorder('VisXYContainer', ['data', 'height', 'yDirection']),
    VisGroupedBar: recorder('VisGroupedBar', ['x', 'y', 'color', 'orientation', 'roundedCorners']),
    VisAxis: recorder('VisAxis', ['type', 'tickFormat', 'tickValues', 'gridLine']),
  }
})

type TickFormat = (tick: number | Date) => string

/** Deliberately not in lifecycle order, so the order drawn can only be the order given. */
const COUNTS: DocumentStatusCount[] = [
  { status: 'COMPLETED', count: 4 },
  { status: 'DRAFT', count: 0 },
  { status: 'PENDING_APPROVAL', count: 7 },
]

function render(counts: DocumentStatusCount[] = COUNTS): VueWrapper {
  return mount(DocumentStatusChart, { props: { counts } })
}

function axis(wrapper: VueWrapper, type: 'x' | 'y') {
  const found = wrapper
    .findAllComponents({ name: 'VisAxis' })
    .find((candidate) => candidate.props('type') === type)

  if (!found) {
    throw new Error(`no ${type} axis was rendered`)
  }

  return found
}

describe('DocumentStatusChart', () => {
  /**
   * **The chart is a picture of numbers stated elsewhere.** A screen reader
   * reading d3's SVG gets tick text without the bars they label, so the card's
   * text list is the accessible form and this is kept out of the way of it.
   */
  it('is hidden from assistive technology, because the card states the counts as text', () => {
    const wrapper = render()

    expect(wrapper.find('[data-testid="status-chart"]').attributes('aria-hidden')).toBe('true')
  })

  it('draws the rows in the order it was given', () => {
    const wrapper = render()
    const container = wrapper.findComponent({ name: 'VisXYContainer' })
    const bars = wrapper.findComponent({ name: 'VisGroupedBar' })
    const position = bars.props('x') as (row: DocumentStatusCount, index: number) => number

    expect(container.props('data')).toEqual(COUNTS)
    expect(COUNTS.map((row, index) => position(row, index))).toEqual([0, 1, 2])
    // The first row at the top, so the chart reads in the same order as the list.
    expect(container.props('yDirection')).toBe('south')
  })

  it('labels every bar with the status name a person reads', () => {
    const statuses = axis(render(), 'y')
    const format = statuses.props('tickFormat') as TickFormat

    expect(statuses.props('tickValues')).toEqual([0, 1, 2])
    expect([0, 1, 2].map((tick) => format(tick))).toEqual([
      'Completed',
      'Draft',
      'Pending approval',
    ])
    expect(format(3)).toBe('')
  })

  it('numbers the count axis in whole documents only', () => {
    const format = axis(render(), 'x').props('tickFormat') as TickFormat

    expect(format(2)).toBe('2')
    expect(format(0)).toBe('0')
    expect(format(0.5)).toBe('')
  })

  /**
   * **One series, one colour, and it is the theme's.** A literal colour here
   * would be right in one theme and wrong in the other; the token is the one
   * `.dark` redefines.
   */
  it('draws its one series in the theme primary colour', () => {
    const bars = render().findComponent({ name: 'VisGroupedBar' })

    expect(bars.props('color')).toBe('var(--color-primary)')
    expect(bars.props('orientation')).toBe('horizontal')
  })
})
