import { mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, describe, expect, it } from 'vitest'

import type { ValidationDetail } from '@/types/api'
import type { JfssAdvancedRule } from '@/types/jfss'

import ValidationRulesSection from './ValidationRulesSection.vue'

/**
 * The rules section's flag on a required parameter (#722 AC15): missing, empty
 * or of the wrong type, under the parameters box, from the moment *Add rule*
 * seeds it. The table it reads is pinned in `propertyEdits.spec.ts`; this file
 * pins that the card shows it, and shows one message at a time.
 *
 * **The flag does not block the save**: the page's save of a freshly seeded
 * rule is driven in `FormBuilderPage.canvas.spec.ts` (*writes a rule the
 * meta-schema takes …*).
 *
 * # Seen to fail (coding standard §2.9)
 *
 * Run 2026-10-15 against this file, `propertyEdits.spec.ts`,
 * `FormPropertiesPanel.spec.ts` and `FormBuilderPage.canvas.spec.ts`, each
 * mutation reverted after:
 *
 * | Mutation | Reddened |
 * |---|---|
 * | the flag never shown (`v-else-if="false"`) | every case here that expects a flag (19), and *writes a rule the meta-schema takes …* (`FormBuilderPage.canvas.spec.ts`) |
 * | the flag shown alongside the server's message (`v-if`, not `v-else-if`) | *shows the server's message in place of the flag*, for `RULE_PARAM_MISSING` and `RULE_PARAM_INVALID` |
 * | the seeds not shared: `seededParams` reads its own table, `notOneOf` seeding `{values: [""]}` | *flags each of the five …: notOneOf*, *flags every rule it offers …*, *takes the flag away when notOneOf's …*, *flags every seeded shape …* (`propertyEdits.spec.ts`), *seeds notOneOf …* (`FormPropertiesPanel.spec.ts`) |
 * | the flag's table loses `notOneOf`, the seeds keep it | *flags each of the five …: notOneOf*, *flags every rule it offers …*, *… the wrong type: notOneOf …*, *takes the flag away when notOneOf's …*, *flags every seeded shape …* |
 * | an empty list counted as filled | *flags each of the five …* for `oneOf` and `notOneOf`, *flags every rule it offers …*, *takes the flag away when notOneOf's …*, *shows the flag on a read-only form*, *flags every seeded shape …* |
 * | the type check dropped (`fits` always `true`) | the five *… the wrong type …* cases, *… in place of the flag: RULE_PARAM_INVALID …*, *flags the wrong JSON type …* (`propertyEdits.spec.ts`) |
 * | the flag hidden on a read-only form (`&& !disabled`) | *shows the flag on a read-only form* |
 * | the page's `blocked` also true for a rule whose `target` is empty | *writes a rule the meta-schema takes, flagged and not blocked* (`FormBuilderPage.canvas.spec.ts`) |
 *
 * **A copied table with the same contents survives every case**: no spec can
 * tell two equal tables from one. The cases catch the copy once it drifts.
 */

let mounted: VueWrapper | undefined

afterEach(() => {
  mounted?.unmount()
  mounted = undefined
})

interface Options {
  rules?: JfssAdvancedRule[]
  details?: ValidationDetail[]
  disabled?: boolean
}

/** The section on its own, its rules replaced with what it emits, as the panel does. */
function section(options: Options = {}): VueWrapper {
  const wrapper = mount(ValidationRulesSection, {
    props: {
      componentId: 'f',
      nodePath: 'components.0',
      rules: options.rules ?? [],
      disabled: options.disabled ?? false,
      details: options.details ?? [],
      'onUpdate:rules': (next: JfssAdvancedRule[]) => wrapper.setProps({ rules: next }),
    },
  })

  mounted = wrapper

  return wrapper
}

function rule(name: string, params?: Record<string, unknown>): JfssAdvancedRule {
  return { rule: name, scope: 'both', ...(params ? { params } : {}), message: 'm' }
}

const flag = (wrapper: VueWrapper, at = 0) =>
  wrapper.find(`[data-testid="rule-params-flag-f-${at}"]`)

describe('the flag on a rule’s required parameter (#722 AC15)', () => {
  it.each([
    ['matchesField', 'target is empty: a matchesField rule with no target refuses every value.'],
    [
      'notMatchesField',
      'target is empty: a notMatchesField rule with no target accepts every value.',
    ],
    ['regex', 'pattern is empty: a regex rule with no pattern accepts every value.'],
    ['oneOf', 'values is empty: a oneOf rule with no values refuses every value.'],
    ['notOneOf', 'values is empty: a notOneOf rule with no values accepts every value.'],
  ])('flags each of the five as soon as it is chosen: %s', async (name, text) => {
    const wrapper = section()

    // Added, then chosen: what the author does, before any Save.
    await wrapper.find('[data-testid="add-rule-f"]').trigger('click')
    await wrapper.find('[data-testid="rule-f-0"]').setValue(name)

    expect(flag(wrapper).text()).toBe(`${text} Saving is refused until it is filled in.`)
    expect(flag(wrapper).find('code').text()).toBe(text.split(' ')[0])
  })

  it('flags *Add rule*’s own seed before anything else is touched', async () => {
    const wrapper = section()

    await wrapper.find('[data-testid="add-rule-f"]').trigger('click')

    expect(flag(wrapper).text()).toContain('target is empty')
  })

  it('flags every rule it offers, chosen from another one (one table)', async () => {
    const wrapper = section()

    await wrapper.find('[data-testid="add-rule-f"]').trigger('click')

    const offered = wrapper
      .find('[data-testid="rule-f-0"]')
      .findAll('option')
      .map((option) => option.attributes('value') ?? '')

    expect(offered).toHaveLength(5)

    for (const name of offered) {
      await wrapper.find('[data-testid="rule-f-0"]').setValue(name)
      expect(flag(wrapper).exists(), name).toBe(true)
    }
  })

  it.each([
    ['matchesField', { target: 5 }, 'target must be a text.'],
    ['notMatchesField', { target: ['a'] }, 'target must be a text.'],
    ['regex', { pattern: null }, 'pattern must be a text.'],
    ['oneOf', { values: 'a' }, 'values must be a list.'],
    ['notOneOf', { values: { a: 1 } }, 'values must be a list.'],
  ])('flags a required parameter of the wrong type: %s %j', (name, params, text) => {
    const wrapper = section({ rules: [rule(name, params)] })

    expect(flag(wrapper).text()).toBe(`${text} Saving is refused until it is one.`)
  })

  it('flags a required parameter that is missing, or parameters that are absent', () => {
    const wrapper = section({ rules: [rule('regex', { flags: 'i' }), rule('oneOf')] })

    expect(flag(wrapper, 0).text()).toBe(
      'pattern is missing: a regex rule with no pattern accepts every value. Saving is refused until it is filled in.',
    )
    expect(flag(wrapper, 1).text()).toContain('values is missing')
  })

  it.each([
    ['matchesField', '{"target": "other"}'],
    ['regex', '{"pattern": "^A"}'],
    ['notOneOf', '{"values": ["admin"]}'],
  ])('takes the flag away when %s’s parameter is filled in', async (name, typed) => {
    const wrapper = section()

    await wrapper.find('[data-testid="add-rule-f"]').trigger('click')
    await wrapper.find('[data-testid="rule-f-0"]').setValue(name)
    expect(flag(wrapper).exists()).toBe(true)

    await wrapper.find('[data-testid="rule-params-f-0"]').setValue(typed)

    expect(flag(wrapper).exists()).toBe(false)

    // Emptied again, it is back.
    await wrapper.find('[data-testid="rule-params-f-0"]').setValue('')

    expect(flag(wrapper).text()).toContain('is missing')
  })

  it('does not ask whether a target names a field: that is the server’s', () => {
    const wrapper = section({ rules: [rule('matchesField', { target: 'no_such_field' })] })

    expect(flag(wrapper).exists()).toBe(false)
  })

  it.each([
    ['RULE_PARAM_MISSING', 'target', { target: '' }, true],
    ['RULE_TARGET_NOT_FOUND', 'target', { target: 'gone' }, false],
    ['RULE_PARAM_INVALID', 'values', { values: 'a' }, true],
  ])(
    'shows the server’s message in place of the flag: %s at …params.%s',
    (code, param, params, flagged) => {
      const name = param === 'target' ? 'matchesField' : 'oneOf'
      const details: ValidationDetail[] = [
        {
          path: `definition.components.0.rules.1.params.${param}`,
          rule: name,
          code,
          message: `the server about rule 1's ${param}`,
        },
      ]
      // Rule 0 carries the same parameters, and no detail: its own flag, if any, stays.
      const wrapper = section({ rules: [rule(name, params), rule(name, params)], details })

      expect(wrapper.find('[data-testid="rule-params-error-f-1"]').text()).toBe(
        `the server about rule 1's ${param}`,
      )
      expect(flag(wrapper, 1).exists()).toBe(false)
      expect(wrapper.find('[data-testid="rule-params-error-f-0"]').exists()).toBe(false)
      expect(flag(wrapper, 0).exists()).toBe(flagged)
    },
  )

  it('flags no rule outside the five', () => {
    const wrapper = section({
      rules: [
        rule('passwordStrength', {}),
        rule('futureRule'),
        rule('unique', { table: '', column: '' }),
      ],
    })

    for (const at of [0, 1, 2]) {
      expect(flag(wrapper, at).exists(), String(at)).toBe(false)
    }
  })

  it('shows the flag on a read-only form: it is information, not a control', () => {
    const wrapper = section({ rules: [rule('oneOf', { values: [] })], disabled: true })

    expect(wrapper.find('[data-testid="rule-params-f-0"]').attributes('disabled')).toBeDefined()
    expect(flag(wrapper).text()).toContain('values is empty')
  })
})
