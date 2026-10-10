# End-to-end harness

Playwright, driving a real browser against a **deployed** Kelir stack. It exists
because of decision **D-14** ([Product Backlog](../projects/planning/02.%20Product%20Backlog.md) §6)
and was built as Sprint 7 item 1 (issue #153).

## Why it lives here and not in `kelir-frontend/`

What it drives is the release stack — the Caddy image serving the built bundle
and proxying `/api/*` to the backend image — not the frontend source tree. Three
consequences follow, and each is the reason for a choice made here:

- **It is not part of the frontend build.** `frontend.Dockerfile` copies the
  whole `kelir-frontend` directory, so a harness inside it would install
  Playwright and its browsers into every release image build.
- **It has its own `package.json`, and that file carries no platform version.**
  The [release process](../docs/standards/04.%20Release%20Process.md) §1 says
  `kelir-backend/Cargo.toml` and `kelir-frontend/package.json` always carry the
  same version; this package is not a released artifact, so it stays at `0.0.0`
  and is not bumped with a release.
- **It has no `webServer` block.** The stack is brought up before the harness
  runs, by the same script a release check uses.

## Running it

Bring the stack up from release images, then point the harness at it:

```bash
cd deploy/staging
KELIR_BOOTSTRAP_ADMIN_USERNAME=admin \
KELIR_BOOTSTRAP_ADMIN_PASSWORD='a-real-bootstrap-password' \
KELIR_COMPOSE_OVERLAY=docker-compose.e2e.yml \
KELIR_E2E_UPSTREAM_TOKEN='a-throwaway-upstream-token' \
  ./deploy-local.sh 0.3.0 8080

cd ../../e2e
npm ci
npx playwright install --with-deps chromium firefox
KELIR_E2E_BASE_URL=http://127.0.0.1:8080 \
KELIR_E2E_USERNAME=admin \
KELIR_E2E_PASSWORD='a-real-bootstrap-password' \
KELIR_E2E_UPSTREAM_TOKEN='a-throwaway-upstream-token' \
  npm test
```

**The two `KELIR_E2E_UPSTREAM_TOKEN` lines must carry the same value, and `KELIR_COMPOSE_OVERLAY` is not optional for the whole suite** ([#593](https://github.com/sujanto-gaws/kelir/issues/593)). One flow needs a system that answers, and the overlay is what starts it; [*The one system the stack can reach*](#the-one-system-the-stack-can-reach) below says what it adds and why. A stack brought up without it runs every other flow. **That one flow is skipped when the harness is given no `KELIR_E2E_UPSTREAM_TOKEN` and is not in CI**, and the report lists it as skipped with the reason. Given the token it always runs, and against a stack without the overlay it fails on its first assertion, which prints the refusal. In CI it is never skipped: no token there is a failure.

**`KELIR_E2E_USERNAME` is in that block even though it has a default**, and it is there because the default is a trap. It matches the `KELIR_BOOTSTRAP_ADMIN_USERNAME` three lines above it, so a stack whose administrator is called anything else signs in as an account that does not exist — and every flow fails on a `401` that reads like a product defect rather than a misconfigured harness. Naming both halves in one command is what keeps them equal. Found by the `v0.4.0` rehearsal ([release 04](../projects/releases/04.%20Release%20v0.4.0.md)).

| Variable | Default | What it is |
|---|---|---|
| `KELIR_E2E_BASE_URL` | `http://127.0.0.1:8080` | Where the deployed stack answers |
| `KELIR_E2E_USERNAME` | `admin` | The account the flow signs in as. **Must equal the stack's `KELIR_BOOTSTRAP_ADMIN_USERNAME`** |
| `KELIR_E2E_PASSWORD` | — **required** | That account's password. No default: a default password in a repository is a credential in a repository |
| `KELIR_E2E_UPSTREAM_TOKEN` | — **required by one flow** | The token the stack's stand-in upstream was started with. **Must equal the stack's `KELIR_E2E_UPSTREAM_TOKEN`**. The flow never sends it; it checks that no screen shows it and that the upstream received it. No default, for the reason the password has none. **Unset outside CI, that flow is skipped; unset in CI, it fails** |
| `KELIR_E2E_UPSTREAM_URL` | `http://127.0.0.1:8089` | Where the harness reads the upstream's journal: the loopback port the overlay publishes, `KELIR_E2E_UPSTREAM_PORT` on the stack's side |

`npm run report` opens the HTML report of the last run. Traces, screenshots and
video are kept for failures only.

## What it covers

~~Twenty-five~~ Twenty-six flows (2026-10-10, with #688's), each the criterion that decides whether an item is Done rather
than a broad sweep. **This table said six until 2026-09-14**, while eight more
specs landed beside it; it is listed in the order the flows were added.

| Flow | Item |
|---|---|
| Sign in → reach the supplier list → filter it (`find-a-supplier.spec.ts`) | #101 |
| Create a tenant with its first administrator (`create-a-tenant.spec.ts`) | #27, **D-18** |
| A published definition renders as a form (`render-a-form.spec.ts`) | #162 |
| The rendered form evaluates its own rules as they are typed (`a-form-calculates-and-validates.spec.ts`) | #163 |
| A filled-in form is submitted, and a payload tampered with in flight is overwritten (`a-form-is-submitted.spec.ts`) | #164 |
| A document is created from a **type**, filled in, submitted, found in the list and moved through a transition (`a-document-is-created-and-submitted.spec.ts`) | #172, and the Phase 4 exit demo |
| A submitted document is approved **by somebody else**, and its status follows (`a-document-is-approved.spec.ts`) | #179, and the Sprint 10 exit demo |
| A file is attached and a conversation held on a document (`a-file-and-a-conversation-on-a-document.spec.ts`) | SRS §9 criteria 6 and 11 |
| A stored list definition becomes a list that sorts, filters and pages (`render-a-list.spec.ts`) | #340 |
| An administrator configures a document type through a screen, and a document is raised from it (`configure-a-document-type.spec.ts`) | #341, **D-64** |
| Signing out ends the session, and the roles screen is driven (`sign-out-and-the-roles-screen.spec.ts`) | #358 |
| A form is built through a screen, and a document is raised against it (`build-a-form.spec.ts`) | #373 |
| A list is built through a screen, and opens with rows in it (`build-a-list.spec.ts`) | #374 |
| The dashboard lists the late task and not the undated one, and the row opens it (`the-dashboard-says-what-is-late.spec.ts`) | #446 |
| The dashboard counts the caller's documents by status as text, leaves out somebody else's, and draws the chart (`the-dashboard-counts-your-documents-by-status.spec.ts`) | #447 |
| The dashboard says how long the caller's documents took to be decided — the approval and the rejection, not the one waiting or somebody else's (`the-dashboard-says-how-long-approval-takes.spec.ts`) | #461 |
| An administrator registers an external system, edits it, adds an endpoint and a credential reference, deactivates it and activates it again — the reference shown as the reference and nothing else (`register-an-external-system.spec.ts`) | #520 AC-8 |
| A value whose pattern match Firefox throws on is not refused by the form, and the server decides it — **in Firefox, the one flow that is** (`a-pattern-the-browser-gives-up-on.spec.ts`) | #496 |
| An administrator deletes a role a claimed task still needs, is shown that task and who holds it, and the role is still there (`a-refused-role-delete-lists-its-open-tasks.spec.ts`) | #532, **D-89** |
| An administrator reassigns the open task a role delete waits on, from that list, to a role that can decide it, and the retried delete gets past the open tasks; to a live user who can decide it, who then holds it; somebody without `workflow:task:reassign` sees the list and no Reassign; and a role deleted while the dialog is open is refused under the role field (`an-open-task-is-reassigned-from-the-roles-page.spec.ts`) | #512, FR-WF-017 |
| A role whose last holder is deactivated says *1 open task, 0 active holders* on its row of the Roles page; the notice opens that role's open-task list with no delete tried, the task is reassigned from it, and the roles read again drop the notice (`a-role-nobody-holds-shows-its-open-tasks.spec.ts`) | #508, **D-91** (2) |
| An administrator runs a test call on an endpoint of a system with no credential from the system's page, confirms it, and is shown the refusal `NO_USABLE_CREDENTIAL` explained, with the integration log id the server named. **A refusal, not an answer**: an answer needs a reachable system and a secret in the backend's environment, which the release stack does not have (`a-test-call-is-refused-and-explained.spec.ts`) | #547, FR-INT-002 |
| An administrator runs a test call whose log id opens the integration log at its row, reaches the log from the navigation, filters it by system so that only that system's row is listed, and reads another system's row in full: its error, its URL, a link to the system, and the request payload showing the credential's type and never its reference. **Two refusals, not a success**, for the reason #547's flow gives: `NO_USABLE_CREDENTIAL` and `SECRET_BACKEND_NOT_CONFIGURED` (`an-integration-log-is-filtered-and-read.spec.ts`) | #548, FR-INT-006 |
| An administrator runs a test call the system **answers**: the dialog says *Success* and *HTTP 200* with the system's body, the log row and its detail say the same, and the log filtered by that system lists it beside a call the same system answered `404`. The system echoes the credential it was sent, so the body Kelir read holds the secret: the dialog and the stored payloads show `[REDACTED]` in each place, and **no response the browser received carries the secret**, plain or in base64. The upstream's own journal holds one call under the log row's correlation id, carrying the bearer token. **The success the two flows above could not show**, on a stack that now has one system to call (`a-test-call-is-answered-and-its-secret-is-masked.spec.ts`) | #593, FR-INT-002, FR-INT-006 |
| An administrator builds a workflow in the editor, with a second approval edge whose condition is built **by keyboard alone** in the logic builder, then saves and publishes it and binds it to a document type on the type screen. A document of that type is approved from somebody else's inbox and ends *Approved*, which only the condition's edge leads to. A second document below the threshold takes the fallback and ends *Completed*, so both branches are proved. Typing through the editor's `v-model` keeps focus, and an unfilled operand survives a write elsewhere in the editor (`build-a-workflow.spec.ts`) | #426 AC6; FR-RAD-009, FR-WF-004/006/013/015, FR-WF-018 (deprecate built in row 6b, #715; covered by the Vue specs, not this flow); #695's two inherited criteria. Seeded over the API: the form, the approver and their role, and the numbering rule |
| An administrator nests a form **by keyboard alone** on the form builder's canvas: a panel from the palette, a field from the panel's own *Add component*, columns with a field in a column, *Move to…* the panel, a removal, and a save; the preview nests the labels and a reload reads the tree back. A second test drags a card by its handle with the pointer (`build-a-nested-form-by-keyboard.spec.ts`) | #688 I3 and D1, part 1; #695's real-browser keyboard criterion, canvas half. The form is created through the screen; nothing is seeded |

**Every flow runs in Chromium except `a-pattern-the-browser-gives-up-on.spec.ts`, which runs in Firefox and only there** ([#496](https://github.com/sujanto-gaws/kelir/issues/496)). It is about a match Firefox throws on. Chromium does not throw on the same match, it keeps matching, so the spec would hold its tab until the test timed out. `playwright.config.ts` holds the split, as two projects.

The suites seed their rows over the API and assert only through the browser —
arranging through HTTP is faster and fails where it is meant to, but an
assertion made against the API would pass on a screen that renders nothing.

**`a-document-is-approved.spec.ts` is the first flow with two people in it**, and
that is the point rather than an incidental detail: an approval one person
raises and the same person approves exercises the mechanism and proves nothing
about the seam it exists for. The task has to reach somebody else's inbox, and
the requester has to see the result without doing anything. It drives two browser
pages in one test for that reason.

Adding a flow means adding a file under `tests/`. Three rules the existing ones
follow:

1. **Seed what you assert on.** The deployment keeps its database between runs,
   so a spec that depends on rows another spec created is a spec that passes in
   the wrong order and fails in the right one. `runSuffix()` keeps each run's
   codes unique, **and anything a screen picks by label, such as a form title
   in a chooser, needs the suffix too**: a fixed title matches every earlier
   run's row, and `selectOption` takes the first of them.
2. **Assert in the browser.** `support/api.ts` deliberately holds no helper that
   reads a list. **One helper reads something else**, and it is not Kelir:
   `support/upstream.ts` reads the stand-in upstream's journal, because whether
   a call left the backend, and with what, is a fact no screen can show.
3. **Find your row, not a position** ([#521](https://github.com/sujanto-gaws/kelir/issues/521)).
   CI starts every run on an empty database, so whatever a spec looks for is
   on page one and is the only row there. A rehearsal, or a second run on one
   stack, is not empty. Reach this run's row by its suffix: through the
   screen's search or filter where it has one, by the save's own response, or
   by opening the row's page. Where a screen has none of those, turn its pages
   with `pageUntilVisible` in `support/paging.ts`. Never wait for a row on the
   first page, count rows across the tenant, or take `.first()` of a list
   other runs write to.

   **What the harness cannot get round:** the new-document type chooser, and
   the form and list choosers in the document type dialog, read one page of
   100 and offer no search. On a database holding more than 100 of those rows
   that sort ahead of the run's own, the flows that use them fail, because a
   person could not make the choice either.

## The one system the stack can reach

**The release stack has nothing an integration test call can reach, and that is correct for a release.** A call that is answered needs a system to answer it, an address the egress guard lets the backend connect to, and a secret in the backend's environment. Until [#593](https://github.com/sujanto-gaws/kelir/issues/593) the browser flows had none of the three, so #547's and #548's flows each end in a refusal, and a `SUCCESS` was proven only by the backend's own tests (`kelir-backend/tests/integration_logs.rs`). The product owner accepted #548's AC6 as partial on 2026-09-29 with #593 as the follow-up.

**The three are added by an overlay, not to the release compose file.** [`deploy/staging/docker-compose.e2e.yml`](../deploy/staging/docker-compose.e2e.yml) is layered over `docker-compose.staging.yml` when `KELIR_COMPOSE_OVERLAY` names it, from the environment or from `.env`. `deploy.sh` reads that one variable; unset, it runs the release stack as before. So the images, the release compose file and the smoke test are what a release contains, and what differs is in one file whose name says what it is for.

| What the overlay adds | Why |
|---|---|
| A service `e2e-upstream`: [`upstream/server.mjs`](upstream/server.mjs), run from a bind mount in the Node image the frontend is built with | The system that answers. It answers `200` only to a call carrying the bearer token, and `401` otherwise, so a success in the browser means the secret was resolved and sent |
| A network `e2e-upstream`, `10.89.93.0/29`, holding the upstream at `10.89.93.2` and the backend | The upstream is on this network **only**. On the default network as well, its name would resolve to two addresses, and the guard holds every address a name resolves to |
| An `ip_range` on that network, `10.89.93.4/30` | Docker hands out addresses from the range only, and the backend's is handed out. So the upstream's address, which is the one the allow-list names, can never be given to the backend. See below |
| `KELIR_INTEGRATION_ALLOWED_CIDRS=10.89.93.2/32` on the backend | See below |
| `KELIR_INTEGRATION_SECRET_SYSTEM__E2E_UPSTREAM_TOKEN` on the backend, from `KELIR_E2E_UPSTREAM_TOKEN` | The secret. Registered by the flow as `env://KELIR_INTEGRATION_SECRET_SYSTEM__E2E_UPSTREAM_TOKEN` |
| The upstream's port published on `127.0.0.1:8089` (`KELIR_E2E_UPSTREAM_PORT`) | So the harness, on the host, can read the journal. The backend does not use it |

**Why `KELIR_INTEGRATION_ALLOWED_CIDRS` is set, and why it is one address.** The guard (`integration::domain::egress`, [ADR-0043](../docs/architectures/adr/0043.%20A%20Test%20Call%20Runs%20in%20the%20Request,%20Resolves%20Only%20env%20Secrets,%20and%20Connects%20Only%20to%20a%20Checked%20Address.md)) refuses every private address unless a listed range holds it, and a compose network is private. Loopback is always refused and no setting opens it, so the upstream cannot be reached on `127.0.0.1` either: this variable is the only knob a deployment has. The entry is the upstream's fixed address as a `/32` and not the network's `/29`, because the backend itself and the network's gateway, which is the host, are on that network too. **Nothing in the product is relaxed for the flow**: the guard, the secret rule and the redaction run as they do in a deployment, with the two settings a deployment with an on-premises system would set.

**The listed address is reserved, because a `/32` allows whoever holds it.** The backend's address on the network is not fixed. Without the `ip_range`, a daemon restart could hand `10.89.93.2` to the backend, which restarts by itself, while the upstream, which does not, is down; the allow-list would then let a test call reach the backend's own port. With it, Docker hands out `10.89.93.4` to `.6` only. Checked on a stack, 2026-10-01: the backend held `10.89.93.5`, and kept it when it was restarted with the upstream stopped. The network is an ordinary bridge, so the upstream itself can open `backend:8080` and the host gateway; it is a stand-in this repository wrote and it calls nothing.

**The secret is the system tenant's.** An `env://` name resolves only under the calling tenant's own prefix, `KELIR_INTEGRATION_SECRET_<CODE>__` ([#618](https://github.com/sujanto-gaws/kelir/issues/618), **D-96** (4) as amended 2026-10-01; [Installation](../docs/operations/01.%20Installation%20and%20Deployment.md) §7.1). The flows sign in as the bootstrap administrator, who is in tenant `SYSTEM`, so the variable is `KELIR_INTEGRATION_SECRET_SYSTEM__E2E_UPSTREAM_TOKEN`. The issue's own text names `KELIR_INTEGRATION_SECRET_E2E`, which was written before the amendment and resolves for nobody now. A stack whose `KELIR_DEFAULT_TENANT_CODE` is not `SYSTEM` needs the overlay's variable name and `support/upstream.ts` changed together.

**What the upstream does, each because the flow asserts on it.** It echoes the `Authorization` header it received under the key `echo`, which is not a key Kelir masks by name, and the token again in base64 under `echoBase64`; and it returns a value of its own under `sessionToken`. So the body the backend reads holds the secret twice, and what a person sees must hold it nowhere. It also keeps a journal of every call, readable at `/__journal`, so the flow can check that the call shown in the browser is the call that arrived.

**If the subnet is taken, or a second overlay stack runs on one host.** `10.89.93.0/29` is outside the pools Docker allocates from by itself. A host that already routes it sets three variables together when bringing the stack up: `KELIR_E2E_UPSTREAM_SUBNET`, `KELIR_E2E_UPSTREAM_POOL` (the `ip_range`, inside the subnet) and `KELIR_E2E_UPSTREAM_ADDRESS` (inside the subnet, outside the pool). The allowed range follows the address. Checked 2026-10-01 with `10.89.94.8/29`, `10.89.94.12/30` and `10.89.94.10`: the flow passed. **A second stack on the same host needs all three, its own `COMPOSE_PROJECT_NAME`, and its own `KELIR_E2E_UPSTREAM_PORT`**, with the harness's `KELIR_E2E_UPSTREAM_URL` set to match that port.

**From a copy of `deploy/staging/`**, as a release rehearsal runs it, set `KELIR_E2E_UPSTREAM_DIR` to this repository's `e2e/upstream`. The overlay's default is `../../e2e/upstream`, relative to the compose files, and beside a copy there is nothing at that path. A wrong path does not pass quietly: the upstream exits on a missing module, and `deploy.sh` refuses the stack and prints its log.

**Taking it down takes both files.** `docker compose` reads the overlay only when it is named, so a `down` with the release file alone leaves the upstream and its network behind:

```bash
cd deploy/staging
KELIR_VERSION=0.3.0 docker compose \
  -f docker-compose.staging.yml -f docker-compose.e2e.yml down
```

`KELIR_VERSION` is there because the release file requires it to be set, and `.env` supplies the rest. `docker compose -p <project> down` does the same by project name, with no file named.

**Seen to fail** (coding standard §2.9), 2026-10-01. The flow was run, with the token given to the harness, against the stack with one piece missing at a time, and went red each time on the piece that was missing:

| Stack | What the flow printed |
|---|---|
| The release stack, no overlay | `422` `SECRET_NOT_FOUND`, naming the variable. The secret is resolved before the host is looked up, so this is the first of the three to be missed |
| The overlay with `KELIR_INTEGRATION_ALLOWED_CIDRS` emptied | `422` `EGRESS_REFUSED`: *a private address, which a test call may not reach unless `KELIR_INTEGRATION_ALLOWED_CIDRS` lists its range* |
| The overlay with the upstream expecting another token | The call is answered `401`, and the dialog says `Failed` where `Success` is expected |
| The overlay with the upstream stopped | `422` `HOST_NOT_RESOLVED`: a stopped container's name leaves the network's DNS |

**And the two paths with no token**, run the same day: outside CI the flow is listed as skipped (`1 skipped`, exit 0); with `CI` set it fails, naming `KELIR_E2E_UPSTREAM_TOKEN`.

## Where it runs

The `End-to-end (browser)` job in [`.github/workflows/ci.yml`](../.github/workflows/ci.yml),
on the same trigger as the frontend job. That job builds both release images,
brings the stack up through `deploy-local.sh` and runs this suite against it, so
what CI exercises is what a release contains. It is the slowest job in the
pipeline for exactly that reason. **Since #593 the job's `.env` names the
overlay above**, so its stack is the release stack and one stand-in system
beside it.

**One thing that job does which the command above does not: it narrows apt to
Ubuntu's own sources before installing the browser**, through
[`scripts/narrow-apt-to-ubuntu.sh`](../scripts/narrow-apt-to-ubuntu.sh). The
divergence is deliberate and is not a step to copy onto your own machine.
`--with-deps` refreshes *every* configured source, and on 2026-09-09 Google's
Chrome repository — which nothing here installs from — served a `Release` file
eight hours newer than the `Packages` file beside it. apt refused the mismatch,
this step exited 100, and because `End-to-end (browser)` is a required context
with bypassing off, **for forty minutes nobody in the project could merge
anything** ([#408](https://github.com/sujanto-gaws/kelir/issues/408)). On a
runner the narrowing costs nothing, because the runner is destroyed with the
job; on your machine it would disable repositories you rely on.

**The premise that narrowing is safe is re-checked on every run rather than
argued once.** `playwright install-deps --dry-run chromium firefox` follows the
install and exits non-zero naming any dependency still missing — so if an
image ever stops carrying one of chromium's or firefox's libraries, the job says which
package instead of `Hash Sum mismatch`.
