/**
 * An approval time as a person reads it (FR-RPT-006, #461).
 *
 * **Formatting only.** The seconds are the server's — one time per document,
 * measured from its first submission — and nothing here measures, rounds to a
 * different question, or compares with the browser's clock.
 *
 * **Two units at most, largest first**, because *2 d 4 h* is what a person
 * compares and *2 d 4 h 13 min 9 s* is noise on a card. The smaller unit is
 * dropped when it is zero, and anything under a minute is said in words: a
 * decision that took eleven seconds is *under a minute*, not *0 min*.
 */
export function durationLabel(seconds: number): string {
  const minute = 60
  const hour = 60 * minute
  const day = 24 * hour

  if (seconds < minute) {
    return 'under a minute'
  }

  if (seconds < hour) {
    return `${Math.floor(seconds / minute)} min`
  }

  if (seconds < day) {
    return pair(Math.floor(seconds / hour), 'h', Math.floor((seconds % hour) / minute), 'min')
  }

  return pair(Math.floor(seconds / day), 'd', Math.floor((seconds % day) / hour), 'h')
}

function pair(large: number, largeUnit: string, small: number, smallUnit: string): string {
  return small === 0 ? `${large} ${largeUnit}` : `${large} ${largeUnit} ${small} ${smallUnit}`
}
