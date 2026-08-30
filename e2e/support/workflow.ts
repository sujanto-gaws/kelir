import { expect } from '@playwright/test'

import { runSuffix, signInOverApi, type ApiSession } from './api'
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
  readonly id: string
  readonly username: string
  readonly password: string
  /**
   * **Not the same string as `username`, and the screens do not agree on which
   * they show.** The task badge renders `delegatedFromDisplayName` and the
   * history renders `onBehalfOfUsername`, so a flow asserting the same value in
   * both places is asserting against one of them wrongly.
   */
  readonly displayName: string
}

/**
 * Above every amount the other flows submit, so **only** the flow that means to
 * branch does ([#241] AC4).
 *
 * The existing flows post 45000, 120000, 99000, 64000 and 900. A threshold
 * between any of those would have silently rerouted a flow that is about
 * something else, and it would have passed — the fallback and the branch both
 * end in `COMPLETED`.
 *
 * [#241]: https://github.com/sujanto-gaws/kelir/issues/241
 */
export const BRANCH_THRESHOLD = 500_000

/**
 * A deadline short enough to elapse inside a test run ([#241] AC3).
 *
 * **The indicator is what is being demonstrated, not the duration.** `dueAt` is
 * stamped in the database when the task is generated and never recomputed, and
 * `isOverdue` is decided by the server against that stamp — so the only thing a
 * browser can add is whether the flag becomes a badge. A realistic deadline
 * would make that unobservable without waiting a day.
 *
 * 0.0004 hours is 1.44 seconds.
 */
export const DIRECTOR_DUE_IN_HOURS = 0.0004

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
  directorUserId: string,
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
      // **The branch, and the only state assigned to a person** (#241 AC2, AC3,
      // AC4). It has to be a `USER` rule rather than a `ROLE` one, because a
      // delegation window applies *after the rule resolves to somebody* — a
      // role task has no assignee for a window to redirect (JWSS §5.1, #184).
      // So this is the state that can demonstrate a window at all.
      {
        code: 'DIRECTOR_APPROVAL',
        name: 'Director approval',
        mapsToDocumentStatus: 'PENDING_APPROVAL',
        task: {
          taskDefinitionKey: 'director_approval',
          taskName: 'Approve the larger request',
          assignment: { assigneeType: 'USER', userId: directorUserId },
          dueInHours: DIRECTOR_DUE_IN_HOURS,
        },
      },
    ],
    // Declared so a condition can read it. JWSS §6.1 puts the form's own data
    // under `formData`, and a declared variable is what pins the value at the
    // instance's start — a condition reading the form directly would be
    // evaluated against whatever the form said at the moment of the decision.
    variables: [
      { key: 'amount', dataType: 'NUMBER', source: { var: 'formData.amount' } },
    ],
    transitions: [
      // **The conditioned edge is written first and evaluated first**, and the
      // unconditioned one below it is the fallback S7 puts last whatever its
      // position. Both end an approval, so a threshold that caught the other
      // flows would have rerouted them invisibly.
      {
        from: 'MANAGER_APPROVAL',
        to: 'DIRECTOR_APPROVAL',
        action: 'APPROVE',
        allowedBy: `ROLE:${roleCode}`,
        condition: { '>': [{ var: 'variables.amount' }, BRANCH_THRESHOLD] },
      },
      {
        from: 'MANAGER_APPROVAL',
        to: 'COMPLETED',
        action: 'APPROVE',
        allowedBy: `ROLE:${roleCode}`,
      },
      {
        from: 'DIRECTOR_APPROVAL',
        to: 'COMPLETED',
        action: 'APPROVE',
        // Whoever holds it decides it, which after a window is the delegate.
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
  options: { readonly label?: string; readonly extraPermissions?: readonly string[] } = {},
): Promise<SeededApprover> {
  const roleId = await createApproverRole(session, roleCode, options.extraPermissions ?? [])

  return addApprover(session, roleId, options.label ?? 'approver')
}

/**
 * A role holding what an approver needs, and nothing else unless asked.
 *
 * The four are the smallest set that lets somebody open their inbox, read the
 * task's document, and decide it. Granting more would make a flow pass for a
 * reason it is not about; granting less would make it fail for one.
 *
 * **`extraPermissions` exists for one caller.** [#241]'s delegator opens a
 * window in its own name, which needs `identity:delegation:create` — and the
 * role every other flow uses must not have it, or the flows that are not about
 * delegation would be run by somebody who could delegate.
 *
 * [#241]: https://github.com/sujanto-gaws/kelir/issues/241
 */
export async function createApproverRole(
  session: ApiSession,
  roleCode: string,
  extraPermissions: readonly string[] = [],
): Promise<string> {
  // The role API takes permission **ids**, so the catalogue is read and each is
  // looked up by code. Looking them up rather than hard-coding ids is what
  // makes a missing permission fail here, loudly, instead of quietly granting
  // nothing — precisely the failure that would make a flow pass for the wrong
  // reason.
  const permissionIds = await permissionIdsFor(session, [
    'workflow:task:read',
    'workflow:task:execute',
    'workflow:instance:read',
    'document:read',
    ...extraPermissions,
  ])

  const role = await session.context.post(`${API_PREFIX}/identity/roles`, {
    data: { roleCode, name: `Approver ${roleCode}`, permissionIds },
  })

  expect(
    role.ok(),
    `seeding the role ${roleCode} failed: ${role.status()} ${await role.text()}`,
  ).toBeTruthy()

  return ((await role.json()) as { data: { id: string } }).data.id
}

/** One more person in a role that already exists. */
export async function addApprover(
  session: ApiSession,
  roleId: string,
  label: string,
): Promise<SeededApprover> {
  const suffix = runSuffix()
  const username = `e2e.${label}.${suffix}`.toLowerCase()
  const password = `Approver-${suffix}-password`
  const displayName = `${label} ${suffix}`

  const user = await session.context.post(`${API_PREFIX}/identity/users`, {
    data: {
      username,
      email: `${username}@example.test`,
      displayName,
      password,
      roleIds: [roleId],
    },
  })

  expect(
    user.ok(),
    `seeding ${username} failed: ${user.status()} ${await user.text()}`,
  ).toBeTruthy()

  const id = ((await user.json()) as { data: { id: string } }).data.id

  return { id, username, password, displayName }
}

/**
 * A delegation window in the delegator's own name ([#241] AC2, FR-IDM-006).
 *
 * **Opened as the delegator and not as the administrator**, which is not a
 * convenience of this helper but the API's shape: `CreateDelegationRequest`
 * carries no `delegatorUserId`, deliberately, so nobody can route somebody
 * else's work to a person of their choosing ([#184]).
 *
 * The window is open **now**, because the task it has to catch is generated by
 * the decision this flow is about to take. `starts_at <= now() < ends_at` is
 * the predicate, so a window starting on the hour would not catch it.
 *
 * [#184]: https://github.com/sujanto-gaws/kelir/issues/184
 * [#241]: https://github.com/sujanto-gaws/kelir/issues/241
 */
export async function openDelegationWindow(
  delegator: SeededApprover,
  delegateUserId: string,
  reason: string,
): Promise<void> {
  const asDelegator = await signInOverApi({
    username: delegator.username,
    password: delegator.password,
  })

  try {
    const startsAt = new Date(Date.now() - 60_000).toISOString()
    const endsAt = new Date(Date.now() + 60 * 60_000).toISOString()

    const opened = await asDelegator.context.post(`${API_PREFIX}/identity/delegations`, {
      data: { delegateUserId, startsAt, endsAt, scope: 'ALL', reason },
    })

    expect(
      opened.ok(),
      `opening the delegation window failed: ${opened.status()} ${await opened.text()}`,
    ).toBeTruthy()
  } finally {
    await asDelegator.context.dispose()
  }
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
