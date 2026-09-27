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
  type SeededApprover,
} from "../support/workflow";

/**
 * A refused role delete says which tasks it waits on, and who holds them
 * ([#532], decision **D-89**).
 *
 * The 409 said how many open tasks need a role and nothing said which. The
 * Roles page now answers `ROLE_HAS_OPEN_TASKS` with a dialog listing them, as
 * User Manual §11.2 says. This flow drives that as the administrator a person
 * would be: delete, confirm, read the list, and find the role still there.
 *
 * # A claimed task, so the holder is somebody
 *
 * A task needs a role when it is offered to the role and unclaimed, **or**
 * when a decision on it is `allowedBy` the role, claimed or not. An unclaimed
 * task would show *nobody* as its holder, and a screen that never rendered the
 * holder would pass. So the approver **claims** the task first: it still needs
 * the role, because its `APPROVE` edge is `allowedBy` it, and the dialog has to
 * name the approver.
 *
 * # What is seeded over the API
 *
 * The form, the type, the workflow, the role and its holder, the submit and the
 * claim. Claiming through the inbox is another flow's subject. Everything this
 * spec asserts, it asserts in the browser.
 *
 * [#532]: https://github.com/sujanto-gaws/kelir/issues/532
 */
const suffix = runSuffix();

const roleCode = `E2E-HELD-${suffix}`.toUpperCase();
const roleName = `Approver ${roleCode}`;
const documentTitle = `Held by somebody ${suffix}`;

/** One optional field, in the shape `the-dashboard-says-what-is-late.spec.ts` publishes. */
const definition = {
  formId: `e2e-held-${suffix}`,
  version: "2.0.1",
  title: "Held request",
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
let approver: SeededApprover;

/** The approver takes the task on `documentId` from the role's queue. */
async function claimDocumentTask(
  holder: SeededApprover,
  documentId: string,
): Promise<void> {
  const asHolder = await signInOverApi({
    username: holder.username,
    password: holder.password,
  });

  try {
    const inbox = await asHolder.context.get(`${API_PREFIX}/tasks`, {
      params: { pageSize: "100" },
    });

    expect(
      inbox.ok(),
      `reading the approver's inbox failed: ${inbox.status()} ${await inbox.text()}`,
    ).toBeTruthy();

    const task = (
      (await inbox.json()) as { data: { id: string; documentId: string }[] }
    ).data.find((row) => row.documentId === documentId);

    expect(
      task,
      `no open task for document ${documentId} in the approver's inbox`,
    ).toBeDefined();

    const claimed = await asHolder.context.post(
      `${API_PREFIX}/workflow/tasks/${task?.id}/claim`,
      {
        data: {},
      },
    );

    expect(
      claimed.ok(),
      `claiming the task failed: ${claimed.status()} ${await claimed.text()}`,
    ).toBeTruthy();
  } finally {
    await asHolder.context.dispose();
  }
}

test.beforeAll(async () => {
  session = await signInOverApi();

  const form = await publishForm(session, definition, `Held request ${suffix}`);
  const documentType = await createDocumentType(
    session,
    form,
    `Held request ${suffix}`,
  );

  // The role and its holder first: an assignment that resolves to nobody
  // refuses the submit (see `a-document-is-approved.spec.ts`).
  const roleId = await createApproverRole(session, roleCode);
  approver = await addApprover(session, roleId, "holder");

  // Its `APPROVE` and `REJECT` edges are `allowedBy` this role, which is what
  // keeps the task needing it once it is claimed.
  const workflow = await publishSingleStepWorkflow(session, roleCode, {
    taskName: "Approve the held request",
  });
  await bindWorkflow(session, documentType.id, workflow);

  const documentId = await createDraft(session, documentType, documentTitle);
  await submitDocument(session, documentId);
  await claimDocumentTask(approver, documentId);
});

test.afterAll(async () => {
  await session?.context.dispose();
});

test("an administrator deletes a role an open task needs, and is shown the task and its holder", async ({
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

  // The run's own role, on whichever page its code sorts it to (#521).
  const roleRow = page
    .getByRole("table")
    .getByRole("row", { name: new RegExp(roleCode) });
  await pageUntilVisible(page, roleRow);

  // --- Delete it, and confirm --------------------------------------------------
  await roleRow.getByRole("button", { name: "Delete" }).click();

  const confirmation = page.getByRole("dialog", { name: "Delete role" });
  await expect(confirmation).toBeVisible();
  await confirmation.getByTestId("confirm-action").click();

  // --- The refusal, and the task it waits on -----------------------------------
  //
  // The confirmation gives way to the list of tasks, headed by the server's
  // own refusal, which carries the count.
  const refused = page.getByRole("dialog", {
    name: `Open tasks need ${roleName}`,
  });

  await expect(refused).toBeVisible();
  await expect(confirmation).toHaveCount(0);
  await expect(refused.getByRole("alert").first()).toContainText(
    "1 open task needs this role to be decided",
  );

  // The role is this run's own, so its one task is the whole list.
  const tasks = refused.getByTestId("role-open-task");

  await expect(tasks).toHaveCount(1);
  await expect(tasks).toContainText(documentTitle);
  await expect(tasks).toContainText("APPROVAL");
  await expect(tasks.getByTestId("role-open-task-holder")).toHaveText(
    approver.displayName,
  );
  await expect(tasks).toContainText(
    "a decision out of APPROVAL is allowedBy the role",
  );

  // --- And the role was not deleted --------------------------------------------
  //
  // Closed, and then read again from the server: a fresh load is what shows
  // nothing was deleted, where the table left on screen would show it either
  // way.
  await refused.getByRole("button", { name: "Close" }).last().click();
  await expect(refused).toHaveCount(0);

  await page.reload();
  await pageUntilVisible(
    page,
    page.getByRole("table").getByRole("row", { name: new RegExp(roleCode) }),
  );
});
