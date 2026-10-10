import { flushPromises, mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi, type Mock } from 'vitest'
import { h, type FunctionalComponent } from 'vue'

/**
 * A configured menu entry's icon, loaded by name (#698, after #688's first-load
 * fix), at the edges `AppLayout.spec.ts` does not reach.
 *
 * Test-engineer campaign, 2026-10-10 (#688 row 11). **The per-icon map is
 * replaced here** by one whose loaders this file controls, so a failed chunk,
 * a name the map must not be asked about, and many entries at once can be
 * seen. The alias map is the real one, built by `vite.config.ts`'s plugin.
 * `menuIcon.ts` caches the map and every resolved name at module level, so each
 * case imports a fresh copy.
 */

const state = vi.hoisted(() => ({
  modules: {} as Record<string, () => Promise<unknown>>,
  /** How many times the per-icon map was read, which is once per fetch of it. */
  reads: 0,
}))

vi.mock('./menuIconModules', () => ({
  get ICON_MODULES() {
    state.reads += 1

    return state.modules
  },
}))

const ICONS = '../../node_modules/@lucide/vue/dist/esm/icons'

/** A functional stand-in for an icon module's component. */
function stub(file: string): FunctionalComponent {
  return () => h('svg', { class: `stub-${file}` })
}

/** A loader per icon file, counting its calls. */
type Loader = Mock<() => Promise<unknown>>

function provide(files: string[], fail: string[] = []): Record<string, Loader> {
  const calls: Record<string, Loader> = {}

  state.modules = Object.fromEntries(
    files.map((file) => {
      calls[file] = vi.fn<() => Promise<unknown>>(() =>
        fail.includes(file)
          ? Promise.reject(new Error(`Failed to fetch dynamically imported module: ${file}`))
          : Promise.resolve(stub(file)),
      )

      return [`${ICONS}/${file}.mjs`, calls[file]]
    }),
  )

  return calls
}

async function draw(names: (string | null)[]) {
  const { menuIcon } = await import('./menuIcon')
  const errors: string[] = []
  const wrapper = mount(
    () =>
      h(
        'ul',
        names.map((name, at) => h('li', { key: at }, [h(menuIcon(name))])),
      ),
    {
      global: {
        config: {
          errorHandler: (error) => {
            errors.push(String(error))
          },
          warnHandler: () => undefined,
        },
      },
    },
  )

  for (let round = 0; round < 6; round += 1) {
    await vi.dynamicImportSettled()
    await flushPromises()
  }

  const classes = wrapper.findAll('li').map((item) => item.find('svg').attributes('class') ?? '')

  return { classes, errors }
}

describe('menuIcon', () => {
  beforeEach(() => {
    vi.resetModules()
    state.modules = {}
    state.reads = 0
  })

  it('draws the neutral circle for no name, an empty one or a blank one, and loads nothing', async () => {
    const calls = provide(['circle', 'file-cog'])
    const { classes, errors } = await draw([null, '', '   '])

    for (const drawn of classes) {
      expect(drawn).toContain('lucide-circle')
    }

    expect(calls['file-cog']).not.toHaveBeenCalled()
    // Not even the map of icons is fetched for an entry that names none.
    expect(state.reads).toBe(0)
    expect(errors).toEqual([])
  })

  it('draws the neutral circle for a name that is a path, and reaches no file by it', async () => {
    const calls = provide(['file-cog', 'x'])
    const { classes, errors } = await draw(['../x', '../../file-cog', 'icons/file-cog'])

    for (const drawn of classes) {
      expect(drawn).toContain('lucide-circle')
    }

    expect(calls.x).not.toHaveBeenCalled()
    expect(calls['file-cog']).not.toHaveBeenCalled()
    expect(errors).toEqual([])
  })

  it('falls back to the circle when an icon’s own chunk fails to load', async () => {
    provide(['file-cog', 'bell'], ['file-cog'])

    const { classes } = await draw(['file-cog', 'bell'])

    expect(classes[0]).toContain('lucide-circle')
    expect(classes[1]).toContain('stub-bell')
  })

  it('fetches each named icon once when many entries draw at once', async () => {
    const calls = provide(['file-cog', 'bell', 'triangle-alert'])
    const { classes, errors } = await draw([
      ...Array.from({ length: 12 }, () => 'file-cog'),
      'FileCog',
      'bell',
      'bell',
      'alert-triangle',
      'triangle-alert',
    ])

    expect(classes.slice(0, 13).every((drawn) => drawn.includes('stub-file-cog'))).toBe(true)
    expect(classes.slice(13, 15)).toEqual(['stub-bell', 'stub-bell'])
    // The old name and the new resolve to the same file.
    expect(classes.slice(15)).toEqual(['stub-triangle-alert', 'stub-triangle-alert'])
    expect(calls['file-cog']).toHaveBeenCalledTimes(1)
    expect(calls.bell).toHaveBeenCalledTimes(1)
    expect(state.reads).toBe(1)
    expect(errors).toEqual([])
  })

  it('draws the neutral circle for a name Object’s own properties answer to', async () => {
    provide(['file-cog'])

    const { classes } = await draw(['constructor', 'toString', 'hasOwnProperty', '__proto__'])

    for (const drawn of classes) {
      expect(drawn).toContain('lucide-circle')
    }
  })

  // DEFECT (test-engineer, 2026-10-10): the alias map is a plain object, so a
  // configured name of `__proto__` (or `__defineGetter__`, …) reads
  // `Object.prototype` as an alias, `pascal()` is called on it, and the loader
  // throws `TypeError: name.split is not a function`. The circle is still
  // drawn — by `errorComponent` — but the error reaches the app's error
  // handler (and `console.error` in production) for a string a tenant typed.
  // Flip to `it` when the lookup is an own-property one.
  it.fails('resolves a name like __proto__ to the circle without raising an error', async () => {
    provide(['file-cog'])

    const { errors } = await draw(['__proto__', '__defineGetter__'])

    expect(errors).toEqual([])
  })

  // `check:bundle` counts `@lucide/vue` modules on first load, and the map of
  // ~1,870 loaders is Kelir code, not the package's: imported statically, it
  // put first load from 93,623 to 158,218 bytes gzipped with the guard green
  // (test-engineer, 2026-10-10). So the one dynamic import is held here.
  it('is the only module that reaches the icon map, and only by a dynamic import', () => {
    const sources = import.meta.glob<string>('/src/**/*.{ts,vue}', {
      query: '?raw',
      import: 'default',
      eager: true,
    })
    const reaching = Object.entries(sources)
      .filter(([path]) => !path.endsWith('.spec.ts'))
      .filter(([, text]) => /menuIconModules['"]/.test(text))

    expect(reaching.map(([path]) => path)).toEqual(['/src/layouts/menuIcon.ts'])

    const text = reaching[0][1]

    expect(text).toMatch(/import\(\s*['"]\.\/menuIconModules['"]\s*\)/)
    expect(text).not.toMatch(/^\s*import\s[^(]*?['"]\.\/menuIconModules['"]/m)
  })
})
