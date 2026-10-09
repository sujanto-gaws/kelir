import type { AssigneeType, AssignmentRule } from '@/types/workflow'

/**
 * An assignment rule as the editor reads it (JWSS §5).
 *
 * `allowedBy` may be a §5.2 shorthand string. It is **read** as the rule it
 * normalises to, and **left as written** until somebody edits it: the editor
 * then writes the object form, which every validator accepts and the engine
 * normalises to anyway. A string that is not a shorthand JWSS names is shown
 * as itself, so it is not mistaken for no rule.
 */
export type ReadRule =
  { kind: 'none' } | { kind: 'rule'; rule: AssignmentRule } | { kind: 'unreadable'; text: string }

export function readRule(value: AssignmentRule | string | undefined): ReadRule {
  if (value === undefined) {
    return { kind: 'none' }
  }

  if (typeof value !== 'string') {
    return { kind: 'rule', rule: value }
  }

  if (value === 'OWNER') {
    return { kind: 'rule', rule: { assigneeType: 'OWNER' } }
  }

  const shorthand = /^(ROLE|USER):(.+)$/.exec(value)

  if (shorthand) {
    return {
      kind: 'rule',
      rule:
        shorthand[1] === 'ROLE'
          ? { assigneeType: 'ROLE', roleCode: shorthand[2] }
          : { assigneeType: 'USER', userId: shorthand[2] },
    }
  }

  return { kind: 'unreadable', text: value }
}

/** Which of a rule's own fields a type uses (JWSS §5.1). */
const FIELDS_BY_TYPE: Record<AssigneeType, readonly (keyof AssignmentRule)[]> = {
  USER: ['userId'],
  ROLE: ['roleCode'],
  DEPARTMENT_ROLE: ['roleCode', 'departmentScope'],
  OWNER: [],
  MANAGER_OF_OWNER: [],
  EXPRESSION: ['expression'],
}

/**
 * The rule after its type changes: the type's own fields kept from the old
 * rule, so switching `ROLE` to `DEPARTMENT_ROLE` keeps the role, and the
 * others left out, so a `USER` does not carry a stale `roleCode`.
 */
export function retype(
  rule: AssignmentRule | undefined,
  assigneeType: AssigneeType,
): AssignmentRule {
  const next: AssignmentRule = { assigneeType }

  for (const field of FIELDS_BY_TYPE[assigneeType]) {
    const kept = rule?.[field]

    if (kept !== undefined) {
      Object.assign(next, { [field]: kept })
    } else if (field === 'roleCode' || field === 'userId') {
      // Present and blank, so the input is there and the server names it.
      Object.assign(next, { [field]: '' })
    }
  }

  return next
}

/**
 * The fields, relative to the rule, the editor draws a message under for this
 * value: the type chooser (which also takes one addressed to the rule itself)
 * and the type's own inputs.
 */
export function drawnRuleFields(value: AssignmentRule | string | undefined): string[] {
  const read = readRule(value)

  if (read.kind !== 'rule') {
    return ['']
  }

  return ['', 'assigneeType', ...FIELDS_BY_TYPE[read.rule.assigneeType]]
}
