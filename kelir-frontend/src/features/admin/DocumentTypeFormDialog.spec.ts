import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import type { AxiosResponse, InternalAxiosRequestConfig } from 'axios'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import DocumentTypeFormDialog from './DocumentTypeFormDialog.vue'
import { apiClient } from '@/api/client'
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

  /*
   * #718's verification campaign. Each test below was seen red under a
   * mutation the tests above let through.
   */

  it('names the note as what describes the chooser it disables', async () => {
    const wrapper = await open({
      ...boundType(wantedFlow.id),
      workflows: [binding(wantedFlow.id), binding(flows[0].id, { priority: 2 })],
    })
    await flushPromises()

    // A disabled control says nothing on its own; a screen reader reaches the
    // reason only through `aria-describedby`.
    const note = wrapper.get('[data-testid="type-workflow-further"]').attributes('id')
    const describedBy = wrapper.get('[data-testid="type-workflow"]').attributes('aria-describedby')

    expect(note).toBeDefined()
    expect(describedBy?.split(' ')).toContain(note)
  })

  it('sends no workflows for a type with several bindings even if its chooser moves', async () => {
    replaceBackend(acceptingPut)
    const wrapper = await open({
      ...boundType(wantedFlow.id),
      workflows: [binding(wantedFlow.id), binding(flows[0].id, { priority: 2 })],
    })
    await flushPromises()

    // The disabled chooser cannot be moved by a user, so the value is moved the
    // way the component itself would: the save must not depend on the control
    // staying disabled to keep the second binding.
    const chooser = wrapper
      .findAllComponents({ name: 'SearchSelect' })
      .find((component) => component.props('testId') === 'type-workflow')

    chooser?.vm.$emit('update:modelValue', flows[1].id)
    await flushPromises()

    expect(selectedValue(wrapper, 'type-workflow')).toBe(flows[1].id)

    await wrapper.get('form').trigger('submit')
    await flushPromises()

    expect(sentWorkflows()).toBe(false)
  })

  it('treats a type read without workflows as unbound, and sends none on a rename', async () => {
    replaceBackend(acceptingPut)
    // The endpoint always sends the array; a reply that does not must not throw
    // the dialog open, nor make a rename unbind anything.
    const withoutWorkflows: Partial<DocumentType> = boundType(wantedFlow.id)

    delete withoutWorkflows.workflows
    const wrapper = await open(withoutWorkflows as DocumentType)
    await flushPromises()

    expect(selectedValue(wrapper, 'type-workflow')).toBe('')
    expect(wrapper.find('[data-testid="type-workflow-further"]').exists()).toBe(false)

    await wrapper.get('[data-testid="type-name"]').setValue('Requisition (renamed)')
    await wrapper.get('form').trigger('submit')
    await flushPromises()

    expect(sentWorkflows()).toBe(false)
    expect(wrapper.emitted('saved')).toHaveLength(1)
  })

  it('still sends an empty workflows array when a type is created unbound', async () => {
    const wrapper = await open()

    await wrapper.get('[data-testid="type-code"]').setValue('REQ')
    await wrapper.get('[data-testid="type-name"]').setValue('Requisition')
    await wrapper.get('form').trigger('submit')
    await flushPromises()

    // Only an edit leaves the key off (#718); a create states its bindings.
    const posted = backend.requests.find((request) => request.method === 'post')

    expect(posted?.body).toMatchObject({ typeCode: 'REQ', workflows: [] })
  })

  /*
   * #716. A refused save says why, under the chooser the server named.
   *
   * After #718 a save that leaves the workflow chooser alone sends no
   * `workflows`, so keeping a deprecated binding is no longer refused (the
   * rename test below). What the dialog can still meet is a refusal of a
   * binding it *changed*: a revision, form or list retired between the chooser
   * listing it and the save — `check_workflow_bindings` and `check_bindings`
   * name it at `workflows.{index}.workflowDefinitionId`, `formId` and `listId`.
   */

  /** A 422 `VALIDATION_ERROR` as `binding_error` gives it: rule `reference`. */
  function refusal(...details: [path: string, code: string, message: string][]): FakeReply {
    return {
      status: 422,
      body: errorBody(
        'VALIDATION_ERROR',
        'Validation failed',
        details.map(([path, code, message]) => ({ path, rule: 'reference', code, message })),
      ),
    }
  }

  /** Every `PUT` refused with `reply`. */
  function refusingPut(reply: FakeReply): void {
    replaceBackend((request) => (request.method === 'put' ? reply : undefined))
  }

  /** `check_workflow_bindings`' refusal of a revision deprecated since it was listed. */
  const workflowNotPublished =
    `workflow definition ${flows[0].id} is DEPRECATED and only a published revision may ` +
    'be bound — a running approval executes the revision it started against'
  /** `check_bindings`' refusal of a form retired since it was listed. */
  const formNotPublished =
    `form definition ${forms[0].id} is DEPRECATED and only a published revision may be ` +
    'bound — a document pins the revision it was created against'
  /** `check_bindings`' refusal of a list deleted since it was listed. */
  const listNotFound = `no list definition ${lists[0].id} in this tenant`

  const CHOOSERS = ['type-form', 'type-list', 'type-workflow'] as const

  /**
   * The message a chooser shows for the server, found the way assistive
   * technology finds it: through the chooser's own `aria-describedby`. Empty
   * when the chooser names no message, or the one it names is not drawn.
   */
  function messageUnder(wrapper: VueWrapper, testId: string): string {
    const chooser = wrapper.get(`[data-testid="${testId}"]`)
    const ids = chooser.attributes('aria-describedby')?.split(' ') ?? []
    const message = wrapper.find(`[data-testid="${testId}-message"]`)

    if (!message.exists()) {
      return ''
    }

    // Named by the chooser it sits under, and drawn beside it rather than
    // beside another: a message described by one chooser and drawn under the
    // next would read right to a screen reader and wrong on the screen.
    expect(ids).toContain(message.attributes('id'))
    expect(message.element.parentElement?.contains(chooser.element)).toBe(true)

    return message.text()
  }

  it.each([
    ['form', 'type-form', forms[0].id, 'formId', 'NOT_PUBLISHED', formNotPublished],
    ['list', 'type-list', lists[0].id, 'listId', 'NOT_FOUND', listNotFound],
    [
      'workflow',
      'type-workflow',
      flows[0].id,
      'workflows.0.workflowDefinitionId',
      'NOT_PUBLISHED',
      workflowNotPublished,
    ],
  ] as const)(
    'says why a changed %s was refused, under that chooser and no other',
    async (_, testId, chosen, path, code, message) => {
      refusingPut(refusal([path, code, message]))
      const wrapper = await open(boundType(wantedFlow.id))
      await flushPromises()

      await wrapper.get(`[data-testid="${testId}"]`).setValue(chosen)
      await wrapper.get('form').trigger('submit')
      await flushPromises()

      // The change reached the wire, so the refusal is one this dialog can meet.
      expect(JSON.stringify(putBody())).toContain(chosen)
      expect(wrapper.emitted('saved')).toBeUndefined()

      expect(messageUnder(wrapper, testId)).toBe(message)
      expect(wrapper.get(`[data-testid="${testId}"]`).attributes('aria-invalid')).toBe('true')

      for (const other of CHOOSERS.filter((candidate) => candidate !== testId)) {
        expect(messageUnder(wrapper, other)).toBe('')
        expect(wrapper.get(`[data-testid="${other}"]`).attributes('aria-invalid')).toBe('false')
      }

      // Placed, so not listed again above the form.
      expect(wrapper.find('[data-testid="document-type-unplaced-errors"]').exists()).toBe(false)
      expect(wrapper.find('[data-testid="document-type-error"]').exists()).toBe(false)
    },
  )

  it('places each of several refusals under its own chooser', async () => {
    refusingPut(
      refusal(
        ['formId', 'NOT_PUBLISHED', formNotPublished],
        ['listId', 'NOT_FOUND', listNotFound],
        ['workflows.0.workflowDefinitionId', 'NOT_PUBLISHED', workflowNotPublished],
      ),
    )
    const wrapper = await open(boundType(wantedFlow.id))
    await flushPromises()

    await wrapper.get('[data-testid="type-form"]').setValue(forms[0].id)
    await wrapper.get('[data-testid="type-list"]').setValue(lists[0].id)
    await wrapper.get('[data-testid="type-workflow"]').setValue(flows[0].id)
    await wrapper.get('form').trigger('submit')
    await flushPromises()

    expect(messageUnder(wrapper, 'type-form')).toBe(formNotPublished)
    expect(messageUnder(wrapper, 'type-list')).toBe(listNotFound)
    expect(messageUnder(wrapper, 'type-workflow')).toBe(workflowNotPublished)
  })

  it('lists a refusal at a path it shows no message for, so none is dropped', async () => {
    refusingPut(
      refusal(
        ['workflows.0.priority', 'OUT_OF_RANGE', 'priority must be at least 1'],
        ['description', 'TOO_LONG', 'description is longer than 2000 characters'],
        ['formId', 'NOT_PUBLISHED', formNotPublished],
      ),
    )
    const wrapper = await open(boundType(wantedFlow.id))
    await flushPromises()

    await wrapper.get('[data-testid="type-form"]').setValue(forms[0].id)
    await wrapper.get('form').trigger('submit')
    await flushPromises()

    const unplaced = wrapper.get('[data-testid="document-type-unplaced-errors"]')

    expect(unplaced.attributes('role')).toBe('alert')
    expect(unplaced.findAll('li').map((item) => item.text())).toEqual([
      'workflows.0.priority: priority must be at least 1',
      'description: description is longer than 2000 characters',
    ])
    // The one with a place is under its chooser, and only there.
    expect(messageUnder(wrapper, 'type-form')).toBe(formNotPublished)
    expect(unplaced.text()).not.toContain(formNotPublished)
  })

  it('places a refusal of the name under the name, as before', async () => {
    refusingPut(refusal(['name', 'REQUIRED', 'name is required']))
    const wrapper = await open(boundType(wantedFlow.id))
    await flushPromises()

    await wrapper.get('[data-testid="type-name"]').setValue('')
    await wrapper.get('form').trigger('submit')
    await flushPromises()

    const input = wrapper.get('[data-testid="type-name"]')
    const message = wrapper.get('[data-testid="type-name-message"]')

    expect(message.text()).toBe('name is required')
    expect(input.attributes('aria-invalid')).toBe('true')
    expect(input.attributes('aria-describedby')).toBe(message.attributes('id'))
    expect(wrapper.find('[data-testid="document-type-unplaced-errors"]').exists()).toBe(false)
  })

  // #716's verification campaign: the other two paths the dialog places, each
  // on the write that can be refused at it. Survived before these: dropping
  // either path from the placed list, its `invalid`, or its `described-by`.
  it.each([
    [
      'type code',
      'post',
      'type-code',
      'typeCode',
      'TOO_LONG',
      'typeCode must be at most 64 characters',
    ],
    [
      'governed record',
      'put',
      'type-governs',
      'targetEntityType',
      'ENTITY_NOT_GOVERNABLE',
      '`PARTY` is not a record type changes can be routed through approval for',
    ],
  ] as const)(
    'places a refusal of the %s under it, and not above the form',
    async (_, method, testId, path, code, message) => {
      replaceBackend((request) =>
        request.method === method ? refusal([path, code, message]) : undefined,
      )
      const wrapper = await open(method === 'post' ? null : boundType(wantedFlow.id))
      await flushPromises()

      if (method === 'post') {
        await wrapper.get('[data-testid="type-code"]').setValue('REQ')
        await wrapper.get('[data-testid="type-name"]').setValue('Requisition')
      } else {
        await wrapper.get('[data-testid="type-governs"]').setValue('PARTY')
      }
      await wrapper.get('form').trigger('submit')
      await flushPromises()

      const control = wrapper.get(`[data-testid="${testId}"]`)
      const shown = wrapper.get(`[data-testid="${testId}-message"]`)

      expect(wrapper.emitted('saved')).toBeUndefined()
      expect(shown.text()).toBe(message)
      expect(control.attributes('aria-invalid')).toBe('true')
      expect(control.attributes('aria-describedby')).toBe(shown.attributes('id'))
      expect(shown.element.parentElement?.contains(control.element)).toBe(true)
      expect(wrapper.find('[data-testid="document-type-unplaced-errors"]').exists()).toBe(false)
      expect(wrapper.find('[data-testid="document-type-error"]').exists()).toBe(false)
    },
  )

  // #716's verification campaign: survived before this, dropping the `reset()`
  // the opening watch makes — a refusal of one type was still shown under the
  // chooser, and listed above the form, when the dialog was opened on another.
  it('opens on another type without the last one’s refusals', async () => {
    refusingPut(
      refusal(
        ['workflows.0.workflowDefinitionId', 'NOT_PUBLISHED', workflowNotPublished],
        ['description', 'TOO_LONG', 'description is longer than 2000 characters'],
      ),
    )
    const wrapper = await open(boundType(retiredFlow.id))
    await flushPromises()

    await wrapper.get('[data-testid="type-workflow"]').setValue(flows[0].id)
    await wrapper.get('form').trigger('submit')
    await flushPromises()

    expect(messageUnder(wrapper, 'type-workflow')).toBe(workflowNotPublished)
    expect(wrapper.find('[data-testid="document-type-unplaced-errors"]').exists()).toBe(true)

    // As the list page does it: close, then open with what is to be edited.
    await wrapper.setProps({ open: false })
    await wrapper.setProps({ editing: boundType(wantedFlow.id) })
    await wrapper.setProps({ open: true })
    await flushPromises()

    expect(messageUnder(wrapper, 'type-workflow')).toBe('')
    expect(wrapper.get('[data-testid="type-workflow"]').attributes('aria-invalid')).toBe('false')
    expect(wrapper.find('[data-testid="document-type-unplaced-errors"]').exists()).toBe(false)
    expect(wrapper.find('[data-testid="document-type-error"]').exists()).toBe(false)
  })

  it('still says why a refusal that names no field failed', async () => {
    refusingPut({
      status: 403,
      body: errorBody('FORBIDDEN', 'You may not configure document types'),
    })
    const wrapper = await open(boundType(wantedFlow.id))
    await flushPromises()

    await wrapper.get('[data-testid="type-name"]').setValue('Requisition (renamed)')
    await wrapper.get('form').trigger('submit')
    await flushPromises()

    expect(wrapper.get('[data-testid="document-type-error"]').text()).toBe(
      'You may not configure document types',
    )
    expect(wrapper.emitted('saved')).toBeUndefined()
  })

  it('stays open when a save rebinding to a revision deprecated since it was listed is refused', async () => {
    refusingPut(
      refusal(['workflows.0.workflowDefinitionId', 'NOT_PUBLISHED', workflowNotPublished]),
    )
    const wrapper = await open(boundType(retiredFlow.id))
    await flushPromises()

    await wrapper.get('[data-testid="type-workflow"]').setValue(flows[0].id)
    await wrapper.get('form').trigger('submit')
    await flushPromises()

    expect(putBody()?.workflows).toEqual([binding(flows[0].id)])
    expect(wrapper.emitted('saved')).toBeUndefined()
    expect(wrapper.find('[data-testid="document-type-dialog"]').exists()).toBe(true)
    // The choice and its name survive the refusal, so it can be changed again.
    expect(selectedValue(wrapper, 'type-workflow')).toBe(flows[0].id)
    expect(optionsOf(wrapper, 'type-workflow')).toContain('Filler flow 001 (r1)')
  })

  // Pinned `it.fails` by #625's campaign (2026-10-10) as "says why a save keeping
  // a deprecated binding was refused": the dialog showed a field's message only
  // for `typeCode`, `name` and `targetEntityType`, so this refusal read as
  // "Validation failed". Re-based by #716 onto a refusal #718 leaves reachable.
  it('says why a save rebinding to a revision deprecated since it was listed was refused', async () => {
    refusingPut(
      refusal(['workflows.0.workflowDefinitionId', 'NOT_PUBLISHED', workflowNotPublished]),
    )
    const wrapper = await open(boundType(retiredFlow.id))
    await flushPromises()

    await wrapper.get('[data-testid="type-workflow"]').setValue(flows[0].id)
    await wrapper.get('form').trigger('submit')
    await flushPromises()

    expect(messageUnder(wrapper, 'type-workflow')).toBe(workflowNotPublished)
    expect(wrapper.get('[data-testid="document-type-dialog"]').text()).not.toContain(
      'Validation failed',
    )
  })

  // #716's *Done when*: the rename that could not be made.
  it('renames a type bound to a deprecated revision, sending no workflows', async () => {
    // As the server answers without #716's option (a): a write naming the
    // deprecated revision is refused, and any other is accepted.
    replaceBackend((request) => {
      if (request.method !== 'put') {
        return undefined
      }

      return JSON.stringify(request.body).includes(retiredFlow.id)
        ? refusal(['workflows.0.workflowDefinitionId', 'NOT_PUBLISHED', 'is DEPRECATED'])
        : { status: 200, body: itemBody({ id: 't-1' }) }
    })
    const wrapper = await open(boundType(retiredFlow.id))
    await flushPromises()

    await wrapper.get('[data-testid="type-name"]').setValue('Requisition (renamed)')
    await wrapper.get('form').trigger('submit')
    await flushPromises()

    expect(putBody()?.name).toBe('Requisition (renamed)')
    expect(sentWorkflows()).toBe(false)
    expect(wrapper.emitted('saved')).toHaveLength(1)
    expect(wrapper.find('[data-testid="document-type-error"]').exists()).toBe(false)
    expect(wrapper.find('[data-testid="document-type-unplaced-errors"]').exists()).toBe(false)
  })

  /*
   * #728 (D-117). An edit that replaces the bindings also sends the ones it
   * read, and the server refuses a stored set that changed since: the dialog
   * says so, and Reload brings in the current set.
   */

  /** Every edit's `PUT` body, in the order they were sent. */
  function putBodies(): Record<string, unknown>[] {
    return backend.requests
      .filter((request) => request.method === 'put')
      .map((request) => request.body as Record<string, unknown>)
  }

  /** One binding with every field set, so a field lost on the way back shows. */
  const routed = binding(wantedFlow.id, {
    priority: 7,
    conditionExpression: '{">":[{"var":"formData.amount"},1000]}',
    validFrom: '2026-01-01',
    validTo: '2026-12-31',
  })

  const workflowsChangedMessage =
    'the workflow bindings of this document type changed since they were read; reload and try again'

  /** The server's refusal of a stale `expectedWorkflows`. */
  const workflowsChanged: FakeReply = {
    status: 409,
    body: errorBody('DOCUMENT_TYPE_WORKFLOWS_CHANGED', workflowsChangedMessage),
  }

  const STALE_ALERT =
    'Another administrator changed the workflow bindings of this document type since this ' +
    'dialog opened. Nothing was saved. Reload brings in the current bindings and discards ' +
    'your unsaved edits.'

  function staleAlert(wrapper: VueWrapper) {
    return wrapper.find('[data-testid="document-type-workflows-changed"]')
  }

  /** Submits the form and lets the save settle. */
  async function submit(wrapper: VueWrapper): Promise<void> {
    await wrapper.get('form').trigger('submit')
    await flushPromises()
  }

  it.each([
    [
      'moves it to another revision',
      flows[0].id,
      [{ ...routed, workflowDefinitionId: flows[0].id }],
    ],
    ['clears it', '', []],
  ] as const)(
    'sends the bindings it read beside workflows when the chooser %s',
    async (_, chosen, workflows) => {
      replaceBackend(acceptingPut)
      const wrapper = await open({ ...boundType(wantedFlow.id), workflows: [routed] })

      await wrapper.get('[data-testid="type-workflow"]').setValue(chosen)
      await submit(wrapper)

      // All five fields as read, not only the one the chooser shows.
      expect(putBody()?.workflows).toEqual(workflows)
      expect(putBody()?.expectedWorkflows).toEqual([routed])
      expect(wrapper.emitted('saved')).toHaveLength(1)
    },
  )

  it.each([
    ['one binding', [routed]],
    ['several bindings', [routed, binding(flows[0].id, { priority: 2 })]],
    ['no binding', []],
  ] as const)(
    'sends neither workflows nor expectedWorkflows when the chooser is left alone on a type with %s',
    async (_, workflows) => {
      replaceBackend(acceptingPut)
      const wrapper = await open({ ...boundType(wantedFlow.id), workflows: [...workflows] })

      await wrapper.get('[data-testid="type-name"]').setValue('Requisition (renamed)')
      await submit(wrapper)

      expect(putBody()?.name).toBe('Requisition (renamed)')
      expect(putBody()).not.toHaveProperty('workflows')
      expect(putBody()).not.toHaveProperty('expectedWorkflows')
    },
  )

  it('sends an empty expectedWorkflows when the type it read had no binding', async () => {
    replaceBackend(acceptingPut)
    const wrapper = await open({ ...boundType(wantedFlow.id), workflows: [] })

    await wrapper.get('[data-testid="type-workflow"]').setValue(flows[0].id)
    await submit(wrapper)

    // `[]` and not absent: an empty read is a claim the server can check.
    expect(putBody()?.workflows).toEqual([{ workflowDefinitionId: flows[0].id }])
    expect(putBody()).toHaveProperty('expectedWorkflows')
    expect(putBody()?.expectedWorkflows).toEqual([])
  })

  it('sends no expectedWorkflows when creating a type', async () => {
    const wrapper = await open()

    await wrapper.get('[data-testid="type-code"]').setValue('REQ')
    await wrapper.get('[data-testid="type-name"]').setValue('Requisition')
    await wrapper.get('[data-testid="type-workflow"]').setValue(flows[0].id)
    await submit(wrapper)

    const posted = backend.requests.find((request) => request.method === 'post')?.body

    expect(posted).toHaveProperty('workflows')
    expect(posted).not.toHaveProperty('expectedWorkflows')
  })

  it('says the bindings changed since it opened, above the form, and that nothing was saved', async () => {
    refusingPut(workflowsChanged)
    const wrapper = await open({ ...boundType(wantedFlow.id), workflows: [routed] })

    await wrapper.get('[data-testid="type-workflow"]').setValue(flows[0].id)
    await submit(wrapper)

    const alert = staleAlert(wrapper)

    expect(alert.exists()).toBe(true)
    expect(alert.attributes('role')).toBe('alert')
    expect(wrapper.get('[data-testid="document-type-workflows-changed-message"]').text()).toBe(
      STALE_ALERT,
    )
    expect(wrapper.get('[data-testid="reload-document-type"]').text()).toBe('Reload')
    // Above the form, as the dialog's other refusals are.
    expect(
      alert.element.compareDocumentPosition(wrapper.get('form').element) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy()
    // Said once, in the dialog's words: not repeated as the plain form error.
    expect(wrapper.find('[data-testid="document-type-error"]').exists()).toBe(false)
    expect(wrapper.emitted('saved')).toBeUndefined()
    expect(wrapper.emitted('close')).toBeUndefined()
    // The edit survives the refusal until Reload is asked for.
    expect(selectedValue(wrapper, 'type-workflow')).toBe(flows[0].id)
  })

  it('branches on the code, not the words, of a refusal', async () => {
    refusingPut({
      status: 409,
      body: errorBody('DOCUMENT_TYPE_WORKFLOWS_CHANGED', 'Worded some other way'),
    })
    const wrapper = await open({ ...boundType(wantedFlow.id), workflows: [routed] })

    await wrapper.get('[data-testid="type-workflow"]').setValue(flows[0].id)
    await submit(wrapper)

    expect(staleAlert(wrapper).exists()).toBe(true)
  })

  it.each([
    ['the same words under another code', 'CONFLICT', workflowsChangedMessage],
    ['another conflict', 'CONFLICT', 'A document type with that code already exists'],
  ])('shows %s as it did before', async (_, code, message) => {
    refusingPut({ status: 409, body: errorBody(code, message) })
    const wrapper = await open({ ...boundType(wantedFlow.id), workflows: [routed] })

    await wrapper.get('[data-testid="type-workflow"]').setValue(flows[0].id)
    await submit(wrapper)

    expect(wrapper.get('[data-testid="document-type-error"]').text()).toBe(message)
    expect(staleAlert(wrapper).exists()).toBe(false)
    expect(wrapper.find('[data-testid="reload-document-type"]').exists()).toBe(false)
  })

  /** One binding as the server's set comparison sees it: all five fields. */
  function bindingKey(sent: Partial<WorkflowBinding>): string {
    return JSON.stringify([
      sent.workflowDefinitionId,
      sent.conditionExpression ?? null,
      sent.priority ?? 1,
      sent.validFrom ?? null,
      sent.validTo ?? null,
    ])
  }

  function sameSet(left: Partial<WorkflowBinding>[], right: Partial<WorkflowBinding>[]): boolean {
    const keys = (set: Partial<WorkflowBinding>[]) => set.map(bindingKey).sort().join('\n')

    return left.length === right.length && keys(left) === keys(right)
  }

  /**
   * A server holding one type, which another administrator edits through
   * `theirEdit` while the dialog is open. Its `PUT` refuses a stale
   * `expectedWorkflows` as `update_type` does, and stores what it accepts.
   */
  function serveOneType(reloadFails = false): { theirEdit(): void } {
    let stored: DocumentType = { ...boundType(wantedFlow.id), workflows: [routed] }

    replaceBackend((request) => {
      if (request.url !== '/document-types/t-1') {
        return undefined
      }

      if (request.method === 'get') {
        return reloadFails
          ? { status: 0, networkError: true }
          : { status: 200, body: itemBody(stored) }
      }

      const sent = request.body as {
        name: string
        workflows?: WorkflowBinding[]
        expectedWorkflows?: WorkflowBinding[]
      }

      if (sent.expectedWorkflows && !sameSet(sent.expectedWorkflows, stored.workflows)) {
        return workflowsChanged
      }

      stored = { ...stored, name: sent.name, workflows: sent.workflows ?? stored.workflows }

      return { status: 200, body: itemBody(stored) }
    })

    return {
      theirEdit() {
        stored = {
          ...stored,
          name: 'Requisition (theirs)',
          workflows: [binding(flows[1].id, { priority: 4, validFrom: '2026-03-01' })],
        }
      },
    }
  }

  it('reloads after a refusal, and then saves against the bindings it reloaded', async () => {
    const server = serveOneType()
    const wrapper = await open({ ...boundType(wantedFlow.id), workflows: [routed] })

    server.theirEdit()
    await wrapper.get('[data-testid="type-name"]').setValue('Requisition (mine)')
    await wrapper.get('[data-testid="type-workflow"]').setValue(flows[0].id)
    await submit(wrapper)

    expect(staleAlert(wrapper).exists()).toBe(true)
    expect(putBodies()[0]?.expectedWorkflows).toEqual([routed])

    await wrapper.get('[data-testid="reload-document-type"]').trigger('click')
    await flushPromises()

    // Re-read, and every field, the stored set and the chooser are the server's now.
    expect(
      backend.requests.filter(
        (request) => request.method === 'get' && request.url === '/document-types/t-1',
      ),
    ).toHaveLength(1)
    expect((wrapper.get('[data-testid="type-name"]').element as HTMLInputElement).value).toBe(
      'Requisition (theirs)',
    )
    expect(selectedValue(wrapper, 'type-workflow')).toBe(flows[1].id)
    expect(staleAlert(wrapper).exists()).toBe(false)
    expect(wrapper.find('[data-testid="document-type-dialog"]').exists()).toBe(true)
    expect(wrapper.emitted('close')).toBeUndefined()
    expect(wrapper.emitted('saved')).toBeUndefined()

    await wrapper.get('[data-testid="type-workflow"]').setValue(flows[0].id)
    await submit(wrapper)

    const theirs = binding(flows[1].id, { priority: 4, validFrom: '2026-03-01' })

    expect(putBodies()).toHaveLength(2)
    expect(putBodies()[1]?.expectedWorkflows).toEqual([theirs])
    expect(putBodies()[1]?.workflows).toEqual([{ ...theirs, workflowDefinitionId: flows[0].id }])
    expect(wrapper.emitted('saved')).toHaveLength(1)
    expect(staleAlert(wrapper).exists()).toBe(false)
  })

  it('says a reload failed and leaves the dialog as it was, still known to be stale', async () => {
    const server = serveOneType(true)
    const wrapper = await open({ ...boundType(wantedFlow.id), workflows: [routed] })

    server.theirEdit()
    await wrapper.get('[data-testid="type-name"]').setValue('Requisition (mine)')
    await wrapper.get('[data-testid="type-workflow"]').setValue(flows[0].id)
    await submit(wrapper)
    await wrapper.get('[data-testid="reload-document-type"]').trigger('click')
    await flushPromises()

    const failed = wrapper.get('[data-testid="document-type-reload-error"]')

    expect(failed.attributes('role')).toBe('alert')
    expect(failed.text()).toContain('The document type could not be reloaded')
    // Nothing half-applied: the edit is still the user's, and the alert still says it is stale.
    expect(staleAlert(wrapper).exists()).toBe(true)
    expect((wrapper.get('[data-testid="type-name"]').element as HTMLInputElement).value).toBe(
      'Requisition (mine)',
    )
    expect(selectedValue(wrapper, 'type-workflow')).toBe(flows[0].id)

    // A later save still sends the read it holds, which the server refuses again.
    await submit(wrapper)

    expect(putBodies()).toHaveLength(2)
    expect(putBodies()[1]?.expectedWorkflows).toEqual([routed])
    expect(staleAlert(wrapper).exists()).toBe(true)
    expect(wrapper.emitted('saved')).toBeUndefined()
  })

  it('opens again without the last opening’s stale alert', async () => {
    refusingPut(workflowsChanged)
    const wrapper = await open({ ...boundType(wantedFlow.id), workflows: [routed] })

    await wrapper.get('[data-testid="type-workflow"]').setValue(flows[0].id)
    await submit(wrapper)

    expect(staleAlert(wrapper).exists()).toBe(true)

    await wrapper.setProps({ open: false })
    await wrapper.setProps({ editing: boundType(wantedFlow.id) })
    await wrapper.setProps({ open: true })
    await flushPromises()

    expect(staleAlert(wrapper).exists()).toBe(false)
  })

  /*
   * #728's verification campaign (test-engineer, 2026-10-16): a reload whose
   * read returns after the dialog has moved on, and a failed reload's alert
   * on the next opening.
   */

  /** Holds every `GET /document-types/{id}` until the returned function is called. */
  function holdTypeReads(): () => void {
    const fake = apiClient.defaults.adapter as (
      config: InternalAxiosRequestConfig,
    ) => Promise<AxiosResponse>
    let release!: () => void
    const gate = new Promise<void>((resolve) => {
      release = resolve
    })

    apiClient.defaults.adapter = async (config: InternalAxiosRequestConfig) => {
      if (config.method === 'get' && config.url?.startsWith('/document-types/')) {
        await gate
      }

      return fake(config)
    }

    return release
  }

  /** Another type, as `GET /document-types/{id}` returns it. */
  function otherType(): DocumentType {
    return {
      ...boundType(flows[2].id),
      id: 't-2',
      typeCode: 'PO',
      name: 'Purchase order',
      workflows: [binding(flows[2].id, { priority: 3, validTo: '2026-06-30' })],
    }
  }

  it('drops a reload that returns after the dialog moved to another type', async () => {
    const server = serveOneType()
    const wrapper = await open({ ...boundType(wantedFlow.id), workflows: [routed] })

    server.theirEdit()
    await wrapper.get('[data-testid="type-workflow"]').setValue(flows[0].id)
    await submit(wrapper)
    expect(staleAlert(wrapper).exists()).toBe(true)

    const release = holdTypeReads()

    await wrapper.get('[data-testid="reload-document-type"]').trigger('click')
    await flushPromises()

    // While t-1's read is out, the dialog is closed and opened on t-2.
    await wrapper.setProps({ open: false })
    await wrapper.setProps({ editing: otherType() })
    await wrapper.setProps({ open: true })
    await flushPromises()

    release()
    await flushPromises()

    // t-1's read arrived, and t-2's dialog is not filled with it.
    expect(
      backend.requests.filter(
        (request) => request.method === 'get' && request.url === '/document-types/t-1',
      ),
    ).toHaveLength(1)
    expect((wrapper.get('[data-testid="type-name"]').element as HTMLInputElement).value).toBe(
      'Purchase order',
    )
    expect(selectedValue(wrapper, 'type-workflow')).toBe(flows[2].id)

    // And a save from t-2 vouches for t-2's bindings, not t-1's.
    await wrapper.get('[data-testid="type-workflow"]').setValue(flows[0].id)
    await submit(wrapper)

    const sent = backend.requests.filter(
      (request) => request.method === 'put' && request.url === '/document-types/t-2',
    )

    expect(sent).toHaveLength(1)
    expect((sent[0].body as Record<string, unknown>).expectedWorkflows).toEqual(
      otherType().workflows,
    )
  })

  it('opens again without the last opening’s reload failure', async () => {
    const server = serveOneType(true)
    const wrapper = await open({ ...boundType(wantedFlow.id), workflows: [routed] })

    server.theirEdit()
    await wrapper.get('[data-testid="type-workflow"]').setValue(flows[0].id)
    await submit(wrapper)
    await wrapper.get('[data-testid="reload-document-type"]').trigger('click')
    await flushPromises()

    expect(wrapper.find('[data-testid="document-type-reload-error"]').exists()).toBe(true)

    await wrapper.setProps({ open: false })
    await wrapper.setProps({ editing: boundType(wantedFlow.id) })
    await wrapper.setProps({ open: true })
    await flushPromises()

    expect(wrapper.find('[data-testid="document-type-reload-error"]').exists()).toBe(false)
    expect(staleAlert(wrapper).exists()).toBe(false)
  })

  /** Holds every `PUT /document-types/{id}` until the returned function is called. */
  function holdTypeWrites(): () => void {
    const fake = apiClient.defaults.adapter as (
      config: InternalAxiosRequestConfig,
    ) => Promise<AxiosResponse>
    let release!: () => void
    const gate = new Promise<void>((resolve) => {
      release = resolve
    })

    apiClient.defaults.adapter = async (config: InternalAxiosRequestConfig) => {
      if (config.method === 'put' && config.url?.startsWith('/document-types/')) {
        await gate
      }

      return fake(config)
    }

    return release
  }

  // Seen to fail, 2026-10-16: the first with Save's `isReloading` dropped, the
  // second with Reload's `isSaving` dropped. Neither was held before.
  it('holds Save while a reload is out', async () => {
    const server = serveOneType()
    const wrapper = await open({ ...boundType(wantedFlow.id), workflows: [routed] })

    server.theirEdit()
    await wrapper.get('[data-testid="type-workflow"]').setValue(flows[0].id)
    await submit(wrapper)

    const release = holdTypeReads()

    await wrapper.get('[data-testid="reload-document-type"]').trigger('click')
    await flushPromises()

    // A save now would vouch for the bindings the reload is replacing.
    expect(wrapper.get('[data-testid="save-document-type"]').attributes('disabled')).toBeDefined()

    release()
    await flushPromises()

    expect(wrapper.get('[data-testid="save-document-type"]').attributes('disabled')).toBeUndefined()
  })

  it('holds Reload while a save is out', async () => {
    const server = serveOneType()
    const wrapper = await open({ ...boundType(wantedFlow.id), workflows: [routed] })

    server.theirEdit()
    await wrapper.get('[data-testid="type-workflow"]').setValue(flows[0].id)
    await submit(wrapper)

    const release = holdTypeWrites()

    await wrapper.get('form').trigger('submit')
    await flushPromises()

    // A reload now would fill the form under the save that is still out.
    expect(wrapper.get('[data-testid="reload-document-type"]').attributes('disabled')).toBeDefined()

    release()
    await flushPromises()

    expect(
      wrapper.get('[data-testid="reload-document-type"]').attributes('disabled'),
    ).toBeUndefined()
  })
})
