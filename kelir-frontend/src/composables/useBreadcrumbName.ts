import { onBeforeUnmount, watch } from 'vue'

import type { PublishedName } from '@/layouts/breadcrumbs'
import { useBreadcrumbStore } from '@/stores/breadcrumb'

/**
 * Publishes the record a page has loaded as its breadcrumb's name (#968 AC5).
 *
 * `source` returns the record's key — the route param it was loaded by — and
 * its name, or nothing while it loads or after a load fails. The trail keeps
 * the route's static label until a name for the route's own key arrives, and
 * the name is withdrawn when the page goes.
 */
export function useBreadcrumbName(source: () => PublishedName | null | undefined): void {
  const store = useBreadcrumbStore()
  let last: string | null = null

  watch(
    [() => source()?.key ?? null, () => source()?.name ?? null],
    ([key, name]) => {
      if (last !== null && last !== key) {
        store.withdraw(last)
        last = null
      }

      if (key !== null && name !== null) {
        store.publish({ key, name })
        last = key
      }
    },
    { immediate: true },
  )

  onBeforeUnmount(() => {
    if (last !== null) {
      store.withdraw(last)
    }
  })
}
