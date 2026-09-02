# Changelog

All notable changes to Kelir are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html) as applied by the
[release process](docs/standards/04.%20Release%20Process.md).

While the major version is `0`, the public API may change in any release.

## [Unreleased]

Phase 6 opens: **a document starts carrying the things people put on it.**

### Added

- **A notification can leave the building** (FR-NTF-004, FR-NTF-005,
  [#257](https://github.com/sujanto-gaws/kelir/issues/257)). A notification written
  by an approval, an assignment or a decision is now delivered by email as
  well as to the notification centre, on whichever channels its tenant has
  turned on.

  **The channel is data, not a branch.** There is no `match notification_type`
  in the sender: it reads `notification_channels`, and a tenant with an enabled
  row and a template gets an email while one without either does not. A channel
  this build has no sender for — `SMS`, a plugin's — is recorded as a failed
  attempt rather than skipped, so an unbuilt channel is distinguishable from a
  working one. `0039_notification_email.sql` seeds the `EMAIL` channel and two
  templates for the system tenant and **adds no DDL**: all four tables have
  been in `0034_notification.sql` since item 2, which said this issue would
  write three of them.

  **The row is the queue.** `notify` still writes the notification inside the
  transaction of the thing it announces; a worker delivers it after that
  transaction commits. An SMTP call inside that transaction would hold a
  database lock open across somebody else's network, which is **D-35**'s shape,
  and would let a mail-server timeout roll back the approval that triggered it.

  **A delivery is attempted once**, and its outcome — either way, with the
  error text — is written to `notification_logs`, a table nothing had ever
  written a row into. Nothing retries: a poll loop that retried every `FAILED`
  row would send a relay refusing one message a copy of it every few seconds,
  and a correct retry needs a backoff, a cap and a terminal state. **What that
  costs is stated rather than discovered** — a relay down for five minutes
  loses those five minutes of email permanently — in **D-56** and
  [ADR-0034](docs/architectures/adr/0034.%20One%20Delivery%20Attempt,%20Recorded%20Rather%20Than%20Retried.md).

  **A failure never costs the notification.** `notifications.status` is the
  state of the delivery, not of the notification: a failed send leaves the
  title, the body and `read_at` exactly as they were, and the centre still
  shows it unread. A template that names a placeholder the sender cannot
  resolve sends the notification's own title and body — silence is the failure
  this epic exists to end, and an email nobody receives because somebody
  mistyped `{{dueDate}}` in a configuration table is that failure wearing a
  different hat.

  `KELIR_NOTIFICATION_POLL_SECONDS` (default `5`) is how often the sender
  looks.

- **The inbox answers a fourth question: what you have decided** (FR-TASK-009,
  FR-SRH-003, [#256](https://github.com/sujanto-gaws/kelir/issues/256)).
  `GET /tasks?scope=completed` lists the tasks that have been through your
  hands, and each row carries **what was decided and the reason given with
  it** — FR-TASK-006's record, taken with the approval in Sprint 11 and
  readable until now only on the document's own history.

  **A fourth point on one axis, not a second endpoint.** `overdue ⊂ open ⊂ all`
  and `completed ⊂ all`, with the two subsets disjoint: a task that is late is
  still open, because a finished one is not late, it is done. A second
  collection over `workflow_tasks` would have been a second implementation of
  the visibility rule, and the two would drift — which is the failure the file
  those statements live in was already carrying (see *Fixed*).

  **Search narrows whichever list is showing** — the task's own name, the
  document's title and its number — through the same statement, so the search
  runs inside the visibility rule rather than over the rows it returned. A `%`
  or `_` typed by a person is a character: `50%` searches for a percent sign,
  not for everything.

  The screen gains one option on the control it already had and a search box
  beside it; a decided row shows the decision where a waiting row shows its
  status.

### Fixed

- **Eleven environment variables the configuration reference did not list**
  ([#316](https://github.com/sujanto-gaws/kelir/issues/316)). `KELIR_CLAMAV_*` and
  `KELIR_STORAGE_*` had been missing since Sprint 12 and
  `KELIR_TRUSTED_PROXY_HOPS` since Sprint 2, and **every one of those defaults
  is the development compose stack's** — so a deployment that followed
  [Installation and Deployment](docs/operations/01.%20Installation%20and%20Deployment.md)
  §7 to the letter pointed object storage at `localhost:9000` and the virus
  scanner at a host named `clamav`, started, reported healthy, and failed at
  the first upload. Each row now says what the default is *for*, and names the
  compose stack where that is what the default is.

  `KELIR_STORAGE_DRIVER`'s row no longer says "Used from Phase 6"; Phase 6 is
  now. `KELIR_BUILD_SHA`, which `build.rs` reads at compile time, has a row of
  its own in a new §7.3.

  **A test holds it, which is the part worth more than the rows.**
  `configuration_reference.rs` fails when a `KELIR_*` name appears in the
  crate's source with no row in §7.1 or §7.3, and again when §7.1 keeps a row
  for something the binary stopped reading. The gap was found by diffing the
  two sides by hand while adding a twelfth variable — a finding that arrived
  by luck, and this is that diff run on every build.

  The SDD's §13.3 copy of the same list — nine of twenty-nine variables, last
  touched in Phase 2 — is gone, and points at §7 instead. Two lists of one
  thing is how the first one drifted.

  **One thing this did not fix**, named in the row that documents it:
  `KELIR_STORAGE_SECRET_KEY` defaults to `minioadmin` and the
  placeholder-secret guard covers `KELIR_JWT_SECRET` and
  `KELIR_BOOTSTRAP_ADMIN_PASSWORD` and nothing else, so the binary does not
  refuse it in staging or production. The staging compose file does.
  [#317](https://github.com/sujanto-gaws/kelir/issues/317).

- **The inbox's count, its page and its detail gate agree about which rows
  exist** ([#279](https://github.com/sujanto-gaws/kelir/issues/279)). The page
  joined `documents` and the count did not, so a task whose document had been
  soft-deleted was **counted and not listed** — an inbox that said 23 and ended
  at 19, which the comment above the count forbids in as many words. The detail
  gate had the same gap in the other direction: it answered *visible* for a task
  the read behind it then answered 404 for.

  All three carry the join now. And the drift is harder to reintroduce than it
  was: #256's search reads the document's own columns in the count as well as in
  the page, so removing that join stops the crate compiling rather than
  quietly changing a number.

- **A master-data change can be routed through an approval** (FR-MDM-010,
  [#255](https://github.com/sujanto-gaws/kelir/issues/255), decision **D-55**,
  recorded as
  [ADR-0033](docs/architectures/adr/0033.%20A%20Governed%20Record%20Parks%20at%20Pending%20Approval.md)).
  A document type that sets `targetEntityType` and binds a workflow makes
  changes to that entity go through approval; `0038_master_data_governance.sql`
  adds `mdm_change_requests`, and **no `ALTER` to anything** — the
  configuration column has been on `document_types` since Sprint 4, waiting.

  **The record parks at `PENDING_APPROVAL` while the change is decided.** That
  value has been in the schema since `0008` and reachable by nothing;
  `record_status`'s own documentation reserved it for this item and named the
  line that would change when the workflow landed. Submitting the change parks
  the record in the submit's transaction, and **a direct edit is refused while
  it is parked** — under the permission that already governs that write, with
  no new permission and no cross-module query.

  **Approving applies the change in the transaction that closes the process**,
  and moves the record to `ACTIVE`. **Refusing writes nothing and puts the
  record back where it was** — not to `DRAFT`: an active supplier whose change
  is rejected is still an active supplier, which is what the change request row
  remembers. Every attempt, refused ones included, is on the record's own
  history at `GET /master-data/parties/{id}/change-requests`.

  **Nothing in the workflow engine learns about master data.** The chain is
  engine → document → master data: the engine projects a status onto a document
  as it does for every document, and the document module asks master data to
  settle whatever change it carried. `grep -rn "modules::master_data"
  kelir-backend/src/modules/workflow/` returns nothing.

  **A governed change carries the record's own scalar fields.** The
  sub-aggregates — contacts, identifications, relationships, a facility's
  address — are written by multi-statement service logic this process does not
  reproduce inside a workflow's closing transaction, and a change naming one is
  **refused when it is raised**, with the field named, rather than approved and
  half-applied.

### Changed

- **`POST /master-data/{entity}/{id}/transition` answers 409 for
  `PENDING_APPROVAL`**, where it answered 422
  ([#255](https://github.com/sujanto-gaws/kelir/issues/255)). It used to be an
  illegal transition because nothing could approve anything; it is now a legal
  move that the **surface** refuses, because parking is a process's move and a
  record parked by hand would await an approval nobody can give. The same 409
  refuses transitioning a record *out* of `PENDING_APPROVAL`, which would
  strand the change document pointing at it.

- **A file can be filed, removed, or replaced by a link to where it really
  lives** (FR-ATT-006, FR-ATT-009, FR-ATT-010,
  [#254](https://github.com/sujanto-gaws/kelir/issues/254), decisions **D-52**
  and **D-53**, recorded as
  [ADR-0031](docs/architectures/adr/0031.%20An%20External%20Reference%20Is%20Not%20an%20Attachment%20Row.md)
  and [ADR-0032](docs/architectures/adr/0032.%20A%20Soft-Deleted%20Attachment%20Keeps%20Its%20Object.md)).
  `0037_attachment_tail.sql` seeds four categories, adds
  `document_external_references`, and adds `attachment:delete` and
  `attachment:reference`.

  **Categories have rows in them for the first time.** `QUOTATION`,
  `CONTRACT`, `APPROVAL` and `EVIDENCE`, seeded for the system tenant, and an
  attachment or a link can carry one. Filing is optional: a file nobody has
  categorized is a normal state, not a refused upload.

  **Deleting is soft and keeps the stored object** (D-52). The row leaves every
  list and the download refuses it — through the predicate in the statement that
  serves the bytes, not a check beside it — and `storage_reference`, the file
  name and the checksum stay, so the delete is recoverable and the audit trail's
  hash still describes something that exists. **When bytes actually leave a
  deployment is a retention question**, `attachments.retention_policy_id` is
  where it will be answered, and nothing writes it yet: storage grows and
  nothing shrinks it. The screen says *the stored copy is kept* before somebody
  confirms, because *delete* means something narrower here than it sounds.

  **An external reference is its own table, never an attachment row with a URL
  in it** (D-53). It has no size, no checksum, no MIME type and **no scan
  status**, so a link is visibly not a file and can never read `CLEAN` — held by
  the shape rather than by a convention. The alternative would have had to store
  sentinel values that say something false, or make the file columns nullable,
  which breaks the previous release's list at run time. **`url` is `http` or
  `https` and nothing else**: the string is rendered as a link, and
  `javascript:` in an `href` is somebody else's script in this product's page.

  **`attachment:reference` is a new permission**, separate from
  `attachment:create`, because recording a link grants something different: no
  bytes enter the product, nothing is scanned, and the risk is what somebody
  else clicks. Deleting is `attachment:delete`, and both writes ask authorship
  as well: no code in this release lets one account remove another's upload.

- **A conversation can be answered, corrected and taken back** (FR-CMT-002,
  FR-CMT-003, FR-CMT-004, [#253](https://github.com/sujanto-gaws/kelir/issues/253),
  decisions **D-50** and **D-51**, recorded as
  [ADR-0029](docs/architectures/adr/0029.%20A%20Comment%20Thread%20Is%20One%20Level%20Deep.md)
  and [ADR-0030](docs/architectures/adr/0030.%20A%20Deleted%20Comment%20Leaves%20a%20Tombstone.md)).
  Replies, editing and deleting on a
  document's comments, on the API and on the screen that renders them.
  `0036_comment_thread.sql` adds `comments.edited_at`, the self-parent
  constraint, and two permissions: `comment:update` and `comment:delete`.

  **Threading is one level** (D-50). A reply is a `POST` to the same collection
  carrying `parentCommentId`; a reply to a reply is refused with a 422 naming
  the field. `comments.parent_comment_id` would have carried any depth, which is
  exactly why the depth is now decided rather than left to fall out of a
  nullable column — and one level is what a conversation people read while
  deciding whether to approve something can afford.

  **Editing and deleting are the author's, and a permission is not enough.**
  `comment:update` and `comment:delete` say whether an account edits or deletes
  comments at all; `comments.created_by` says whose. There is deliberately no
  moderator permission — a code nothing checks is the `delegations` situation
  **D-13** spent two decisions undoing.

  **An edit is visible as an edit.** `editedAt` is stamped when the body
  changes and by nothing else — not `updatedAt`, which moves for any write to
  the row, the delete included — because a comment whose text changed with
  nothing saying so is a conversation somebody can rewrite after the fact. The
  previous text is **not kept**: what survives an edit is that it happened,
  when, and who did it.

  **Deleting is soft, and it does not take the replies** (D-51). A deleted
  comment that has been answered stays in the conversation as a tombstone —
  author, time, no body — so the answers under it still have something to
  answer; one nobody replied to leaves the list entirely. The row keeps its
  text, withheld at the read boundary, which is **D-45**'s shape applied to a
  second table.

  **All three land on the document's timeline in their own transaction** —
  `Comment.Replied`, `Comment.Edited`, `Comment.Deleted` — and none of them
  carries the body, its length, or the words an edit replaced. The timeline says
  what happened to the document and links to the comment; what it says is behind
  `comment:read`.

  **Still not the decision comment**, and the screen now says so to the person:
  a comment is a conversation its author can edit, and the reason an approver
  gives with a decision is recorded with that decision and cannot be changed.

- **The audit trail is searchable** (FR-AUD-004,
  [#252](https://github.com/sujanto-gaws/kelir/issues/252), decision **D-49**).
  `GET /api/v1/audit`, behind a new `audit:read`, filtered by actor, object
  type, object id, event type and date range, paginated and totally ordered.
  The trail itself is old — it has been written since Sprint 3, which is what
  **D-44** found; what was missing was the ability to ask.

  **A row's recorded values need the object's own read permission**, which is
  **D-12** generalized from one module to nineteen. That decision found a
  party's field values reachable through its change history by a caller refused
  `GET /parties/{id}`; a search crosses every module at once, so the split is by
  what a row is made of: *somebody updated party X at 09:05* is the trail, and
  `{"statusId": "SUSPENDED"}` is the party. **The row is withheld, never
  hidden** — a search that dropped rows would teach an auditor the trail is
  shorter than it is — and `valuesWithheld` says which happened, so an empty
  payload is never ambiguous. An object type the build cannot place withholds.

  **`master-data:audit:read` is unchanged and still opens
  `GET /parties/{id}/audit`.** Two surfaces, two questions, two permissions —
  which is the test D-47 applied to `activity:read` and found it failing.

  **The search does not verify the hash chain**, and says so. Reading the trail
  and proving it unbroken are different questions, and a search implying the
  second would claim something it has not checked.

- **The product tells people things** (FR-NTF-001, FR-NTF-002, FR-NTF-003,
  [#251](https://github.com/sujanto-gaws/kelir/issues/251), decision **D-48**).
  `0034_notification.sql`, an in-app notification centre at
  `/notifications` with an unread badge in the shell, and two triggers: **a
  task reaching somebody tells them**, and **a decision on somebody's document
  tells whoever raised it**.

  **Written in the transaction of the thing it announces.** A notification that
  outlives a rolled-back approval is a lie somebody acts on; one lost when the
  approval commits is a person who never heard, and nothing anywhere records
  that they did not. `notify` takes the caller's transaction and returns its
  error — `activity::record`'s shape, and deliberately not `audit`'s tolerated
  failure.

  **Delegation is honoured because the notification follows the task**, not the
  definition: a window that redirected the task redirects the notification with
  it, and the trigger is handed the task's own holder so re-resolving is not
  something a future author can do by accident.

  **A role task tells every current holder** (D-48). The inbox offers it to all
  of them, so notifying one would be a lottery and notifying none would leave
  this product's commonest approval shape silent. Those rows go stale when
  somebody claims — they are a record of what reached you, and My Tasks is the
  live view, which the centre says on the screen.

  **Marking read is idempotent**, and a second call does not move the
  timestamp. **Nothing notifies about lateness** (FR-NTF-006/007 are `Could`
  and unscheduled, and depend on FR-WF-010, which is too), **nothing sends
  email** (FR-NTF-004 is [#257](https://github.com/sujanto-gaws/kelir/issues/257),
  and this migration creates its three tables unwritten), and there are **no
  preferences** (FR-NTF-005, unscheduled) — every account with the permission
  gets everything addressed to it.

- **A document has an activity timeline, on a screen** (FR-ACT-005,
  [#250](https://github.com/sujanto-gaws/kelir/issues/250), decision **D-47**).
  **SRS §9 criterion 12.** Sprint 12 wrote the events and nothing read them —
  the right order, because a timeline with four sources and no screen is still
  worth having and a screen over one source is not. The workspace gains an
  **Activity** tab: one list, newest first, paged, showing the document
  lifecycle, the workflow, attachments and comments together.

  **All four sources or none.** A timeline showing three of four is worse than
  one showing none, because a reader cannot tell an empty category from a
  missing one — so every entry carries the source it came from, and the panel is
  the place that difference is visible.

  **The actor is rendered as recorded**, never joined to a current name, so the
  timeline still reads correctly after somebody is renamed or removed. And the
  panel says plainly that it is **not the audit trail**, and that the History
  tab beside it is a third thing again — this document's status changes. Those
  three records have been distinguished in prose since #247; this is the first
  screen where a person could otherwise merge them.

- **A file can be attached to a document** (FR-ATT-001, FR-ATT-003,
  [#244](https://github.com/sujanto-gaws/kelir/issues/244)). `POST
  /api/v1/documents/{id}/attachments` takes a `multipart/form-data` body with a
  `file` part and an optional `description`, stores the bytes in object storage
  and records the metadata. **MinIO has been in the compose stack since Sprint 0
  and used by nothing; this is the first byte in it.**
- **A document's attachments can be listed and downloaded** (FR-ATT-002,
  [#245](https://github.com/sujanto-gaws/kelir/issues/245)). `GET
  /api/v1/documents/{id}/attachments` lists them newest first with their scan
  status, and `GET .../attachments/{attachment_id}` serves the bytes. **Both
  resolve through the document's own read permission**, and the download's
  statement is scoped by document as well as by id, so an attachment hanging on
  a document the caller cannot read is *not found* rather than found and
  refused — the answer is the same whether or not it exists.
- **Attachments are scanned, and only a cleared file is served** (FR-ATT-001,
  FR-ATT-002, [#246](https://github.com/sujanto-gaws/kelir/issues/246)). An
  upload returns immediately with `virus_scan_status = PENDING`; a background
  worker streams the bytes to ClamAV over INSTREAM and records what it says. The
  `clamav` service the system design reserved on 2026-08-11 is now in the
  compose stack, **sized rather than guessed**: ~1 GB resident, 25 MiB in
  ~169 ms, and no gain from concurrency.
- **Attachments and comments reach the timeline** (FR-ACT-002, FR-ACT-003,
  FR-ATT-008, FR-CMT-007, [#248](https://github.com/sujanto-gaws/kelir/issues/248)).
  Attaching, downloading and commenting each write an event carrying the id of
  what it describes, so a timeline can offer the file or the comment rather than
  only mention that one exists. **A download that cannot be recorded is
  refused**: if this product cannot note that somebody took a copy of a file, it
  does not hand over the copy.
- **A document has a timeline** (FR-ACT-001, FR-ACT-004,
  [#247](https://github.com/sujanto-gaws/kelir/issues/247)). `GET
  /api/v1/documents/{id}/activity` returns what happened to a document, newest
  first, behind that document's own read permission. Creating, submitting and
  transitioning a document write an event, and so do deciding and delegating a
  task. **The actor's name is denormalized at event time**, so a rename does not
  rewrite the past.
- **A document carries a conversation** (FR-CMT-001,
  [#249](https://github.com/sujanto-gaws/kelir/issues/249)). `POST` and `GET
  /api/v1/documents/{id}/comments` add a comment and read a document's comments,
  oldest first — a conversation is read in the order it was said, which is the
  opposite of every other list in this product. **This is not the decision
  comment**: FR-TASK-006 shipped in Sprint 11 as three columns written with the
  decision and immutable because it is, and `modules::comment`'s documentation
  states the difference before the first row is written.
- **`MultipartBody`**, a fourth request extractor, so a body that is not
  `multipart/form-data` is refused **inside the error envelope** rather than with
  axum's own 400 and a null body. `crate::extract`'s header had claimed three
  extractors were enough for that property; the first route to take a file is
  what made the claim false.

### Changed

- **The activity timeline no longer asks for `activity:read`** (#250 AC2,
  decision **D-47**). It reads through the document's own read permission and
  nothing else, which is what `modules::activity`'s four-record table,
  `0033_activity.sql`'s own `COMMENT ON TABLE` and the [Database
  Schema](docs/design/02.%20Database%20Schema.md) §10 had all said from the
  start.

  **D-45 is what made that safe rather than merely consistent.** While an entry
  carried an attachment's file name, `activity:read` was the only thing between
  a document's reader and another module's data — accidentally, and badly, since
  one grant opened all three. Once `details` says only what happened to the
  document, the second permission guarded nothing the first does not, and all it
  could still do was refuse the person who raised the document a view of their
  own document's history.

  **The permission row is still seeded and is now checked by nothing.** It
  cannot be dropped in the release that stops using it — [release
  process](docs/standards/04.%20Release%20Process.md) §7's N−1 rule is
  *deprecate in release N, remove in N+1*, and the previous release still calls
  `require("activity:read")`. [#301](https://github.com/sujanto-gaws/kelir/issues/301)
  removes it after `v0.6.0`. **No grant needs changing**: an account that holds
  the permission is unaffected, and one that does not can now read timelines it
  could already read the documents of.

### Fixed

- **A task handed back to the person who delegated it is no longer decided
  "on their own behalf"** (FR-WF-009, FR-TASK-008,
  [#280](https://github.com/sujanto-gaws/kelir/issues/280)). Ani hands her task
  to Budi and Budi hands it back: the self-delegation check compares Budi with
  Ani and passes, and `COALESCE` kept Ani in `delegated_from_user_id` — so the
  task said Ani held it on **Ani's** behalf, and her approval's history row
  carried her in both columns. The Workflow tab rendered it as *"ani … on ani's
  behalf"*.

  It is the row `0028_delegation.sql` made the column nullable to prevent:
  *acting for themselves* and *acting for somebody who happens to be them* have
  to stay different rows. **The hand-off now clears the column when it would
  name the incoming assignee**, rather than the decision suppressing a second
  party it was handed — because the task's own row is what the `allowedBy`
  check reads and what the inbox renders, and correcting only the history would
  leave the row untrue. Handing on again after a hand-back names the delegator
  again.

  **No authorization changed.** `assignment::permits` checked the duplicated
  candidate twice and widened nothing; what was wrong was the record.

- **A form whose calculation engine fails to load now says so**
  (FR-RAD-008, [#273](https://github.com/sujanto-gaws/kelir/issues/273),
  decision **D-54**). The load was fired and its rejection went nowhere — an
  unhandled promise rejection, no engine, and a form that computed nothing and
  said nothing. *Still coming* and *never coming* were the same screen: totals
  blank or stale, conditional sections all showing, in silence.

  The rejection is caught, and a form whose engine failed carries **one
  form-level banner** naming what the person can see — totals not updating,
  sections not hiding — and what to do: **the submission is still correct**,
  because the server recomputes every calculated field, and a reload usually
  fixes it. **A form whose engine is merely slow still says nothing**, which is
  **D-10**'s bundle condition and the half a fix here could most easily have
  broken; the submit path is untouched, because a browser that cannot compute
  has no business refusing a submission over values it was never the authority
  on.

- **A download that failed is no longer recorded as one that happened**
  (FR-ACT-002, [#293](https://github.com/sujanto-gaws/kelir/issues/293)). The
  `Attachment.Downloaded` event was committed and the object read afterwards, so
  a storage failure gave the caller a 500 and the timeline an entry saying they
  had taken a copy. **The object is now read first and the event still written
  before the bytes are served**, so the ordering that matters is unchanged — if
  this product cannot record that somebody took a copy, it does not hand over
  the copy — and the false record is gone. Both consequences are named in the
  code, because over-recording remains the safe direction and reversing the
  order entirely would trade this defect for a worse one.

- **The scan write carries a tenant in its predicate**
  ([#294](https://github.com/sujanto-gaws/kelir/issues/294)). It was the one
  write in the Sprint 12 surface scoped by id alone. Nothing was wrong — every
  id came from `pending_scans`, which is right to read across tenants — but a
  statement whose scope depends on its caller having chosen correctly is the
  shape [#106](https://github.com/sujanto-gaws/kelir/issues/106) and
  [#121](https://github.com/sujanto-gaws/kelir/issues/121) cost this project
  three sprints of findings over. The tenant now comes from the row the worker
  read, and `pending_scans` says in its own documentation why reading across
  tenants is deliberate: it is a worker, and there is no caller to scope to.

- **A document with a live approval could be discarded, and the approval
  survived it** (FR-DOC-005, FR-WF-003,
  [#278](https://github.com/sujanto-gaws/kelir/issues/278), decision **D-46**).
  `DELETE /api/v1/documents/{id}` asked one question — *is this a draft* — and
  that was a **proxy** for *has no live process*. The proxy is false: a
  workflow state's `mapsToDocumentStatus` may be `DRAFT`, the projection writes
  what the state says by design, and JWSS §10's own worked example maps its
  initial state that way. So a document could be `DRAFT` while holding a
  number, a running instance and an open task.

  What was left behind was worse than a wrong refusal. The document was
  soft-deleted; the instance stayed `RUNNING` and kept holding the
  one-live-instance index; **the task stayed claimable**, so an approver could
  take it and only then find the document gone; and every decision after that
  answered `404 Document`, because the read filters `deleted_at IS NULL`. The
  process could not be moved again by anybody, an administrator included.

  The delete now asks `workflow_instances` — the fact, not the status — under
  the row lock the submit also takes, so a process starting alongside a discard
  loses the race rather than being stranded by it. The refusal names the
  instance, as the status route's does. **Reproduced before it was fixed**: the
  finding came from a pass with no toolchain and had been traced in source
  rather than run.

  **A state mapping to `DRAFT` stays permitted** (D-46). It is the
  specification's own example, Kelir projects it correctly, and refusing it
  would not have closed the class — `RETURNED` runs with a live process too.

  **No migration, and nothing repairs a strand that already happened.** Every
  definition in this repository maps its running states to `PENDING_APPROVAL`
  or `IN_REVIEW`, so no deployment built from these fixtures can have reached
  it; a tenant that authored its own definition can find out with

  ```sql
  SELECT i.id, i.document_id
  FROM   workflow_instances i JOIN documents d ON d.id = i.document_id
  WHERE  i.status IN ('STARTED', 'RUNNING', 'SUSPENDED')
    AND  d.deleted_at IS NOT NULL;
  ```

  Rows that come back are instances with nothing to decide. Clearing
  `documents.deleted_at` makes them decidable again, and that is a deliberate
  act on a support engineer's part rather than something this release does on
  its own — an undelete is not a thing this product otherwise has.

- **`audit_events.ip_address` was always null** (FR-AUD-005,
  [#248](https://github.com/sujanto-gaws/kelir/issues/248), decision **D-44**).
  `middleware::client_address` resolved the caller's address and defended it
  against a spoofed `X-Forwarded-For`, its own documentation said the audit row
  keyed off it, and **all 53 audit call sites passed `None`** — so the column
  read as evidence that the address had been unavailable. The resolved address
  now travels on the authenticated caller, which made it one line at each site.
  One site still passes `None` and always will: the first-run administrator is
  created at startup with no request behind it, and an invented address is the
  thing that module exists to prevent.

### Security

- **A document's timeline no longer names the files on it**
  (FR-ACT-001, FR-ATT-002, [#292](https://github.com/sujanto-gaws/kelir/issues/292),
  decision **D-45**). Reading a timeline takes `activity:read` and the
  document's own read — and the entries carried an attachment's **original file
  name** and size, a comment's length, and the second party to a delegated
  decision. So a caller holding neither `attachment:read` nor `comment:read`
  learned the name of every file on the document, and a file name is routinely
  the sensitive part: *2026-redundancy-list.pdf* needs no contents to do damage.
  Not a leak across tenants or documents — both are in the statement — but a
  **missing second permission**.

  **The entry now carries the link and not the subject.** Nothing is lost, only
  put back where it was already guarded: the name is in `attachments` behind
  `attachment:read`, the comment and its length in `comments` behind
  `comment:read`, the delegation's second party in `workflow_history` behind the
  workflow's read — and `attachmentId`, `commentId`, `taskId` and
  `workflowInstanceId` are now served, so a reader who holds those permissions
  has somewhere to go and ask. `Workflow.Decided` keeps `action`, `from` and
  `to`, which are what moved *this document*. **The alternative — filtering
  entries a caller may not read — was rejected** because it has to be
  remembered by every module that adds an event type, and forgetting silently is
  precisely the defect being fixed.

  **Rows written by `0.5.x` are covered too.** `activity_events` is append-only,
  so the read serves each entry through an allow-list by event type — `{}` for
  one it does not know — rather than trusting what is in the column.

- **An activity event cannot outlive the action it describes.** It is written
  in the action's own transaction, where an audit row is written on its own
  connection and deliberately survives a failure — two function signatures hold
  the difference rather than a convention.
- **Nothing can produce a false `CLEAN`**, and three separate things hold it —
  in three different places on purpose. The scanner client returns
  `Result<ScanOutcome, ScanError>`, so *did not answer* is a different **type**
  from *answered*; the worker writes a status only on the answered arm, leaving
  an unreachable scanner's files `PENDING` and therefore undownloadable; and the
  `UPDATE` writes only over `PENDING`, so no result can overwrite a decided row
  and nothing moves back out of `INFECTED`. A reply the client does not
  recognise is a refusal, not a pass.
- **There is no setting that turns scanning off.** A deployment that loses its
  scanner stops serving files rather than starting to serve unscanned ones. A
  flag saying *skip the scan* would be a flag saying *serve unscanned bytes*.
- **Nothing is served until a scan clears it.** Download refuses `PENDING`,
  `INFECTED` and `FAILED` alike, with three distinguishable messages, and
  `FAILED` is a refusal rather than a pass — a scan that could not run has
  cleared nothing. The gate is enforced **where the bytes are served**. This is
  [#246](https://github.com/sujanto-gaws/kelir/issues/246)'s download half,
  landed one item early on purpose: nothing sets `CLEAN` yet, so a download
  without it would have served every unscanned byte in the product. **Every
  attachment is currently listed and none is downloadable**, which is the
  intended state until the scanner exists.
- **A file's type is decided by its content, never by its name**
  (FR-ATT-005). The extension and the `Content-Type` are caller-written text;
  the allow-list is matched against the leading bytes. A type nothing recognises
  is refused, and an empty allow-list refuses everything — the failure direction
  for a misconfiguration is to store nothing.
- **Size is refused on the request body before any of it is read**
  (FR-ATT-004). A limit applied to bytes already accepted is a limit on the
  disk, not on the upload.
- **Attachments are never served inline.** `Content-Disposition: attachment`,
  always, with quotes and control characters stripped from the file name — an
  uploaded HTML or SVG served inline is stored cross-site scripting with this
  product's own session behind it, and the allow-list and this are two
  independent controls because neither wants to be the only one.

- **An uploaded file name never becomes an object key.** The stored name is the
  basename, with everything outside `[A-Za-z0-9._-]` replaced and leading dots
  dropped; the name as uploaded is kept beside it as data. `storage_reference` is
  generated from the tenant, the document and the attachment's own id and is
  **never taken from the request** (#244 AC6) — a caller-supplied path is a
  caller-chosen destination — and it is not serialized back to any caller.
- **An attachment is as private as the document it hangs on.** Upload requires
  `attachment:create` *and* the document's own read, and a document the caller
  cannot see answers 404 — the same answer reading it gives, so the refusal does
  not confirm the document exists.
- **Nothing is downloadable yet, deliberately.** `virus_scan_status` is `PENDING`
  on every row and nothing sets it; the gate is
  [#246](https://github.com/sujanto-gaws/kelir/issues/246). An attachment that
  read `CLEAN` because nothing scanned it would be worse than one that reads
  `PENDING` for ever.

### Upgrading

Two new migrations. `0032_comment.sql` adds three tables and nothing else, so
the previous release starts against it unchanged; two of the three
(`comment_mentions`, `comment_attachments`) are created and written by nothing,
and their `COMMENT ON` says which sprint fills them.

`0031_attachment.sql`: three new tables and one foreign key
added to `document_type_attachment_rules`, whose `category_id` has been `NOT
NULL` with no referent since `0015`. **Nothing has ever written that table**, so
the constraint validates against no rows and the previous release starts against
this schema unchanged.

**The scanner is new and required.** `KELIR_CLAMAV_HOST` and
`KELIR_CLAMAV_PORT` default to the compose stack's service;
`KELIR_CLAMAV_POLL_SECONDS` bounds how long a file waits before anything looks
at it. **`StreamMaxLength` on the scanner must be strictly greater than
`KELIR_STORAGE_MAX_UPLOAD_BYTES`** — clamd's own sample config documents 25M,
which is exactly the upload default, and at that value every maximum-size upload
is refused by the scanner, recorded `FAILED`, and permanently undownloadable.

**Two new limits, both configuration** (#245 AC5).
`KELIR_STORAGE_MAX_UPLOAD_BYTES` defaults to 25 MB — the figure the scanner will
be sized against, so the two want to move together — and
`KELIR_STORAGE_ALLOWED_MIME_TYPES` defaults to PDF, PNG, JPEG, DOCX and XLSX.
`deploy/env/.env.example` carries both with their reasoning.

**Object storage must be configured and its bucket must already exist.**
`KELIR_STORAGE_ENDPOINT`, `KELIR_STORAGE_BUCKET`, `KELIR_STORAGE_ACCESS_KEY`,
`KELIR_STORAGE_SECRET_KEY` and `KELIR_STORAGE_REGION` are new, and
`deploy/env/.env.example` carries them with the compose stack's defaults. **The
application does not create the bucket** — a process that can create buckets can
create the one an attacker names — so the compose stack grows a one-shot
`minio-init` service and CI's backend job creates it before the tests run. A
deployment that leaves object storage unconfigured still boots and serves every
other route; uploads are refused with a message naming the variables.

**`0035_audit_search.sql` adds one permission row** (`audit:read`, granted to
`ROLE-ADMIN`) and nothing else — no table, no column, no index: `audit_events`
and the three indexes the search uses have been in place since `0003_audit.sql`.
**Grant `audit:read` to whoever should be able to search the trail**, and grant
the object read permissions alongside it to whoever should see the recorded
values; `audit:read` on its own is a coherent grant — *who did what, when*,
without the payloads.

**One new migration, `0034_notification.sql`.** Four tables and one permission
row (`notification:read`, granted to `ROLE-ADMIN`); nothing is altered and
nothing dropped, so the previous release names none of it and starts against
this schema unchanged. **Grant `notification:read` to the roles that should have
a notification centre** — an account without it has no centre and is told
nothing, which is the only control this release offers (FR-NTF-005's preferences
are unscheduled).

Three of the four tables — `notification_templates`, `notification_channels`,
`notification_logs` — are created and written by nothing. They belong to the
email channel (#257) and their `COMMENT ON` says so.

**The activity timeline's payload changed shape, and no migration goes with it**
(#292, **D-45**). `GET /api/v1/documents/{id}/activity` gains
`workflowInstanceId`, `taskId`, `attachmentId` and `commentId` — the columns
`0033_activity.sql` has always written and nothing served — and its `details`
object loses every key that described the entry's subject rather than the
document. **Rows already in `activity_events` keep what they were written with**:
the redaction happens at the read, because the table is append-only and there is
no honest migration that edits history. Nothing in this repository consumed
`details`; FR-ACT-005's screen is Sprint 13 and is written against the links.

## [0.5.0] — 2026-08-30

Phase 5 opens: **a submitted document enters an approval it cannot leave by
accident.** A workflow definition is authored and published; a document type
carries one; submitting a document of that type starts a process in the same
transaction that numbers it; the process generates a task; somebody else
approves or rejects it from their inbox; and the document's status follows from
that rather than beside it.

**This is not the MVP.** **D-1** puts that at `v0.6.0`, at the end of Phase 6 —
attachments, comments, the activity timeline, notifications and audit search are
not in this release. A reader meeting "approvals, end to end" is likely to
assume otherwise, which is why it is said here rather than left to the roadmap.

**Upgrading:** seven new migrations. `0025_workflow.sql` adds ten tables, three
constraints and eight permission rows; `0027_workflow_history.sql` adds one
table; `0028_delegation.sql` and `0029_workflow_routing.sql` add one nullable column
to it each; `0030_workflow_self_transition.sql` **drops** a `CHECK` that `0027`
had added, which widens what the table accepts and so cannot break a reader;
`0024_one_live_role_per_party.sql` tightens one unique index on
`mdm_party_roles` (see *Changed*); and `0026_form_section_not_its_own_parent.sql`
adds a self-parent `CHECK` to a table nothing writes yet. Both are
compatible with the `v0.4.0` binary, which starts against a `v0.5.0` schema —
the workflow one because it is additive, the index one because `v0.4.0`'s only
writer into that table holds the party row under `FOR UPDATE` and updates in
place, so it cannot reach the violation the index now refuses. **A database that
already holds a party with the same role type twice will refuse that migration
by name** rather than apply it; no released binary can have written such a row,
so it is a guard for development databases rather than an expected upgrade step.
`ROLE-ADMIN`
receives the eight `workflow:*` permissions automatically; every other role
receives none, so an approver needs `workflow:task:read` and
`workflow:task:execute` granted deliberately.

**One behaviour changes for a document under an approval.** Setting a document's
status by hand through `PUT /api/v1/documents/{id}/status` is now **refused
while a workflow is deciding it**, naming the process. The synchronization is
one-way by design: a transition sets the status, and the status does not move
the process. A document of a type that binds no workflow is unaffected, and one
whose process has finished is transitionable again.

### Added

- **Conditional routing** (FR-WF-015,
  [#186](https://github.com/sujanto-gaws/kelir/issues/186)). A transition taken
  because a condition holds — the approval that goes to a senior approver above
  a threshold.

  **Most of it was already built, and that was the point of the item.** The
  evaluator is `rad::evaluator` and has been since **D-10** adopted
  `datalogic-rs` on both sides; the operator bound is the registry's, checked at
  save; `engine::fire` has selected an edge by condition since Sprint 10, with
  S7's fallback-last ordering. So this item is mostly *binding an existing
  evaluator to a new context*, and the tests for those parts say plainly that
  they are regression assertions rather than new behaviour.

  **What changed is what happens when a condition breaks.** An expression that
  fails to evaluate — a division by zero, an argument of the wrong shape — now
  **stops the transition** with a refusal naming the edge. It used to read as
  `false`, which sent the process down the fallback: a *different branch, chosen
  because the intended one broke*, with nothing anywhere recording that the
  routing rule never ran. A workflow that routes wrongly on a bad expression is
  worse than one that refuses to move. **This reverses Kelir's own earlier
  answer**, and the reasoning that produced it is quoted where it was replaced.

  Because the failure depends on the data, no save-time check can catch it: the
  expression may be well formed, use only registered operators, and break on the
  third document it meets.

  **A state whose conditions were all false, with no fallback, now says so.**
  That is a different problem for an administrator than an action the state does
  not declare at all — the definition has a gap, and the process is sitting still
  because of it. The refusal names how many conditions were tried, and the trail
  is logged.

  **And the history answers "why did this go to her and not to him."**
  `workflow_history.routing_json` records every condition the engine actually
  evaluated, in order, each with its outcome — including the ones that said no,
  which are the half that explains why *not* the other branch. The chosen edge's
  own condition is a tautology on a history row, so recording only that would
  have answered half the question and looked like all of it. The document
  workspace renders which branches were considered and what each said; the
  expression travels on the wire and is deliberately not drawn, because a JSON
  Logic blob is not what the person deciding an approval needs.

  **Upgrading:** `0029_workflow_routing.sql` adds one nullable column.

- **Task due dates, and the indicator that makes them mean something**
  (FR-WF-011, FR-TASK-007,
  [#185](https://github.com/sujanto-gaws/kelir/issues/185)). **One item, because
  a due date nobody is shown is a column.** A JWSS task declares `dueInHours`;
  the engine stamps `workflow_tasks.due_at` when it generates the task; the
  inbox marks what is late and can be narrowed to it.

  **The window is relative and the stamp is absolute, and that is the point.** A
  definition outlives every instance that runs it, so an absolute date in one
  would be wrong for every instance after the first. The stamp is written once,
  at generation — **a deadline does not move because somebody published a new
  revision afterwards**, which would be a deadline nobody agreed to.

  **One clock, named.** The stamp and every later *is this overdue* comparison
  are both `now()` in PostgreSQL, and the answer reaches the client as
  `isOverdue` rather than as a date to compare. A browser judging `dueAt`
  against its own clock would be a second opinion, and a task that is late on
  one screen and not on another is a bug report nobody can reproduce. An
  application fleet with drifting clocks would produce the same failure at the
  writing end.

  **A task with no due date is not overdue**, written as an explicit null check
  rather than left to three-valued logic — the usual shorthand, a null coalesced
  to the epoch, reports every undated task as years late. **And a task finished
  after its date passed is done, not late:** the indicator says what needs doing
  now, and marking completed rows would bury the ones that do.

  **`scope` gains `overdue` as a third point on one axis** — `all ⊃ open ⊃
  overdue` — rather than a flag beside `open`. A late task is still an open one,
  so combining them would let a caller ask for *completed and overdue*, which is
  a question with no answer, and would need two controls on the screen to
  express one choice.

  **Nothing acts on lateness**, and the specification now says so where a reader
  would otherwise assume otherwise. FR-WF-010 (escalation) is `Could` and
  unscheduled, and the due-task reminders of FR-NTF-006/007 depend on it; a
  JWSS task's `escalation` block is stored and executed by nothing, exactly as
  `guards` and `actions` are.

  **No migration.** `workflow_tasks.due_at` has existed since
  `0025_workflow.sql`; what arrived is a writer.

- **Delegation, end to end** (FR-IDM-006, FR-WF-009, FR-TASK-008,
  [#184](https://github.com/sujanto-gaws/kelir/issues/184)). Somebody goes on
  leave and their approvals keep moving. Three surfaces, one item by decision
  **D-17**: the identity-side **window**, the workflow-side **hand-off**, and the
  **record** that keeps both names on a decision one person made for another.

  **A window is opened in your own name and nobody else's.**
  `POST /api/v1/identity/delegations` takes a delegate, a start and an end — and
  no delegator, because the request type does not have the field. That omission
  is the security property rather than a UI choice: a holder of
  `identity:delegation:create` who could name somebody else would be able to
  point the finance director's approvals at themselves, and the row would look
  exactly like legitimate cover. Reading and ending are administrative
  (`identity:delegation:read`, `:delete`), because the window somebody has to be
  able to find is the one whose owner went on leave without ending it. The
  `delegations` table has carried its window and not-self checks since
  `0002_identity.sql`; what arrived here is a writer and — the point of **D-17**
  scheduling all three requirements together — a reader.

  **The reader sits exactly where JWSS §5.1 says it does.** Sprint 10 left the
  assignment resolver in two named halves and a paragraph saying a window applies
  between them ([#176](https://github.com/sujanto-gaws/kelir/issues/176) AC4);
  `redirect` is now that function, and neither half changed to accommodate it.
  A task the definition addressed to Ani reaches Budi while the window is open,
  and the task records that it is still Ani's approval.

  **A window redirects a person's work and never a role's.** A task offered to a
  role has no assignee to redirect — it is unclaimed, and every other holder is
  still being offered it — so `ROLE`-scoped windows are refused at creation with
  that reason rather than stored as rows that would route nothing. `ALL` and
  `DOCUMENT_TYPE` are honoured, most specific first.

  **Tasks already assigned when a window opens do not move**, and the hand-off is
  why that is a decision rather than a gap. `POST /api/v1/workflow/tasks/{id}/delegation`
  gives one open task to somebody else, explicitly, by the person holding it.
  Opening a window that silently reassigned work already in progress would move
  approvals out from under people mid-decision on a schedule nobody triggered;
  a window with no complement would strand those tasks for the length of the
  leave. **It is not a decision**: nothing about the document is answered and the
  process does not move, which is why it is its own route rather than a fourth
  `DecisionAction`, and why it writes to the task's history and not the
  document's. An unclaimed role task is refused with a 409 saying to claim it
  first.

  **A delegated decision records both parties** —
  `workflow_history.on_behalf_of_user_id`, new in `0028_delegation.sql`, rendered
  in the document workspace as "budi on ani's behalf". It goes on the history and
  not on `approval_decisions`, whose approver is the signature: a history showing
  only the delegate answers *who decided* and loses *on whose authority*, which
  is the accountability delegation exists to preserve. It is read off the task
  row the server wrote, never off a request, so acting on somebody's behalf is
  not something a caller can assert.

  **Delegation grants nothing.** The delegate acts with their own account and
  their own permissions: without `workflow:task:execute` they are refused at the
  gate, holding a task addressed to them. What the hand-off does give them is the
  *delegator's* satisfaction of a transition's `allowedBy` — checked against the
  rule as that person, so they can do what the delegator could and nothing the
  delegator could not. A second hand-off keeps the original name: the authority
  being exercised has not changed hands, only the work has.

  **A window stops the moment it is over.** Expired, switched off, or pointing at
  an account that has since been deactivated — each is a clause in the one
  statement that routes, so it stops on the next transition rather than on a
  sweep, and work falls back to the person the definition named.

  **`workflow_tasks.status` gains no new value.** A delegated task stays
  `ASSIGNED`; `DELEGATED` is in the column's `CHECK` and outside both the
  one-open-task-per-instance index and the inbox's open filter, so writing it
  would leave a running process with no open task and hide the work from the
  person just given it. Who holds a task and where the work has got to are two
  questions, and `delegated_from_user_id` answers the first.

  **A known limitation, stated rather than hidden:** the "hand it to" picker on
  the task screen reads the user list, so it needs `identity:user:read` — the
  narrowest read the identity module has that answers *who could take this*. A
  deployment that has not granted it to its approvers sees the panel say so
  instead of opening onto nothing. A narrower "people I may delegate to" read is
  not built.

- **The return action, and the resubmission that closes the loop** (FR-WF-008,
  [#183](https://github.com/sujanto-gaws/kelir/issues/183)). An approver's third
  answer. **Reject is terminal and return is not**: a document with a wrong
  figure and a right intent goes back to its author, is corrected, and comes back
  up **with the same number, the same history and the same place in the queue** —
  which is the outcome return exists to preserve, and previously cost a
  recreation from scratch.

  **The target is the definition's, never inferred.** A `RETURN` transition's
  `to` names where the document lands; *send it back one step* is ambiguous the
  moment a workflow has a branch, and each reader would resolve it differently.
  **No JWSS change was needed** — the specification already declared `RETURN`,
  `RESUBMIT`, and the stateless `RETURNED` state its §10 example uses.

  **The resubmission comes back through `POST /documents/{id}/submission`** —
  the same button the first submit used (**D-42**). A `RETURNED` state declares
  no task, because the document is with its author and not in anybody's inbox,
  so there is no task id to address; the edge is authorized by its own
  `allowedBy` instead, which is the first caller of the control
  [#226](https://github.com/sujanto-gaws/kelir/issues/226) built. A resubmission
  **allocates no number at all** under either gap policy — not one it then
  discards, which would leave a permanent hole per correction round on a
  gap-tolerant rule.

  **A returned document is editable and is not deletable**, and those are now two
  predicates rather than one. It holds a number, a status history and a live
  process waiting for it: deleting it would strand the instance that returned it
  and retire a number an auditor can see was issued. *I opened this by mistake*
  and *this request is withdrawn* stay different questions.

  Returning is subject to the same one-decision-per-task rule as approve and
  reject, at the same concurrency, and the history records the return, its target
  and its reason — *"why is this back with me"* is the question history exists to
  answer.

- **Approve and reject from the task, with the reason** (FR-TASK-004, 005, 006,
  [#182](https://github.com/sujanto-gaws/kelir/issues/182)). A decision and the
  reason for it are entered together on the task screen and sent in one request,
  because they are one interaction: a screen that recorded the decision and then
  asked for a reason would have already committed the half nobody can take back.

  The comment lands on three rows in one transaction — `workflow_tasks.comment`,
  `approval_decisions.comment` and `workflow_history.comment` — so the task
  surface, reporting and the account a person reads cannot disagree about what
  an approver said. **The document workspace's Workflow tab now shows the
  history** #181 records, which is where the reason becomes visible: one
  captured where the decision is not visible would not be read.

  **A transition may require it.** JWSS gains `transitions[].requiresComment`
  (§4.1), defaulting to `false`, and the engine refuses the transition without
  one — a 422 on `comment`, checked against the edge `condition` actually
  selected. Per *transition* rather than hard-coded, because that is where the
  answer differs: an approval explains itself and a refusal does not, and which
  is which belongs to whoever wrote the workflow (**D-41**). The task detail
  carries `requiresComment` so the screen refuses an empty box before sending,
  under the server's rule rather than one of its own.

  **The comment is not copied into the audit trail**, which records only that
  there was one. `audit_events` is read through `master-data:audit:read` by
  people holding no permission over the document, and a decision comment is
  prose an approver wrote about somebody's requisition — the line **D-12** and
  **D-32** already drew. The reason itself is on the history row, behind
  `workflow:instance:read`, which is what the people it was written for hold.

- **A workflow history record per transition** (FR-WF-012,
  [#181](https://github.com/sujanto-gaws/kelir/issues/181)). Every move a
  process makes writes a row — the submit entering the initial state and every
  decision after it — carrying both ends, the action, the actor, the task it
  came from and the timestamp. It is written in the transition's own
  transaction, from `engine::fire`, which is the one place a process moves: a
  transition that committed without its history would leave a gap in the answer
  to *how did this document get here* that nothing could see.

  Read at `GET /api/v1/documents/{id}/workflow/history`, oldest first and
  **paginated** — a returned-and-resubmitted document accumulates rows without
  bound. Behind `workflow:instance:read` and deliberately **not**
  `master-data:audit:read`: this is the document's own account of its progress,
  shown to the approver deciding it, and requiring the governance permission
  would refuse it to the people it is for.

  **It is not the audit trail, and Database Schema §7.11 states the
  relationship** rather than leaving it to be inferred — three tables record
  something about a workflow event, and history, task history and
  `audit_events` answer three different questions for three different readers.
  Neither history is derived from the audit trail: an audit row a user-facing
  screen depends on becomes an audit row nobody can correct.

  **Append-only by construction.** `workflow_history` has no `deleted_at`,
  `updated_at` or `updated_by`, so a soft delete has nowhere to write and an
  edit has nothing to stamp.


- **Workflow definitions** (FR-WF-001, 002, 003). Authored as
  [JWSS](docs/schema/JSON%20Workflow%20Schema.md) documents, validated **on
  save** against the meta-schema, the operator registry and the structural rules
  S1–S10 — so a workflow that could deadlock, strand a state or route
  ambiguously is refused before it is stored rather than discovered by whoever
  was waiting for an approval that could not move. Published revisions are
  immutable and project their states and transitions.
- **Process instances with workflow variables** (FR-WF-014). An instance pins
  the definition revision it started against, so a running approval does not
  change shape underneath its approver, and it cannot be in a state its own
  definition does not declare — held by a foreign key rather than by convention.
- **User tasks generated on transition** (FR-WF-004), assigned to a user *or*
  offered to a role. Claiming a role task is a compare-and-swap: two people
  claiming at once produce one owner and one refusal.
- **Approve and reject** (FR-WF-006, 007), the API the task screen above sits
  on. A task already decided cannot be decided again, and the check runs in the
  transaction that writes, under a lock covering what it read.
- **The document–workflow seam** (FR-DOC-012, FR-WF-013). A document links to at
  most one live process, and its status is a projection of that process's state
  — mapped by the *definition*, so a new workflow says what its own states mean
  for a document without a backend change.
- **A workflow on a document type** (FR-RAD-009), checked against a definition
  that exists and is published. A type binding nothing still submits: not every
  document is approved.
- **The task inbox and task detail** (FR-TASK-001, 002, 003) at `/tasks`. Tasks
  assigned to you and tasks offered to roles you hold, shown apart because they
  are different situations, with a detail that names the document, the process
  and the decision being asked.
- The JWSS meta-schema is **extracted** to
  [`docs/schema/jwss-meta-v1.0.0.json`](docs/schema/jwss-meta-v1.0.0.json), and
  a test keeps it identical to the specification's own block.

### Changed

- **A party holds a role once, and the database is what says so**
  ([#115](https://github.com/sujanto-gaws/kelir/issues/115)).
  `uq_mdm_party_roles_party_id_role_type_id_starts_at` included the assignment's
  start date, so two live rows for one party and one role type were legal
  whenever their `fromDate`s differed — the hole
  [#105](https://github.com/sujanto-gaws/kelir/issues/105) fell through, where
  `assign_role` checked for an existing assignment and the database did not.
  `0024_one_live_role_per_party.sql` replaces it with
  `uq_mdm_party_roles_party_id_role_type_id` on `(party_id, role_type_id) WHERE
  deleted_at IS NULL`. The lock #105 added stays: the index catches what a
  future writer forgets, the lock keeps the outcome a correct 200/201 rather
  than the 500 a unique violation surfaces as.

  **No API behaviour changes.** `assign_role` was already idempotent, and a role
  that is ended and assigned again still keeps both periods, because the closed
  one carries `deleted_at` and the partial index does not see it — which is why
  `starts_at` was never load-bearing in the key, and why taking it out is safe.

  **The migration refuses rather than repairs.** Two live rows differ in
  `starts_at`, `comments` and `attributes_json` and share one profile, so
  choosing which survives is not a migration's call. It names every offending
  pair and stops, as `0018_party_code_is_not_released.sql` does for a duplicated
  party code.

- `deploy.sh` checks every `KELIR_*` variable `.env.staging.example` declares,
  rather than a hard-coded list of four that had fallen behind the seven the
  backend reads. A `.env` copied from an older release now fails fast naming the
  variable instead of restarting a container. `KELIR_VERSION` is excluded,
  because the script takes the version as its argument and exports it over
  `.env` — demanding it would refuse a deployment for omitting the one value
  the caller had already supplied.

### Fixed

**Every one of these was found after the notes above were written, by the Phase 5
exit steps rather than by the construction that produced them** — the
[independent pass](projects/verifications/09.%20Sprint%2011%20Independent%20Pass.md)
and the [exit demo](projects/verifications/10.%20Phase%205%20Exit%20Demo.md). They
are listed together because that provenance is the useful thing about them.

- **A workflow definition could publish and then fail at run time**
  ([#259](https://github.com/sujanto-gaws/kelir/issues/259)). The meta-schema did
  not bound what the columns bounded, so a definition whose `taskName` ran past
  200 characters — or `taskDefinitionKey` past 64, or `states[].name` past 200,
  or `variables[].key` past 64, or `version` past 40 — saved, published, and then
  **500ed on the submit**, nowhere near the definition that caused it. Five
  `maxLength` keywords now refuse it at save, where it can be acted on.

  **A self-transition is legal and now runs.** `REVIEW --RETURN--> REVIEW` —
  *send it round again* — satisfies S3, S4, S6 and S7, and JWSS forbids nothing
  about it, but a `CHECK` on the history table refused the row it produces.
  `0030` drops the constraint rather than the construct: what the table records
  is a decision, and a reviewer who acted and left the document where it was is
  the row a reader most needs. Recorded as **JWSS R-5**, the first narrowing on
  the `1.x` line.

- **The workflow tab said an approval had generated no steps when the reader
  simply could not see them** ([#263](https://github.com/sujanto-gaws/kelir/issues/263)).
  The API returns an empty task list to a caller holding `workflow:instance:read`
  and not `workflow:task:read` — deliberately — and the tab rendered that under
  *"These are the steps this approval has generated"*, with the history
  immediately below listing the decisions people had taken on those steps. **The
  permission combination is the requester's**: whoever raised a document holds no
  tasks. Three outcomes now read differently: not allowed to look, looked and
  found nothing, and the list itself.

- **The task screen promised transitions that are not coming**
  ([#264](https://github.com/sujanto-gaws/kelir/issues/264)). `AUTO` edges
  reached the screen as things a person might one day do; they never can, because
  JWSS gives an `AUTO` transition no caller. The API now filters them. And
  `DELEGATE` was described as arriving in a later release when delegation had
  already shipped as a route — the component's own header said so sixty lines
  above the template that contradicted it.

- **A button named a destination it could not know**
  ([#271](https://github.com/sujanto-gaws/kelir/issues/271)), found by the exit
  demo on its first run. Where a state declares two transitions for one action,
  the engine picks between them when the decision arrives — and the button named
  the first-written one, so a request below a routing threshold read *"Approve →
  Director approval"* when it would complete. The verb alone where the engine
  chooses; the destination where it cannot.

- **A definition is now loaded only for the tenant that pointed at it**
  ([#260](https://github.com/sujanto-gaws/kelir/issues/260)). The read had no
  tenant predicate, deliberately, defended by a comment naming one caller where
  there were five. No cross-tenant read was possible — every caller passed an id
  from an already-scoped row — but that was a property of the callers rather than
  of the read. It is checked now: with the check removed, an instance pointed at
  another tenant's definition is decided successfully.

### Known limitations

- **Escalation is not built.** FR-WF-010 is `Could` and unscheduled: a definition
  may declare `escalation` and it is stored, validated and never acted on,
  because there is no scheduler. Lateness is made *visible* — a due date and the
  inbox's overdue indicator — and nothing acts on it automatically.
- **A `DELEGATE` transition fires nothing, and that is not a gap this release
  left.** Delegation is built, as a route of its own: handing a task over
  answers nothing about the document and moves no process, so it is not a
  transition. A definition declaring a `DELEGATE` edge is declaring one nothing
  will ever fire.
- **Delegation, conditional routing and due dates left this list in this
  release**, built by #184, #186 and #185. `RETURN` left it when #183 built it.
- **Guards and actions are stored and never executed**, because there is no
  lifecycle hook chain yet. A stored handler is not evidence that it runs.
- **`AUTO` transitions do not fire.** Nothing drives one until system tasks land.
- **Two of JWSS's six assignee types are refused at save** —
  `MANAGER_OF_OWNER` and `EXPRESSION` — with a message naming what to use
  instead. [JWSS §5.3](docs/schema/JSON%20Workflow%20Schema.md) records why.
- **Approvals are sequential.** One open task per process, held by a database
  index rather than by intention.

## [0.4.0] — 2026-08-28

Phase 4, whole: **a document exists.** It is created from a configured document
type, rendered as the form that type binds, validated on every write by a server
that recomputes what the browser computed, submitted with its number taken in
the same transaction, moved through a lifecycle of its own, found in a list and
opened in a workspace. Everything the two sprints before it built was
scaffolding for this one, and the phase's exit demo now runs end to end in a
real browser rather than being described.

**Upgrading:** one new migration (`0023_document.sql`) adding one table, one
index and six permission rows. It is additive — the `v0.3.0` binary starts
against a `v0.4.0` schema, rehearsed. No configuration action is required. One
behaviour changes for anyone who set a numbering rule to `ALLOW_GAPS`: a
submission the server refuses now loses the number it took, which is what that
policy has always meant and did not do.

**Read the *Fixed* section before deploying under load.** The most serious defect
this release closes is one it also introduced: a submit held two pooled
connections and self-deadlocked at a concurrency the pool could otherwise serve.
It was written twice in one function and the first fix did not find the second.

Phase 4 opens: the RAD metadata tables and the definition APIs over them, the
document table group with document types and their numbering rules, one JSON
Logic engine shared by both sides, a browser harness that drives a real
deployment, and the first form a person can actually fill in — rendered from a
stored definition, evaluating its own rules as they type, and submitted through
a server that recomputes every figure rather than trusting the one it was sent.
An administrator binds a type to a published form without a developer, which is
the last piece of that loop. Two Phase 2 carry-overs land with them, and a third — tenant
management — returns from the unscheduled backlog and takes multi-tenant mode
with it.

**All seven of the Sprint 7 verification pass's findings are closed**, four of
them in the sprint that inherited them. A department-scoped sequence keeps a
counter per department rather than one per rule, which needed a migration and a
table; a `sum` that would silently total zero is refused when the definition is
saved rather than confirmed by the server that re-evaluates it; the
forgot-password answer no longer waits for the mail server; the audit chain hash
tells an absent field from an empty one; the last quarantined test in the suite
runs; and the repository predicates that were the only guard on their behaviour
have tests that go red when they are defeated.

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
- **A document type is configured against a published form, by an administrator
  rather than by a developer (FR-RAD-008,
  [#165](https://github.com/sujanto-gaws/kelir/issues/165)).** This closes the
  loop Sprint 7 opened: form definitions are stored, document types are stored,
  and the two are now joined through the API rather than through a migration or
  a seed script. Choosing the form, choosing the numbering rule and saving a
  type documents can be created from are all configuration.

  **Re-pointing a type at a newer form revision is allowed, and existing
  documents keep the revision they were filled against** (decision **D-30**).
  Refusing outright was the alternative and it is worse: a form is revised by
  publishing the next revision, so a type that could never be re-pointed would
  be stuck on revision 1 from the moment its first document existed.

  **It is refused in exactly one case — while a document of that type pinned no
  revision at all.** A document carries its own `form_id` and renders through
  that, so moving the type's binding cannot reach it; but the column is
  nullable, and a document that pinned nothing has only the type's *current*
  binding to render against. That is the difference between a guarantee and a
  comment describing one, and it is enforced by the foreign key rather than by
  a convention: creating a document takes a lock on the type row that a
  rebinding conflicts with.

  **Two refusals that #157 wrote were only ever checked on create.** Binding a
  form that does not exist, or one that is still a draft, was asserted on
  `POST` and by nothing on `PUT` — removing the check from the update path left
  every test green. Both are now checked where the update makes them.

  **And the three numbering-rule routes were serving traffic with their
  authorization asserted by nobody.** #158 added them after the table that binds
  each document-type route to its permission was written, and nothing extended
  it. `GET` needs `document-type:read`; `PUT` and `DELETE` need
  `document-type:update`. The behaviour has not changed — the checks were
  always in the service — but until now a mutation removing one would have gone
  unnoticed.

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

  **No rules, deliberately.** Validation and submitting are the two items below,
  and the evaluator is not imported by this surface at all — which is also what
  keeps its 588 KB off the render path per decision D-10.

- **A rendered form evaluates its own rules as they are typed into (FR-RAD-010,
  FR-RAD-006, [#163](https://github.com/sujanto-gaws/kelir/issues/163)).**
  Validation and calculation in the browser, over the JSON Logic engine decision
  **D-10** adopted. A `calculate` expression recomputes as its inputs change,
  branched on the **declared** `calculateMode` and never on whether the
  operators look deterministic (JFSS S8.1.1); a `conditional` shows, hides,
  enables or disables the component that carries it; and §5's validation
  keywords and §6's `rules` decide each field, in the definition's own words
  where it supplies them.

  **A form shows its verdict only once a submit has been attempted** (decision
  **D-25**), after which the messages track live so a corrected field clears
  immediately. Marking every empty box red on a form nobody has touched tells
  the person in front of it nothing — and on a zero-filled payload an average
  field fails before the first keystroke, so the same is true of the
  calculations.

  **A rule the Validation Rule Registry does not define is a defect that is
  shown, and a rule it defines that the browser cannot decide is named rather
  than skipped** (decision **D-26**). A check that quietly did not run is
  indistinguishable from a check that passed, which is the whole of the argument.

  **This is the evaluation, not the rule engine.** The catalogue, the dependency
  graph, cycle detection and error mapping stay in Sprints 14–16 under decision
  **D-2**.

- **A filled-in form is submitted, and the server does not believe what the
  browser computed (FR-RAD-010, FR-RAD-006,
  [#164](https://github.com/sujanto-gaws/kelir/issues/164)).**
  `POST /api/v1/rad/forms/{id}/submissions` takes a payload carrying every data
  key the definition declares — visible or not, which is JFSS S10.1 — and stores
  a row in `rad_form_submissions` whose payload is **the server's own answer**.

  **This is the Tamper-Proof Pattern, and it is the security-critical control of
  the phase.** JFSS S8.1 requires the backend to re-evaluate every `calculate`
  expression and overwrite the submitted value before persistence; S10.2
  requires the same for every `conditional`, discarding the values of components
  that resolve to hidden. A submitted total the server accepted because the
  browser said so is not a rounding bug — it is an invoice for the wrong amount.
  A `sequenceKey` is overwritten with the row's real position for the same
  reason, and a key the definition does not declare is dropped rather than
  stored.

  **A refusal is a refusal, never a partial write.** An expression that produces
  no value — which since decision **D-24** includes every division by zero — a
  field that fails its `validation`, and a rule name outside the registry each
  refuse the whole submission with the S10.3 envelope, whose dot-notation `path`
  names the field: `line_items.2.quantity` addresses a row. The frontend places
  those messages against the fields they name.

  **What the server stored comes back, and the page says so if it differs from
  the screen.** Both sides run one engine compiled for two runtimes, and
  `parity/forms.json` now holds them to the same answer over whole *submissions*
  rather than over expressions alone — so a difference is a parity defect rather
  than a routine correction, which is why it is on the screen instead of in a
  log. *A form that changes your number without saying so is its own defect.*

  **Filling in a form needs `rad:form:submit`**, which is separate from
  `rad:form:read`: opening a requisition to read it and raising one are
  different questions. Only a **published** revision can be filled in.

  **It is not a document yet.** FR-DOC-001..007 — creating a document from a
  type, its number, its status, its versions — are the next sprint's under
  decision **D-16**, and the re-evaluation is deliberately callable without a
  submission row so that it can run inside the transaction that takes a
  document's number.

- **Documents (FR-DOC-001..007, 011, 013, 014) — the thing the platform is
  about.** A document is created from a document type, holds the data somebody
  fills into the form that type binds, takes a number when it is submitted, and
  moves through its own statuses until it is finished. Nine routes under
  `/api/v1/documents`, six permissions, and a screen for each half of it.
  - **The form revision is pinned at creation.** A document renders against the
    definition it was actually filled in against, not against whatever its type
    points at today — which is what makes **D-30**'s guarantee true rather than
    described, and what keeps a type re-pointable once documents exist.
  - **Form data is validated on every write, not only at submit.** A draft
    holding data its own form would reject is a draft that cannot be submitted,
    discovered at the worst moment. A value that is *present and wrong* is
    refused; a value that is *missing* is unfinished (**D-33**).
  - **The stored payload is always the server's answer.** The Tamper-Proof
    Pattern applies to a draft save as much as to a submission, so a client
    cannot launder a computed total through storage and submit it later.
- **Submit, with the number taken in the same transaction (FR-DOC-003, 004).**
  One transaction re-evaluates the payload, takes the number, moves the status
  and writes the history — committing whole or not at all. The re-evaluation
  runs **before** the allocation: numbering first burns a number on every
  refused submission, and on a gapless rule it also holds the counter for the
  length of the re-evaluation, so one document about to be refused would
  serialise every concurrent submit of its type.
- **The document's own status and its transitions (FR-DOC-007).** A legality
  table decides what may move where; an illegal move is refused naming both
  ends, and a concurrent one loses a compare-and-swap with a 409.
  `PENDING_APPROVAL` and `ARCHIVED` are in the column and reachable from
  nothing — the first is the workflow's, the second is FR-DOC-010's, and the
  table says which rather than leaving a reader to grep.
- **A link to the master-data entity a document concerns (FR-DOC-011).**
  Reading a document hands back `entityType` and `entityId` and nothing about
  the record they name; resolving them requires the entity's **own** read
  permission, enforced by calling the master-data service rather than by
  checking a string. A document cannot open what the master-data surface does
  not — the same answer the lookup fields gave.
- **The document list, with search and filter (FR-DOC-013, FR-SRH-001).** Paged,
  searchable by number, reference or title, filterable by type, status, priority
  and linked entity. FR-SRH-001 is this list rather than a second endpoint, per
  the SRS's own note. Its visibility rule is stated in full — tenant scope plus
  `document:read`, and no third condition.
- **The document workspace (FR-DOC-014).** Status, number, reference and linked
  entity visible without opening a tab; the form rendered through the renderer
  in read or edit mode by status; submit and the transitions reachable from it.
  The tabs Phase 5 and Phase 6 will fill say what will fill them and when —
  neither empty nor silent.
- **A screen that goes from a document *type* to a document.** Sprint 8 could
  open a form by form id and nothing traversed the type-to-form binding; now
  nobody types a form id, because choosing a type is how a document is started.
- **`document_ref_sequences`** (`0023_document.sql`), the tenant-and-year counter
  behind `documents.document_ref` — a shape the schema documented and nothing
  produced.
- **Six permissions**: `document:create`, `read`, `update`, `delete`, `submit`,
  `transition`. `submit` and `transition` are separate from `update` and neither
  is a convenience — submitting takes a number the document keeps forever, and a
  transition has a from-state, a legal set and its own audit action.

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

- **A form-data change is audited by its changed *keys*, not its values**
  (**D-32**). A form's data is arbitrary tenant content — salaries, bank
  details, the medical grounds for a leave request — and the audit trail is read
  through its own permission by people who hold none over the document. **D-12**
  already refused to hand a record's field values back through its change
  history; copying every keystroke of every form into that table would be that
  finding at scale, over data nobody classified. Every other field audits
  normally, with its values.
- **A gap-tolerant numbering rule loses the number a refused submission took**
  (**D-35**). It did not before, and the arrangement that kept it was bought
  with a rule violation — see *Fixed*. This is the trade that policy has always
  named.
- **Database Schema section headers stop naming a migration that is not written
  yet** (**D-34**), and say what the migration creates instead. Seven of them
  named files two numbers out of date; the mapping table is the sequence and a
  header is not.

### Fixed

- **A forgot-password request no longer waits for the mail server, and the
  module no longer claims the answer is untimeable
  ([#202](https://github.com/sujanto-gaws/kelir/issues/202), decision **D-31**).**
  `request_reset` awaited a complete SMTP transaction before writing its `202`,
  so the answer for a known account was measurably slower than for an unknown
  one: against mailpit on the loopback interface, a p50 of **90.1ms against
  9.8ms**, ranges not overlapping — an account-enumeration oracle on a route
  that is deliberately not rate-limited. The send is now handed to the runtime
  (`Mailer::send_detached`), which costs nothing, because a send never reported
  a failure to the caller anyway. Re-measured: **26.4ms against 10.5ms**. The
  oracle is narrowed rather than closed, and the module header now says so with
  both numbers instead of promising "no branch a caller can time"; D-31 records
  why the remaining local database work stays on the request's path.
- **The audit chain hash tells an absent optional field from an empty one
  ([#203](https://github.com/sujanto-gaws/kelir/issues/203)).** **A hash format
  change**, the second and — like the first — taken while nothing has ever
  verified a chain, so no stored value is invalidated in practice and no
  re-chaining is owed. An absent field was hashed as zero bytes and so was a
  present-but-empty one, so four entries differing only in whether `ip_address`
  and `reason` were `NULL` or `''` produced one digest: either column could be
  rewritten either way and the chain still verified. An absent field is now a
  length prefix of `2^64-1`, which no present field can produce.
  `0022_audit_hash_tells_absent_from_empty.sql` corrects the column comment
  `0019` had just corrected, because a merged migration is never edited.

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

- **A submit took two pooled connections and deadlocked under load, twice over.**
  `numbering_service::allocate` read the rule's gap policy from the pool while
  its caller held a transaction; and on a gap-tolerant rule the allocation
  itself then committed in a transaction of its own, *also* while the caller
  held one. Twenty-four concurrent submits answered `500` after the acquire
  timeout. Both halves lived in one function twelve lines apart, and the first
  fix — aimed at the test that had gone red — did not find the second, which
  took a re-read against the coding standard's one-connection rule. The number
  is now taken before the submit's transaction opens.
- **One document's metadata edit could have wiped every document's metadata in
  the tenant.** `replace_metadata` deletes before it inserts and its
  `document_id` predicate was the only thing scoping that delete; nothing
  exercised it. Found by reading the mutation campaign's survivors one at a
  time.
- **Five predicates that were the only guard on their behaviour**
  ([#218](https://github.com/sujanto-gaws/kelir/issues/218)) have tests that go
  red when they are defeated. One of the five turned out to be dead code and was
  deleted rather than pinned.

### Testing

- **A browser harness (`e2e/`).** Playwright drives a real deployment — the
  release images brought up by `deploy-local.sh` — through one full flow, and
  runs in CI as `End-to-end (browser)`. Not a released artefact, so it is not
  versioned with the product. A second flow, tenant creation, joined it.
- **Two security controls were accepted only after the defect was reintroduced
  and the test seen to fail** (coding standard §2.9): the administering-tenant
  check on the tenant routes, and the composite foreign key that refuses a
  cross-tenant role grant. Each test names its mutation in a comment.
- **The cross-tenant isolation test quarantined on decision D-7 now runs
  ([#204](https://github.com/sujanto-gaws/kelir/issues/204)).** **D-18**
  superseded D-7 inside the sprint that left the `#[ignore]` in place, and the
  condition the quarantine named — a token that can carry a tenant other than
  the default — was met by it. The test runs against a multi-tenant app with
  the foreign caller holding `identity:user:read`, so it reaches tenant scoping
  rather than stopping at the permission gate, which is what the quarantined
  body would have done. The suite reports **zero** ignored tests.
- **The predicates a repository writes as a second line of defence now have
  tests, and a rule that outlives them
  ([#206](https://github.com/sujanto-gaws/kelir/issues/206)).** Four predicates
  that were the only guard on their behaviour gained tests that go red when they
  are defeated — a form key being taken per tenant, a cleared numbering rule
  stopping, a numbering rule belonging to one tenant, and a starting sequence
  judged against its own bucket. The publish race that `AND status = 'DRAFT'`
  exists for is now arranged rather than raced for, by holding the row's lock in
  one transaction while the second statement blocks on it. Coding standard §2.5
  carries the rule for the next such predicate, §2.9 carries the three-move rule
  for tests over a shared resource, and sprint plan §2 names the per-sprint
  mutation campaign whose ratio the status report reports.

- **The Phase 4 exit demo is driven rather than described.**
  `e2e/tests/a-document-is-created-and-submitted.spec.ts` configures a type,
  creates a document from it, fills the form with live calculation, watches an
  unfinished submit refused, submits, sees the number, finds the document in the
  list, opens it and moves it through a transition. Eight browser flows now run
  against the release stack in CI.
- **The predicate-coverage ratio rose for the first time: 48%**, against 22%,
  37% and 65% for the three sprints before it. It did not rise by itself — the
  campaign was run twice and the first run's number was discarded, because five
  of its reds were one flaky test rather than the predicates being mutated.
- **A flaky test was found and repaired by the campaign it contaminated.** It
  asserted that every loser of a concurrent transition answered 409; a caller
  that acquires its connection after the winner commits reads the new status and
  is refused as an *illegal move* instead, which is equally correct. Six clean
  runs had not shown it.
- **Three second lines of defence are reached rather than commented**: an edit,
  a discard and a second submit each blocked by a submit that reached the row
  first, with the interleaving arranged rather than raced for.

### Known limitations

- **No Sprint 9 item is independently verified.** The sprint that closes this
  phase had one author, who built it, verified it and wrote its record. Every
  row of the [Sprint 9 status report](projects/status/10.%20Sprint%209%20Status.md)
  reads `author-verified` rather than Done, and
  [record 06](projects/verifications/06.%20Sprint%209%20Surface%20Verification.md)
  §1 states what that costs before it states anything else. An independent read
  of `modules/document/` is Sprint 10's first action.
- **Phase 4 ships its `Must` scope and none of its `Should` scope** — attachment
  and metadata rules (FR-DTYPE-005, 006), the document security level
  (FR-DTYPE-008), version history (FR-DOC-008), cancel-and-archive storage
  (FR-DOC-009, 010) and retention storage (FR-DTYPE-007). Decision **D-16**
  accepted that when it gave the phase a third sprint instead of a fourth.
  `documents.security_level` therefore exists in the column and **nothing reads
  it**: document visibility is tenant scope plus `document:read`, stated in full
  rather than inherited from a requirement that did not ship.
- **`PENDING_APPROVAL` and `ARCHIVED` are values nothing can reach.** The first
  belongs to the workflow Phase 5 builds; the second to FR-DOC-010. The legality
  table says so rather than leaving a reader to find out by trying.
- **`lock_linked_entity`'s facility arm is exercised by no test**, because no
  test links a document to a facility. The predicate is identical to the party
  arm's, which is held; the gap is named in record 06 §6 rather than left for
  the next campaign to find.
- **[#122](https://github.com/sujanto-gaws/kelir/issues/122) is still open.** The
  document list refuses a bad `page` inside the error envelope; the routes that
  answer outside it are unchanged.
- **No staging host and no production environment** (decision **D-9**). The
  release check runs against the Docker Compose stack built from the release
  images, which does not serve TLS — an IP address cannot be issued a
  certificate — so NFR-SEC-010 is not exercised by it.
- **Rolling back to `0.1.0` still needs manual work.** That binary predates
  `set_ignore_missing`. Rollback to `0.2.0` and later is rehearsed and works.

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

[Unreleased]: https://github.com/sujanto-gaws/kelir/compare/v0.4.0...HEAD
[0.4.0]: https://github.com/sujanto-gaws/kelir/releases/tag/v0.4.0
[0.3.0]: https://github.com/sujanto-gaws/kelir/releases/tag/v0.3.0
[0.2.0]: https://github.com/sujanto-gaws/kelir/releases/tag/v0.2.0
[0.1.0]: https://github.com/sujanto-gaws/kelir/releases/tag/v0.1.0
