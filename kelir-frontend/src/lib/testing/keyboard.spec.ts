import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import { press, tab, tabStops, tabTo, tabToLabel, type } from './keyboard'

/**
 * The keyboard helper, held to what a browser does (#686 C15).
 *
 * `JsonLogicBuilder.spec.ts` proves the builder works "by keyboard alone"
 * through this helper, so **a helper that did more than a browser would make
 * that test vacuous**: one that clicked a disabled button, chose a disabled
 * option, or ignored a cancelled `keydown` would pass a tree a keyboard user
 * cannot operate. Each test here pins one default action and one refusal.
 */

let root: HTMLDivElement

beforeEach(() => {
  root = document.createElement('div')
  document.body.appendChild(root)
})

afterEach(() => {
  root.remove()
})

function names(elements: HTMLElement[]): (string | null)[] {
  return elements.map((element) => element.getAttribute('aria-label'))
}

describe('Tab', () => {
  beforeEach(() => {
    root.innerHTML = `
      <button aria-label="first">1</button>
      <button aria-label="disabled" disabled>2</button>
      <input aria-label="skipped" tabindex="-1" />
      <div style="display: none"><input aria-label="hidden inside" /></div>
      <select aria-label="select"><option>a</option></select>
      <div hidden><button aria-label="hidden attribute">3</button></div>
      <textarea aria-label="text"></textarea>
      <a aria-label="link" href="#x">x</a>
      <a aria-label="anchor without href">y</a>
      <span aria-label="focusable span" tabindex="0">z</span>
    `
  })

  it('stops on enabled, rendered controls in document order', () => {
    expect(names(tabStops())).toEqual(['first', 'select', 'text', 'link', 'focusable span'])
  })

  it('moves focus to the next stop, and wraps from the last to the first', () => {
    expect(tab().getAttribute('aria-label')).toBe('first')
    expect(tab().getAttribute('aria-label')).toBe('select')
    expect(document.activeElement?.getAttribute('aria-label')).toBe('select')

    tabToLabel('focusable span')

    expect(tab().getAttribute('aria-label')).toBe('first')
  })

  it('stays put when the keydown is cancelled', () => {
    tabToLabel('select')
    root
      .querySelector('select')
      ?.addEventListener('keydown', (event) => event.preventDefault(), { once: true })

    expect(tab().getAttribute('aria-label')).toBe('select')
    expect(document.activeElement?.getAttribute('aria-label')).toBe('select')
  })

  it('fails to reach a control a keyboard cannot reach', () => {
    expect(() => tabToLabel('disabled')).toThrow('reachable')
    expect(() => tabToLabel('hidden inside')).toThrow('reachable')
    expect(() => tabTo((element) => element.tagName === 'NONE')).toThrow('reachable')
  })
})

describe('Enter and Space', () => {
  it('press the focused button, once per key', () => {
    let clicks = 0

    root.innerHTML = '<button aria-label="go">Go</button>'
    root.querySelector('button')?.addEventListener('click', () => (clicks += 1))

    tabToLabel('go')
    press('Enter')
    press(' ')

    expect(clicks).toBe(2)
  })

  it('do not press a button whose keydown is cancelled', () => {
    let clicks = 0

    root.innerHTML = '<button aria-label="go">Go</button>'

    const button = root.querySelector('button') as HTMLButtonElement

    button.addEventListener('click', () => (clicks += 1))
    button.addEventListener('keydown', (event) => event.preventDefault())

    tabToLabel('go')
    press('Enter')
    press(' ')

    expect(clicks).toBe(0)
  })

  it('do not press a control that is not a button', () => {
    let clicks = 0

    root.innerHTML = '<input aria-label="field" />'
    root.querySelector('input')?.addEventListener('click', () => (clicks += 1))

    tabToLabel('field')
    press('Enter')

    expect(clicks).toBe(0)
  })

  it('refuse to press when nothing has focus', () => {
    ;(document.activeElement as HTMLElement | null)?.blur()

    expect(() => press('Enter')).toThrow('Nothing has focus')
  })
})

describe('the arrow keys on a select', () => {
  let changes: string[]
  let select: HTMLSelectElement

  beforeEach(() => {
    changes = []
    root.innerHTML = `
      <select aria-label="pick">
        <option value="a">a</option>
        <option value="b" disabled>b</option>
        <option value="c">c</option>
        <option value="d" disabled>d</option>
      </select>
    `
    select = root.querySelector('select') as HTMLSelectElement
    select.addEventListener('change', () => changes.push(select.value))
    tabToLabel('pick')
  })

  it('move to the next and previous enabled option, firing change each time', () => {
    press('ArrowDown')

    expect(select.value).toBe('c')

    press('ArrowUp')

    expect(select.value).toBe('a')
    expect(changes).toEqual(['c', 'a'])
  })

  it('stop at the ends without firing change, even past a disabled last option', () => {
    press('ArrowUp')
    press('ArrowDown')
    press('ArrowDown')

    expect(select.value).toBe('c')
    expect(changes).toEqual(['c'])
  })

  it('do nothing when the keydown is cancelled', () => {
    select.addEventListener('keydown', (event) => event.preventDefault())

    press('ArrowDown')

    expect(select.value).toBe('a')
    expect(changes).toEqual([])
  })
})

describe('typing', () => {
  it('appends each character to the field, with an input event for each', () => {
    const inputs: string[] = []

    root.innerHTML = '<input aria-label="field" value="4" />'

    const field = root.querySelector('input') as HTMLInputElement

    field.addEventListener('input', () => inputs.push(field.value))

    tabToLabel('field')
    type('2.5')

    expect(field.value).toBe('42.5')
    expect(inputs).toEqual(['42', '42.', '42.5'])
  })

  it('skips a character whose keydown is cancelled', () => {
    root.innerHTML = '<textarea aria-label="box"></textarea>'

    const box = root.querySelector('textarea') as HTMLTextAreaElement

    box.addEventListener('keydown', (event) => {
      if (event.key === 'x') {
        event.preventDefault()
      }
    })

    tabToLabel('box')
    type('axb')

    expect(box.value).toBe('ab')
  })

  it('refuses a control that does not take text', () => {
    root.innerHTML = '<button aria-label="go">Go</button>'

    tabToLabel('go')

    expect(() => type('a')).toThrow('does not take text')
  })
})
