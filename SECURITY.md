# Security Policy

## Supported versions

Kelir gives security fixes to its **latest release line** only. A fix ships in a new patch release of that line, and it is not backported to earlier ones.

| Version | Supported |
|---|---|
| `0.9.x` | Yes, until `1.0.0` is released |
| `< 0.9` | No |

When `v1.0.0` is tagged, the `1.0.x` line replaces `0.9.x` in this table, and `0.9.x` stops receiving fixes. Upgrading follows the release's `CHANGELOG.md` entry and its *Upgrade notes*.

## Reporting a vulnerability

**Please do not open a public issue, pull request or discussion for a vulnerability.**

Report it privately through GitHub: open the repository's **Security** tab and choose **Report a vulnerability**. Only the maintainers can see the report and the conversation that follows.

Include as much as you can of:

- the affected version or commit;
- the component, such as the API route, the workflow engine, the document or attachment handling, or the deployment stack;
- the steps to reproduce, or a proof of concept;
- the impact you expect: what an attacker gains, and what access they need first;
- whether the issue crosses a tenant boundary.

Test only against an installation you run yourself. Do not test against anyone else's data or service.

## What happens next

- **Acknowledgement:** within **3 business days** of the report.
- **Assessment:** a first assessment, with whether the report is accepted, within **10 business days**.
- **Updates:** at least every **14 days** while a fix is in progress.
- **If it is accepted:** the fix is developed in a private security advisory and released in a patch of the supported line. The advisory is then published with credit to the reporter, unless they ask not to be named.
- **If it is declined:** the reply says why, and whether it is treated as an ordinary bug instead.

## Scope

In scope: this repository's code, which is the backend (`kelir-backend/`) and the frontend (`kelir-frontend/`), its database migrations (`kelir-backend/migrations/`), and the deployment configuration it ships (`deploy/`).

Out of scope: vulnerabilities in third-party dependencies that are already publicly known, unless Kelir's use of the dependency makes them exploitable; denial of service by volume alone; and findings that need a compromised host or a compromised administrator account.

The security requirements Kelir is built against, NFR-SEC-001 to NFR-SEC-010, and the pass that checked them, are recorded in [`projects/security/01. NFR-SEC Security Pass.md`](projects/security/01.%20NFR-SEC%20Security%20Pass.md).
