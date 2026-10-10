import { expect, type Locator, type Page } from '@playwright/test'

/**
 * Reaching a control by keyboard alone (#426, inherited from #695).
 *
 * `locator.focus()` and `locator.click()` put focus where they are told, so a
 * control that Tab can never reach would still pass a flow that used them.
 * This presses Tab, or Shift+Tab, until `target` has focus, and fails if it is
 * not reached within `limit` presses. **That makes tab order part of the
 * assertion.**
 *
 * **It counts presses instead of expecting an exact number** because a control
 * that replaces itself, such as the logic builder's *No expression* chooser
 * becoming the expression it chose, takes focus with it. Where focus lands next
 * is the browser's choice, so the count after that is not fixed. Where a step
 * has a fixed order, the flow presses Tab itself and asserts each stop.
 */
export async function tabTo(
  page: Page,
  target: Locator,
  options: { readonly backward?: boolean; readonly limit?: number } = {},
): Promise<void> {
  const key = options.backward ? 'Shift+Tab' : 'Tab'
  const limit = options.limit ?? 40

  for (let pressed = 0; pressed < limit; pressed += 1) {
    if (await isFocused(target)) {
      return
    }

    await page.keyboard.press(key)
  }

  await expect(
    target,
    `${limit} presses of ${key} did not reach the control: it is out of the tab order`,
  ).toBeFocused({ timeout: 1_000 })
}

async function isFocused(target: Locator): Promise<boolean> {
  return target
    .evaluate((element) => element === document.activeElement, undefined, { timeout: 2_000 })
    .catch(() => false)
}
