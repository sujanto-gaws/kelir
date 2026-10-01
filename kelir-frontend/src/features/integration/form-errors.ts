/** A blank optional text field as `null`, trimmed otherwise. */
export function blankToNull(value: string): string | null {
  const trimmed = value.trim()

  return trimmed === '' ? null : trimmed
}

/**
 * A number input's value, or `undefined` when it is blank.
 *
 * Not validated beyond being a number: the ranges are the backend's rules, and
 * its 422 names the field that broke one.
 */
export function numberOrUndefined(value: string | number): number | undefined {
  if (typeof value === 'number') {
    return Number.isFinite(value) ? value : undefined
  }

  const trimmed = value.trim()

  if (trimmed === '') {
    return undefined
  }

  const parsed = Number(trimmed)

  return Number.isFinite(parsed) ? parsed : undefined
}
