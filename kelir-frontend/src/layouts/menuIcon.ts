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
 * **How a name resolves.** It is converted to PascalCase as the namespace
 * lookup converted it (`file-cog` → `FileCog`; a PascalCase name passes
 * through), then matched to the icon file of that name. The package's other
 * spellings of a name, `FileCogIcon` and `LucideFileCog` (so `file-cog-icon`
 * and `lucide-file-cog` too), are matched by their bare name. Then an old name
 * the package still exports (`alert-triangle` → `triangle-alert`), read from a
 * small alias map that is loaded only for such a name, and only by its own
 * keys: `__proto__` is not an alias. A name that is none of these draws the
 * neutral `circle`, rather than nothing: the icon set is the client's, and a
 * definition outlives a release of it.
 *
 * **What the namespace answered and this does not**: an export of the package
 * that is not an icon, such as `Icon` or `createLucideIcon`, which the old
 * lookup drew or tried to draw. Each is a circle here.
 *
 * **A failed fetch of the map is not kept**: the next draw asks again, so a
 * dropped connection on the first draw does not leave every menu icon a circle
 * until the page is reloaded.
 */

/** `FileCogIcon` and `LucideFileCog` → `FileCog`: the package's other spellings. */
function bare(name: string): string {
  return name.replace(/^Lucide(?=[A-Z0-9])/, '').replace(/(?<=.)Icon$/, '')
}

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
  byName ??= import('./menuIconModules')
    .then(
      ({ ICON_MODULES }) =>
        new Map(
          Object.entries(ICON_MODULES).map(([path, load]) => [
            pascal(path.slice(path.lastIndexOf('/') + 1, -'.mjs'.length)),
            load,
          ]),
        ),
    )
    .catch((error: unknown) => {
      // Not cached: the next draw fetches the map again.
      byName = null
      throw error
    })

  return byName
}

async function load(name: string): Promise<Component> {
  const byFile = await loaders()
  const spellings = [...new Set([name, bare(name)])]

  for (const spelling of spellings) {
    const own = byFile.get(spelling)

    if (own) {
      return own()
    }
  }

  const aliases = (await import('virtual:lucide-icon-aliases')).default

  for (const spelling of spellings) {
    const file = Object.prototype.hasOwnProperty.call(aliases, spelling)
      ? aliases[spelling]
      : undefined
    const aliased = file ? byFile.get(pascal(file)) : undefined

    if (aliased) {
      return aliased()
    }
  }

  return Circle
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
