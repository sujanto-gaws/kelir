import { flushPromises, mount } from '@vue/test-utils'
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'

import purchaseRequisition from '../__fixtures__/purchase-requisition.json'
import JfssForm from '../JfssForm.vue'
import { loadEvaluator } from '@/lib/jsonlogic'
import type { JfssDefinition } from '@/types/jfss'

/**
 * A rendered form evaluating its own rules (#163).
 *
 * Mounted through [`JfssForm`](../JfssForm.vue) against the same fixture
 * `JfssRenderer.spec.ts` renders, because the thing under test is a *form*
 * doing arithmetic on its own payload — a composable tested against a plain
 * object would pass on a version that never reached a field.
 *
 * **These do not replace the browser harness** (#163 AC5). They cover the rules
 * over a payload; `e2e/tests/a-form-calculates-and-validates.spec.ts` is where
 * a real browser types a quantity and watches a total move.
 *
 * **The engine is the real one.** `loadEvaluator` resolves the same
 * `@goplasmatic/datalogic-wasm` build the browser gets, so `sum` here is the
 * custom operator `lib/jsonlogic.ts` registers and not a stand-in — which is
 * what AC2 asks for and what a mock would have quietly replaced.
 */

/**
 * The engine is the real one; only the *call* to fetch it is observed.
 *
 * D-10's bundle condition is a property of the chunk graph and no unit test can
 * see one — `scripts/check-bundle-split.mjs` is what holds it. What is
 * observable here is the tighter property the composable adds on top: a form
 * with no expression in it does not reach for 588 KB to discover that.
 */
const evaluatorLoads = vi.hoisted(() => vi.fn())

vi.mock('@/lib/jsonlogic', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/jsonlogic')>()

  return {
    ...actual,
    loadEvaluator: () => {
      evaluatorLoads()

      return actual.loadEvaluator()
    },
  }
})

vi.mock('@/api/rad', () => ({
  getForm: vi.fn(),
  listLookupOptions: vi.fn().mockResolvedValue({
    items: [{ value: 'supplier-1', label: 'Acme Supplies', description: 'SUP-0001' }],
    meta: { page: 1, pageSize: 20, total: 1, totalPages: 1 },
  }),
}))

/**
 * The engine, before any test mounts ([#266]).
 *
 * **`render` below waits two `flushPromises` and its comment says it waits for
 * the engine.** It cannot: `loadEvaluator` instantiates WebAssembly, and no
 * number of microtask flushes bounds that. The mock above spies on the loader
 * and delegates to the real one, so these tests run the real engine and raced
 * it — usually winning, and on a loaded machine sometimes not, which surfaced
 * as whole groups of calculation and conditional tests failing on values.
 *
 * `loadEvaluator` resolves a module-level promise the mock and the composable
 * share, so awaiting it once here means every later mount finds the engine
 * already there.
 *
 * **The spy is unaffected.** The one describe block that counts calls clears
 * the spy in its own `beforeEach`, so this call is not in any count — including
 * the test asserting a form with no expression never reaches for the engine.
 *
 * [#266]: https://github.com/sujanto-gaws/kelir/issues/266
 */
beforeAll(async () => {
  await loadEvaluator()
})

const requisition = purchaseRequisition as unknown as JfssDefinition

/** Mounts, and waits for the first calculation pass — the engine is already
 * here, which `beforeAll` above is what makes true. */
async function render(
  definition: JfssDefinition = requisition,
  initialValues?: Record<string, unknown>,
) {
  const wrapper = mount(JfssForm, { props: { definition, initialValues } })

  // Twice: the first settles the composable's `.then` on the already-resolved
  // engine promise, the second the calculation pass it wakes
  // that the engine's arrival wakes.
  await flushPromises()
  await flushPromises()

  return wrapper
}

function valueOf(wrapper: Awaited<ReturnType<typeof render>>, selector: string): string {
  return (wrapper.find(selector).element as HTMLInputElement).value
}

async function type(
  wrapper: Awaited<ReturnType<typeof render>>,
  selector: string,
  value: string,
): Promise<void> {
  await wrapper.find(selector).setValue(value)
  await flushPromises()
}

describe('calculate, JFSS §4.2.3 Case B — derived', () => {
  it('computes a line total inside a repeater row, against that row', async () => {
    const wrapper = await render()

    await type(wrapper, '#jfss-row-0-line-quantity', '3')
    await type(wrapper, '#jfss-row-0-line-unit-price', '12.5')

    // The row's own `unit_price` and `quantity`, not a top-level field of that
    // name — §4.3.1's rule about what a template's keys address.
    expect(valueOf(wrapper, '#jfss-row-0-line-total')).toBe('37.5')
  })

  it('computes the registry §6.1 invoice total over sum and map', async () => {
    const wrapper = await render()

    await type(wrapper, '#jfss-row-0-line-quantity', '2')
    await type(wrapper, '#jfss-row-0-line-unit-price', '10')
    await wrapper.find('button.bg-secondary').trigger('click')
    await flushPromises()
    await type(wrapper, '#jfss-row-1-line-quantity', '2')
    await type(wrapper, '#jfss-row-1-line-unit-price', '11')

    // 42, which is the figure the whole Tamper-Proof argument is built on — and
    // the figure the operator-parity spike watched become 0 on an engine that
    // returned unknown operators instead of rejecting them.
    expect(valueOf(wrapper, '#jfss-grand-total-field')).toBe('42')
  })

  it('recomputes when a row is removed, not only when one is typed into', async () => {
    const wrapper = await render()

    await type(wrapper, '#jfss-row-0-line-quantity', '2')
    await type(wrapper, '#jfss-row-0-line-unit-price', '10')
    expect(valueOf(wrapper, '#jfss-grand-total-field')).toBe('20')

    await wrapper
      .findAll('button')
      .find((button) => button.text() === 'Remove')!
      .trigger('click')
    await flushPromises()

    expect(valueOf(wrapper, '#jfss-grand-total-field')).toBe('0')
  })

  it('emits change with the calculations already applied', async () => {
    // `flush: 'post'`, and the reason for it: a listener watching for a total
    // would otherwise be handed the total from before the row that changed.
    const wrapper = await render()

    await type(wrapper, '#jfss-row-0-line-quantity', '2')
    await type(wrapper, '#jfss-row-0-line-unit-price', '10')

    const changes = wrapper.emitted('change')!
    const payload = changes[changes.length - 1][0] as Record<string, unknown>

    expect(payload.grand_total).toBe(20)
  })

  it('makes a derived field read-only whether or not the definition says so', async () => {
    const wrapper = await render()

    // Case B: *"It MUST be `readOnly`"*. The fixture's `grand_total` carries no
    // `readOnly` property at all, so a renderer that only honoured the
    // definition would offer an input whose value is overwritten as it is typed
    // into.
    expect(requisition.components.length).toBeGreaterThan(0)
    expect(wrapper.find('#jfss-grand-total-field').attributes('disabled')).toBeDefined()
    expect(wrapper.find('#jfss-row-0-line-total').attributes('disabled')).toBeDefined()
  })

  it('overwrites an existing payload value, because the computed value always wins', async () => {
    // Case B has no priority list — no other source may ever take effect. A
    // stored grand total of 999 against one line worth 20 is exactly the shape
    // the Tamper-Proof Pattern exists for.
    const wrapper = await render(requisition, {
      grand_total: 999,
      line_items: [{ line_no: 1, description: 'A', quantity: 2, unit_price: 10, line_total: 0 }],
    })

    expect(valueOf(wrapper, '#jfss-grand-total-field')).toBe('20')
  })
})

describe('calculate, JFSS §4.2.3 Case C — generated', () => {
  it('resolves once and does not follow the field it read', async () => {
    const wrapper = await render()

    // `baseline_budget` reads `budget` under `calculateMode: "generated"`, so
    // it takes a figure and keeps it. `grand_total` beside it is `derived` and
    // tracks continuously — same payload, two modes, two behaviours, and the
    // mode is **declared** rather than inferred from the operators (S8.1.1).
    await type(wrapper, '#jfss-budget-field', '5000')

    expect(valueOf(wrapper, '#jfss-budget-field')).toBe('5000')
    expect(valueOf(wrapper, '#jfss-baseline-field')).toBe('0')
  })

  it('never recomputes a persisted value', async () => {
    // Case C priority 1: *"A persisted generated value is **never** recomputed
    // or overwritten"*. Re-opening a document must not renumber it.
    const wrapper = await render(requisition, { baseline_budget: 1200, budget: 90 })

    expect(valueOf(wrapper, '#jfss-baseline-field')).toBe('1200')
  })

  it('falls back to defaultValue when the expression yields nothing', async () => {
    // Case C priority 3, and the branch that has to sit *before* §7.3's numeric
    // wrapper: the wrapper turns a null into 0, and a 0 that arrives that way
    // is indistinguishable from a computed one, so the default would never be
    // reached.
    const definition: JfssDefinition = {
      formId: 'generated',
      version: '2.0.1',
      components: [
        {
          id: 'g',
          role: 'data',
          type: 'number',
          key: 'g',
          label: 'G',
          defaultValue: 7,
          calculate: { var: 'nothing_here' },
          calculateMode: 'generated',
          validation: { type: 'number' },
        },
      ],
    }

    expect(valueOf(await render(definition), '#jfss-g')).toBe('7')
  })
})

describe('a calculation that produces nothing', () => {
  const average: JfssDefinition = {
    formId: 'average',
    version: '2.0.1',
    components: [
      {
        id: 'total',
        role: 'data',
        type: 'number',
        key: 'total',
        label: 'Total',
        validation: { type: 'number' },
      },
      {
        id: 'count',
        role: 'data',
        type: 'number',
        key: 'count',
        label: 'Count',
        validation: { type: 'number' },
      },
      {
        id: 'average',
        role: 'data',
        type: 'number',
        key: 'average',
        label: 'Average',
        calculate: { '/': [{ var: 'total' }, { var: 'count' }] },
        validation: { type: 'number' },
      },
    ],
  }

  /**
   * Construction plan §5.6, and the reason **D-24** needed a rendering rule
   * beside it.
   *
   * A division by zero fails evaluation rather than yielding 0, and on a
   * zero-filled payload it fails **before the first keystroke** — `count` is
   * still the 0 S12.4 put there. So the field is blank and nothing about it
   * blocks typing: *a form that greets its user with three red fields it filled
   * in itself is a worse failure than the one D-24 prevented*.
   */
  it('renders blank, with no error and nothing blocking typing', async () => {
    const wrapper = await render(average)

    expect(valueOf(wrapper, '#jfss-average')).toBe('')
    expect(wrapper.find('[data-testid="field-error"]').exists()).toBe(false)
    expect(wrapper.find('#jfss-total').attributes('disabled')).toBeUndefined()
  })

  it('fills in as soon as the payload supports it', async () => {
    const wrapper = await render(average)

    await type(wrapper, '#jfss-total', '90')
    await type(wrapper, '#jfss-count', '4')

    expect(valueOf(wrapper, '#jfss-average')).toBe('22.5')
  })
})

describe('conditional, JFSS §7', () => {
  it('opens a branch the payload asks for and closes it again', async () => {
    const wrapper = await render()

    expect(wrapper.find('#jfss-justification-field').exists()).toBe(false)

    await type(wrapper, '#jfss-budget-field', '5000')
    expect(wrapper.find('#jfss-justification-field').exists()).toBe(true)

    await type(wrapper, '#jfss-budget-field', '10')
    expect(wrapper.find('#jfss-justification-field').exists()).toBe(false)
  })

  it('keeps a hidden field in the payload', async () => {
    // S10.1.1: every data key is submitted, visible or not, and the server
    // discards the hidden ones. Omitting them makes the two engines evaluate
    // the same condition against different data.
    const wrapper = await render()

    await type(wrapper, '#jfss-title-field', 'x')

    const changes = wrapper.emitted('change')!
    const payload = changes[changes.length - 1][0] as Record<string, unknown>

    expect(Object.keys(payload)).toContain('justification')
  })

  it('disables rather than hides where the action says so', async () => {
    const definition: JfssDefinition = {
      formId: 'disable',
      version: '2.0.1',
      components: [
        {
          id: 'locked',
          role: 'data',
          type: 'checkbox',
          key: 'locked',
          label: 'Locked',
          validation: { type: 'boolean' },
        },
        {
          id: 'note',
          role: 'data',
          type: 'textfield',
          key: 'note',
          label: 'Note',
          conditional: { action: 'disable', logic: { var: 'locked' } },
          validation: { type: 'string' },
        },
      ],
    }

    const wrapper = await render(definition)

    expect(wrapper.find('#jfss-note').attributes('disabled')).toBeUndefined()

    await wrapper.find('#jfss-locked').setValue(true)
    await flushPromises()

    expect(wrapper.find('#jfss-note').attributes('disabled')).toBeDefined()
  })
})

describe('validate, and what it does to submitting', () => {
  async function submit(wrapper: Awaited<ReturnType<typeof render>>): Promise<void> {
    await wrapper
      .findAll('button')
      .find((button) => button.text() === 'Submit request')!
      .trigger('click')
    await flushPromises()
  }

  it('shows nothing until the form is submitted', async () => {
    // Construction plan §5.6's argument applied to `required`: a form that
    // greets its user with red boxes it drew itself has told them nothing.
    const wrapper = await render()

    expect(wrapper.find('[data-testid="field-error"]').exists()).toBe(false)
  })

  it('blocks the submit and shows the message the definition supplies', async () => {
    const wrapper = await render()

    await submit(wrapper)

    // `title` is required and its `validation.messages.required` is the
    // definition's own words. #163 AC1 asks for *the definition's* message, not
    // a message the renderer invented.
    expect(wrapper.find('#jfss-title-field-error').text()).toBe('Every request needs a title.')
    expect(wrapper.emitted('action')).toBeUndefined()
  })

  it('points the control at the message so a screen reader hears it', async () => {
    const wrapper = await render()

    await submit(wrapper)

    const describedBy = wrapper.find('#jfss-title-field').attributes('aria-describedby')

    // Both ids: dropping the help text as soon as something goes wrong removes
    // the sentence explaining how to put it right.
    expect(describedBy).toContain('jfss-title-field-error')
    expect(describedBy).toContain('jfss-title-field-description')
    expect(wrapper.find('#jfss-title-field').attributes('aria-invalid')).toBe('true')
  })

  it('lets the submit through once the form is acceptable', async () => {
    const wrapper = await render()

    await type(wrapper, '#jfss-title-field', 'Two standing desks')
    await type(wrapper, '#jfss-supplier-field', 'supplier-1')
    await type(wrapper, '#jfss-priority-field', '1')
    await type(wrapper, '#jfss-row-0-line-description', 'A desk')
    await type(wrapper, '#jfss-row-0-line-quantity', '2')

    await submit(wrapper)

    expect(wrapper.emitted('action')).toBeTruthy()
    expect(wrapper.emitted('action')![0][0]).toBe('submit')
  })

  it('clears a message as soon as the field is fixed', async () => {
    const wrapper = await render()

    await submit(wrapper)
    expect(wrapper.find('#jfss-title-field-error').exists()).toBe(true)

    await type(wrapper, '#jfss-title-field', 'Two standing desks')
    expect(wrapper.find('#jfss-title-field-error').exists()).toBe(false)
  })

  it('does not block an action that is not a submit', async () => {
    // Refusing to let somebody leave a form because a field they never reached
    // is empty would be a worse form than one with no validation at all.
    const definition: JfssDefinition = {
      formId: 'cancel',
      version: '2.0.1',
      components: [
        {
          id: 'n',
          role: 'data',
          type: 'textfield',
          key: 'n',
          label: 'N',
          validation: { type: 'string', required: true },
        },
        { id: 'c', role: 'action', type: 'button', label: 'Cancel', action: 'cancel' },
      ],
    }

    const wrapper = await render(definition)

    await wrapper
      .findAll('button')
      .find((button) => button.text() === 'Cancel')!
      .trigger('click')

    expect(wrapper.emitted('action')![0][0]).toBe('cancel')
  })

  it('does not let a hidden required field block the submit', async () => {
    // `justification` is required and its conditional keeps it closed while the
    // budget is small. S10.1.1 submits it anyway and the server discards it; a
    // client that refused over a box nobody can see would make the form
    // unsubmittable with no way to find out why.
    const wrapper = await render()

    await type(wrapper, '#jfss-title-field', 'Two standing desks')
    await type(wrapper, '#jfss-supplier-field', 'supplier-1')
    await type(wrapper, '#jfss-priority-field', '1')
    await type(wrapper, '#jfss-row-0-line-description', 'A desk')
    await type(wrapper, '#jfss-row-0-line-quantity', '2')

    await submit(wrapper)
    expect(wrapper.emitted('action')).toBeTruthy()

    // And the moment the branch opens, it counts.
    await type(wrapper, '#jfss-budget-field', '5000')
    await submit(wrapper)

    expect(wrapper.find('#jfss-justification-field-error').text()).toBe(
      'A request over 1,000 needs a justification.',
    )
  })

  it('decides an advanced rule against the value beside it', async () => {
    const wrapper = await render()

    await type(wrapper, '#jfss-cost-centre-field', 'fn-142')
    await submit(wrapper)

    expect(wrapper.find('#jfss-cost-centre-field-error').text()).toBe(
      'A cost centre looks like FN-0142.',
    )

    await type(wrapper, '#jfss-cost-centre-field', 'FN-0142')
    expect(wrapper.find('#jfss-cost-centre-field-error').exists()).toBe(false)
  })

  it('puts a row field message under that row', async () => {
    const wrapper = await render()

    await type(wrapper, '#jfss-row-0-line-quantity', '0')
    await submit(wrapper)

    expect(wrapper.find('#jfss-row-0-line-quantity-error').text()).toBe(
      'A line orders at least one.',
    )
  })
})

describe('a rule name nobody defines (#163 AC3)', () => {
  const mistyped: JfssDefinition = {
    formId: 'mistyped',
    version: '2.0.1',
    components: [
      {
        id: 'p',
        role: 'data',
        type: 'textfield',
        key: 'p',
        label: 'P',
        validation: { type: 'string' },
        rules: [
          { rule: 'matchesFeild', scope: 'both', params: { target: 'q' }, message: 'No match.' },
        ],
      },
      { id: 's', role: 'action', type: 'button', label: 'Submit', action: 'submit' },
    ],
  }

  it('is reported on the form rather than swallowed', async () => {
    const wrapper = await render(mistyped)

    expect(wrapper.find('[data-testid="form-defect"]').text()).toContain('matchesFeild')
  })

  it('makes the form unsubmittable, because nothing checked it', async () => {
    // A skipped branch would have reported this rule as passed. The renderer
    // does not get to say a `both`-scoped check held on a form where it never
    // ran.
    const wrapper = await render(mistyped)

    await wrapper
      .findAll('button')
      .find((button) => button.text() === 'Submit')!
      .trigger('click')
    await flushPromises()

    expect(wrapper.emitted('action')).toBeUndefined()
  })

  it('still renders the rest of the form', async () => {
    expect((await render(mistyped)).find('#jfss-p').exists()).toBe(true)
  })
})

describe('a rule the registry defines and this side does not decide', () => {
  it('says so on the form rather than passing quietly', async () => {
    const definition: JfssDefinition = {
      formId: 'strength',
      version: '2.0.1',
      components: [
        {
          id: 'pw',
          role: 'data',
          type: 'textfield',
          key: 'pw',
          label: 'Password',
          validation: { type: 'string' },
          rules: [
            {
              rule: 'passwordStrength',
              scope: 'client',
              params: { minScore: 3 },
              message: 'Too weak.',
            },
          ],
        },
      ],
    }

    const wrapper = await render(definition)

    expect(wrapper.find('[data-testid="form-undecided"]').text()).toContain('passwordStrength')
  })

  it('says nothing about a server-scoped rule, which is §3.3 working', async () => {
    const definition: JfssDefinition = {
      formId: 'unique',
      version: '2.0.1',
      components: [
        {
          id: 'e',
          role: 'data',
          type: 'textfield',
          key: 'e',
          label: 'Email',
          validation: { type: 'string' },
          rules: [
            {
              rule: 'unique',
              scope: 'server',
              params: { table: 'users', column: 'email' },
              message: 'Already registered.',
            },
          ],
        },
      ],
    }

    const wrapper = await render(definition)

    expect(wrapper.find('[data-testid="form-undecided"]').exists()).toBe(false)
  })
})

describe('a display that computes its own text (JFSS §4.4)', () => {
  it('shows what the expression produced, and formats nothing', async () => {
    const wrapper = await render()

    await type(wrapper, '#jfss-row-0-line-quantity', '2')
    await type(wrapper, '#jfss-row-0-line-unit-price', '10')

    // `20`, not `Rp 20.00`. Formatting decided inside a renderer is the
    // reference implementation's defect and #162 AC3's stated anti-pattern.
    expect(wrapper.text()).toContain('20')
  })
})

describe("D-10's bundle condition, from the side a unit test can see", () => {
  beforeEach(() => {
    evaluatorLoads.mockClear()
  })

  it('reaches for the engine on a definition that carries an expression', async () => {
    await render()

    expect(evaluatorLoads).toHaveBeenCalled()
  })

  it('does not reach for it on a definition with nothing to evaluate', async () => {
    await render({
      formId: 'plain',
      version: '2.0.1',
      components: [
        {
          id: 'a',
          role: 'data',
          type: 'textfield',
          key: 'a',
          label: 'A',
          validation: { type: 'string', required: true },
        },
      ],
    })

    expect(evaluatorLoads).not.toHaveBeenCalled()
  })
})
