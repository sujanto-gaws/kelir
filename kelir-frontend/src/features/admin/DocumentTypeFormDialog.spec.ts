import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import DocumentTypeFormDialog from './DocumentTypeFormDialog.vue'
import {
  errorBody,
  installFakeBackend,
  itemBody,
  type FakeBackendHandle,
  type FakeReply,
  type RecordedRequest,
} from '@/lib/testing/fake-backend'
import { filler, searchedPage } from '@/lib/testing/searched-page'
import type { DocumentType, WorkflowBinding } from '@/types/document-type'

/**
 * The binding choosers of the document type dialog (#525).
 *
 * Each read one page of a hundred and, for forms and lists, filtered it by
 * status after it arrived — so a form that sorted past the hundredth row could
 * not be bound, and fewer than a hundred were offered with nothing saying so.
 * Every test here seeds more than a hundred rows ahead of the one it wants,
 * with rows of the wrong status among them, against a fake that searches and
 * filters as the endpoint does.
 */

function form(n: string, status = 'PUBLISHED') {
  return {
    id: `f-${n}`,
    formKey: `aa_form_${n}`,
    title: `Filler form ${n}`,
    revision: 1,
    jfssVersion: '2.0.1',
    status,
    entityId: null,
    createdAt: '2026-09-01T00:00:00Z',
    updatedAt: '2026-09-01T00:00:00Z',
  }
}

function list(n: string, status = 'ACTIVE') {
  return {
    id: `l-${n}`,
    listKey: `aa_list_${n}`,
    title: `Filler list ${n}`,
    entityId: null,
    pageSize: 20,
    status,
    createdAt: '2026-09-01T00:00:00Z',
    updatedAt: '2026-09-01T00:00:00Z',
  }
}

function workflow(n: string, status = 'ACTIVE') {
  return {
    id: `w-${n}`,
    workflowKey: `aa_flow_${n}`,
    name: `Filler flow ${n}`,
    version: 1,
    jwssVersion: '1.0.0',
    status,
    initialState: 'DRAFT',
    createdAt: '2026-09-01T00:00:00Z',
    updatedAt: '2026-09-01T00:00:00Z',
  }
}

const wantedForm = { ...form('zz'), formKey: 'zz_requisition', title: 'Requisition' }
const wantedList = { ...list('zz'), listKey: 'zz_requisitions', title: 'Requisitions' }
const wantedFlow = { ...workflow('zz'), workflowKey: 'zz_approval', name: 'Approval' }
/** A revision deprecated after a type was bound to it (#715): no search offers it. */
const retiredFlow = {
  ...workflow('zy'),
  workflowKey: 'zy_retired_approval',
  name: 'Retired approval',
  version: 2,
  status: 'DEPRECATED',
}

/** 105 of each ahead of the wanted row, with drafts and deprecated rows among them. */
const forms = [
  ...filler(105, (n) => form(n)),
  ...filler(10, (n) => form(`d${n}`, 'DRAFT')),
  wantedForm,
]
const lists = [
  ...filler(105, (n) => list(n)),
  ...filler(10, (n) => list(`d${n}`, 'DEPRECATED')),
  wantedList,
]
const flows = [
  ...filler(105, (n) => workflow(n)),
  ...filler(10, (n) => workflow(`d${n}`, 'DRAFT')),
  retiredFlow,
  wantedFlow,
]

function route(request: RecordedRequest): FakeReply {
  if (request.method === 'post') {
    return { status: 201, body: itemBody({ id: 't-new' }) }
  }

  if (request.url === '/rad/forms') {
    return searchedPage(request, forms, ['formKey', 'title'])
  }

  if (request.url === '/rad/lists') {
    return searchedPage(request, lists, ['listKey', 'title'])
  }

  if (request.url === '/workflow/definitions') {
    return searchedPage(request, flows, ['workflowKey', 'name'])
  }

  const one = request.url.match(/^\/rad\/(forms|lists)\/(.+)$/)

  if (one) {
    const found = [...forms, ...lists].find((row) => row.id === one[2])

    return found ? { status: 200, body: itemBody(found) } : { status: 404 }
  }

  const flow = request.url.match(/^\/workflow\/definitions\/(.+)$/)

  if (flow) {
    const found = flows.find((row) => row.id === flow[1])

    return found ? { status: 200, body: itemBody({ ...found, definition: {} }) } : { status: 404 }
  }

  return { status: 404 }
}

describe('DocumentTypeFormDialog choosers', () => {
  let backend: FakeBackendHandle

  beforeEach(() => {
    setActivePinia(createPinia())
    backend = installFakeBackend(route)
  })

  afterEach(() => backend.restore())

  async function open(editing: DocumentType | null = null): Promise<VueWrapper> {
    const wrapper = mount(DocumentTypeFormDialog, { props: { open: true, editing } })
    await flushPromises()

    return wrapper
  }

  function optionsOf(wrapper: VueWrapper, testId: string): string[] {
    return wrapper
      .get(`[data-testid="${testId}"]`)
      .findAll('option')
      .map((option) => option.text())
  }

  async function search(wrapper: VueWrapper, testId: string, text: string): Promise<void> {
    const box = wrapper.get(`[data-testid="${testId}-search"]`)
    await box.setValue(text)
    await box.trigger('keydown', { key: 'Enter' })
    await flushPromises()
  }

  function lastParams(url: string): Record<string, unknown> | undefined {
    return backend.requests.filter((request) => request.url === url).slice(-1)[0]?.params
  }

  it('asks the server for bindable rows only, rather than filtering a page', async () => {
    const wrapper = await open()

    expect(lastParams('/rad/forms')).toEqual({ pageSize: 100, status: 'PUBLISHED' })
    expect(lastParams('/rad/lists')).toEqual({ pageSize: 100, status: 'ACTIVE' })
    expect(lastParams('/workflow/definitions')).toEqual({ pageSize: 100, status: 'ACTIVE' })

    // A full page of what can be bound, and a sentence saying there is more.
    expect(optionsOf(wrapper, 'type-form')).toHaveLength(101)
    expect(wrapper.get('[data-testid="type-form-status"]').text()).toContain('Showing 100 of 106')
    expect(optionsOf(wrapper, 'type-form')).not.toContain('Requisition (r1)')
  })

  it('binds a form, a list and a workflow that each sort past the first hundred', async () => {
    const wrapper = await open()

    await search(wrapper, 'type-form', 'zz_req')
    await search(wrapper, 'type-list', 'requisitions')
    await search(wrapper, 'type-workflow', 'APPROVAL')

    expect(optionsOf(wrapper, 'type-form')).toEqual(['No form', 'Requisition (r1)'])
    expect(optionsOf(wrapper, 'type-list')).toEqual(['No list', 'Requisitions (zz_requisitions)'])
    expect(optionsOf(wrapper, 'type-workflow')).toEqual(['No workflow', 'Approval (r1)'])
    expect(lastParams('/rad/forms')).toEqual({
      search: 'zz_req',
      pageSize: 100,
      status: 'PUBLISHED',
    })

    await wrapper.get('[data-testid="type-form"]').setValue(wantedForm.id)
    await wrapper.get('[data-testid="type-list"]').setValue(wantedList.id)
    await wrapper.get('[data-testid="type-workflow"]').setValue(wantedFlow.id)
    await wrapper.get('[data-testid="type-code"]').setValue('REQ')
    await wrapper.get('[data-testid="type-name"]').setValue('Requisition')
    await wrapper.get('form').trigger('submit')
    await flushPromises()

    const posted = backend.requests.find((request) => request.method === 'post')

    expect(posted?.url).toBe('/document-types')
    expect(posted?.body).toMatchObject({
      formId: wantedForm.id,
      listId: wantedList.id,
      workflows: [{ workflowDefinitionId: wantedFlow.id }],
    })
  })

  /**
   * One binding as the server returns it (`domain::WorkflowBinding`): all five
   * wire fields, and nothing else, because the endpoint denies unknown ones.
   */
  function binding(
    workflowDefinitionId: string,
    rest: Partial<Omit<WorkflowBinding, 'workflowDefinitionId'>> = {},
  ): WorkflowBinding {
    return {
      workflowDefinitionId,
      conditionExpression: null,
      priority: 1,
      validFrom: null,
      validTo: null,
      ...rest,
    }
  }

  /** A type bound to these, as `GET /document-types/{id}` returns it. */
  function boundType(workflowDefinitionId: string): DocumentType {
    return {
      id: 't-1',
      typeCode: 'REQ',
      name: 'Requisition',
      description: null,
      category: null,
      formId: wantedForm.id,
      listId: wantedList.id,
      defaultSecurityLevel: 'INTERNAL',
      retentionPolicyId: null,
      targetEntityType: null,
      status: 'ACTIVE',
      workflows: [binding(workflowDefinitionId)],
      createdAt: '2026-09-01T00:00:00Z',
      updatedAt: '2026-09-01T00:00:00Z',
    }
  }

  function selectedValue(wrapper: VueWrapper, testId: string): string {
    return (wrapper.get(`[data-testid="${testId}"]`).element as HTMLSelectElement).value
  }

  it('names a bound form, list and workflow that no search has returned', async () => {
    const wrapper = await open(boundType(wantedFlow.id))
    await flushPromises()

    expect(selectedValue(wrapper, 'type-form')).toBe(wantedForm.id)
    expect(optionsOf(wrapper, 'type-form')[1]).toBe('Requisition (r1)')
    expect(optionsOf(wrapper, 'type-list')[1]).toBe('Requisitions (zz_requisitions)')

    // 105 active flows sort ahead of it, so the first page does not hold it (#625).
    expect(selectedValue(wrapper, 'type-workflow')).toBe(wantedFlow.id)
    expect(optionsOf(wrapper, 'type-workflow').slice(0, 3)).toEqual([
      'No workflow',
      'Approval (r1)',
      'Filler flow 001 (r1)',
    ])
    // Read once, by its id, rather than by paging through the list.
    expect(
      backend.requests.filter(
        (request) => request.url === `/workflow/definitions/${wantedFlow.id}`,
      ),
    ).toHaveLength(1)
  })

  it('names a bound revision that has since been deprecated as deprecated', async () => {
    const wrapper = await open(boundType(retiredFlow.id))
    await flushPromises()

    expect(selectedValue(wrapper, 'type-workflow')).toBe(retiredFlow.id)
    expect(optionsOf(wrapper, 'type-workflow')[1]).toBe('Retired approval (r2, deprecated)')

    // Searching for it finds nothing, because only an active revision can be
    // bound; the stored choice stays named while the search moves on.
    await search(wrapper, 'type-workflow', 'retired')

    expect(optionsOf(wrapper, 'type-workflow')).toEqual([
      'No workflow',
      'Retired approval (r2, deprecated)',
    ])
    expect(wrapper.get('[data-testid="type-workflow-status"]').text()).toContain(
      'Nothing matches “retired”.',
    )
  })

  /*
   * #625's verification campaign. The tests above prove the two cases the fix
   * names; these probe the seams around them: a read that fails, a value the
   * first page already names, create, clearing, reopening, two bindings and the
   * save that a deprecated binding refuses.
   */

  /** Reads by id of a workflow definition, as opposed to the list. */
  function workflowReads(): string[] {
    return backend.requests
      .filter((request) => /^\/workflow\/definitions\/.+$/.test(request.url))
      .map((request) => request.url)
  }

  /** The route above, with `override` answering first wherever it answers. */
  function replaceBackend(override: (request: RecordedRequest) => FakeReply | undefined): void {
    backend.restore()
    backend = installFakeBackend((request) => override(request) ?? route(request))
  }

  /** An edit's `PUT`, answered as the endpoint answers a successful one. */
  function acceptingPut(request: RecordedRequest): FakeReply | undefined {
    return request.method === 'put' ? { status: 200, body: itemBody({ id: 't-1' }) } : undefined
  }

  /**
   * The edit's `PUT` body as it crossed the wire: the fake records what the
   * adapter was handed, which is the serialised JSON, parsed back. A key the
   * dialog set to `undefined` is therefore absent here, as it is to the server.
   */
  function putBody(): Record<string, unknown> | undefined {
    return backend.requests.find((request) => request.method === 'put')?.body as
      Record<string, unknown> | undefined
  }

  /**
   * Whether the edit sent `workflows` at all. Absent is what leaves the stored
   * set alone (`service::update_type`); any array, `[]` included, replaces it.
   */
  function sentWorkflows(): boolean {
    const sent = putBody()

    expect(sent).toBeDefined()

    return 'workflows' in (sent ?? {})
  }

  it('reads no workflow by id when creating a type', async () => {
    const wrapper = await open()
    await flushPromises()

    expect(selectedValue(wrapper, 'type-workflow')).toBe('')
    expect(optionsOf(wrapper, 'type-workflow')[1]).toBe('Filler flow 001 (r1)')
    expect(workflowReads()).toEqual([])
  })

  it('reads no workflow by id when the first page already names the bound one', async () => {
    const onFirstPage = flows[0]
    const wrapper = await open(boundType(onFirstPage.id))
    await flushPromises()

    expect(selectedValue(wrapper, 'type-workflow')).toBe(onFirstPage.id)
    // Named once, where the page put it, rather than again above the results.
    expect(optionsOf(wrapper, 'type-workflow').slice(0, 3)).toEqual([
      'No workflow',
      'Filler flow 001 (r1)',
      'Filler flow 002 (r1)',
    ])
    expect(workflowReads()).toEqual([])
  })

  it('names a resolved revision as a search names it, and lists it once', async () => {
    const wrapper = await open(boundType(wantedFlow.id))
    await flushPromises()

    const resolved = optionsOf(wrapper, 'type-workflow')[1]

    await search(wrapper, 'type-workflow', 'zz_approval')

    expect(optionsOf(wrapper, 'type-workflow')).toEqual(['No workflow', resolved])
    expect(resolved).toBe('Approval (r1)')
    expect(selectedValue(wrapper, 'type-workflow')).toBe(wantedFlow.id)
  })

  it.each([
    ['deleted since it was bound (404)', { status: 404 }],
    ['not readable by this caller (403)', { status: 403 }],
    ['unreachable', { status: 0, networkError: true }],
  ] as const)(
    'keeps a bound revision whose read fails, %s, and leaves it bound on save',
    async (_, reply) => {
      replaceBackend((request) =>
        request.url === `/workflow/definitions/${wantedFlow.id}` ? reply : acceptingPut(request),
      )
      const wrapper = await open(boundType(wantedFlow.id))
      await flushPromises()

      // Not named, but still chosen, and nothing thrown or announced as broken.
      expect(selectedValue(wrapper, 'type-workflow')).toBe(wantedFlow.id)
      expect(optionsOf(wrapper, 'type-workflow')[1]).toBe(
        'The current choice, outside these results',
      )
      expect(wrapper.find('[data-testid="type-workflow-error"]').exists()).toBe(false)
      expect(wrapper.find('[data-testid="document-type-error"]').exists()).toBe(false)
      // The other choosers are untouched by it.
      expect(optionsOf(wrapper, 'type-form')[1]).toBe('Requisition (r1)')

      await wrapper.get('[data-testid="type-name"]').setValue('Requisition (renamed)')
      await wrapper.get('form').trigger('submit')
      await flushPromises()

      // Left off the wire, so the stored binding stays as it is (#718): the
      // dialog could not read the revision, and a rename is no reason to resend it.
      expect(putBody()?.name).toBe('Requisition (renamed)')
      expect(sentWorkflows()).toBe(false)
      expect(wrapper.emitted('saved')).toHaveLength(1)
    },
  )

  it('unbinds a deprecated revision when the chooser is cleared, and cannot offer it again', async () => {
    replaceBackend(acceptingPut)
    const wrapper = await open(boundType(retiredFlow.id))
    await flushPromises()

    await wrapper.get('[data-testid="type-workflow"]').setValue('')

    expect(selectedValue(wrapper, 'type-workflow')).toBe('')
    expect(optionsOf(wrapper, 'type-workflow')).not.toContain('Retired approval (r2, deprecated)')

    await search(wrapper, 'type-workflow', 'retired')

    expect(optionsOf(wrapper, 'type-workflow')).toEqual(['No workflow'])

    await wrapper.get('form').trigger('submit')
    await flushPromises()

    expect(putBody()?.workflows).toEqual([])
    expect(wrapper.emitted('saved')).toHaveLength(1)
  })

  it('starts each opening from what it is opening, between edit and create', async () => {
    const wrapper = await open(boundType(retiredFlow.id))
    await flushPromises()

    expect(optionsOf(wrapper, 'type-workflow')[1]).toBe('Retired approval (r2, deprecated)')

    // As the list page does it: close, then open with what is to be edited.
    await wrapper.setProps({ open: false })
    await wrapper.setProps({ editing: null })
    await wrapper.setProps({ open: true })
    await flushPromises()

    expect(selectedValue(wrapper, 'type-workflow')).toBe('')
    expect(optionsOf(wrapper, 'type-workflow')).not.toContain('Retired approval (r2, deprecated)')
    expect(workflowReads()).toEqual([`/workflow/definitions/${retiredFlow.id}`])

    await wrapper.setProps({ open: false })
    await wrapper.setProps({ editing: boundType(wantedFlow.id) })
    await wrapper.setProps({ open: true })
    await flushPromises()

    expect(selectedValue(wrapper, 'type-workflow')).toBe(wantedFlow.id)
    expect(optionsOf(wrapper, 'type-workflow').slice(0, 2)).toEqual([
      'No workflow',
      'Approval (r1)',
    ])
    expect(optionsOf(wrapper, 'type-workflow')).not.toContain('Retired approval (r2, deprecated)')
  })

  it('shows the first of two bindings, in the order the server gave them', async () => {
    const wrapper = await open({
      ...boundType(retiredFlow.id),
      workflows: [
        binding(retiredFlow.id, { priority: 10 }),
        binding(wantedFlow.id, { priority: 20, conditionExpression: '{"==":[1,1]}' }),
      ],
    })
    await flushPromises()

    expect(selectedValue(wrapper, 'type-workflow')).toBe(retiredFlow.id)
    expect(optionsOf(wrapper, 'type-workflow')[1]).toBe('Retired approval (r2, deprecated)')
    expect(workflowReads()).toEqual([`/workflow/definitions/${retiredFlow.id}`])
  })

  // Found by #625's campaign (2026-10-10) and pinned `it.fails` until #718: an
  // edit that left the chooser alone sent `workflows: [{ workflowDefinitionId }]`,
  // and the array replaces the stored set (`replace_workflows`), so renaming a
  // type with two bindings deleted the second and dropped the first one's
  // priority and condition.
  it('keeps every stored binding when an edit leaves the workflow chooser alone', async () => {
    const stored = [
      binding(wantedFlow.id, { priority: 10, conditionExpression: '{"==":[1,1]}' }),
      binding(flows[0].id, { priority: 20 }),
    ]
    replaceBackend(acceptingPut)
    const wrapper = await open({ ...boundType(wantedFlow.id), workflows: stored })
    await flushPromises()

    await wrapper.get('[data-testid="type-name"]').setValue('Requisition (renamed)')
    await wrapper.get('form').trigger('submit')
    await flushPromises()

    // Left off the wire, which is what leaves the stored set alone.
    const sent = putBody()

    expect(sent).toBeDefined()
    expect('workflows' in (sent ?? {})).toBe(false)
  })

  /*
   * #718. An edit sends `workflows` only when the chooser changed, and then
   * changes only what the chooser shows; a type whose bindings the chooser
   * cannot show whole is not edited through it at all.
   */

  it.each([
    ['the name', 'type-name', 'Requisition (renamed)', 'name'],
    ['the description', 'type-description', 'Buying things', 'description'],
    ['the category', 'type-category', 'PROCUREMENT', 'category'],
    ['the status', 'type-status', 'DEPRECATED', 'status'],
    ['the governed record', 'type-governs', 'PARTY', 'targetEntityType'],
    ['the security level', 'type-security', 'RESTRICTED', 'defaultSecurityLevel'],
    ['the form', 'type-form', '', 'formId'],
    ['the list', 'type-list', '', 'listId'],
  ] as const)(
    'sends no workflows when an edit changes only %s',
    async (_, testId, value, field) => {
      replaceBackend(acceptingPut)
      const wrapper = await open({
        ...boundType(wantedFlow.id),
        workflows: [binding(wantedFlow.id, { priority: 3, conditionExpression: 'x' })],
      })
      await flushPromises()

      await wrapper.get(`[data-testid="${testId}"]`).setValue(value)
      await wrapper.get('form').trigger('submit')
      await flushPromises()

      expect(putBody()?.[field]).toBe(value === '' ? null : value)
      expect(sentWorkflows()).toBe(false)
      expect(wrapper.emitted('saved')).toHaveLength(1)
    },
  )

  it('sends no workflows when the chooser is changed and changed back', async () => {
    replaceBackend(acceptingPut)
    // Both on the first page, so each stays a choice after the other is made.
    const wrapper = await open(boundType(flows[0].id))
    await flushPromises()

    await wrapper.get('[data-testid="type-workflow"]').setValue(flows[1].id)
    await wrapper.get('[data-testid="type-workflow"]').setValue(flows[0].id)

    expect(selectedValue(wrapper, 'type-workflow')).toBe(flows[0].id)

    await wrapper.get('[data-testid="type-name"]').setValue('Requisition (renamed)')
    await wrapper.get('form').trigger('submit')
    await flushPromises()

    expect(sentWorkflows()).toBe(false)
  })

  it('sends no workflows when a type with none is edited and the chooser left blank', async () => {
    replaceBackend(acceptingPut)
    const wrapper = await open({ ...boundType(wantedFlow.id), workflows: [] })
    await flushPromises()

    await wrapper.get('[data-testid="type-name"]').setValue('Requisition (renamed)')
    await wrapper.get('form').trigger('submit')
    await flushPromises()

    expect(sentWorkflows()).toBe(false)
  })

  it('keeps a binding’s routing when the chooser moves it to another revision', async () => {
    const stored = binding(wantedFlow.id, {
      priority: 7,
      conditionExpression: '{">":[{"var":"formData.amount"},1000]}',
      validFrom: '2026-01-01',
      validTo: '2026-12-31',
    })
    replaceBackend(acceptingPut)
    const wrapper = await open({ ...boundType(wantedFlow.id), workflows: [stored] })
    await flushPromises()

    await wrapper.get('[data-testid="type-workflow"]').setValue(flows[0].id)
    await wrapper.get('form').trigger('submit')
    await flushPromises()

    // Only the revision moves; the four fields the dialog does not author are
    // sent back as they were read, and nothing the server would refuse is added.
    expect(putBody()?.workflows).toEqual([{ ...stored, workflowDefinitionId: flows[0].id }])
    expect(wrapper.emitted('saved')).toHaveLength(1)
  })

  it('binds a type that had no binding by the revision alone', async () => {
    replaceBackend(acceptingPut)
    const wrapper = await open({ ...boundType(wantedFlow.id), workflows: [] })
    await flushPromises()

    await wrapper.get('[data-testid="type-workflow"]').setValue(flows[0].id)
    await wrapper.get('form').trigger('submit')
    await flushPromises()

    // The server defaults the rest: priority 1, no condition, no window.
    expect(putBody()?.workflows).toEqual([{ workflowDefinitionId: flows[0].id }])
  })

  it.each([
    [2, '1 further binding is not edited here.'],
    [3, '2 further bindings are not edited here.'],
  ])('disables the chooser on a type with %i bindings, and says why', async (count, note) => {
    const stored = [
      binding(wantedFlow.id, { priority: 1, conditionExpression: '{"==":[1,1]}' }),
      binding(flows[0].id, { priority: 2, validFrom: '2026-01-01', validTo: '2026-06-30' }),
      binding(flows[1].id, { priority: 3 }),
    ].slice(0, count)
    replaceBackend(acceptingPut)
    const wrapper = await open({ ...boundType(wantedFlow.id), workflows: stored })
    await flushPromises()

    // Still showing the first, so it can be read; neither it nor its search moves.
    expect(selectedValue(wrapper, 'type-workflow')).toBe(wantedFlow.id)
    expect(wrapper.get('[data-testid="type-workflow"]').attributes('disabled')).toBeDefined()
    expect(wrapper.get('[data-testid="type-workflow-search"]').attributes('disabled')).toBeDefined()
    expect(wrapper.get('[data-testid="type-workflow-further"]').text()).toContain(note)

    await wrapper.get('[data-testid="type-name"]').setValue('Requisition (renamed)')
    await wrapper.get('form').trigger('submit')
    await flushPromises()

    expect(sentWorkflows()).toBe(false)
  })

  it.each([
    ['one binding', [binding(wantedFlow.id)]],
    ['no binding', []],
  ])('shows no note and leaves the chooser enabled on a type with %s', async (_, workflows) => {
    const wrapper = await open({ ...boundType(wantedFlow.id), workflows })
    await flushPromises()

    expect(wrapper.find('[data-testid="type-workflow-further"]').exists()).toBe(false)
    expect(wrapper.get('[data-testid="type-workflow"]').attributes('disabled')).toBeUndefined()
    expect(
      wrapper.get('[data-testid="type-workflow-search"]').attributes('disabled'),
    ).toBeUndefined()
  })

  it('shows no note and leaves the chooser enabled when creating a type', async () => {
    const wrapper = await open()

    expect(wrapper.find('[data-testid="type-workflow-further"]').exists()).toBe(false)
    expect(wrapper.get('[data-testid="type-workflow"]').attributes('disabled')).toBeUndefined()
  })

  /** The refusal `check_workflow_bindings` gives a binding to a deprecated revision. */
  const notPublished =
    `workflow definition ${retiredFlow.id} is DEPRECATED and only a published revision may ` +
    'be bound — a running approval executes the revision it started against'

  function refusingDeprecated(request: RecordedRequest): FakeReply | undefined {
    return request.method === 'put'
      ? {
          status: 422,
          body: errorBody('VALIDATION_ERROR', 'Validation failed', [
            {
              path: 'workflows.0.workflowDefinitionId',
              rule: 'reference',
              code: 'NOT_PUBLISHED',
              message: notPublished,
            },
          ]),
        }
      : undefined
  }

  it('stays open when a save keeping a deprecated binding is refused', async () => {
    replaceBackend(refusingDeprecated)
    const wrapper = await open(boundType(retiredFlow.id))
    await flushPromises()

    await wrapper.get('[data-testid="type-name"]').setValue('Requisition (renamed)')
    await wrapper.get('form').trigger('submit')
    await flushPromises()

    // A rename leaves the binding off the wire (#718); the refusal is the fake's,
    // standing in for any 422 an edit can meet.
    expect(sentWorkflows()).toBe(false)
    expect(wrapper.emitted('saved')).toBeUndefined()
    expect(wrapper.get('[data-testid="document-type-error"]').text()).toContain('Validation failed')
    // The choice and its name survive the refusal, so it can be changed.
    expect(selectedValue(wrapper, 'type-workflow')).toBe(retiredFlow.id)
    expect(optionsOf(wrapper, 'type-workflow')[1]).toBe('Retired approval (r2, deprecated)')
  })

  // DEFECT (#625 campaign, 2026-10-10): the dialog shows a field's message only
  // for `typeCode`, `name` and `targetEntityType`. The refusal of a deprecated
  // binding names `workflows.0.workflowDefinitionId`, so the screen says only
  // "Validation failed" and drops the reason the server gave. Remove `.fails`
  // when it is fixed.
  it.fails('says why a save keeping a deprecated binding was refused', async () => {
    replaceBackend(refusingDeprecated)
    const wrapper = await open(boundType(retiredFlow.id))
    await flushPromises()

    await wrapper.get('[data-testid="type-name"]').setValue('Requisition (renamed)')
    await wrapper.get('form').trigger('submit')
    await flushPromises()

    expect(wrapper.get('[data-testid="document-type-dialog"]').text()).toContain(notPublished)
  })
})
