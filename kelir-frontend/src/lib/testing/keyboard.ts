/**
 * Keyboard operation for component tests, as a browser performs it.
 *
 * jsdom dispatches a `keydown` and then does nothing: no focus moves on Tab,
 * no button is pressed on Enter, no `select` changes on an arrow. A test that
 * claims a control works "by keyboard alone" needs those default actions, so
 * this module performs them — and only them, and only after the `keydown` it
 * dispatches was not cancelled, as a browser does. What it emulates:
 *
 * - **Tab** moves focus to the next control in document order that a browser
 *   would stop on: enabled, rendered, and not `tabindex="-1"`;
 * - **Enter** and **Space** on a `button` press it;
 * - **ArrowDown** and **ArrowUp** on a closed `select` move to the next or
 *   previous enabled option and fire `change`, as Windows and Linux browsers do;
 * - **typing** into an `input` or `textarea` appends each character and fires
 *   `input`.
 *
 * It does not open menus, does not emulate a screen reader, and is not a WCAG
 * check. The test's mount must be attached to the document (`attachTo`), or
 * nothing in it can take focus.
 */

const CONTROLS = 'button, select, input, textarea, a[href], [tabindex]'

function isRendered(element: HTMLElement): boolean {
  for (let at: HTMLElement | null = element; at; at = at.parentElement) {
    if (at.hidden || at.style.display === 'none') {
      return false
    }
  }

  return true
}

/** The controls Tab stops on, in order. */
export function tabStops(root: ParentNode = document): HTMLElement[] {
  return Array.from(root.querySelectorAll<HTMLElement>(CONTROLS)).filter(
    (element) =>
      !(element as HTMLButtonElement).disabled &&
      element.getAttribute('tabindex') !== '-1' &&
      isRendered(element),
  )
}

function active(): HTMLElement {
  const element = document.activeElement

  if (!(element instanceof HTMLElement) || element === document.body) {
    throw new Error('Nothing has focus')
  }

  return element
}

function keydown(target: HTMLElement, key: string): boolean {
  return target.dispatchEvent(
    new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }),
  )
}

/** Presses Tab: focus moves to the next stop, wrapping to the first. */
export function tab(): HTMLElement {
  const from = document.activeElement
  const stops = tabStops()

  if (from instanceof HTMLElement && from !== document.body) {
    if (!keydown(from, 'Tab')) {
      return from
    }
  }

  const at = stops.indexOf(from as HTMLElement)
  const next = stops[(at + 1) % stops.length]

  next.focus()

  return next
}

/**
 * Presses Tab until `matches` holds for the focused control, failing if a full
 * cycle never reaches one — which is what "not reachable by keyboard" means.
 */
export function tabTo(matches: (element: HTMLElement) => boolean): HTMLElement {
  const limit = tabStops().length + 1

  for (let pressed = 0; pressed < limit; pressed += 1) {
    const element = tab()

    if (matches(element)) {
      return element
    }
  }

  throw new Error('No control matching the predicate is reachable by Tab')
}

/** Tabs to the control whose accessible name (`aria-label`) is `name`. */
export function tabToLabel(name: string): HTMLElement {
  return tabTo((element) => element.getAttribute('aria-label') === name)
}

/** Presses a key on the focused control and performs the browser's default action. */
export function press(key: 'Enter' | ' ' | 'ArrowDown' | 'ArrowUp'): void {
  const target = active()

  if (!keydown(target, key)) {
    return
  }

  if (target instanceof HTMLButtonElement && (key === 'Enter' || key === ' ')) {
    target.click()

    return
  }

  if (target instanceof HTMLSelectElement && (key === 'ArrowDown' || key === 'ArrowUp')) {
    const step = key === 'ArrowDown' ? 1 : -1
    let index = target.selectedIndex + step

    while (index >= 0 && index < target.options.length && target.options[index].disabled) {
      index += step
    }

    if (index >= 0 && index < target.options.length) {
      target.selectedIndex = index
      target.dispatchEvent(new Event('change', { bubbles: true }))
    }
  }
}

/** Types `text` into the focused field, one character and one `input` event at a time. */
export function type(text: string): void {
  const target = active()

  if (!(target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement)) {
    throw new Error('The focused control does not take text')
  }

  for (const character of text) {
    if (keydown(target, character)) {
      target.value += character
      target.dispatchEvent(new Event('input', { bubbles: true }))
    }
  }
}
