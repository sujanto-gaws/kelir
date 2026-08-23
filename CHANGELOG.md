# Changelog

All notable changes to Kelir are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html) as applied by the
[release process](docs/standards/04.%20Release%20Process.md).

While the major version is `0`, the public API may change in any release.

## [Unreleased]

### Added

- **Master-data schema (Database Schema §4).** `0008_master_data.sql` creates the
  whole master-data table group in one file — the party model, role types and
  party roles, the four role profiles, facilities, products, services and
  external source references — and adds the two foreign keys `0002` deferred
  until `mdm_parties` existed. Seeds the six system role types and the four
  `master-data:party:*` permissions, granted to `ROLE-ADMIN`.
- **Party master data (FR-MDM-001, FR-MDM-003).** `/api/v1/master-data/parties`
  creates, lists, reads, updates and soft-deletes parties. The payload is the
  `PartyAggregate` of architecture document 05: a person or a party group with
  its identifications, status history, relationships in both directions,
  classifications and contact mechanisms in one document. Create, update and
  delete are audited; a status change is audited as a status change rather than
  as an ordinary update.
- **Party roles and role profiles (FR-MDM-002).** A party is given a role, with
  its role-specific profile, through
  `PUT /api/v1/master-data/parties/{id}/roles/{roleTypeId}`; the same party can
  hold SUPPLIER and CUSTOMER at once without being stored twice. Assignment is
  idempotent — 201 the first time, 200 after, updating the assignment and its
  profile in place. Removing a role leaves the party and its other roles alone,
  keeps the assignment as history rather than erasing it, and closes the profile
  with it. Role types stay open: a tenant adds one by inserting a row, with no
  migration.
- **Role and profile data is separately permissioned.** A supplier profile
  carries a bank account number and a customer profile a credit limit, so the
  party aggregate omits `roles` and `profiles` entirely for a caller holding
  `master-data:party:read` without `master-data:party-role:read`. Absent means
  not visible; `[]` means the party holds no roles.
- **Role views (FR-MDM-002, FR-MDM-008).** `GET /api/v1/master-data/suppliers`,
  `/customers` and `/employees` list the parties holding that role, each row
  carrying the supplier, customer or employee number that makes it one — a
  supplier list without supplier numbers would be a party list with a filter on
  it. Search runs over the party code, the party name and the role number;
  `statusId`, `partyTypeId` and `roleStatusId` filter; paging is the standard
  envelope, with the page size clamped rather than refused. Removing a role
  takes the party out of the view while keeping the assignment as history.
  **No new permission:** a view requires both `master-data:party:read` and
  `master-data:party-role:read`, because a row is made of both surfaces and a
  view needing only one would be a way around the other.
- **Facility master data (FR-MDM-004).** `/api/v1/master-data/facilities`
  creates, lists, reads, updates and soft-deletes facilities — the last `Must`
  entity in the master-data epic and the only one that is not a party. A
  facility nests: `parentFacilityId` makes Building → Floor → Room a tree, and
  because a self-referencing column cannot express "and not one of its own
  descendants", the service walks up from the proposed parent and refuses a
  move that would close a loop. The walk is depth-bounded, so a cycle reaching
  the table some other way is a wrong answer rather than a request that never
  returns. A delete refuses while anything still sits under the facility rather
  than cascading — one call should not retire a hundred rows. `ownerPartyId`
  resolves to a live party in the tenant and is refused by name if it does not;
  `facilityTypeId` is a closed vocabulary in code, because the column carries no
  `CHECK` and would store anything; `address` reuses the `postalAddress` shape
  the party contact mechanisms already define. On an update, `parentFacilityId`
  and `ownerPartyId` tell an omitted field from an explicit `null` — without
  that a facility could be given a parent and never taken out from under it.
  `0010_facility_permissions.sql` seeds `master-data:facility:create`, `:read`,
  `:update` and `:delete`; no table was added, because `0008` already created
  `mdm_facilities`.

### Fixed

- **Deleting a party burned the supplier, customer or employee number it held,
  permanently (#103).** `delete_party` soft-deleted only the `mdm_parties` row,
  and the unique indexes on those numbers are partial on `deleted_at IS NULL` —
  so the profile kept the number while no route could reach it to release it
  (`remove_role` refuses at the party lookup). The party code *was* released, so
  a re-created party could take the old code and then be refused its old number.
  The delete now closes the party, its live roles and its profiles in one
  transaction, keeping them as closed history rather than erasing them.
- **Two concurrent role assignments left the party holding one role twice
  (#105).** `assign_role` read whether the party already held the role on the
  pool and then opened a transaction to act on what it read — check-then-act
  across a connection boundary. The database did not catch it either:
  `uq_mdm_party_roles_party_id_role_type_id_starts_at` includes `starts_at`, so
  two inserts with different `fromDate` do not collide. Reproduced 28 times in
  30. For the profiled roles it surfaced instead as a spurious
  `409 That profile number is already in use` on a request that did nothing
  wrong. The party row is now locked for the transaction that writes, so the
  second request reads what the first wrote. The same lock closes a second
  race: a party deleted mid-assignment no longer ends up holding a live role.
- **Assigning a role handed back every profile the party held, without
  `master-data:party-role:read` (#104).** `PUT .../roles/{roleTypeId}` answered
  with the whole role collection while requiring only
  `master-data:party-role:assign`, so a caller who could write a role could read
  the bank account and the credit limit that permission was introduced to gate —
  the aggregate one URL away withholds both. The route now answers with the
  assignment it wrote. A caller who wants the profiles asks `GET .../roles`,
  under the permission that governs them.
- **Ten concurrent role assignments deadlocked the endpoint (#118).** The fix
  for #105 opened the transaction before calling `resolve_profile_references`,
  which runs on the pool — so a request held one connection for its
  transaction and then asked for a second while still holding the first. At the
  pool ceiling of ten, ten concurrent assignments carrying a profile that names
  a department or another party waited on connections held by each other,
  stalled for the five-second acquire timeout and all answered 500. A
  self-deadlock rather than contention: nothing was waiting on the database.
  The references are now resolved before the transaction opens, where
  `create_party` and `update_party` already resolve theirs, so the request
  takes one connection at a time. The party is looked up ahead of them so that
  a request aimed at a party that does not exist is still answered with that
  rather than with which of its profile references was wrong; the locked
  lookup inside the transaction remains the authority. Coding standard §2.5
  now carries the rule this broke.
- **Four tenant and soft-delete predicates were exercised by no test (#121).**
  The direct successor to #106, found the same way: of 25 mutations over the
  party and role-view surface, four came back green. `soft_delete_party`'s
  tenant predicate is the only cross-tenant guard on `DELETE /parties/{id}` —
  the route does not go through `find_party` first — and nothing had ever
  written a cross-tenant write. The role view's own `p.deleted_at IS NULL` was
  masked by #113: the test that covers it deletes through the API, which since
  #113 closes the party's roles as well, so the role predicate absorbed the
  mutation. The other two, `find_party_role` and `soft_delete_party_roles`,
  were added *by* the fixes for #104 and #103, whose mutation runs were aimed
  at the defects they were closing rather than at the queries they were
  introducing. No product behaviour changed for three of them. The fourth did:
  `find_party_role` looked its row up again by
  `(tenant_id, party_id, role_type_code)`, which matches one row only because
  of the tenant predicate — dropping it made the query match two and
  `fetch_optional` return an unspecified one, so no test could pin it without
  asserting on undefined behaviour. The assign route now reads its answer back
  by the assignment's own primary key, which cannot be ambiguous, and
  `insert_party_role` returns the id it wrote. The read-back also moves inside
  the transaction, so the route answers with the row as this call left it.
- **Four tenant and soft-delete tests asserted a query they never exercised
  (#106).** No product behaviour changed: the queries were already scoped, and
  nothing in CI would have noticed them becoming unscoped. Two list tests
  checked only `meta.total`, which `count_parties` produces, while the rows come
  from `list_parties` — under a mutation that dropped the soft-delete filter the
  deleted party came back in `data` and the test still passed, leaving the
  module's highest-traffic read with no tenant or soft-delete coverage at all.
  Two more put their party in *another* tenant, so every route refused at the
  `find_party` gate and nothing downstream ran; the gate absorbed six mutations
  beneath it. The tests now assert the rows as well as the count, and the
  child-query probes keep the party in the caller's own tenant and point its
  child rows at a foreign tenant instead, so the query under test is the only
  thing left standing.

### Changed

- **`PUT /api/v1/master-data/parties/{id}/roles/{roleTypeId}` answers with the
  role assignment rather than with the party's whole `roles` and `profiles`
  collection**, as part of the fix above. Nothing consumed the old shape — the
  party surface has not been released — so this is a narrowing of an unreleased
  contract rather than a break.
- **The three master-data files past ~1000 lines are split (#112).** No
  behaviour change and no test edited — that is the acceptance criterion, and
  a split that needed a test changed would not be one. `service.rs` becomes a
  directory beside `domain/` and `repository/`, which were already directories;
  `domain/party.rs` sheds its validation rules and `repository/party.rs` its
  child-collection queries. Every file in the module is now under 900 lines.
  Each layer re-exports flat, so `service::create_party`, `repo::find_party`
  and `domain::PartyAggregate` all still name what they named before.
- The planned migrations shift down again: `0010_facility_permissions.sql` took
  the next free number, so RAD is now `0011_rad.sql` and the plugin migration
  `0019_plugin.sql`. Nothing merged was renumbered; the Database Schema mapping
  table is the sequence and carries the correction, along with the two inline
  forward references that named the old numbers.
- The planned migrations shift down by one: `0009_party_role_permissions.sql`
  took the next free number, so RAD is now `0010_rad.sql` and the plugin
  migration `0018_plugin.sql`. Nothing merged was renumbered. Four inline
  forward references in the Database Schema were already pointing at the wrong
  migration and are corrected rather than mechanically bumped.
- Bounded string columns in Database Schema §4 take a `§1.3.1` length instead of
  `TEXT`. The section had `status VARCHAR(40)` beside `party_type TEXT`, and six
  of the affected columns sit inside unique indexes — the failure
  `0004_string_lengths.sql` was written to fix. Applied at `CREATE TABLE` time,
  so no existing table is rewritten; recorded as §14 deviation #15.

## [0.2.0] — 2026-08-20

Phase 2: the application signs in, an administrator manages users and roles, and
every identity route is enforced server-side against its own permission.

**Staging is not verified, and rolling back to `0.1.0` still needs manual work.**
`kelir-staging-01` is still unprovisioned, so release checklist item 7 is
outstanding for the second release running. Item 2, N−1 compatibility, was
rehearsed for the first time at this release and failed; the cause is fixed here,
but the fix cannot reach `0.1.0`, which was tagged without it — see *Known
limitations*. Treat `0.2.0`, like `0.1.0`, as cut rather than proven.

### Added

- **Authentication (FR-AUTH-001..005).** Password sign-in with Argon2id hashing,
  JWT access tokens and rotating refresh tokens; logout, `/auth/me`, refresh and
  change-password. Reuse of a rotated refresh token is detected and revokes the
  family. Sign-in resolves its tenant from deployment config (FR-IDM-009,
  single-tenant default).
- **Identity administration (FR-IDM-001..005, 007).** User and role CRUD,
  role assignment, active/inactive status, and the role–permission editor, with
  Vue screens over all of it.
- **Server-side authorization (FR-IDM-005, FR-API-008).** An `Authenticated`
  extractor on every protected route and a `module:resource:action` permission
  named by each service function. Matching is exact — a prefix never grants a
  longer permission.
- **First-run bootstrap.** One administrator is created at startup when `users`
  is empty, once, under an advisory lock, holding the same password rules the
  API enforces and required to change it at first sign-in.
- **Authentication rate limiting and account lockout (NFR-SEC-008).** Ten failed
  attempts per address per minute, then a fifteen-minute block; five failed
  logins lock the account for fifteen minutes. The address is taken from the
  socket unless the deployment declares how many proxies sit in front.
- **Audit trail for identity and authentication.** Sign-in, sign-in failure,
  password change, and every identity write, hash-chained per tenant.
- **Integration test harness.** A private, freshly migrated PostgreSQL database
  per test, driving the real router over the real state. It cannot silently
  skip: a missing database fails as a harness error, not a passed test.

### Fixed

- The login rate limit keyed on a caller-supplied `X-Forwarded-For`, so it was
  evadable by rotation and could be aimed at a third party's address (#54), and
  it covered only `/auth/login` (#56).
- The account lockout was permanent, against a requirement baselining fifteen
  minutes. Five wrong passwords against a known username left an account
  unusable, and a single-administrator deployment unadministrable, with no
  in-product recovery (#55).
- The first-run bootstrap was not one-shot against a soft-deleted administrator,
  its password bypassed the validation every API-set password gets, and it never
  set `must_change_password` (#57).
- A transport failure cleared the browser session, and a cross-tab token refresh
  tripped replay detection and signed both tabs out (#66).
- Bounded string columns carried no explicit lengths, so an oversized value
  succeeded or failed depending on how compressible it was.
- The startup migrator refused to run against a database holding migrations it
  did not recognise, so a redeployed previous image could not start — rollback
  was impossible without editing `_sqlx_migrations` by hand. Unknown *newer*
  migrations are now tolerated; an edited migration is still refused by
  checksum (#76).

### Changed

- **FR-IDM-004 narrowed** from "manage permissions" to maintaining the permission
  catalogue that authorization checks resolve against. The catalogue is
  system-defined — seeded by migration, extended at plugin-installation time —
  because a permission is an identifier the code checks: a row an administrator
  invents is inert, and a check whose row is deleted becomes ungrantable. The
  administrative surface is role–permission mapping (FR-IDM-005). SRS v0.6,
  decision D-6.
- Migration numbering shifted twice as unplanned migrations landed ahead of
  master data. The mapping table in the Database Schema is authoritative.

### Known limitations

- **Rolling back to `0.1.0` still requires manual database work**, despite the
  migrator fix above. The `0.1.0` binary was tagged without it, so it refuses to
  start against a `0.2.0` database (`migration 2 was previously applied but is
  missing in the resolved migrations`). The *schema* is N−1 compatible — every
  change is additive and the `0.1.0` code compiles and queries against it — so
  the obstacle is migration bookkeeping, not the columns. Recovery is to delete
  the rows above the old version's highest migration from `_sqlx_migrations`
  before starting the old image. Rollback from the *next* release needs none of
  this.
- **Staging is still unprovisioned** (#12), so nothing here has run anywhere but
  a developer machine and CI.
- **Multi-tenant mode is not usable from the UI.** Enabling
  `KELIR_MULTI_TENANT` makes sign-in impossible, because the login form has no
  tenant field (#67). The single-tenant default is the supported configuration.
- Forgot/reset password over email, delegation, department and position
  management, and tenant management are Phase 2 scope that moved to Sprint 5.
- Request payloads ignore unknown fields, so a misspelled property is silently
  dropped rather than rejected (#62).

## [0.1.0] — 2026-08-12

First tagged release: the Phase 1 skeleton — a backend that serves and migrates,
a frontend shell that talks to it, and the means to deploy both.

**This release has not been verified on staging.** The environment is defined and
tested locally but not yet provisioned, so release checklist item 7 is
outstanding. Treat `0.1.0` as cut, not proven.

### Added

- **Backend API.** Axum service under `/api/v1` with the standard response
  envelope (`{success, data}`, `{success, data, meta}`, `{success: false, error}`)
  and a central `AppError` mapping failures to stable machine-readable codes.
  Pagination is available to every list endpoint, clamped so a caller cannot
  request an unbounded scan.
- **Operational endpoints.** `/health`, `/health/live`, `/health/ready` and
  `/version`. Liveness never touches a dependency; readiness reports `503` when
  the database is unreachable, so a load balancer drains the instance instead of
  the orchestrator killing it.
- **Database.** SQLx pool and migration runner; migrations apply at startup.
  `0001_core.sql` creates `tenants` and `system_settings` and seeds the reserved
  system tenant.
- **Configuration.** `KELIR_*` environment loading with typed environments.
  `KELIR_JWT_SECRET` has no default, and staging and production refuse the
  development placeholder.
- **OpenAPI.** Generated document at `/api/docs/openapi.json`, never hand-edited.
- **Frontend shell.** Vue 3 application with navigation, a dark theme, lazy
  routes and a Pinia store; a login page (presentation only until Phase 2); and
  the Tailwind CSS v4 plus shadcn-vue baseline.
- **Typed API client.** Unwraps the response envelope so callers receive `data`,
  and normalises every failure — HTTP, network, timeout, malformed body — into an
  `ApiError` carrying the backend's code and JFSS validation details.
- **Deployment.** Multi-stage release images for both stacks; a staging stack
  behind Caddy serving one origin with automatic TLS; `provision-ubuntu-24.sh`
  for a fresh Ubuntu 24.04 host, including PostgreSQL, firewall and daily
  backups; `deploy.sh` and `deploy-local.sh`, the latter deploying to an IP for
  testing before a tag exists.
- **CI.** Formatting, clippy, tests and builds for both stacks, plus commit
  message validation on pull requests.
- **Documentation.** The full set — requirements, architecture, design, database
  schema, the JSON standards family, engineering standards, and an installation
  and deployment guide.

### Changed

- Renamed every `BHUVARLOKA_*` environment variable to `KELIR_*`.
- Compose host ports are configurable through `KELIR_*_PORT`, so the stack can
  run alongside other projects.

### Fixed

- The backend served nothing: `main.rs` printed one line and exited, so the
  compose stack came up without an API.
- Compose wrote into the host's `node_modules` and `target/`, leaving the working
  tree dirty after every run.
- The frontend could not reach the backend at all — no CORS layer existed, so the
  browser refused every response.

### Removed

- Eleven placeholder migrations (`0002`–`0012`). SQLx records a checksum per
  applied migration, so an empty migration applied now would have refused to run
  once its real DDL was written. Each phase adds its migration when it writes it.

### Known limitations

- No authentication. The login page does not sign in; identity arrives in Phase 2.
- No business endpoints. `/api/v1` is mounted and empty.
- No production environment, image registry, or rehearsed database restore.

[Unreleased]: https://github.com/sujanto-gaws/kelir/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/sujanto-gaws/kelir/releases/tag/v0.2.0
[0.1.0]: https://github.com/sujanto-gaws/kelir/releases/tag/v0.1.0
