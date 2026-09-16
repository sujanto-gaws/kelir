import { describe, expect, it } from 'vitest'

import { durationLabel } from './duration'

const MINUTE = 60
const HOUR = 60 * MINUTE
const DAY = 24 * HOUR

/**
 * The approval time card's durations (FR-RPT-006, #461). Mutations are recorded
 * in `DashboardPage.spec.ts`'s FR-RPT-006 section, beside the card's own.
 */
describe('durationLabel', () => {
  it('says a decision under a minute in words rather than as zero minutes', () => {
    expect(durationLabel(0)).toBe('under a minute')
    expect(durationLabel(59)).toBe('under a minute')
  })

  it('counts whole minutes below an hour', () => {
    expect(durationLabel(MINUTE)).toBe('1 min')
    expect(durationLabel(59 * MINUTE + 59)).toBe('59 min')
  })

  it('gives hours and minutes below a day, dropping a zero minute', () => {
    expect(durationLabel(HOUR)).toBe('1 h')
    expect(durationLabel(3 * HOUR + 25 * MINUTE + 40)).toBe('3 h 25 min')
  })

  it('gives days and hours from a day up, dropping a zero hour and the minutes', () => {
    expect(durationLabel(DAY)).toBe('1 d')
    expect(durationLabel(2 * DAY + 4 * HOUR + 59 * MINUTE)).toBe('2 d 4 h')
    expect(durationLabel(45 * DAY)).toBe('45 d')
  })
})
