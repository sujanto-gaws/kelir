import { expect, test } from "@playwright/test";

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
} from "../support/workflow";

/**
 * A role whose last holder leaves says so on the Roles page, and its open task
 * is reassigned from there without a delete attempt ([#508], **D-91** (2)).
 *
 * Deactivating the last holder of a role is not refused, so the role stays
 * live and the task offered to it stays open with nobody to decide it. The
 * roles list now carries each role's `liveHolders` and `openTasks` to a caller
 * holding `workflow:task:reassign`, and the row of a role with open tasks and
 * no live holder says *"1 open task, 0 active holders"*. That notice opens the
 * same list a refused delete opens ([#532]), where the same Reassign sits
 * ([#512]).
 *
 * # Offered to one role, decided by another
 *
 * As in `an-open-task-is-reassigned-from-the-roles-page.spec.ts`: a reassign is
 * refused to a role no decision names, so the task is offered to the role
 * being stranded and its decision reserved to a second one, which is where it
 * is reassigned to.
 *
 * # What is seeded over the API
 *
 * The form, type, both roles and a holder each, the workflow and the submit,
 * under this run's suffix. Then the offered role's only holder is deactivated
 * over the API: that is the event this flow is about, and an administrator's
 * screen for it is not what is under test. Everything after it is asserted in
 * the browser, as the seeded administrator, whose `ROLE-ADMIN` holds both
 * `workflow:task:reassign` and `identity:role:delete`.
 *
 * [#508]: https://github.com/sujanto-gaws/kelir/issues/508
 * [#512]: https://github.com/sujanto-gaws/kelir/issues/512
 * [#532]: https://github.com/sujanto-gaws/kelir/issues/532
 */

let session: ApiSession;

test.beforeAll(async () => {
  session = await signInOverApi();
});

test.afterAll(async () => {
  await session?.context.dispose();
});

test("an administrator finds a role whose last holder left, reassigns its open task from the notice, and the notice goes", async ({
  page,
}) => {
  const suffix = runSuffix();
  const strandedRoleCode = `E2E-STRANDED-${suffix}`.toUpperCase();
  const deciderRoleCode = `E2E-DECIDER-${suffix}`.toUpperCase();
  const strandedRoleName = `Approver ${strandedRoleCode}`;
  const deciderRoleName = `Approver ${deciderRoleCode}`;
  const documentTitle = `Left without a holder ${suffix}`;

  // --- Seed a task offered to a role with one holder --------------------------
  const form = await publishForm(
    session,
    {
      formId: `e2e-stranded-${suffix}`.toLowerCase(),
      version: "2.0.1",
      title: "Stranded request",
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
    `Stranded request ${suffix}`,
  );
  const documentType = await createDocumentType(
    session,
    form,
    `Stranded request ${suffix}`,
  );

  // Each role needs a holder at submit: an assignment that resolves to nobody
  // refuses it.
  const strandedRoleId = await createApproverRole(session, strandedRoleCode);
  const lastHolder = await addApprover(session, strandedRoleId, "last");
  const deciderRoleId = await createApproverRole(session, deciderRoleCode);
  await addApprover(session, deciderRoleId, "decider");

  const workflow = await publishSingleStepWorkflow(session, strandedRoleCode, {
    taskName: "Approve the stranded request",
    deciderRoleCode,
  });
  await bindWorkflow(session, documentType.id, workflow);

  const documentId = await createDraft(session, documentType, documentTitle);
  await submitDocument(session, documentId);

  // --- The last holder leaves -------------------------------------------------
  //
  // Not refused (D-91 (2)): the role stays, and so does its open task.
  const deactivated = await session.context.delete(
    `${API_PREFIX}/identity/users/${lastHolder.id}`,
  );

  expect(
    deactivated.ok(),
    `deactivating ${lastHolder.username} failed: ${deactivated.status()} ${await deactivated.text()}`,
  ).toBeTruthy();

  // --- The Roles page says so, on the role's row ------------------------------
  const { username, password } = credentials();

  await page.goto("/login");
  await page.getByLabel("Username or email").fill(username);
  await page.getByLabel("Password").fill(password);
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page).toHaveURL(/\/$/);

  await page.getByRole("link", { name: "Roles" }).click();
  await expect(page).toHaveURL(/\/admin\/roles/);

  // On whichever page its code sorts it to (#521).
  const row = page
    .getByRole("table")
    .getByRole("row", { name: new RegExp(strandedRoleCode) });
  await pageUntilVisible(page, row);

  const notice = row.getByTestId("role-stranded-tasks");

  await expect(notice).toHaveText("1 open task, 0 active holders");

  // --- It opens the open-task list, with no delete tried -----------------------
  await notice
    .getByRole("button", { name: "1 open task, 0 active holders" })
    .click();

  const listed = page.getByRole("dialog", {
    name: `Open tasks need ${strandedRoleName}`,
  });

  await expect(listed).toBeVisible();
  // Nothing was refused, so there is no refusal: the list says why instead.
  await expect(listed.getByRole("alert")).toHaveCount(0);
  await expect(listed.getByTestId("role-open-tasks-stranded")).toContainText(
    "Nobody holds this role any more",
  );

  const tasks = listed.getByTestId("role-open-task");

  await expect(tasks).toHaveCount(1);
  await expect(tasks).toContainText(documentTitle);
  await expect(tasks).toContainText("offered to the role, and unclaimed");

  // --- Reassign it to the role its decision is reserved to ---------------------
  await tasks.getByRole("button", { name: "Reassign" }).click();

  const reassign = page.getByRole("dialog", { name: /^Reassign TASK-/ });
  await expect(reassign).toBeVisible();

  await reassign.getByLabel("A role").check();
  await reassign.getByLabel("Role", { exact: true }).selectOption({
    label: `${deciderRoleName} (${deciderRoleCode})`,
  });
  await reassign
    .locator("#reassign-task-comment")
    .fill("Nobody holds the old role any more.");
  await reassign.getByRole("button", { name: "Reassign", exact: true }).click();

  // --- The list empties, and the roles read again drop the notice --------------
  await expect(reassign).toHaveCount(0);
  await expect(tasks).toHaveCount(0);
  await expect(listed.getByTestId("role-open-tasks-empty")).toHaveText(
    "No open task needs this role any more.",
  );

  await listed.getByRole("button", { name: "Close" }).last().click();
  await expect(listed).toHaveCount(0);

  await expect(row).toBeVisible();
  await expect(row.getByTestId("role-stranded-tasks")).toHaveCount(0);
});
