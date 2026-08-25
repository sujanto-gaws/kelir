import { afterEach, describe, expect, it, vi } from 'vitest'

import { notifySessionLost, onSessionLost, resetSessionListeners } from './session-events'

describe('session-events', () => {
  afterEach(() => {
    resetSessionListeners()
  })

  it('tells every listener that the session is gone', () => {
    const first = vi.fn()
    const second = vi.fn()

    onSessionLost(first)
    onSessionLost(second)
    notifySessionLost()

    expect(first).toHaveBeenCalledTimes(1)
    expect(second).toHaveBeenCalledTimes(1)
  })

  it('stops telling a listener that has been removed', () => {
    // The registry outlives any one router or store, so a caller that can be
    // built more than once — a test, a re-mounted app — has to be able to let
    // go. Without this each rebuild would add another listener holding a router
    // nobody is looking at.
    const listener = vi.fn()
    const stop = onSessionLost(listener)

    stop()
    notifySessionLost()

    expect(listener).not.toHaveBeenCalled()
  })

  it('carries on when a listener throws', () => {
    // Otherwise the first listener registered decides whether the rest run, and
    // the one that matters — getting the user off a dead page — might be second.
    const angry = vi.fn(() => {
      throw new Error('no')
    })
    const other = vi.fn()

    onSessionLost(angry)
    onSessionLost(other)

    expect(() => notifySessionLost()).not.toThrow()
    expect(other).toHaveBeenCalledTimes(1)
  })

  it('survives a listener that unregisters during the notification', () => {
    // Iterating the live set would skip a listener when an earlier one removes
    // itself, which is exactly what a one-shot listener would do.
    const seen: string[] = []
    const stopFirst = onSessionLost(() => {
      seen.push('first')
      stopFirst()
    })
    onSessionLost(() => seen.push('second'))

    notifySessionLost()

    expect(seen).toEqual(['first', 'second'])
  })
})
