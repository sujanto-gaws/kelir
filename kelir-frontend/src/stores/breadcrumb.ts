import { defineStore } from 'pinia'
import { shallowRef } from 'vue'

import type { PublishedName } from '@/layouts/breadcrumbs'

/**
 * The readable name of the record on screen, for the shell's trail (#968).
 *
 * **The page publishes it; the trail never fetches it.** The page has already
 * loaded the record, so the crumb can show nothing the page could not read,
 * and costs no request. The name is held with the record's key, and the trail
 * shows it only while the route names that same key — so moving from one
 * record to another can never show the first one's name.
 */
export const useBreadcrumbStore = defineStore('breadcrumb', () => {
  const published = shallowRef<PublishedName | null>(null)

  function publish(name: PublishedName | null): void {
    published.value = name
  }

  /** Clears the name, unless another page has replaced it since. */
  function withdraw(key: string): void {
    if (published.value?.key === key) {
      published.value = null
    }
  }

  return { published, publish, withdraw }
})
