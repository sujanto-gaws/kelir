/**
 * The field errors a form has no input for.
 *
 * `useFormErrors` binds a 422's details by `path`, and an input shows the one
 * addressed to it. **A detail addressed to a path the form does not render
 * would otherwise vanish** — the backend's `UNKNOWN_FIELD`, or a rule on a
 * field this build does not offer — and the form would refuse to save while
 * every visible input looked valid. Those are listed on the form instead.
 */
export function unplacedErrors(
  fieldErrors: Record<string, string>,
  placed: readonly string[],
): { path: string; message: string }[] {
  return Object.entries(fieldErrors)
    .filter(([path]) => !placed.includes(path))
    .map(([path, message]) => ({ path, message }))
}

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
