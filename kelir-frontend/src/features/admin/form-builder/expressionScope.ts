import type { LogicVariable } from '@/components/logic-builder/logicTree'
import type { JfssComponent, JfssDefinition } from '@/types/jfss'

/**
 * **What an expression on a node may read** (#688 AC6): the `variables` the
 * properties panel hands `JsonLogicBuilder`.
 *
 * A form has two kinds of scope, as the renderer evaluates it
 * (`useFormEvaluation.ts`, `provideValueScope`):
 *
 * - **the top level**: every data key outside a row template, through any
 *   panel, fieldset, column or tab, a data grid's own key included;
 * - **a row template**: that row's data keys, since a row is evaluated against
 *   its own object (JFSS §4.3.1).
 *
 * A node's scope is the nearest row template it sits in, else the top level.
 * The data grid itself belongs to the scope it sits in, not the one it holds.
 *
 * **Offering is not refusing.** A loaded `var` outside these is kept by the
 * builder and badged; the server's save-time check is the verdict on it.
 */

type Node = Record<string, unknown>

function isIndex(token: string | undefined): boolean {
  return token !== undefined && /^\d+$/.test(token)
}

function listAt(value: unknown): JfssComponent[] {
  return Array.isArray(value) ? (value as JfssComponent[]) : []
}

/**
 * The list a node's expressions are scoped to: the root `components`, or the
 * row template of the nearest data grid the node sits in.
 */
export function scopeListOf(definition: JfssDefinition, nodePath: string): JfssComponent[] {
  const tokens = nodePath.split('.')
  let scope = listAt(definition.components)
  let node: Node | null = null
  let at = 0

  while (at < tokens.length) {
    const [first, second, third, fourth] = tokens.slice(at, at + 4)

    if (first === 'components' && isIndex(second)) {
      const list: JfssComponent[] = node ? listAt(node.components) : listAt(definition.components)

      // Into a data node's `components` is into a row template.
      if (node?.role === 'data') {
        scope = list
      }

      node = (list[Number(second)] as unknown as Node | undefined) ?? null
      at += 2
    } else if (
      node &&
      (first === 'columns' || first === 'tabs') &&
      isIndex(second) &&
      third === 'components' &&
      isIndex(fourth)
    ) {
      const slot = listAt(node[first])[Number(second)] as unknown as Node | undefined

      node = (listAt(slot?.components)[Number(fourth)] as unknown as Node | undefined) ?? null
      at += 4
    } else {
      break
    }

    if (!node) {
      break
    }
  }

  return scope
}

/**
 * The data keys of one scope: through layout containers, never into a row
 * template. First declaration wins, as it does on the server.
 */
export function dataVariables(list: readonly JfssComponent[]): LogicVariable[] {
  const found: LogicVariable[] = []

  const walk = (nodes: readonly unknown[]): void => {
    for (const value of nodes) {
      if (typeof value !== 'object' || value === null) {
        continue
      }

      const node = value as Node

      if (node.role === 'data') {
        const key = node.key

        if (typeof key === 'string' && key !== '' && !found.some((v) => v.path === key)) {
          const validation = node.validation as Node | undefined
          const label = typeof node.label === 'string' && node.label !== '' ? node.label : key

          found.push(
            typeof validation?.type === 'string'
              ? { path: key, label, type: validation.type }
              : { path: key, label },
          )
        }

        // A data node's `components` is a row template: another scope.
        continue
      }

      if (node.role === 'layout') {
        walk(listAt(node.components))

        for (const slots of [node.columns, node.tabs]) {
          for (const slot of listAt(slots) as unknown as Node[]) {
            walk(listAt(slot?.components))
          }
        }
      }
    }
  }

  walk(list)

  return found
}

/** The variables an expression on the node at `nodePath` is offered (AC6). */
export function scopeVariables(definition: JfssDefinition, nodePath: string): LogicVariable[] {
  return dataVariables(scopeListOf(definition, nodePath))
}
