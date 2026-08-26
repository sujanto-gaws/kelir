# Changelog

All notable changes to Kelir are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html) as applied by the
[release process](docs/standards/04.%20Release%20Process.md).

While the major version is `0`, the public API may change in any release.

## [Unreleased]

Phase 4 opens: the RAD metadata tables and the definition APIs over them, the
document table group with document types and their numbering rules, one JSON
Logic engine shared by both sides, and a browser harness that drives a real
deployment. Two Phase 2 carry-overs land with them, and a third — tenant
management — returns from the unscheduled backlog and takes multi-tenant mode
with it.

Two of the Sprint 7 verification pass's findings are closed with them. A
department-scoped sequence keeps a counter per department rather than one per
rule, which needed a migration and a table; and a `sum` that would silently
total zero is refused when the definition is saved rather than confirmed by the
server that re-evaluates it.

Alongside them, every open defect the three verification passes had filed and
left standing is closed. Four of the eight are contract defects rather than
behavioural ones — documentation that described something the code did not do —
and two of those needed a migration to correct, because a merged migration is
never edited. Two carry a behaviour change worth reading before upgrading: a
deleted party keeps its `partyId`, and the audit chain's hash format has
changed.

### Added

- **Multi-tenant mode runs, and tenants are administrable (FR-ORG-001,
  FR-IDM-009; decision D-18, superseding D-7).** `KELIR_MULTI_TENANT` no longer
  refuses to start the backend. `/api/v1/organization/tenants` creates, lists,
  renames, suspends and removes tenants, and **creating one creates the
  administrator who can sign in to it, in the same transaction** — the objection
  that kept this surface unscheduled for four sprints was that it would create
  rows nobody could reach. A tenant's code is fixed once it exists, because that
  code is what its users type at sign-in; suspending or deleting a tenant
  revokes its refresh tokens, so sessions end rather than merely failing to
  renew.
- **`GET /deployment`.** Unauthenticated, root-level, and one field: whether this
  deployment is multi-tenant. The login form reads it to decide whether to ask
  for a tenant code, which it must do before it has any credentials. A
  build-time `VITE_` flag was rejected because the frontend image bakes only a
  relative API base today, so one build serves every deployment.
- **A tenant-code field on the sign-in form.** Shown when the deployment says so,
  and **also shown when the server answers that one was required** — so a
  deployment that could not be probed costs one attempt rather than locking
  somebody out. That second path is [#67](https://github.com/sujanto-gaws/kelir/issues/67),
  which had been closed by refusing to boot rather than by building the field.
- **A Tenants screen**, behind `organization:tenant:read`, with a Playwright flow
  covering sign-in → list → create (`e2e/tests/create-a-tenant.spec.ts`).

- **The parity corpus covers the `sum` edge cases its two implementations
  promise to agree on.** Both sides' comments name three — a non-array argument,
  an empty array, and non-numeric members — and `parity/cases.json` carried only
  the empty array. All three are in it now, with the shorthand and the
  multi-argument shape, so a change to either `sum` fails the build instead of
  passing quietly.
- **One JSON Logic engine on both sides (decision D-10).** The backend evaluates
  JFSS calculation and validation rules with `datalogic-rs` and the browser with
  `@goplasmatic/datalogic-wasm`, which are the same Rust core behind two
  runtimes rather than two implementations that agree by inspection. A shared
  corpus in `parity/` is replayed by a test on each side, so a rule that
  computes one answer on the server and another in the form is a build failure
  rather than a support ticket.
- **RAD metadata tables (Database Schema §5).** `0014_rad.sql` creates the form
  and list definition tables, their revisions, sections, components, columns,
  filters and lookups, and seeds nine `rad:*` permissions.
- **Form and list definitions (FR-RAD-001, 002, 003).**
  `/api/v1/rad/forms` and `/api/v1/rad/lists` create, list, read, update,
  publish, revise and soft-delete definitions. A published revision is
  immutable; editing means a new draft revision. A form's JFSS document is
  validated against the vendored JFSS v2.0.1 meta-schema on the way in, so a
  definition that no renderer could read is refused at the API rather than
  discovered by a user.
- **Document tables and document types (Database Schema §6).**
  `0015_document.sql` creates the document table group, and
  `/api/v1/document-types` manages the types that documents are created from —
  each bound to a form definition, so a type whose form does not exist cannot be
  saved.
- **Numbering rules (FR-DTYPE-002).** A document type carries a numbering rule
  with a scoped sequence, so numbers are unique within the scope that matters
  (tenant, facility, year) rather than globally. `0016_numbering_gap_policy.sql`
  makes the gap policy explicit: a sequence that must not skip is allocated
  inside the transaction that uses it and is therefore contended, and one that
  may skip is not — the trade is stated in the schema instead of being implied
  by the code.
- **Self-service password reset (FR-AUTH-006).**
  `POST /api/v1/auth/forgot-password` emails a single-use link and
  `POST /api/v1/auth/reset-password` redeems it, with pages behind both and a
  "Forgot your password?" link on sign-in. The link is good for 30 minutes,
  redeeming it signs the account out everywhere and voids every other
  outstanding link for it, and a resend is throttled per account. **The request
  endpoint answers the same way whatever it is given** — unknown identifier,
  suspended account, throttled resend, mail server down — because any difference
  would tell an unauthenticated caller whether an account exists. Mail goes to
  mailpit in the development and staging stacks; a deployment with no
  `KELIR_SMTP_HOST` logs instead of failing to start.
  `password_reset_tokens` has existed since `0006` and until now nothing read
  it.
- **Lookup fields bound to master data (FR-RAD-007,
  [#161](https://github.com/sujanto-gaws/kelir/issues/161)).** A form definition
  can declare a field whose options come from a master-data query rather than
  from the definition, and `GET /api/v1/rad/lookups/{source}/options` resolves
  them — paged, searched and filtered on the server. Four sources: `supplier`,
  `customer`, `employee` and `facility`.

  **A lookup cannot become a way to read master data the caller could not read
  directly.** It requires exactly what the master-data endpoint it projects
  requires — `master-data:party:read` with `master-data:party-role:read` for the
  three role-backed sources, `master-data:facility:read` for facilities — and it
  requires them by *calling that module's service* rather than by checking a
  string of its own, so the two cannot drift apart. No `rad:lookup:read` exists,
  deliberately: a deployment able to grant the lookup without the list would be
  the gap the permission was meant to close. A caller without the permission gets
  **403 rather than an empty page**, because an empty page is a false statement
  about the data that nobody filling in a form can tell from a tenant with no
  suppliers yet.

  The binding lives in the form's `settings.lookups`, mapping a component `id` to
  a source. JFSS is frozen at v2.0.1 and closes a component to new properties, so
  `settings` — the one object it leaves open to an implementation — is where a
  Kelir extension may go; a definition carrying a lookup is therefore still a
  conformant JFSS v2.0.1 document. Bindings are checked when the definition is
  **saved**, in both directions: a source nobody serves, a lookup field nothing
  binds, a binding naming no component, an ambiguous `id`, and a bound field that
  also carries static `options` are each a 422 at the API rather than a chooser
  that opens empty in front of a user.

  Nothing is stored in `rad_lookup_definitions` and it still has no endpoint. The
  sources are a code allow-list, because a source decides both which query runs
  and which permission it needs, and a row that chose the second would make a
  misconfigured lookup a permission bypass that reads as a typo.

- **A published form definition renders as a form (FR-RAD-010,
  [#162](https://github.com/sujanto-gaws/kelir/issues/162)).** `/forms/{id}`
  reads a definition through `GET /api/v1/rad/forms/{id}` and produces a form
  from it — the first RAD surface in the frontend, and the first thing to
  consume #161's lookup endpoint. Nine `data` types render (`textfield`,
  `textarea`, `number`, `select`, `radio`, `checkbox`, `date`, `lookup`,
  `datagrid`), four containers, four display types and `button`; every label,
  help text, required marker, option list and column count comes from the
  definition and nothing about a specific form is in the code.

  **All three of JFSS §4.3.1's child-container shapes are traversed.** A
  renderer that followed only `components` would silently drop every field
  nested inside a `columns` or a `tabs` container, which §4.3.1 names as the
  failure — so each container owns its own shape, and a repeater's `components`
  is treated as the row template it is rather than as a set of siblings.
  Inactive tabs stay mounted: a required field on a tab nobody opened must
  still count once rules arrive.

  **Kelir's component vocabulary is one file, and a test holds it there.**
  JFSS §4.4 makes `type` an open vocabulary defined by each implementation's
  registry, and the meta-schema enumerates none — so nothing upstream decides
  which component types exist and the backend cannot refuse a definition for
  using one this frontend has no component for. `features/rad/renderer/registry.ts`
  is that vocabulary; a type it neither supports nor declares missing renders as
  a **visible placeholder naming the type**, because a form silently missing a
  field is indistinguishable from a form that never had one. The registry's test
  discovers every JFSS fixture in the repository rather than listing types, so a
  fixture using an undeclared type fails the suite.

  **No rules, deliberately.** Validation is #163 and submitting is #164; the
  evaluator is not imported by this surface at all, which is also what keeps its
  588 KB off the render path per decision D-10. A button raises its action and
  the page says submitting is not built yet, rather than appearing to work.

### Changed

- **Roles are tenant-scoped, and the database now enforces it
  ([#65](https://github.com/sujanto-gaws/kelir/issues/65)).** Every tenant has
  its own `ROLE-ADMIN`; the permission catalogue stays global. Three identity
  reads that had been joining across the boundary — `roles_of_user`,
  `permissions_for_user`, `role_codes_for_user` — now filter `tenant_id` like
  their siblings, and `0017_tenant_administration.sql` adds composite foreign
  keys that make a cross-tenant grant unwritable. **The first-run bootstrap was
  writing exactly such a row** on any deployment whose
  `KELIR_DEFAULT_TENANT_CODE` was not `SYSTEM`; it now looks its role up inside
  the tenant it is creating the account in.
- **Tenant administration is restricted to the deployment's default tenant.**
  Holding `organization:tenant:manage` is not enough — the request must come
  from the tenant `KELIR_DEFAULT_TENANT_CODE` names. This is the boundary rather
  than a convenience: the permission catalogue is global and a tenant's own
  administrator holds `identity:role:update`, so they can grant themselves any
  code in it. A provisioned tenant's role is also created without the
  `organization:tenant:*` family, which is defence in depth on top.
- **`KELIR_SMTP_PORT` and `KELIR_MAIL_FROM` are read by the backend.** Both have
  defaults that match the mailpit the local stack runs, so no deployment needs
  to set them; a deployment that relays for a real domain must own the address
  in `KELIR_MAIL_FROM`. `KELIR_FRONTEND_URL` now also determines what a reset
  link points at, so it must be an address a person's browser can reach.

### Fixed

- **A `DEPARTMENT_YEAR` numbering rule no longer issues `000001` to every
  document
  ([#200](https://github.com/sujanto-gaws/kelir/issues/200), decision **D-21**).**
  `document_type_numbering_rules` held a single counter, and the schema said so:
  *"One bucket per rule."* A department-scoped sequence needs one bucket **per
  department**, live at the same time, so every allocation that changed
  department reset the only bucket there was — allocating for department A, then
  B, then A, then B issued `000001` four times, and a second document in either
  department would have been refused at submit by
  `uq_documents_tenant_id_document_number`. `0020_numbering_buckets.sql` moves
  the counters into `document_type_sequence_buckets`, one row per scope value,
  keyed on the document type so that correcting a template does not restart a
  sequence. Allocation is now a single `INSERT … ON CONFLICT DO UPDATE …
  RETURNING`: no read to race, and two scope values do not contend at all.
  Nothing numbers documents yet — the document surface is Sprint 9 — so a
  deployment carrying a configured rule keeps its counter and loses nothing.
- **A `sum` that would silently evaluate to zero is refused when the form
  definition is saved
  ([#201](https://github.com/sujanto-gaws/kelir/issues/201), decision **D-22**).**
  `sum` takes one argument and sums the array it evaluates to. Given an argument
  *list* of any other length — `{"sum": [a, b]}`, the natural mistake, because
  `+` sits beside it in the registry with the same bracket syntax and does take
  a list of operands — it answered `0`. On both engines, identically, which is
  what hid it: the server-side re-evaluation behind JFSS S8.1 catches a client
  that *disagrees* with the server, so a shape both sides get wrong together was
  confirmed rather than caught. Such a definition is now refused at the API with
  `SUM_TAKES_ONE_ARRAY`. **Nothing about evaluation changed**, so no parity risk:
  the shorthand `{"sum": {"var": "line_totals"}}` still works and is still
  accepted, which was measured rather than assumed.
- **A bad `page` or `pageSize` is refused inside the error envelope
  ([#122](https://github.com/sujanto-gaws/kelir/issues/122)).** The two
  parameters were deserialized by the extractor, so a value that was not a `u32`
  was rejected before any handler ran — a bare `400` with an **empty body**, on
  every list endpoint in the product. A client written against `error.code`
  found `null`. `QueryParams` and `PathParam` join the existing `JsonBody`, so no
  refusal under `/api/v1` leaves the envelope; a bad query parameter is now a 422
  naming the parameter as the caller spelled it, and a bad path segment stays a
  400 with a body to read.
- **An over-long field is a 422, not a 500
  ([#109](https://github.com/sujanto-gaws/kelir/issues/109)).**
  `contactMechanisms[].purposeTypeId` had no length check and its column is
  `VARCHAR(64)`, so the value reached the INSERT and came back as
  `INTERNAL_ERROR`. The sweep that came with the fix found six more of the same
  shape on the party and four on the role profiles; Database Schema §1.3.1
  records the rule and the width-to-constant mapping, and two tests assert the
  boundary rather than describing it.
- **Restating a role no longer hands back what somebody else wrote
  ([#119](https://github.com/sujanto-gaws/kelir/issues/119)).** #104 narrowed
  this route's answer to one assignment, and that answer still carried `comments`
  and `additionalAttributes` — both merged on update — so a caller holding only
  `master-data:party-role:assign` read back values they never sent. The route now
  answers with the request it was given.
- **The published contract says which fields a role `PUT` replaces and which it
  merges ([#120](https://github.com/sujanto-gaws/kelir/issues/120)).** The
  asymmetry is deliberate and its reason was written down — in a doc comment on a
  repository function, where no caller could read it. The behaviour was
  discoverable only by losing a `thruDate`. No behaviour change.
- **Every master-data join is scoped by tenant, and the module doc is true
  ([#108](https://github.com/sujanto-gaws/kelir/issues/108)).**
  `repository/mod.rs` opened by claiming every query filters by `tenant_id`. It
  was true of the base tables and false of the joins, so a cross-tenant row
  present in storage would have rendered another tenant's `party_code` inside
  `GET /parties/{mine}`. Latent — no request could create such a row — and one
  bulk import away from live.
- **A deleted party keeps its `partyId`
  ([#107](https://github.com/sujanto-gaws/kelir/issues/107)).** The unique index
  was partial on `deleted_at`, so a delete released the code while every stored
  reference kept pointing at the row by id — a customer's `billingPartyId` went
  on reading `PARTY-BILL` after a different legal entity took the freed code.
  `0018_party_code_is_not_released.sql` makes the index total. **Creating a party
  whose code a deleted party holds is now a 409**, and says so in as many words,
  because the caller cannot see that party in any list. The matching question for
  *profile* numbers is [#103](https://github.com/sujanto-gaws/kelir/issues/103)
  and is still open.
- **The audit chain covers what a record says it changed
  ([#145](https://github.com/sujanto-gaws/kelir/issues/145)).** `chain_hash`
  covered ten inputs and neither payload column was among them, nor `created_at`
  — so all three could be rewritten without disturbing any hash, and the chain
  still verified. A control that protects who and when but not *what* protects
  the half nobody would bother to forge. The format changed while that was free:
  FR-AUD-003 is Phase 6 and nothing has ever verified a chain, so no stored hash
  had been relied on. Fields are now length-prefixed as well, and payloads are
  hashed in `jsonb`'s own text form so a row read back recomputes to the value
  stored with it. `0019_audit_hash_covers_the_payload.sql` carries the corrected
  column comments, which a merged migration cannot.
- **Losing a session no longer leaves a dead page
  ([#68](https://github.com/sujanto-gaws/kelir/issues/68)).** The route guard
  runs on navigation, so a session lost while the user sat on a page redirected
  nothing: an administrator editing a role submitted the form, got a 401, and had
  no explanation. The store now announces an ending it did not ask for — a
  refused refresh, a revoked token, another tab signing out — and the router
  leaves the page, saying why on arrival.
- **Database Schema §3.9 and the Sprint 4 record.** Both the section and
  `0006_password_reset_tokens.sql`'s header said Sprint 4 "added the reset token
  flow". It added the table and no flow. The migration comment cannot be
  corrected — `sqlx` checksums the whole file, comments included — so §3.9
  carries the correction.
- **The main navigation landmark.** `aria-label="Main navigation"` sat on the
  `<aside>` rather than on the `<nav>` inside it, so assistive technology found
  no named navigation landmark. Found by the browser harness on its first run.

### Testing

- **A browser harness (`e2e/`).** Playwright drives a real deployment — the
  release images brought up by `deploy-local.sh` — through one full flow, and
  runs in CI as `End-to-end (browser)`. Not a released artefact, so it is not
  versioned with the product. A second flow, tenant creation, joined it.
- **Two security controls were accepted only after the defect was reintroduced
  and the test seen to fail** (coding standard §2.9): the administering-tenant
  check on the tenant routes, and the composite foreign key that refuses a
  cross-tenant role grant. Each test names its mutation in a comment.

## [0.3.0] — 2026-08-24

Phase 3: a party is created and given the roles that make it a supplier, a
customer or an employee; facilities form a hierarchy that stays a tree; master
data moves through a governed lifecycle; and every change to it can be read back
off the record it happened to.

**This is the first release whose rollback was rehearsed and worked.** `0.1.0`
deferred the rehearsal and `0.2.0` failed it. The check was run here at the
sprint close rather than at the tag: the `0.2.0` image boots against this
release's schema and reaches `/health/ready`, which is what
`Migrator::set_ignore_missing(true)` was added for. Rolling back to `0.1.0`
still needs manual work and always will — see *Known limitations*.

**No action is required of a deployment.** The six new migrations apply at
startup. One permission rule narrowed (decision **D-12**, below), but nothing
except `ROLE-ADMIN` holds the permission it affects by default, so no existing
grant loses access.

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

- **Master-data records move through a governance lifecycle (FR-MDM-007).**
  `record_status` had been a column since `0008` and nothing moved it: every
  party, facility, product and service sat at `DRAFT` and always would.
  `POST /api/v1/master-data/parties/{id}/transition` and the same route under
  `/facilities/{id}` now move it, against a legal set stated in one place
  rather than implied by match arms per entity — `DRAFT → ACTIVE → SUSPENDED
  → ACTIVE → INACTIVE → ARCHIVED`, with `ARCHIVED` terminal because an archive
  a record can leave is a filter rather than a decision. **Nothing reaches
  `PENDING_APPROVAL`**: it is the workflow's state (FR-MDM-010, Phase 5+) and
  a record put there today would await an approver that does not exist, which
  is the overstatement this change set out to remove rather than move one value
  over. A transition is not a field edit — it has its own permission
  (`master-data:record-status:transition`, seeded by
  `0011_record_status_permissions.sql`), its own audit action
  (`RECORD_STATUS_CHANGE`, distinct from the `STATUS_CHANGE` that
  `mdm_parties.status` uses), and neither update payload accepts
  `recordStatusId`. The write is conditional on the row still holding the
  status the move was checked against, so two concurrent transitions cannot
  both move a record away from the same state. `recordStatusId` is now readable
  on the party aggregate and on a facility; it was off the wire only because
  nothing could change it.
- **A master-data record's change history reads back (FR-MDM-009).**
  `GET /api/v1/master-data/parties/{id}/audit`, and the same route under
  `/facilities/{id}`. The *write* path shipped with #80's first endpoint —
  every create, update, delete, role assignment, role removal and lifecycle
  transition was already hash-chained into `audit_events`; what was missing was
  the ability to ask, which is what makes the requirement worth having. Oldest
  first, because the question is "how did this get here", paged in the standard
  envelope, with who, when, and both ends of what changed. **The surface does
  not leak what the aggregate withholds**: #81 keeps a party's roles and
  profiles from a caller without `master-data:party-role:read`, and a role
  assignment's audit record names the role type, so those rows are excluded in
  SQL — and excluded from `meta.total` with them, rather than leaving a page
  with holes in it. `previousHash` and `currentHash` are never selected:
  nothing verifies the chain until FR-AUD-003, and publishing it would let a
  client show "verified" beside a chain nobody checked. A sub-resource per
  entity rather than a module-wide feed, because "what happened to this
  supplier" and "what changed last week" are different questions and the second
  belongs to the audit module's own surface (FR-AUD-004, Phase 6).
  `0012_master_data_audit_permission.sql` seeds `master-data:audit:read` — a
  master-data row rather than the audit module's own `audit:read`, which is
  that module's to define when it has endpoints. It is not sufficient on its
  own: see **Changed** for the record's own read permission, which #136
  requires alongside it.
- **Master data has a screen (FR-MDM-008).** `/master-data/parties` and the
  three role views, as **one component over four endpoints** — the backend
  shaped the role-view row so a client rendering all three needs one component
  and not three, and this is that decision honoured. **The server paginates and
  the server filters**: search, the three filters and the page all go on the
  wire, and nothing fetches a population and narrows it locally, which is the
  failure FR-MDM-008 and NFR-PERF-002 exist to prevent. **The URL is the
  state** — page, search and filters live in the query string, so a filtered
  list can be linked to and survives a reload. Loading, failed and empty are
  three states and not two: a screen that showed "nothing matches" over a
  failed request would be lying about the data. The pager trusts the page size
  the server says it used rather than the one it asked for, because the backend
  clamps rather than refuses. A caller holding `master-data:party:read` without
  `master-data:party-role:read` gets the Parties tab and no others — the
  permitted subset, rather than three tabs that can only answer 403. The
  `/parties` list offers no search or filter controls at all, because its
  endpoint accepts none and a control that silently did nothing would be worse
  than its absence. First frontend feature since Phase 2, and the screen the
  `v0.3.0` demo is shown from.
### Fixed

- **An added migration did not rebuild the binary that embeds it.**
  `sqlx::migrate!("./migrations")` reads the directory at compile time and
  nothing declared it a build input, so on an incremental build `db.rs` kept the
  previous set: `0013` was on disk, applied by nothing, and every test still
  passed except the one that counts. `build.rs` now emits
  `cargo:rerun-if-changed=migrations`. CI never saw it — it builds from cold —
  which is exactly why it survived to be found by hand.
- **An update's audit record stated the request rather than the change, and
  reported untouched fields as cleared (#135).** Every field of an update
  request is optional — that is what makes a partial update partial — so a field
  the caller never mentioned serialised as `null`, and `new_value` was built
  from the request. Changing only a facility's address produced a record whose
  `newValue` said the name and the facility type had been cleared; both were
  still there, and the address, the only thing that had actually changed, was in
  neither half. `oldValue` came from the row, so the two halves were not even
  descriptions of the same thing.

  **Both halves now come from the row** — read before the write, read again
  after — **and only the fields whose value moved are recorded.** A field that
  did not move is absent from both halves, which is also what restores the
  distinction `Option<Option<String>>` exists for: an omitted `parentFacilityId`
  leaves the column alone and says nothing in the record, while an explicitly
  cleared one moves to `null` and is recorded as such. The two were
  indistinguishable before, so the trail could not tell a facility taken out
  from under its parent from one whose parent was never mentioned. `address` and
  `additionalAttributes` are covered for the first time; they are updatable and
  had never appeared on either side. A `CREATE` record likewise reads its values
  off the stored row, which differs from the request wherever a name was
  trimmed.

  **This was never a Sprint 6 regression.** `update_party` has had the same
  shape since #80, with the same symptom — changing only a description reported
  `externalId` and `statusId` as cleared — and #98 copied a pattern that was
  already there. Both surfaces are fixed together. The party aggregate's
  members (person, group, identifications, relationships, classifications,
  contact mechanisms) are still absent from the record: they are replaced
  wholesale by their own statements, they have never been recorded, and what a
  *replacement of a list* means as a before and an after is a wider question
  than this one.

  The shared piece is `modules::audit::ChangeSet`, beside `AuditEntry`, because
  every module that audits a partial update meets the same problem. Five tests,
  each seen to fail against the code before this change (§2.9), and the failure
  output of each is the symptom the issue describes.
- **Eight predicates were exercised by no test, including the facility
  transition's compare-and-swap (#139).** The third of these in three sprints,
  after #106 and #121, and found the same way: of 48 mutations over the Sprint 6
  surface, 17 came back green. The sharpest was `move_record_status`'s
  `record_status = $3` on the **facility** statement — the whole of FR-MDM-007's
  concurrency design. Removing it from the party statement turns the
  two-concurrent-transitions test red; removing it from the facility statement
  changed nothing, because `transition()` is one service function over a `match`
  with one statement per entity and every test that exercised a *statement*
  rather than the machine happened to use a party. Thirteen passing tests, half
  the file's own statements untouched.

  Nine tests close the eight: the facility compare-and-swap asserted against the
  repository so the property is deterministic rather than reproduced-sometimes,
  twenty rounds of concurrent facility transitions beside it, a foreign
  *facility* where the existing tenant test inserts a foreign party, a retired
  party refused by the lifecycle read and another refused by the lifecycle
  *write* — the window a delete lands in between the two statements, which no
  route-level test can open on purpose. On facilities: a retired facility leaves
  `meta.total` and not only the page, a retired facility cannot be named as a
  parent, and neither a retired parent nor a retired owner is shown as one, the
  last two written directly into the table because nothing can reach that state
  through the API any more. Seven of the eight mutations are now red.

  **The eighth changed category rather than being covered.**
  `find_facility_id_by_code`'s soft-delete predicate is no longer isolable: since
  #137 its only caller re-reads the parent under the hierarchy lock before
  pointing at it, so dropping the predicate produces the same 422 naming the same
  field, one guard later. Confirmed by removing both and watching the test fail.
  A fix made a predicate redundant, and the mutation that used to prove the
  predicate now proves the fix.

  No product behaviour changed. Both test modules now record which predicates no
  fixture can isolate and why, so the next reader does not file the gap a fourth
  time.
- **No Sprint 6 route reached the OpenAPI document (#138).** Nine handlers —
  the five facility routes, both lifecycle transitions and both change-history
  routes — carried `#[utoipa::path]` annotations that nothing collected, because
  `utoipa` publishes only what `paths(...)` lists and none of them was listed.
  They compiled, routed and served traffic while existing for no client
  generated from the spec, and nothing warned: an unreferenced annotation is not
  an error. The published document listed 22 paths and now lists 28, with
  `Facility`, `CreateFacilityRequest`, `TransitionRequest`, `AuditRecord` and
  the rest of their schemas alongside. Definition of Done §2 requires "API
  changes reflected in OpenAPI", so #98, #99 and #100 had not met it.

  **What let it stand for a sprint was the test.** `the_openapi_document_lists_every_party_route`
  asserted this property by naming eleven party routes, and it passed throughout
  — a checklist of routes has the same failure mode as the list it is checking,
  and both have to be remembered. It is replaced by a test that names none:
  `every_annotated_route_reaches_the_document` scans the source for
  `#[utoipa::path]` annotations and for `.route(` literals, and asserts every
  annotation reaches the document and every served route carries an annotation.
  Both directions were seen to fail — the first against the nine unregistered
  handlers, the second against a route literal with no annotation to match.
  What remains of the party test is what only it can assert: the query
  parameters the role views publish, and the aggregate's response shape.
- **A facility hierarchy could be made cyclic two different ways, and the module
  said it could not (#133, #134).** `parent_facility_id` is a self-reference, so
  the service walks up from the proposed parent and refuses a move that would
  close a loop — but the walk ran on the pool and the write followed it, which
  is check-then-act. Two callers each walked a path the other was about to
  change, both were told the move was legal, and the pair closed a loop neither
  could see alone: reproduced in 18 of 20 rounds. Row locks do not close it,
  because two re-parentings can form a loop while touching four different
  facilities and each caller's own row and its proposed parent are then disjoint
  sets. The check and the write it guards are now one transaction, serialised
  per tenant by an advisory lock; re-parenting a facility is a rare
  administrative act, so taking it one at a time costs nothing measurable and is
  correct without an argument about which rows to lock.

  The second route needed no concurrency at all. Nothing limits how deep a
  hierarchy may be built, and past the walk's depth bound the answer was a
  *prefix* of the ancestor path — so the root was simply not in it, "is this
  facility an ancestor?" answered no about one that was, and moving a root under
  its own descendant returned `200 OK`. The bound existed to survive a cycle
  that reached the table some other way; it had become a way to create one. The
  walk now reports that it stopped early, and a move that cannot be verified is
  refused with `422 TOO_DEEP` naming `parentFacilityId` rather than allowed.
  Both were found by the [Sprint 6 verification pass](projects/verifications/03.%20Sprint%206%20Surface%20Verification.md).
- **Deleting a facility could race a child being created under it (#137).** The
  no-cascade refusal — *this facility still has children, decide what happens to
  them* — counted children on the pool and deleted afterwards, while a create
  resolved its parent on the pool and inserted afterwards. A create that
  resolved the parent a moment before the delete landed produced a live facility
  under a deleted one in 19 of 20 rounds, and nobody decided it: the delete
  answered 204, the create answered 201, and the decision the refusal exists to
  force was never put to anyone. The failure also hid, because both reads join
  the parent on `deleted_at IS NULL` and report the dangling reference as no
  parent at all — the row looks like a root while its column still names a
  retired facility. The count and the delete are now one transaction under the
  same per-tenant lock a re-parenting takes, and a create or a re-parent re-reads
  the parent under that lock before pointing at it. A parent retired in the
  meantime is the same `422` naming `parentFacilityId` that an unknown one gets,
  because from the caller's side the two are indistinguishable and neither is a
  conflict they can resolve.

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

- **A record's change history now requires the record's own read permission as
  well as `master-data:audit:read` (#136, decision D-12).**
  `GET /parties/{id}/audit` needs `master-data:party:read` alongside it and
  `GET /facilities/{id}/audit` needs `master-data:facility:read`. A record's
  `oldValue` and `newValue` **are** the record's own field values — the party
  code, its type, its status, a facility's name and both its references — so a
  caller holding only `master-data:audit:read` was refused at
  `GET /parties/{id}` and answered at `GET /parties/{id}/audit` with the same
  values. The surface already applied that reasoning to the role half of the
  same list, and #97 stated it in so many words for the role views: a row made
  of two surfaces must not be reachable through one of them.

  The previous rule was deliberate and tested, so this is a decision revisited
  rather than a slip; **D-12** records why it went the other way and what the
  alternative was. `master-data:party-role:read` still decides whether the role
  records are in the page, unchanged. Nothing but `ROLE-ADMIN` holds
  `master-data:audit:read` by default, so no seeded grant loses access.
  `0013_master_data_audit_permission_scope.sql` rewrites the catalogue row's
  description to say what the permission grants, and both `403` descriptions in
  the OpenAPI document name both permissions.
- The planned migrations shift down once more:
  `0013_master_data_audit_permission_scope.sql` took the next free number, so
  RAD is now `0014_rad.sql` and the plugin migration `0022_plugin.sql`. Nothing
  merged was renumbered; the Database Schema mapping table is the sequence and
  carries the correction, along with the System Design Document's file listing
  and its one inline forward reference.
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
- The planned migrations shift down three times more:
  `0010_facility_permissions.sql`, `0011_record_status_permissions.sql` and
  `0012_master_data_audit_permission.sql` each took the next free number, so
  RAD is now `0013_rad.sql` and the plugin migration `0021_plugin.sql`. Nothing merged was renumbered; the Database Schema mapping
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

### Known limitations

- **One tenant per deployment.** `tenant_id` scopes every read, but nothing
  resolves a tenant per request, and the backend refuses to start with
  `KELIR_MULTI_TENANT` set rather than serving a sign-in nobody can complete
  (decision **D-7**). Tenant management and the roles-across-tenants question
  (#65) are unscheduled together under **D-13**.
- **Rolling back to `0.1.0` still needs manual work.** That binary predates
  `set_ignore_missing`, so it cannot start against a database carrying
  migrations it does not know. Rollback to `0.2.0` and later is rehearsed and
  works.
- **No staging host and no production environment** (decision **D-9**). The
  release check runs against the Docker Compose stack built from the release
  images, which does not serve TLS — an IP address cannot be issued a
  certificate — so NFR-SEC-010 is not exercised by it.
- **The audit hash chain does not cover the values a record reports** (#145).
  `old_value`, `new_value` and `created_at` are outside `chain_hash`, so a
  record's payload can be rewritten without breaking the chain. Nothing verifies
  a chain yet; the fix is argued for before FR-AUD-003 builds anything that does.
- **Six `Should` findings stay open** on the Phase 3 milestone (#107, #108,
  #109, #115, #119, #120). None gates this release; each is deferred by name in
  the sprint plan.
- **Products and services** (FR-MDM-005/006) and **external source references**
  (FR-MDM-011) have tables and no surface — `Should`, and unscheduled until a
  consumer needs them.

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

[Unreleased]: https://github.com/sujanto-gaws/kelir/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/sujanto-gaws/kelir/releases/tag/v0.3.0
[0.2.0]: https://github.com/sujanto-gaws/kelir/releases/tag/v0.2.0
[0.1.0]: https://github.com/sujanto-gaws/kelir/releases/tag/v0.1.0
