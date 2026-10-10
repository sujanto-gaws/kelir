import type { Component } from 'vue'

/**
 * Every `@lucide/vue` icon module, as a loader, keyed by file path.
 *
 * **A module of its own, imported lazily by `menuIcon.ts`**, because the map
 * is large: one entry per icon, about 1,870, each naming its own chunk. Kept
 * in the layout's chunk it put some 70 KB gzipped back on the first load it
 * exists to keep the icons off (measured 2026-10-10), so it is fetched only
 * when a configured menu entry names an icon.
 */
export const ICON_MODULES = import.meta.glob<Component>(
  '../../node_modules/@lucide/vue/dist/esm/icons/*.mjs',
  { import: 'default' },
)
