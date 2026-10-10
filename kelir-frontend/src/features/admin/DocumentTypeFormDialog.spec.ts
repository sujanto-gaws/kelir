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
import type { DocumentType } from '@/types/document-type'

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
      workflows: [{ workflowDefinitionId }],
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

  function putBody(): Record<string, unknown> | undefined {
    return backend.requests.find((request) => request.method === 'put')?.body as
      Record<string, unknown> | undefined
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
    'keeps a bound revision whose read fails, %s, and saves it back unchanged',
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

      // The server, not the dialog, decides whether it still binds.
      expect(putBody()?.workflows).toEqual([{ workflowDefinitionId: wantedFlow.id }])
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
        { workflowDefinitionId: retiredFlow.id, priority: 10, condition: null },
        { workflowDefinitionId: wantedFlow.id, priority: 20, condition: { '==': [1, 1] } },
      ],
    })
    await flushPromises()

    expect(selectedValue(wrapper, 'type-workflow')).toBe(retiredFlow.id)
    expect(optionsOf(wrapper, 'type-workflow')[1]).toBe('Retired approval (r2, deprecated)')
    expect(workflowReads()).toEqual([`/workflow/definitions/${retiredFlow.id}`])
  })

  // DEFECT (#625 campaign, 2026-10-10): an edit that leaves the workflow chooser
  // alone still sends `workflows: [{ workflowDefinitionId }]`, built from the
  // chooser alone. The array replaces the stored set (`replace_workflows`), so
  // renaming a type with two bindings deletes the second, and drops the first
  // one's priority and condition. Remove `.fails` when it is fixed.
  it.fails(
    'keeps every stored binding when an edit leaves the workflow chooser alone',
    async () => {
      const stored = [
        { workflowDefinitionId: wantedFlow.id, priority: 10, condition: { '==': [1, 1] } },
        { workflowDefinitionId: flows[0].id, priority: 20, condition: null },
      ]
      replaceBackend(acceptingPut)
      const wrapper = await open({ ...boundType(wantedFlow.id), workflows: stored })
      await flushPromises()

      await wrapper.get('[data-testid="type-name"]').setValue('Requisition (renamed)')
      await wrapper.get('form').trigger('submit')
      await flushPromises()

      // Either left off the wire, which leaves the stored set alone, or sent whole.
      const sent = putBody()

      expect(sent).toBeDefined()
      expect(sent?.workflows ?? stored).toEqual(stored)
    },
  )

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

    expect(putBody()?.workflows).toEqual([{ workflowDefinitionId: retiredFlow.id }])
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
