import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

import { describe, expect, it } from 'vitest'

/**
 * What the form builder must not bring in, and where its one new dependency
 * may be reached from (#688 E4, F1, G1, G2; **D-86**; ADR-0046).
 *
 * Read from the source and the manifests, not from the bundle: `check:bundle`
 * holds the bundle, and ESLint's `no-restricted-imports` holds the imports.
 * This holds the things neither sees — a name that is not an import, a
 * manifest pin, and which files may import the drag-and-drop library.
 *
 * # Seen to fail (coding standard §2.9)
 *
 * Run 2026-10-10: `vue-draggable-plus` imported from `FormBuilderPage.vue`
 * reddens *imports the drag library from the two canvas components only*;
 * `"^0.6.1"` in `package.json` reddens *pins vue-draggable-plus exactly*.
 */

const sources = import.meta.glob<string>('/src/**/*.{ts,vue}', {
  query: '?raw',
  import: 'default',
  eager: true,
})

/** Application code: no spec, nothing under `lib/testing`. */
const application = Object.entries(sources).filter(
  ([path]) => !path.endsWith('.spec.ts') && !path.startsWith('/src/lib/testing/'),
)

const manifest = JSON.parse(readFileSync(resolve(process.cwd(), 'package.json'), 'utf8')) as {
  dependencies: Record<string, string>
  devDependencies: Record<string, string>
}
const lockfile = JSON.parse(readFileSync(resolve(process.cwd(), 'package-lock.json'), 'utf8')) as {
  packages: Record<string, { version?: string; dependencies?: Record<string, string> }>
}

function importers(pattern: RegExp): string[] {
  return application.filter(([, text]) => pattern.test(text)).map(([path]) => path)
}

describe('the form builder’s boundaries', () => {
  it('reads some application source, so the checks below are not vacuous', () => {
    expect(application.length).toBeGreaterThan(100)
    expect(application.some(([path]) => path.endsWith('FormBuilderPage.vue'))).toBe(true)
  })

  it('imports the E3 meta-schema helper from tests alone (E4, ADR-0035)', () => {
    expect(importers(/lib\/testing\/jfss-meta-schema/)).toEqual([])
    expect(importers(/from ['"]ajv['"]/)).toEqual([])
  })

  it.each([
    ['zod', /from ['"]zod['"]/],
    ['json-logic-js', /json-logic-js['"]/],
    ['customEndpoint', /customEndpoint/],
    ['maxFileSize', /maxFileSize/],
    ['matchesField.targetField', /targetField/],
    ['a re-registered map', /add_operation|addOperation/],
  ])('has no %s in application source (F1, D-86)', (_name, pattern) => {
    expect(importers(pattern)).toEqual([])
  })

  it.each(['zod', 'json-logic-js', 'vuedraggable', 'lucide-vue-next', 'elkjs', 'dagre'])(
    'declares no %s dependency (F1, H1)',
    (name) => {
      expect(manifest.dependencies).not.toHaveProperty(name)
      expect(manifest.devDependencies).not.toHaveProperty(name)
    },
  )

  it('pins vue-draggable-plus exactly at 0.6.1, and the lockfile agrees (G1)', () => {
    expect(manifest.dependencies['vue-draggable-plus']).toBe('0.6.1')
    expect(lockfile.packages[''].dependencies?.['vue-draggable-plus']).toBe('0.6.1')
    expect(lockfile.packages['node_modules/vue-draggable-plus']?.version).toBe('0.6.1')
  })

  it('pins @lucide/vue exactly, and the lockfile carries no lucide-vue-next (H1)', () => {
    const pin = manifest.dependencies['@lucide/vue']

    expect(pin).toMatch(/^\d+\.\d+\.\d+$/)
    expect(lockfile.packages['node_modules/@lucide/vue']?.version).toBe(pin)
    expect(lockfile.packages).not.toHaveProperty('node_modules/lucide-vue-next')
    expect(importers(/lucide-vue-next/)).toEqual([])
  })

  it('imports the drag library from the two canvas components only (G2)', () => {
    expect(importers(/from ['"]vue-draggable-plus['"]/)).toEqual([
      '/src/features/admin/form-builder/FormBuilderPalette.vue',
      '/src/features/admin/form-builder/FormCanvasList.vue',
    ])
  })

  it('reaches the canvas components from the form builder page alone (G2)', () => {
    const outside = (name: string) =>
      importers(new RegExp(`${name}\\.vue['"]`)).filter(
        (path) => !path.startsWith('/src/features/admin/form-builder/'),
      )

    expect(outside('FormBuilderPalette')).toEqual(['/src/features/admin/FormBuilderPage.vue'])
    expect(outside('FormBuilderCanvas')).toEqual(['/src/features/admin/FormBuilderPage.vue'])
    expect(outside('FormCanvasList')).toEqual([])
    expect(outside('FormCanvasNode')).toEqual([])
  })
})
