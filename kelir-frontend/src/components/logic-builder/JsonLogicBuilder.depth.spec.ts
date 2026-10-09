import { mount } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'
import { defineComponent, h } from 'vue'

import JsonLogicBuilder from './JsonLogicBuilder.vue'
import { MAX_VISUAL_DEPTH } from './logicTree'

/**
 * The builder's depth cap, mounted **cold** (#686; campaign addendum, 2026-10-09).
 *
 * This file is separate on purpose. Vitest gives each spec file fresh modules,
 * so nothing here has been run before, and V8 has not yet optimised the render
 * path. That is the state of a page that opens a deep expression first. A
 * render that fits the stack only after earlier tests have warmed the code is
 * what `JsonLogicBuilder.spec.ts`'s 100,000-deep test proves. Measured in
 * jsdom on Node's default stack, a cold mount of a `!` chain overflows from
 * about 248 levels, against the 300–400 a warm one reaches.
 */

/** `{"!": …}` wrapped `levels` times round `{"var": "a"}`. */
function notChain(levels: number): unknown {
  let expr: unknown = { var: 'a' }

  for (let depth = 0; depth < levels; depth += 1) {
    expr = { '!': expr }
  }

  return expr
}

function mountCold(expr: unknown) {
  // Through a host, not as a mount prop: Vue Test Utils walks mount props
  // recursively, which would be the harness overflowing and not the builder.
  return mount(
    defineComponent({
      setup: () => () =>
        h(JsonLogicBuilder, { modelValue: expr, tier: 'conditional', variables: [] }),
    }),
  )
}

describe('JsonLogicBuilder mounted cold at its depth cap', () => {
  it.fails(
    // Defect, found by the campaign's addendum: MAX_VISUAL_DEPTH (256) is
    // above what a cold render fits in the stack, so an expression the cap
    // still draws throws `RangeError: Maximum call stack size exceeded` on
    // mount. The 100,000-deep test in JsonLogicBuilder.spec.ts passes only
    // because the tests before it warmed the render path: run alone (`-t`),
    // it fails the same way. A cap with margin under the cold limit, such as
    // 128, would close it.
    'mounts an expression as deep as the cap still draws',
    () => {
      expect(() => mountCold(notChain(MAX_VISUAL_DEPTH)).unmount()).not.toThrow()
    },
  )

  it('mounts an expression half as deep as the cap', () => {
    // The control: the render itself works cold, well inside the stack.
    const wrapper = mountCold(notChain(MAX_VISUAL_DEPTH / 2))

    // Drawn whole: no advanced block anywhere, and the var at the bottom shown.
    expect(wrapper.find('textarea').exists()).toBe(false)
    expect(wrapper.findAll('select').length).toBeGreaterThan(MAX_VISUAL_DEPTH / 2)
    wrapper.unmount()
  })
})
