import { defineAsyncComponent, type Component } from 'vue'
import { Circle } from '@lucide/vue'

/**
 * A configured menu entry's icon, loaded on its own when it is first drawn
 * (FR-RAD-004; #688, after #698).
 *
 * **A menu's `icon` is data**: any Lucide name a tenant typed
 * (`MenuFormDialog`, *"A Lucide icon name"*), stored as an unvalidated string
 * of up to 64 characters (`rad::domain::menu`). So the layout cannot list the
 * icons it may need, and until #688 it imported the whole icon package as a
 * namespace to look one up, which put every icon on the first load of every
 * session. The swap to `@lucide/vue`'s larger set made that 34.7 KB gzipped
 * more, which is what this file removes.
 *
 * **Each icon is its own chunk now.** [`menuIconModules.ts`](./menuIconModules.ts)
 * names every one of `@lucide/vue`'s per-icon modules with `import.meta.glob`,
 * so the build emits one small chunk per icon. That map is itself loaded
 * lazily, the first time an entry names an icon, and then only the icons the
 * menu names are fetched. `check:bundle` holds first load to a handful of
 * Lucide modules.
 *
 * **A name resolves as it did** when it was looked up in the namespace:
 * converted to PascalCase (`file-cog` → `FileCog`; a PascalCase name passes
 * through), then matched to the icon file of that name, else to an old name
 * the package still exports (`alert-triangle` → `triangle-alert`), read from a
 * small alias map that is loaded only for such a name. A name that is neither
 * draws the neutral `circle`, rather than nothing: the icon set is the
 * client's, and a definition outlives a release of it.
 */

function pascal(name: string): string {
  return name
    .split('-')
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join('')
}

type Loader = () => Promise<Component>

let byName: Promise<Map<string, Loader>> | null = null

/** `FileCog` → `file-cog.mjs`'s loader, keyed as the namespace lookup keyed it. */
function loaders(): Promise<Map<string, Loader>> {
  byName ??= import('./menuIconModules').then(
    ({ ICON_MODULES }) =>
      new Map(
        Object.entries(ICON_MODULES).map(([path, load]) => [
          pascal(path.slice(path.lastIndexOf('/') + 1, -'.mjs'.length)),
          load,
        ]),
      ),
  )

  return byName
}

async function load(name: string): Promise<Component> {
  const byFile = await loaders()
  const own = byFile.get(name)

  if (own) {
    return own()
  }

  const aliases = (await import('virtual:lucide-icon-aliases')).default
  const file = aliases[name]
  const aliased = file ? byFile.get(pascal(file)) : undefined

  return aliased ? aliased() : Circle
}

const resolved = new Map<string, Component>()

/** The component for a configured entry's icon name; `Circle` for none. */
export function menuIcon(iconName: string | null): Component {
  const name = pascal((iconName ?? '').trim())

  if (name === '') {
    return Circle
  }

  let component = resolved.get(name)

  if (!component) {
    component = defineAsyncComponent({ loader: () => load(name), errorComponent: Circle })
    resolved.set(name, component)
  }

  return component
}
