import { expect, test } from "@playwright/test";

import { runSuffix, signInOverApi, type ApiSession } from "../support/api";
import {
  createDocumentType,
  createDraft,
  submitDocument,
} from "../support/documents";
import { credentials } from "../support/env";
import { publishForm } from "../support/forms";
import { pageUntilVisible } from "../support/paging";
import {
  addApprover,
  bindWorkflow,
  createApproverRole,
  publishSingleStepWorkflow,
  type SeededWorkflow,
} from "../support/workflow";

/**
 * An administrator reassigns the open task a role delete waits on, from the
 * Roles page, and the delete gets past it ([#512], FR-WF-017, ADR-0042).
 *
 * [#532] made a refused delete list its open tasks. User Manual §11.2 says *a
 * task is reassigned from that list*, and this flow drives that as the
 * administrator would: delete, read the list, reassign the task to a role that
 * can decide it, watch the list empty, and delete again.
 *
 * # Offered to one role, decided by another
 *
 * A reassign is refused to a role no decision names (`TARGET_CANNOT_DECIDE`),
 * and a task whose decision is `allowedBy` the role being deleted still needs
 * that role after any reassign. So the workflow offers its task to the role
 * being deleted and reserves its decisions to a second one. Reassigning to the
 * second releases the first, and the task stays unclaimed so the list's
 * reason is *offered to the role, and unclaimed*.
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
 * The form, the type, both roles and a holder each, the workflow and the
 * submit. The administrator is the seeded one, whose `ROLE-ADMIN` migration
 * 0048 grants `workflow:task:reassign`. Everything asserted is asserted in the
 * browser.
 *
 * [#512]: https://github.com/sujanto-gaws/kelir/issues/512
 * [#532]: https://github.com/sujanto-gaws/kelir/issues/532
 */
const suffix = runSuffix();

const offeredRoleCode = `E2E-OFFERED-${suffix}`.toUpperCase();
const offeredRoleName = `Approver ${offeredRoleCode}`;
const deciderRoleCode = `E2E-DECIDER-${suffix}`.toUpperCase();
const deciderRoleName = `Approver ${deciderRoleCode}`;
const documentTitle = `Moved on ${suffix}`;

/** One optional field, in the shape the other role-delete flow publishes. */
const definition = {
  formId: `e2e-moved-${suffix}`,
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
};

let session: ApiSession;
let workflow: SeededWorkflow;

test.beforeAll(async () => {
  session = await signInOverApi();

  const form = await publishForm(
    session,
    definition,
    `Moved request ${suffix}`,
  );
  const documentType = await createDocumentType(
    session,
    form,
    `Moved request ${suffix}`,
  );

  // Both roles, each with a holder: an assignment that resolves to nobody
  // refuses the submit, and a role nobody holds is a reassign target nobody
  // would work.
  const offeredRoleId = await createApproverRole(session, offeredRoleCode);
  await addApprover(session, offeredRoleId, "offered");
  const deciderRoleId = await createApproverRole(session, deciderRoleCode);
  await addApprover(session, deciderRoleId, "decider");

  workflow = await publishSingleStepWorkflow(session, offeredRoleCode, {
    taskName: "Approve the moved request",
    deciderRoleCode,
  });
  await bindWorkflow(session, documentType.id, workflow);

  const documentId = await createDraft(session, documentType, documentTitle);
  await submitDocument(session, documentId);
});

test.afterAll(async () => {
  await session?.context.dispose();
});

test("an administrator reassigns the open task a role delete waits on, and the delete gets past it", async ({
  page,
}) => {
  const { username, password } = credentials();

  // --- Sign in and reach the roles screen through the navigation ------------
  await page.goto("/login");
  await page.getByLabel("Username or email").fill(username);
  await page.getByLabel("Password").fill(password);
  await page.getByRole("button", { name: "Sign in" }).click();

  await expect(page).toHaveURL(/\/$/);

  await page.getByRole("link", { name: "Roles" }).click();
  await expect(page).toHaveURL(/\/admin\/roles/);

  const roleRow = page
    .getByRole("table")
    .getByRole("row", { name: new RegExp(offeredRoleCode) });
  await pageUntilVisible(page, roleRow);

  // --- Delete it, and be shown the task it waits on ----------------------------
  await roleRow.getByRole("button", { name: "Delete" }).click();

  const confirmation = page.getByRole("dialog", { name: "Delete role" });
  await expect(confirmation).toBeVisible();
  await confirmation.getByTestId("confirm-action").click();

  const refused = page.getByRole("dialog", {
    name: `Open tasks need ${offeredRoleName}`,
  });

  await expect(refused).toBeVisible();
  await expect(refused.getByRole("alert").first()).toContainText(
    "decided or reassigned first",
  );

  const tasks = refused.getByTestId("role-open-task");

  await expect(tasks).toHaveCount(1);
  await expect(tasks).toContainText(documentTitle);
  await expect(tasks).toContainText("offered to the role, and unclaimed");

  // --- Reassign it to the role its decisions are reserved to -------------------
  await tasks.getByRole("button", { name: "Reassign" }).click();

  const reassign = page.getByRole("dialog", { name: /^Reassign TSK-/ });
  await expect(reassign).toBeVisible();

  // The role being deleted is not offered: reassigning to it would change
  // nothing.
  const roleChoice = reassign.getByLabel("Role", { exact: true });
  await reassign.getByLabel("A role").check();
  await expect(
    roleChoice.locator("option", { hasText: offeredRoleCode }),
  ).toHaveCount(0);

  await roleChoice.selectOption({
    label: `${deciderRoleName} (${deciderRoleCode})`,
  });
  await reassign
    .locator("#reassign-task-comment")
    .fill("The decider's role decides these now.");
  await reassign.getByRole("button", { name: "Reassign", exact: true }).click();

  // --- The list is read again, and nothing in it waits on the role any more ---
  await expect(reassign).toHaveCount(0);
  await expect(tasks).toHaveCount(0);
  await expect(refused.getByTestId("role-open-tasks-empty")).toContainText(
    "can be tried again",
  );

  await refused.getByRole("button", { name: "Close" }).last().click();
  await expect(refused).toHaveCount(0);

  // --- Try the delete again: it gets past the open tasks -----------------------
  //
  // The published workflow still offers its task to the role, so D-91 (3)
  // refuses next, in the confirmation, naming the definition. No open-task
  // list opens.
  await roleRow.getByRole("button", { name: "Delete" }).click();
  await expect(confirmation).toBeVisible();
  await confirmation.getByTestId("confirm-action").click();

  await expect(confirmation.getByRole("alert")).toContainText(
    "published workflow definition",
  );
  await expect(confirmation.getByRole("alert")).toContainText(
    workflow.workflowKey,
  );
  await expect(refused).toHaveCount(0);
});
