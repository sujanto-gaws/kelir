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
  ./deploy-local.sh 0.3.0 8080

cd ../../e2e
npm ci
npx playwright install --with-deps chromium
KELIR_E2E_BASE_URL=http://127.0.0.1:8080 \
KELIR_E2E_PASSWORD='a-real-bootstrap-password' \
  npm test
```

| Variable | Default | What it is |
|---|---|---|
| `KELIR_E2E_BASE_URL` | `http://127.0.0.1:8080` | Where the deployed stack answers |
| `KELIR_E2E_USERNAME` | `admin` | The account the flow signs in as |
| `KELIR_E2E_PASSWORD` | — **required** | That account's password. No default: a default password in a repository is a credential in a repository |

`npm run report` opens the HTML report of the last run. Traces, screenshots and
video are kept for failures only.

## What it covers

One flow, deliberately: **sign in → reach the supplier list → filter it**
(`tests/find-a-supplier.spec.ts`). The suite seeds its two rows over the API and
asserts only through the browser — arranging through HTTP is faster and fails
where it is meant to, but an assertion made against the API would pass on a
screen that renders nothing.

Adding a flow means adding a file under `tests/`. Two rules the existing one
follows:

1. **Seed what you assert on.** The deployment keeps its database between runs,
   so a spec that depends on rows another spec created is a spec that passes in
   the wrong order and fails in the right one. `runSuffix()` keeps each run's
   codes unique.
2. **Assert in the browser.** `support/api.ts` deliberately holds no helper that
   reads a list.

## Where it runs

The `End-to-end (browser)` job in [`.github/workflows/ci.yml`](../.github/workflows/ci.yml),
on the same trigger as the frontend job. That job builds both release images,
brings the stack up through `deploy-local.sh` and runs this suite against it, so
what CI exercises is what a release contains. It is the slowest job in the
pipeline for exactly that reason.
