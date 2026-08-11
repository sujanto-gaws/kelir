import { describe, expect, it } from 'vitest'

import { cn } from './utils'

describe('cn', () => {
  it('lets a later tailwind utility win over an earlier one', () => {
    // Without tailwind-merge both survive and CSS order decides, which makes
    // component overrides unpredictable.
    expect(cn('p-2', 'p-4')).toBe('p-4')
  })

  it('drops falsy values, so conditional classes can be inlined', () => {
    const isHidden = false

    expect(cn('flex', isHidden && 'hidden', undefined, 'gap-2')).toBe('flex gap-2')
  })
})
