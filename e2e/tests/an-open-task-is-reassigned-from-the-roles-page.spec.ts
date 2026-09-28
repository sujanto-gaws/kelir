import { expect, test, type Locator, type Page } from "@playwright/test";

import { runSuffix, signInOverApi, type ApiSession } from "../support/api";
import {
  createDocumentType,
  createDraft,
  submitDocument,
} from "../support/documents";
import { API_PREFIX, credentials } from "../support/env";
import { publishForm } from "../support/forms";
import { pageUntilVisible } from "../support/paging";
import {
  addApprover,
  bindWorkflow,
  createApproverRole,
  publishSingleStepWorkflow,
  type SeededApprover,
  type SeededWorkflow,
} from "../support/workflow";

/**
 * An administrator reassigns the open task a role delete waits on, from the
 * Roles page, and the delete gets past it ([#512], FR-WF-017, ADR-0042).
 *
 * [#532] made a refused delete list its open tasks. User Manual §11.2 says *a
 * task is reassigned from that list*, and these flows drive that as the
 * administrator would: delete, read the list, reassign the task to a role or a
 * user who can decide it, watch the list empty, and delete again. Two more
 * flows drive what a screen could get wrong: the action is not drawn for
 * somebody without `workflow:task:reassign`, and a role deleted while the
 * dialog is open is refused under the role field.
 *
 * # Offered to one role, decided by another
 *
 * A reassign is refused to a role no decision names (`TARGET_CANNOT_DECIDE`),
 * and a task whose decision is `allowedBy` the role being deleted still needs
 * that role after any reassign. So each workflow offers its task to the role
 * being deleted and reserves its decisions to a second one. Reassigning to the
 * second, or to a user holding it, releases the first, and the task stays
 * unclaimed so the list's reason is *offered to the role, and unclaimed*.
 *
 * # The next refusal is the right ending
 *
 * The published workflow still names the first role in its assignment, so the
 * retried delete reaches **D-91** (3)'s refusal rather than succeeding. That
 * is the point: the open-task refusal is gone, and the next one names the
 * definition to revise.
 *
 * # What is seeded over the API
 *
 * Each flow seeds its own form, type, both roles and a holder each, workflow
 * and submit, under its own suffix, so none depends on another's leftovers.
 * The administrator is the seeded one, whose `ROLE-ADMIN` migration 0048
 * grants `workflow:task:reassign`. What a flow asserts it asserts in the
 * browser, except where the screen no longer shows it: a task reassigned away
 * leaves the list, so who holds it next is read over the API.
 *
 * [#512]: https://github.com/sujanto-gaws/kelir/issues/512
 * [#532]: https://github.com/sujanto-gaws/kelir/issues/532
 */

/** One flow's seed: a task offered to one role and decided by another. */
interface BlockedRole {
  readonly offeredRoleCode: string;
  readonly offeredRoleName: string;
  readonly deciderRoleCode: string;
  readonly deciderRoleName: string;
  /** Holds the decider role, so a reassign to them can decide the task. */
  readonly decider: SeededApprover;
  readonly workflow: SeededWorkflow;
  readonly documentId: string;
  readonly documentTitle: string;
}

/** A task in `GET /documents/{id}/workflow`, as much of it as is read here. */
interface WorkflowTaskRow {
  readonly status: string;
  readonly assigneeUserId: string | null;
  readonly candidateRoleCode: string | null;
  readonly completedAt: string | null;
}

let session: ApiSession;

test.beforeAll(async () => {
  session = await signInOverApi();
});

test.afterAll(async () => {
  await session?.context.dispose();
});

async function seedBlockedRole(label: string): Promise<BlockedRole> {
  const suffix = runSuffix();
  const offeredRoleCode = `E2E-OFFERED-${suffix}`.toUpperCase();
  const deciderRoleCode = `E2E-DECIDER-${suffix}`.toUpperCase();
  const documentTitle = `${label} ${suffix}`;

  // One optional field, in the shape the other role-delete flow publishes.
  const form = await publishForm(
    session,
    {
      formId: `e2e-moved-${suffix}`.toLowerCase(),
      version: "2.0.1",
      title: "Moved request",
      components: [
        {
          id: "amount-field",
          role: "data",
          type: "number",
          key: "amount",
          label: "Amount",
          validation: { type: "number" },
        },
      ],
    },
    `Moved request ${suffix}`,
  );
  const documentType = await createDocumentType(
    session,
    form,
    `Moved request ${suffix}`,
  );

  // Both roles, each with a holder: an assignment that resolves to nobody
  // refuses the submit, and the decider's holder is who a reassign to a user
  // names.
  const offeredRoleId = await createApproverRole(session, offeredRoleCode);
  await addApprover(session, offeredRoleId, "offered");
  const deciderRoleId = await createApproverRole(session, deciderRoleCode);
  const decider = await addApprover(session, deciderRoleId, "decider");

  const workflow = await publishSingleStepWorkflow(session, offeredRoleCode, {
    taskName: "Approve the moved request",
    deciderRoleCode,
  });
  await bindWorkflow(session, documentType.id, workflow);

  const documentId = await createDraft(session, documentType, documentTitle);
  await submitDocument(session, documentId);

  return {
    offeredRoleCode,
    offeredRoleName: `Approver ${offeredRoleCode}`,
    deciderRoleCode,
    deciderRoleName: `Approver ${deciderRoleCode}`,
    decider,
    workflow,
    documentId,
    documentTitle,
  };
}

async function signIn(
  page: Page,
  username: string,
  password: string,
): Promise<void> {
  await page.goto("/login");
  await page.getByLabel("Username or email").fill(username);
  await page.getByLabel("Password").fill(password);
  await page.getByRole("button", { name: "Sign in" }).click();

  await expect(page).toHaveURL(/\/$/);
}

/**
 * Reaches the Roles screen through the navigation, deletes the run's role,
 * confirms, and returns the open-task list the delete is refused with.
 */
async function refuseDelete(
  page: Page,
  blocked: BlockedRole,
): Promise<{ row: Locator; refused: Locator; tasks: Locator }> {
  await page.getByRole("link", { name: "Roles" }).click();
  await expect(page).toHaveURL(/\/admin\/roles/);

  // On whichever page its code sorts it to (#521).
  const row = page
    .getByRole("table")
    .getByRole("row", { name: new RegExp(blocked.offeredRoleCode) });
  await pageUntilVisible(page, row);

  await row.getByRole("button", { name: "Delete" }).click();

  const confirmation = page.getByRole("dialog", { name: "Delete role" });
  await expect(confirmation).toBeVisible();
  await confirmation.getByTestId("confirm-action").click();

  const refused = page.getByRole("dialog", {
    name: `Open tasks need ${blocked.offeredRoleName}`,
  });

  await expect(refused).toBeVisible();
  await expect(refused.getByRole("alert").first()).toContainText(
    "decided or reassigned first",
  );

  const tasks = refused.getByTestId("role-open-task");

  await expect(tasks).toHaveCount(1);
  await expect(tasks).toContainText(blocked.documentTitle);
  await expect(tasks).toContainText("offered to the role, and unclaimed");

  return { row, refused, tasks };
}

async function openReassign(page: Page, tasks: Locator): Promise<Locator> {
  await tasks.getByRole("button", { name: "Reassign" }).click();

  const reassign = page.getByRole("dialog", { name: /^Reassign TASK-/ });
  await expect(reassign).toBeVisible();

  return reassign;
}

/** The reassign closed, and the list read again is empty and says so. */
async function expectListEmptied(
  refused: Locator,
  reassign: Locator,
  tasks: Locator,
): Promise<void> {
  await expect(reassign).toHaveCount(0);
  await expect(tasks).toHaveCount(0);
  await expect(refused.getByTestId("role-open-tasks-empty")).toContainText(
    "can be tried again",
  );
}

/** The document's one open task, read over the API. */
async function openTaskOf(documentId: string): Promise<WorkflowTaskRow> {
  const response = await session.context.get(
    `${API_PREFIX}/documents/${documentId}/workflow`,
  );

  expect(
    response.ok(),
    `reading the document's workflow failed: ${response.status()} ${await response.text()}`,
  ).toBeTruthy();

  const open = (
    (await response.json()) as { data: { tasks: WorkflowTaskRow[] } }
  ).data.tasks.filter((task) => task.completedAt === null);

  expect(open, "the document should have exactly one open task").toHaveLength(
    1,
  );

  return open[0];
}

test("an administrator reassigns the open task a role delete waits on to a role, and the delete gets past it", async ({
  page,
}) => {
  const blocked = await seedBlockedRole("Moved to a role");
  const { username, password } = credentials();

  await signIn(page, username, password);
  const { row, refused, tasks } = await refuseDelete(page, blocked);

  // --- Reassign it to the role its decisions are reserved to -------------------
  const reassign = await openReassign(page, tasks);

  // The role being deleted is not offered: reassigning to it would change
  // nothing.
  const roleChoice = reassign.getByLabel("Role", { exact: true });
  await reassign.getByLabel("A role").check();
  await expect(
    roleChoice.locator("option", { hasText: blocked.offeredRoleCode }),
  ).toHaveCount(0);

  await roleChoice.selectOption({
    label: `${blocked.deciderRoleName} (${blocked.deciderRoleCode})`,
  });
  await reassign
    .locator("#reassign-task-comment")
    .fill("The decider's role decides these now.");
  await reassign.getByRole("button", { name: "Reassign", exact: true }).click();

  // --- The list is read again, and nothing in it waits on the role any more ---
  await expectListEmptied(refused, reassign, tasks);

  await refused.getByRole("button", { name: "Close" }).last().click();
  await expect(refused).toHaveCount(0);

  // --- Try the delete again: it gets past the open tasks -----------------------
  //
  // The published workflow still offers its task to the role, so D-91 (3)
  // refuses next, in the confirmation, naming the definition. No open-task
  // list opens.
  const confirmation = page.getByRole("dialog", { name: "Delete role" });

  await row.getByRole("button", { name: "Delete" }).click();
  await expect(confirmation).toBeVisible();
  await confirmation.getByTestId("confirm-action").click();

  await expect(confirmation.getByRole("alert")).toContainText(
    "published workflow definition",
  );
  await expect(confirmation.getByRole("alert")).toContainText(
    blocked.workflow.workflowKey,
  );
  await expect(refused).toHaveCount(0);
});

test("an administrator reassigns the open task to a live user who can decide it, and that user holds it", async ({
  page,
}) => {
  const blocked = await seedBlockedRole("Moved to a user");
  const { username, password } = credentials();

  await signIn(page, username, password);
  const { refused, tasks } = await refuseDelete(page, blocked);

  // --- Reassign it to somebody holding the role its decisions need ------------
  const reassign = await openReassign(page, tasks);

  await reassign.getByLabel("A user").check();
  await reassign.getByLabel("User", { exact: true }).selectOption({
    label: `${blocked.decider.displayName} (${blocked.decider.username})`,
  });
  await reassign.getByRole("button", { name: "Reassign", exact: true }).click();

  await expectListEmptied(refused, reassign, tasks);

  // --- Who holds it now -------------------------------------------------------
  //
  // The task has left the list, so the screen can no longer say. Read over the
  // API: held by the user, and offered to no role, since a reassign to a user
  // clears the role.
  const task = await openTaskOf(blocked.documentId);

  expect(task.assigneeUserId).toBe(blocked.decider.id);
  expect(task.candidateRoleCode).toBeNull();
  expect(task.status).toBe("ASSIGNED");
});

test("somebody who may delete roles but not reassign tasks is shown the open tasks and no Reassign", async ({
  page,
}) => {
  const blocked = await seedBlockedRole("Not theirs to move");

  // A role that may read and delete roles, and not `workflow:task:reassign`.
  // `createApproverRole`'s own four are about working one's own tasks, and
  // none of them reassigns.
  const deleterRoleCode = `E2E-DELETER-${runSuffix()}`.toUpperCase();
  const deleterRoleId = await createApproverRole(session, deleterRoleCode, [
    "identity:role:read",
    "identity:role:delete",
  ]);
  const deleter = await addApprover(session, deleterRoleId, "deleter");

  await signIn(page, deleter.username, deleter.password);
  const { refused, tasks } = await refuseDelete(page, blocked);

  // The list is theirs to read; the action on each task is not theirs to take.
  await expect(tasks.getByRole("button", { name: "Reassign" })).toHaveCount(0);
  await expect(
    refused.getByRole("columnheader", { name: "Actions" }),
  ).toHaveCount(0);
});

test("a role deleted while the reassign dialog is open is refused under the role field", async ({
  page,
}) => {
  const blocked = await seedBlockedRole("Moved to a role that went");

  // A role no workflow names and nobody holds, so nothing stops its delete.
  const doomedRoleCode = `E2E-DOOMED-${runSuffix()}`.toUpperCase();
  const doomedRoleId = await createApproverRole(session, doomedRoleCode);

  const { username, password } = credentials();

  await signIn(page, username, password);
  const { tasks } = await refuseDelete(page, blocked);

  // --- Choose it while it is live ---------------------------------------------
  const reassign = await openReassign(page, tasks);

  await reassign.getByLabel("A role").check();
  await reassign.getByLabel("Role", { exact: true }).selectOption({
    label: `Approver ${doomedRoleCode} (${doomedRoleCode})`,
  });

  // --- And delete it before submitting ----------------------------------------
  const deleted = await session.context.delete(
    `${API_PREFIX}/identity/roles/${doomedRoleId}`,
  );

  expect(
    deleted.ok(),
    `deleting the role failed: ${deleted.status()} ${await deleted.text()}`,
  ).toBeTruthy();

  await reassign.getByRole("button", { name: "Reassign", exact: true }).click();

  // ASSIGNMENT_UNRESOLVED on `roleCode`, placed under the role field in the
  // server's words. The dialog stays open, and the task stays listed.
  await expect(reassign.locator("#reassign-task-role-error")).toContainText(
    "A task is reassigned only to somebody who can act on it",
  );
  await expect(reassign).toBeVisible();
  await expect(tasks).toHaveCount(1);
});
