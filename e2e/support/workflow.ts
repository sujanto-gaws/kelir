import { expect } from '@playwright/test'

import { runSuffix, type ApiSession } from './api'
import { API_PREFIX } from './env'

/**
 * Seeding a workflow definition and the approver who will decide against it.
 *
 * Beside `documents.ts` rather than inside it, for the reason that file gives
 * about `api.ts`: one module per subject, so neither grows a dependency on the
 * other.
 *
 * **What is seeded here is the administrator's half.** Authoring a workflow has
 * no screen until the designer (FR-RAD-011, Sprints 14–16), and binding one to a
 * document type has none either — the document-type configuration screen is the
 * same sprint's. Doing both over the API is what lets the browser flow be about
 * the **approval**, which is what Sprint 10's exit asks to see driven.
 */

export interface SeededWorkflow {
  readonly id: string
  readonly workflowKey: string
  readonly roleCode: string
}

export interface SeededApprover {
  readonly username: string
  readonly password: string
}

/**
 * A workflow with one approval step, offered to a role.
 *
 * A **role** rather than a named user, deliberately: it is the case the inbox
 * has to be able to show apart from work that is already somebody's, and it is
 * the shape a real approval chain has — *whoever holds the finance role*, not
 * *this person*.
 */
export async function publishWorkflow(
  session: ApiSession,
  roleCode: string,
): Promise<SeededWorkflow> {
  const workflowKey = `e2e_approval_${runSuffix()}`.toLowerCase().replace(/[^a-z0-9_]/g, '_')

  const definition = {
    workflowKey,
    version: '1.0.0',
    name: 'Standard approval',
    initialState: 'MANAGER_APPROVAL',
    states: [
      {
        code: 'MANAGER_APPROVAL',
        name: 'Manager approval',
        mapsToDocumentStatus: 'PENDING_APPROVAL',
        task: {
          taskDefinitionKey: 'manager_approval',
          taskName: 'Approve the request',
          assignment: { assigneeType: 'ROLE', roleCode },
        },
      },
      {
        code: 'COMPLETED',
        name: 'Completed',
        mapsToDocumentStatus: 'COMPLETED',
        isFinal: true,
      },
      {
        code: 'REJECTED',
        name: 'Rejected',
        mapsToDocumentStatus: 'REJECTED',
        isFinal: true,
      },
      // **Stateless, and that is JWSS §10's own shape** (#183). A returned
      // document is with its author, not in anybody's inbox, so the state
      // declares no task and the `RESUBMIT` edge out of it is authorized by its
      // own `allowedBy` instead.
      {
        code: 'RETURNED',
        name: 'Sent back',
        mapsToDocumentStatus: 'RETURNED',
      },
    ],
    transitions: [
      {
        from: 'MANAGER_APPROVAL',
        to: 'COMPLETED',
        action: 'APPROVE',
        allowedBy: `ROLE:${roleCode}`,
      },
      {
        from: 'MANAGER_APPROVAL',
        to: 'REJECTED',
        action: 'REJECT',
        allowedBy: `ROLE:${roleCode}`,
        // A refusal has to say why (JWSS §4.1, FR-TASK-006). Marked here and
        // not on the APPROVE, because the asymmetry is what the browser flow
        // has to be able to show: a screen that asked for a reason on both
        // would pass against a product that hard-coded the rule.
        requiresComment: true,
      },
      {
        from: 'MANAGER_APPROVAL',
        to: 'RETURNED',
        action: 'RETURN',
        allowedBy: `ROLE:${roleCode}`,
        // "Why is this back with me" is the question return exists to answer,
        // so the definition asks for it.
        requiresComment: true,
      },
      {
        from: 'RETURNED',
        to: 'MANAGER_APPROVAL',
        action: 'RESUBMIT',
        // The owner sends it back up, through the same submit button they used
        // the first time — not through a task, because there is none.
        allowedBy: 'OWNER',
      },
    ],
  }

  const created = await session.context.post(`${API_PREFIX}/workflow/definitions`, {
    data: { workflowKey, name: 'Standard approval', definition },
  })

  expect(
    created.ok(),
    `seeding the workflow failed: ${created.status()} ${await created.text()}`,
  ).toBeTruthy()

  const id = ((await created.json()) as { data: { id: string } }).data.id

  // Publishing is what makes it bindable **and** startable: the projection this
  // writes is what the instance's state foreign key points at.
  const published = await session.context.post(
    `${API_PREFIX}/workflow/definitions/${id}/publication`,
    { data: {} },
  )

  expect(
    published.ok(),
    `publishing the workflow failed: ${published.status()} ${await published.text()}`,
  ).toBeTruthy()

  return { id, workflowKey, roleCode }
}

/** Points an existing document type at a published workflow. */
export async function bindWorkflow(
  session: ApiSession,
  documentTypeId: string,
  workflow: SeededWorkflow,
): Promise<void> {
  const bound = await session.context.put(`${API_PREFIX}/document-types/${documentTypeId}`, {
    data: { workflows: [{ workflowDefinitionId: workflow.id }] },
  })

  expect(
    bound.ok(),
    `binding the workflow to the type failed: ${bound.status()} ${await bound.text()}`,
  ).toBeTruthy()
}

/**
 * A role holding what an approver needs, and a user holding that role.
 *
 * The permissions are the smallest set that lets somebody open their inbox, read
 * the task's document, and decide it. Granting more would make the flow pass for
 * a reason it is not about; granting less would make it fail for one.
 */
export async function createApprover(
  session: ApiSession,
  roleCode: string,
): Promise<SeededApprover> {
  const suffix = runSuffix()

  // The role API takes permission **ids**, so the catalogue is read and the
  // four are looked up by code. Looking them up rather than hard-coding ids is
  // what makes a missing permission fail here, loudly, instead of quietly
  // granting nothing — which is precisely the failure that would make this flow
  // pass for the wrong reason.
  const permissionIds = await permissionIdsFor(session, [
    'workflow:task:read',
    'workflow:task:execute',
    'workflow:instance:read',
    'document:read',
  ])

  const role = await session.context.post(`${API_PREFIX}/identity/roles`, {
    data: { roleCode, name: `Approver ${suffix}`, permissionIds },
  })

  expect(
    role.ok(),
    `seeding the approver role failed: ${role.status()} ${await role.text()}`,
  ).toBeTruthy()

  const roleId = ((await role.json()) as { data: { id: string } }).data.id

  const username = `e2e.approver.${suffix}`.toLowerCase()
  const password = `Approver-${suffix}-password`

  const user = await session.context.post(`${API_PREFIX}/identity/users`, {
    data: {
      username,
      email: `${username}@example.test`,
      displayName: `Approver ${suffix}`,
      password,
      roleIds: [roleId],
    },
  })

  expect(
    user.ok(),
    `seeding the approver failed: ${user.status()} ${await user.text()}`,
  ).toBeTruthy()

  return { username, password }
}

/** The catalogue ids of the permissions named, failing if any is absent. */
async function permissionIdsFor(session: ApiSession, codes: string[]): Promise<string[]> {
  // The catalogue is small and the endpoint pages; asking for one page big
  // enough to hold it beats paging for a lookup.
  const response = await session.context.get(`${API_PREFIX}/identity/permissions`, {
    params: { pageSize: '200' },
  })

  expect(
    response.ok(),
    `reading the permission catalogue failed: ${response.status()} ${await response.text()}`,
  ).toBeTruthy()

  const catalogue = ((await response.json()) as {
    data: { id: string; permissionCode: string }[]
  }).data

  return codes.map((code) => {
    const found = catalogue.find((permission) => permission.permissionCode === code)

    expect(found, `the permission catalogue has no \`${code}\``).toBeTruthy()

    return (found as { id: string }).id
  })
}
