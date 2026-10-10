import { describe, expect, it } from 'vitest'

import { drawnRuleFields, readRule, retype } from './assignmentRule'

/**
 * Reading an assignment rule, and changing its type (JWSS §5).
 *
 * Seen red, 2026-10-09: `retype` keeping every field of the old rule
 * (*keeps the fields the new type uses and drops the rest*).
 */
describe('readRule', () => {
  it('reads each §5.2 shorthand as the rule it normalises to', () => {
    expect(readRule('OWNER')).toEqual({ kind: 'rule', rule: { assigneeType: 'OWNER' } })
    expect(readRule('ROLE:APPROVER')).toEqual({
      kind: 'rule',
      rule: { assigneeType: 'ROLE', roleCode: 'APPROVER' },
    })
    expect(readRule('USER:0199')).toEqual({
      kind: 'rule',
      rule: { assigneeType: 'USER', userId: '0199' },
    })
  })

  it('reads no value as no rule, and a string JWSS does not name as itself', () => {
    expect(readRule(undefined)).toEqual({ kind: 'none' })
    expect(readRule('MANAGER')).toEqual({ kind: 'unreadable', text: 'MANAGER' })
  })
})

describe('retype', () => {
  it('keeps the fields the new type uses and drops the rest', () => {
    expect(retype({ assigneeType: 'ROLE', roleCode: 'APPROVER' }, 'DEPARTMENT_ROLE')).toEqual({
      assigneeType: 'DEPARTMENT_ROLE',
      roleCode: 'APPROVER',
    })
    expect(
      retype(
        { assigneeType: 'DEPARTMENT_ROLE', roleCode: 'APPROVER', departmentScope: 'X' },
        'USER',
      ),
    ).toEqual({ assigneeType: 'USER', userId: '' })
    expect(retype({ assigneeType: 'USER', userId: 'u' }, 'OWNER')).toEqual({
      assigneeType: 'OWNER',
    })
  })
})

describe('drawnRuleFields', () => {
  it('names the inputs drawn for the rule as it is', () => {
    expect(drawnRuleFields('ROLE:APPROVER')).toEqual(['', 'assigneeType', 'roleCode'])
    expect(drawnRuleFields({ assigneeType: 'DEPARTMENT_ROLE', roleCode: 'A' })).toEqual([
      '',
      'assigneeType',
      'roleCode',
      'departmentScope',
    ])
    expect(drawnRuleFields(undefined)).toEqual([''])
  })
})
